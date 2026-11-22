//! Windows media state confirms a source-identified command; it never
//! produces a media notification by itself.
use crate::{Event, media_diagnostics, talk_audio_windows::endpoint_id};
use nothingx_desktop::{
    media::{Correlator, HardwareCommand, Output, Playback, Sample, Session, Track},
    preferences::MediaOptions,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
use tao::event_loop::EventLoopProxy;
use windows::{
    Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as WindowsPlayback,
    },
    Win32::{
        Media::Audio::{
            Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator, MMDeviceEnumerator, eMultimedia,
            eRender,
        },
        System::{
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
            WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
        },
    },
    core::{IUnknown, Interface},
};
use windows_future::AsyncStatus;

pub struct MediaObserver {
    control: Option<SyncSender<HardwareCommand>>,
    track: Option<SyncSender<HardwareCommand>>,
    stopped: Arc<AtomicBool>,
}

impl MediaObserver {
    pub fn start(proxy: EventLoopProxy<Event>, generation: u64, options: MediaOptions) -> Self {
        let stopped = Arc::new(AtomicBool::new(false));
        let start_worker = |options| {
            let (commands, receiver) = mpsc::sync_channel(8);
            let stop = stopped.clone();
            let proxy = proxy.clone();
            thread::spawn(move || observe(proxy, generation, options, receiver, &stop));
            commands
        };
        // Metadata providers can reply slowly. Keep Play/Pause and endpoint
        // observations independent of track lookup and never mix sample intervals.
        let control = (options.play_pause || options.volume).then(|| {
            start_worker(MediaOptions {
                track: false,
                ..options
            })
        });
        let track = options.track.then(|| {
            start_worker(MediaOptions {
                track: true,
                play_pause: false,
                volume: false,
            })
        });
        Self {
            control,
            track,
            stopped,
        }
    }

    pub fn command(&self, command: HardwareCommand) {
        use nothingx_desktop::media::Command::{Next, Previous};
        let sender = if matches!(command.action, Next | Previous) {
            &self.track
        } else {
            &self.control
        };
        if let Some(sender) = sender
            && sender.try_send(command).is_err()
        {
            media_diagnostics::record(format_args!(
                "source-identified command suppressed: observer queue unavailable"
            ));
        }
    }
}

impl Drop for MediaObserver {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        // A Windows API can be in flight. Never wait on it in the UI thread.
        // Dropping the last sender also wakes up a waiting observer.
    }
}

fn await_operation<T: windows::core::RuntimeType>(
    result: windows::core::Result<windows_future::IAsyncOperation<T>>,
    stop: &AtomicBool,
) -> Option<T> {
    let operation = result.ok()?;
    for _ in 0..15 {
        match operation.Status().ok()? {
            AsyncStatus::Completed => return operation.GetResults().ok(),
            AsyncStatus::Started if !stop.load(Ordering::Acquire) => {
                thread::sleep(Duration::from_millis(20))
            }
            _ => break,
        }
    }
    let _ = operation.Cancel();
    None
}

fn read_volume(enumerator: &IMMDeviceEnumerator) -> Option<Output> {
    unsafe {
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .ok()?;
        let name = endpoint_id(&device)?;
        let endpoint: IAudioEndpointVolume = device.Activate(CLSCTX_INPROC_SERVER, None).ok()?;
        let scalar = endpoint.GetMasterVolumeLevelScalar().ok()?;
        (scalar.is_finite() && (0. ..=1.).contains(&scalar)).then_some(Output {
            id: name,
            scalar,
            muted: endpoint.GetMute().ok()?.as_bool(),
        })
    }
}

fn read_session(manager: &Manager, tracks: bool, stop: &AtomicBool) -> Option<(IUnknown, Session)> {
    let session = manager.GetCurrentSession().ok()?;
    let identity = session.cast::<IUnknown>().ok()?;
    let app = session.SourceAppUserModelId().ok()?.to_string_lossy();
    if app.is_empty() {
        return None;
    }
    let info = session.GetPlaybackInfo().ok()?;
    let playback = match info.PlaybackStatus().ok()? {
        WindowsPlayback::Playing => Playback::Playing,
        WindowsPlayback::Paused => Playback::Paused,
        WindowsPlayback::Stopped => Playback::Stopped,
        _ => Playback::Other,
    };
    let mut can_next = false;
    let mut can_previous = false;
    let mut track = None;
    if tracks {
        if let Ok(controls) = info.Controls() {
            can_next = controls.IsNextEnabled().unwrap_or(false);
            can_previous = controls.IsPreviousEnabled().unwrap_or(false);
        }
        track = await_operation(session.TryGetMediaPropertiesAsync(), stop).and_then(|info| {
            let title = info.Title().ok()?.to_string_lossy();
            if title.trim().is_empty() {
                return None;
            }
            Some(Track {
                title,
                artist: info
                    .Artist()
                    .map(|v| v.to_string_lossy())
                    .unwrap_or_default(),
                album: info
                    .AlbumTitle()
                    .map(|v| v.to_string_lossy())
                    .unwrap_or_default(),
                number: info.TrackNumber().unwrap_or_default(),
            })
        });
    }
    let sample = Session {
        identity: identity.as_raw() as usize,
        app,
        playback,
        can_next,
        can_previous,
        track,
    };
    Some((identity, sample))
}

fn observe(
    proxy: EventLoopProxy<Event>,
    generation: u64,
    options: MediaOptions,
    commands: Receiver<HardwareCommand>,
    stop: &AtomicBool,
) {
    if unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_err() {
        return;
    }
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe {
                RoUninitialize();
            }
        }
    }
    let _apartment = Apartment;
    let manager = (options.play_pause || options.track)
        .then(|| await_operation(Manager::RequestAsync(), stop))
        .flatten();
    let volume: Option<IMMDeviceEnumerator> = if options.volume {
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER).ok() }
    } else {
        None
    };
    media_diagnostics::record(format_args!(
        "observers initialized: playback={} tracks={} volume={}",
        manager.is_some(),
        options.track,
        volume.is_some()
    ));
    let mut tracker = Correlator::default();
    // Retain COM identities over the baseline window. An object can't be
    // recycled at the same address while its IUnknown is still retained.
    let mut identities = std::collections::VecDeque::with_capacity(8);
    let mut last_playback = None;
    while !stop.load(Ordering::Acquire) {
        let delay = if tracker.armed() { 65 } else { 170 };
        match commands.recv_timeout(Duration::from_millis(delay)) {
            Ok(command) => {
                tracker.arm(command, Instant::now());
                media_diagnostics::record(format_args!(
                    "command {:?}: {}",
                    command.action,
                    if tracker.armed() {
                        "armed with fresh pre-command baseline"
                    } else {
                        "suppressed: stale command or baseline unavailable"
                    }
                ));
                for command in commands.try_iter().take(8) {
                    tracker.arm(command, Instant::now());
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if stop.load(Ordering::Acquire) {
            break;
        }
        let started = Instant::now();
        let session = manager
            .as_ref()
            .and_then(|m| read_session(m, options.track, stop));
        let output = volume.as_ref().and_then(read_volume);
        let finished = Instant::now();
        if identities.len() == 8 {
            identities.pop_front();
        }
        let session = session.map(|(identity, sample)| {
            identities.push_back(identity);
            sample
        });
        let playback = session.as_ref().map(|session| session.playback);
        if playback != last_playback {
            media_diagnostics::record(format_args!(
                "playback {last_playback:?} -> {playback:?}; armed={}",
                tracker.armed()
            ));
            last_playback = playback;
        }
        let was_armed = tracker.armed();
        let result = tracker.observe(Sample {
            started,
            finished,
            session,
            output,
        });
        if let Some(outcome) = result
            && !stop.load(Ordering::Acquire)
        {
            let _ = proxy.send_event(Event::Media(generation, outcome));
        } else if was_armed && !tracker.armed() {
            media_diagnostics::record(format_args!(
                "source-identified command suppressed: outcome expired or did not match"
            ));
        }
    }
}
