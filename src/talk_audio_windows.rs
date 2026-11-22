//! Core Audio operations run in an MTA worker. The recovery journal belongs to
//! one endpoint ID and records only WinX's own volume writes.
use crate::{Event, pnp_windows};
use nothingx_desktop::preferences;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tao::event_loop::EventLoopProxy;
use windows::{
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_ContainerId,
        Media::Audio::{
            Endpoints::IAudioEndpointVolume, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
            eConsole, eRender,
        },
        System::{
            Com::StructuredStorage::PropVariantClear,
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, STGM_READ},
            Variant::VT_CLSID,
            WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
        },
    },
    core::{GUID, PCWSTR},
};

static AUDIO_WORKER: std::sync::Mutex<()> = std::sync::Mutex::new(());
const JOURNAL: &str = "winx-talk-audio.json";
const VOLUME_CONTEXT: GUID = GUID::from_u128(0x4b3a845d_163f_45a1_b07b_eaf56bc79b84);
const EPSILON: f32 = 0.0015;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Result {
    Prepared,
    Applied(u8),
    Restored,
    Unchanged,
    Preserved,
    Unavailable,
    Failed,
    Deferred,
    PendingOutput,
}

#[derive(Deserialize, Serialize)]
struct Journal {
    id: String,
    original: f32,
    last: f32,
    next: f32,
}

impl Journal {
    fn valid(&self) -> bool {
        !self.id.is_empty()
            && self.id.len() <= 4096
            && [self.original, self.last, self.next]
                .into_iter()
                .all(|v| (0. ..=1.).contains(&v))
    }

    fn owns(&self, volume: f32) -> bool {
        close(volume, self.last) || close(volume, self.next)
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= EPSILON
}

struct Apartment;
impl Apartment {
    fn open() -> Option<Self> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED).ok()? };
        Some(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() }
    }
}

pub fn endpoint_id(device: &IMMDevice) -> Option<String> {
    unsafe {
        let id = device.GetId().ok()?;
        let text = id.to_string().ok();
        CoTaskMemFree(Some(id.0.cast()));
        text
    }
}

fn selected_output(enumerator: &IMMDeviceEnumerator, address: &str) -> Option<IMMDevice> {
    let headset = pnp_windows::headset_container(address)?;
    unsafe {
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let mut value = store.GetValue(&PKEY_Device_ContainerId).ok()?;
        let prop = &*value.Anonymous.Anonymous;
        let pointer = if prop.vt == VT_CLSID {
            prop.Anonymous.puuid.as_ref()
        } else {
            None
        };
        let matches = pointer.is_some_and(|guid| {
            let id = windows_sys::core::GUID {
                data1: guid.data1,
                data2: guid.data2,
                data3: guid.data3,
                data4: guid.data4,
            };
            pnp_windows::same_container(&headset, &id)
        });
        let _ = PropVariantClear(&mut value);
        matches.then_some(device)
    }
}

fn restore(enumerator: &IMMDeviceEnumerator, path: &Path, duration: Duration) -> Result {
    if !path.exists() {
        return Result::Unchanged;
    }
    let Some(mut journal) = preferences::load_json::<Option<Journal>>(path).filter(Journal::valid)
    else {
        return Result::Failed;
    };
    if journal.original == journal.last && journal.original == journal.next {
        return clear(path, Result::Unchanged);
    }
    let id: Vec<_> = journal.id.encode_utf16().chain([0]).collect();
    let endpoint = unsafe {
        enumerator
            .GetDevice(PCWSTR(id.as_ptr()))
            .and_then(|device| device.Activate::<IAudioEndpointVolume>(CLSCTX_INPROC_SERVER, None))
    };
    let Ok(endpoint) = endpoint else {
        return Result::Deferred;
    };
    let target = journal.original;
    match fade(
        &endpoint,
        path,
        &mut journal,
        target,
        duration,
        &AtomicBool::new(false),
        || true,
    ) {
        Result::Applied(_) | Result::Unchanged
            if scalar(&endpoint).is_some_and(|actual| close(actual, target)) =>
        {
            clear(path, Result::Restored)
        }
        Result::Preserved => clear(path, Result::Preserved),
        _ => Result::Deferred,
    }
}

fn clear(path: &Path, outcome: Result) -> Result {
    if std::fs::remove_file(path).is_ok() || !path.exists() {
        outcome
    } else {
        Result::Failed
    }
}

fn scalar(endpoint: &IAudioEndpointVolume) -> Option<f32> {
    unsafe { endpoint.GetMasterVolumeLevelScalar().ok() }.filter(|v| (0. ..=1.).contains(v))
}

fn fade(
    endpoint: &IAudioEndpointVolume,
    path: &Path,
    journal: &mut Journal,
    target: f32,
    duration: Duration,
    cancelled: &AtomicBool,
    route_ok: impl Fn() -> bool,
) -> Result {
    let Some(start) = scalar(endpoint) else {
        return Result::Failed;
    };
    if close(start, target) {
        return Result::Unchanged;
    }
    if !journal.owns(start) {
        return Result::Preserved;
    }
    let begin = Instant::now();
    let steps = (duration.as_millis() as u32 / 25).max(1);
    for index in 1..=steps {
        let deadline = begin + duration * index / steps;
        thread::sleep(deadline.saturating_duration_since(Instant::now()));
        if cancelled.load(Ordering::Acquire) || !route_ok() {
            return Result::Failed;
        }
        let Some(current) = scalar(endpoint) else {
            return Result::Failed;
        };
        if !journal.owns(current) {
            return Result::Preserved;
        }
        let t = index as f32 / steps as f32;
        journal.last = current;
        journal.next = start + (target - start) * (t * t * (3. - 2. * t));
        if preferences::save_json(path, journal).is_err() {
            return Result::Failed;
        }
        if !scalar(endpoint).is_some_and(|level| close(level, current)) {
            return Result::Preserved;
        }
        if unsafe { endpoint.SetMasterVolumeLevelScalar(journal.next, &VOLUME_CONTEXT) }.is_err() {
            return Result::Failed;
        }
        let Some(actual) = scalar(endpoint) else {
            return Result::Failed;
        };
        journal.last = actual;
        journal.next = actual;
        if preferences::save_json(path, journal).is_err() {
            return Result::Failed;
        }
    }
    Result::Applied((journal.last * 100.).round() as u8)
}

pub enum Job {
    Prepare { address: String },
    Apply { target: u8, cancel: Arc<AtomicBool> },
    Restore { retry_parked: bool },
}

pub fn start(
    proxy: EventLoopProxy<Event>,
    directory: PathBuf,
    generation: u64,
    duration: Duration,
    job: Job,
) {
    thread::spawn(move || {
        let result = (|| {
            let _guard = AUDIO_WORKER.try_lock().ok()?;
            let _apartment = Apartment::open()?;
            let enumerator: IMMDeviceEnumerator =
                unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER).ok()? };
            let path = directory.join(JOURNAL);
            Some(match job {
                Job::Prepare { address } => prepare(&enumerator, &address, &path),
                Job::Apply { target, cancel } => {
                    apply(&enumerator, &path, target, duration, &cancel)
                }
                Job::Restore { retry_parked } => {
                    recover(&enumerator, &path, duration, retry_parked)
                }
            })
        })()
        .unwrap_or(Result::Unavailable);
        let _ = proxy.send_event(Event::TalkAudio(generation, result));
    });
}

fn prepare(enumerator: &IMMDeviceEnumerator, address: &str, path: &Path) -> Result {
    let Some(device) = selected_output(enumerator, address) else {
        return Result::Unavailable;
    };
    let Some(id) = endpoint_id(&device) else {
        return Result::Unavailable;
    };
    if !prepare_recovery(enumerator, path, &id) {
        return Result::PendingOutput;
    }
    let Ok(endpoint) =
        (unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_INPROC_SERVER, None) })
    else {
        return Result::Unavailable;
    };
    let Some(original) = scalar(&endpoint) else {
        return Result::Unavailable;
    };
    let journal = Journal {
        id,
        original,
        last: original,
        next: original,
    };
    if preferences::save_json(path, &journal).is_ok() {
        Result::Prepared
    } else {
        Result::Failed
    }
}

fn parked(path: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<_> = path
        .parent()
        .into_iter()
        .flat_map(|parent| std::fs::read_dir(parent).into_iter().flatten())
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("winx-talk-output-") && name.ends_with(".json")
                })
        })
        .collect();
    entries.sort();
    entries
}

pub fn pending_outputs(directory: &Path) -> usize {
    let path = directory.join(JOURNAL);
    usize::from(path.exists()) + parked(&path).len()
}

fn load_journal(path: &Path) -> Option<Journal> {
    preferences::load_json::<Option<Journal>>(path).filter(Journal::valid)
}

fn park(path: &Path) -> bool {
    if !path.exists() {
        return true;
    }
    for index in 0..64 {
        let target = path.with_file_name(format!("winx-talk-output-{index}.json"));
        if !target.exists() {
            return std::fs::rename(path, target).is_ok();
        }
    }
    false
}

fn prepare_recovery(enumerator: &IMMDeviceEnumerator, path: &Path, id: &str) -> bool {
    if path.exists() && !park(path) {
        return false;
    }
    for record in parked(path) {
        let Some(journal) = load_journal(&record) else {
            return false;
        };
        if journal.id == id
            && matches!(
                restore(enumerator, &record, Duration::from_millis(150)),
                Result::Deferred | Result::Failed
            )
        {
            return false;
        }
    }
    true
}

fn recover(
    enumerator: &IMMDeviceEnumerator,
    path: &Path,
    duration: Duration,
    retry_parked: bool,
) -> Result {
    let records = if retry_parked {
        parked(path)
    } else {
        Vec::new()
    };
    let mut result = restore(enumerator, path, duration);
    if matches!(result, Result::Deferred | Result::Failed) {
        let _ = park(path);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    for record in records {
        if Instant::now() >= deadline {
            return Result::Deferred;
        }
        let attempt = restore(enumerator, &record, duration);
        if matches!(
            attempt,
            Result::Deferred | Result::Failed | Result::Preserved
        ) {
            result = attempt;
        }
    }
    result
}

// Keep abandoned files as a local backup. They are excluded from future writes.
pub fn archive_outputs(directory: &Path) -> std::io::Result<()> {
    let _guard = AUDIO_WORKER
        .try_lock()
        .map_err(|_| std::io::Error::other("Windows audio is still busy"))?;
    let path = directory.join(JOURNAL);
    for record in std::iter::once(path.clone()).chain(parked(&path)) {
        if record.exists() {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            std::fs::rename(&record, record.with_extension(format!("{stamp}.archived")))?;
        }
    }
    Ok(())
}

fn apply(
    enumerator: &IMMDeviceEnumerator,
    path: &Path,
    target: u8,
    duration: Duration,
    cancel: &AtomicBool,
) -> Result {
    let Some(mut journal) = preferences::load_json::<Option<Journal>>(path).filter(Journal::valid)
    else {
        return Result::Failed;
    };
    let id = journal.id.clone();
    let route_ok = || {
        unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
            .ok()
            .and_then(|device| endpoint_id(&device))
            .is_some_and(|current| current == id)
    };
    if !route_ok() || cancel.load(Ordering::Acquire) {
        return Result::Failed;
    }
    let wide_id: Vec<_> = id.encode_utf16().chain([0]).collect();
    let Ok(endpoint) = (unsafe {
        enumerator
            .GetDevice(PCWSTR(wide_id.as_ptr()))
            .and_then(|device| device.Activate::<IAudioEndpointVolume>(CLSCTX_INPROC_SERVER, None))
    }) else {
        return Result::Failed;
    };
    let Some(current) = scalar(&endpoint) else {
        return Result::Failed;
    };
    if !journal.owns(current) {
        return clear(path, Result::Preserved);
    }
    let target_scalar = f32::from(target) / 100.;
    if close(current, target_scalar) {
        return clear(path, Result::Applied((current * 100.).round() as u8));
    }
    let result = fade(
        &endpoint,
        path,
        &mut journal,
        target_scalar,
        duration,
        cancel,
        route_ok,
    );
    if result == Result::Preserved {
        return clear(path, Result::Preserved);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_matches_last_write_or_an_interrupted_write_only() {
        let journal = Journal {
            id: "original endpoint".into(),
            original: 0.62,
            last: 0.14,
            next: 0.12,
        };
        assert!(journal.valid());
        assert!(journal.owns(0.14) && journal.owns(0.12));
        assert!(!journal.owns(0.5) && !journal.owns(journal.original));
    }
}
