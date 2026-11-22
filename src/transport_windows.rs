//! Native Bluetooth Classic RFCOMM. All sockets belong to one worker;
//! Winsock events wake it for network data, writes and shutdown (no polling).
use super::*;
use std::{
    collections::VecDeque,
    mem::size_of,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};
use windows_sys::{
    Win32::{
        Devices::Bluetooth::*,
        Foundation::{CloseHandle, ERROR_NO_MORE_ITEMS, GetLastError},
        Networking::WinSock::*,
    },
    core::GUID,
};

const INITIAL_RETRY_MS: u32 = 3_000;
const MAX_RETRY_MS: u32 = 30_000;
const LINK_SAMPLE: Duration = Duration::from_secs(3);

fn next_retry_ms(current: u32, manual_refresh: bool) -> u32 {
    if manual_refresh {
        INITIAL_RETRY_MS
    } else {
        current.saturating_mul(2).min(MAX_RETRY_MS)
    }
}

pub struct Transport {
    tx: Sender<Vec<u8>>,
    wake: WSAEVENT,
    stopping: Arc<AtomicBool>,
    preferred: Arc<AtomicU64>,
    worker: Option<thread::JoinHandle<()>>,
}
struct Socket(SOCKET);
impl Drop for Socket {
    fn drop(&mut self) {
        unsafe {
            closesocket(self.0);
        }
    }
}
struct NetEvent(WSAEVENT);
impl Drop for NetEvent {
    fn drop(&mut self) {
        unsafe {
            WSACloseEvent(self.0);
        }
    }
}

impl Transport {
    pub fn start(proxy: EventLoopProxy<Event>, preferred: u64) -> Result<Self, String> {
        // SAFETY: valid, initialized Winsock storage; lifetime ends after worker join.
        unsafe {
            let mut data = WSADATA::default();
            let error = WSAStartup(0x0202, &mut data);
            if error != 0 {
                return Err(format!("Windows Bluetooth startup failed ({error})"));
            }
            let wake = WSACreateEvent();
            if wake == 0 {
                WSACleanup();
                return Err("Could not create Bluetooth command event".into());
            }
            let (tx, rx) = mpsc::channel();
            let stopping = Arc::new(AtomicBool::new(false));
            let stop = stopping.clone();
            let preferred = Arc::new(AtomicU64::new(preferred));
            let selection = preferred.clone();
            let worker = thread::spawn(move || run(proxy, rx, wake, stop, selection));
            Ok(Self {
                tx,
                wake,
                stopping,
                preferred,
                worker: Some(worker),
            })
        }
    }
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.tx.send(bytes.to_vec()).map_err(|e| e.to_string())?;
        unsafe {
            WSASetEvent(self.wake);
        }
        Ok(())
    }
    pub fn select(&mut self, address: u64) {
        self.preferred.store(address, Ordering::Release);
        self.refresh();
    }

    pub fn refresh(&mut self) {
        unsafe {
            WSASetEvent(self.wake);
        }
    }
    pub fn stop(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.stopping.store(true, Ordering::Release);
            unsafe {
                WSASetEvent(self.wake);
            }
            let _ = worker.join();
            unsafe {
                WSACloseEvent(self.wake);
                WSACleanup();
            }
        }
    }
}
impl Drop for Transport {
    fn drop(&mut self) {
        self.stop();
    }
}

fn is_nothing_like_name(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    lower.contains("nothing")
        || lower.contains("cmf")
        || lower.starts_with("ear (")
        || lower.starts_with("ear(")
        || lower.contains("buds pro")
        || lower.contains("neckband pro")
}

fn run(
    proxy: EventLoopProxy<Event>,
    rx: Receiver<Vec<u8>>,
    wake: WSAEVENT,
    stop: Arc<AtomicBool>,
    preferred: Arc<AtomicU64>,
) {
    let mut retry = INITIAL_RETRY_MS;
    let mut failed_scans: u32 = 0;
    let mut last_unrecognized: Option<String> = None;
    let diagnostics = std::env::var_os("WINX_BLUETOOTH_DIAGNOSTICS").is_some();
    while !stop.load(Ordering::Acquire) {
        let devices = observe_links(&proxy);
        let mut connected_any = false;
        let selection = preferred.load(Ordering::Acquire);
        for (name, address) in &devices {
            if selection != 0 && selection != *address {
                continue;
            }
            if stop.load(Ordering::Acquire) {
                return;
            }
            let (socket, event) = match open(*address, wake, &stop) {
                Ok(connection) => connection,
                Err((stage, error)) => {
                    if diagnostics {
                        eprintln!("RFCOMM {stage} failed (WinSock {error})");
                    }
                    continue;
                }
            };
            let selection = preferred.load(Ordering::Acquire);
            if selection != 0 && selection != *address {
                break;
            }
            connected_any = true;
            failed_scans = 0;
            last_unrecognized = None;
            // Commands from a previous connection may never leak into a new one.
            while rx.try_recv().is_ok() {}
            if proxy
                .send_event(Event::Connected(name.clone(), format!("{address:012X}")))
                .is_err()
            {
                return;
            }
            connected(&socket, &event, wake, &rx, &stop, &proxy, || {
                let selection = preferred.load(Ordering::Acquire);
                selection == 0 || selection == *address
            });
            if stop.load(Ordering::Acquire) {
                return;
            }
            let _ = proxy.send_event(Event::Disconnected);
            drop(socket);
            observe_links(&proxy);
            retry = INITIAL_RETRY_MS;
            break;
        }
        if !connected_any {
            let next = if let Some((name, _)) =
                devices.iter().find(|(name, _)| is_nothing_like_name(name))
            {
                failed_scans = failed_scans.saturating_add(1);
                if failed_scans >= 2 {
                    Some(name.clone())
                } else {
                    last_unrecognized.clone()
                }
            } else {
                failed_scans = 0;
                None
            };
            if next != last_unrecognized {
                last_unrecognized = next.clone();
                let _ = proxy.send_event(Event::Unrecognized(next));
            }
        }
        if stop.load(Ordering::Acquire) {
            break;
        }
        if selection != preferred.load(Ordering::Acquire) {
            continue;
        }
        // Discard writes while disconnected. Manual-reset command event also cancels retries.
        unsafe {
            WSAResetEvent(wake);
        }
        while rx.try_recv().is_ok() {}
        if stop.load(Ordering::Acquire) {
            break;
        }
        let woken =
            unsafe { WSAWaitForMultipleEvents(1, &(wake as *mut core::ffi::c_void), 0, retry, 0) };
        unsafe {
            WSAResetEvent(wake);
        }
        retry = next_retry_ms(retry, woken == WSA_WAIT_EVENT_0 as u32);
    }
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub name: String,
    pub address: u64,
    pub linked: bool,
    audio: bool,
}

impl Candidate {
    pub fn compatible_audio(&self) -> bool {
        nothingx_desktop::models::find(&self.name).is_some()
            || self.audio && is_nothing_like_name(&self.name)
    }
}

pub fn candidates() -> Option<Vec<Candidate>> {
    let mut result = Vec::new();
    // SAFETY: structure sizes follow the Windows ABI; no inquiry or pairing changes.
    unsafe {
        let search = BLUETOOTH_DEVICE_SEARCH_PARAMS {
            dwSize: size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
            fReturnAuthenticated: 1,
            fReturnRemembered: 1,
            fReturnConnected: 1,
            ..Default::default()
        };
        let mut d = BLUETOOTH_DEVICE_INFO {
            dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
            ..Default::default()
        };
        let find = BluetoothFindFirstDevice(&search, &mut d);
        if find.is_null() {
            return (GetLastError() == ERROR_NO_MORE_ITEMS).then_some(result);
        }
        loop {
            let end = d
                .szName
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(d.szName.len());
            let name = String::from_utf16_lossy(&d.szName[..end]);
            if is_nothing_like_name(&name)
                || nothingx_desktop::models::find(&name).is_some()
                || d.fConnected != 0 && d.ulClassofDevice & 0x1f00 == 0x0400
            {
                result.push(Candidate {
                    name,
                    address: d.Address.Anonymous.ullLong,
                    linked: d.fConnected != 0,
                    audio: d.ulClassofDevice & 0x1f00 == 0x0400,
                });
            }
            d.dwSize = size_of::<BLUETOOTH_DEVICE_INFO>() as u32;
            if BluetoothFindNextDevice(find, &mut d) == 0 {
                break;
            }
        }
        BluetoothFindDeviceClose(find);
    }
    result.sort_by_key(|device| {
        (
            nothingx_desktop::models::find(&device.name).is_none(),
            !is_nothing_like_name(&device.name),
            device.address,
        )
    });
    let mut seen = std::collections::BTreeSet::new();
    result.retain(|device| seen.insert(device.address));
    Some(result)
}

fn adapters() -> Vec<u64> {
    let mut ids = Vec::new();
    let mut radio = ptr::null_mut();
    let parameters = BLUETOOTH_FIND_RADIO_PARAMS {
        dwSize: size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32,
    };
    unsafe {
        let find = BluetoothFindFirstRadio(&parameters, &mut radio);
        if find.is_null() {
            return ids;
        }
        loop {
            let mut info = BLUETOOTH_RADIO_INFO {
                dwSize: size_of::<BLUETOOTH_RADIO_INFO>() as u32,
                ..Default::default()
            };
            if BluetoothGetRadioInfo(radio, &mut info) == 0 {
                ids.push(info.address.Anonymous.ullLong);
            }
            CloseHandle(radio);
            if BluetoothFindNextRadio(find, &mut radio) == 0 {
                break;
            }
        }
        BluetoothFindRadioClose(find);
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn observe_links(proxy: &EventLoopProxy<Event>) -> Vec<(String, u64)> {
    let adapters = adapters();
    let observed = Instant::now();
    let devices = candidates();
    let listed = devices.as_deref().unwrap_or_default();
    let _ = proxy.send_event(Event::BluetoothDevices(
        listed
            .iter()
            .filter(|device| device.compatible_audio())
            .cloned()
            .collect(),
    ));
    let links = devices
        .as_ref()
        .filter(|_| !adapters.is_empty())
        .map(|devices| nothingx_desktop::awareness::LinkSnapshot {
            adapters,
            present: devices
                .iter()
                .filter(|device| device.linked)
                .map(|device| format!("{:012X}", device.address))
                .collect(),
            observed,
        });
    let _ = proxy.send_event(Event::LinkSnapshot(links));
    devices
        .unwrap_or_default()
        .into_iter()
        .filter(|device| device.linked)
        .map(|device| (device.name, device.address))
        .collect()
}

fn open(
    address: u64,
    wake: WSAEVENT,
    stop: &AtomicBool,
) -> Result<(Socket, NetEvent), (&'static str, i32)> {
    // SAFETY: sockets and network events are RAII-owned. SOCKADDR_BTH is passed
    // at its exact packed ABI size; UUID resolution is performed by Windows SDP.
    unsafe {
        let raw = socket(i32::from(AF_BTH), SOCK_STREAM, BTHPROTO_RFCOMM as i32);
        if raw == INVALID_SOCKET {
            return Err(("socket", WSAGetLastError()));
        }
        let socket = Socket(raw);
        let raw_event = WSACreateEvent();
        if raw_event == 0 {
            return Err(("event creation", WSAGetLastError()));
        }
        let event = NetEvent(raw_event);
        if WSAEventSelect(raw, raw_event, FD_CONNECT as i32) != 0 {
            return Err(("event registration", WSAGetLastError()));
        }
        let remote = SOCKADDR_BTH {
            addressFamily: AF_BTH,
            btAddr: address,
            serviceClassId: GUID::from_u128(0xaeac4a03_dff5_498f_843a_34487cf133eb),
            port: 0,
        };
        if connect(
            raw,
            ptr::from_ref(&remote).cast(),
            size_of::<SOCKADDR_BTH>() as i32,
        ) != 0
        {
            let error = WSAGetLastError();
            if error != WSAEWOULDBLOCK {
                return Err(("connect", error));
            }
            let events = [
                wake as *mut core::ffi::c_void,
                raw_event as *mut core::ffi::c_void,
            ];
            let deadline = Instant::now() + Duration::from_secs(8);
            loop {
                if stop.load(Ordering::Acquire) {
                    return Err(("connect canceled", WSAEINTR));
                }
                let remaining = deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis() as u32;
                if remaining == 0 {
                    return Err(("connect timeout", WSAETIMEDOUT));
                }
                let selected = WSAWaitForMultipleEvents(2, events.as_ptr(), 0, remaining, 0);
                if stop.load(Ordering::Acquire) {
                    return Err(("connect canceled", WSAEINTR));
                }
                if selected == 0 {
                    WSAResetEvent(wake);
                    continue;
                }
                if selected == 1 {
                    break;
                }
                if selected == WSA_WAIT_TIMEOUT {
                    return Err(("connect timeout", WSAETIMEDOUT));
                }
                return Err(("connect event wait", WSAGetLastError()));
            }
            let mut network = WSANETWORKEVENTS::default();
            if WSAEnumNetworkEvents(raw, raw_event, &mut network) != 0 {
                return Err(("connect event", WSAGetLastError()));
            }
            let error = network.iErrorCode[FD_CONNECT_BIT as usize];
            if error != 0 {
                return Err(("connect completion", error));
            }
        }
        if WSAEventSelect(raw, raw_event, (FD_READ | FD_WRITE | FD_CLOSE) as i32) != 0 {
            return Err(("I/O event registration", WSAGetLastError()));
        }
        Ok((socket, event))
    }
}

fn connected(
    socket: &Socket,
    event: &NetEvent,
    wake: WSAEVENT,
    rx: &Receiver<Vec<u8>>,
    stop: &AtomicBool,
    proxy: &EventLoopProxy<Event>,
    still_selected: impl Fn() -> bool,
) {
    let mut output = VecDeque::<u8>::new();
    let mut bytes = [0u8; 4096];
    let mut shutdown_deadline = None;
    let mut next_observation = Instant::now() + LINK_SAMPLE;
    loop {
        if !still_selected() {
            return;
        }
        if stop.load(Ordering::Acquire) {
            shutdown_deadline.get_or_insert_with(|| Instant::now() + Duration::from_millis(500));
        }
        let handles = [
            wake as *mut core::ffi::c_void,
            event.0 as *mut core::ffi::c_void,
        ];
        let timeout = shutdown_deadline
            .unwrap_or(next_observation)
            .saturating_duration_since(Instant::now())
            .as_millis()
            .max(1) as u32;
        let selected = unsafe { WSAWaitForMultipleEvents(2, handles.as_ptr(), 0, timeout, 0) };
        if !still_selected()
            || selected == WSA_WAIT_FAILED
            || selected == WSA_WAIT_TIMEOUT && shutdown_deadline.is_some()
        {
            return;
        }
        if !stop.load(Ordering::Acquire) && Instant::now() >= next_observation {
            observe_links(proxy);
            next_observation = Instant::now() + LINK_SAMPLE;
        }
        unsafe {
            WSAResetEvent(wake);
        }
        while let Ok(frame) = rx.try_recv() {
            output.extend(frame);
        }
        // Flush already queued commands, including Stop Sound, before closing.
        // The next wait is bounded if the remote device stops accepting writes.
        let stopping = stop.load(Ordering::Acquire);
        let mut network = WSANETWORKEVENTS::default();
        if unsafe { WSAEnumNetworkEvents(socket.0, event.0, &mut network) } != 0 {
            return;
        }
        // Drain all available bytes; reading re-arms FD_READ.
        if !stopping {
            loop {
                let count = unsafe { recv(socket.0, bytes.as_mut_ptr(), bytes.len() as i32, 0) };
                if count == 0 {
                    return;
                }
                if count < 0 {
                    if unsafe { WSAGetLastError() } == WSAEWOULDBLOCK {
                        break;
                    }
                    return;
                }
                if proxy
                    .send_event(Event::Data(bytes[..count as usize].to_vec()))
                    .is_err()
                {
                    return;
                }
            }
        }
        if network.lNetworkEvents & FD_CLOSE as i32 != 0 {
            return;
        }
        while !output.is_empty() {
            let part = output.make_contiguous();
            let count = unsafe { send(socket.0, part.as_ptr(), part.len() as i32, 0) };
            if count < 0 {
                if unsafe { WSAGetLastError() } == WSAEWOULDBLOCK {
                    break;
                }
                return;
            }
            if count == 0 {
                return;
            }
            output.drain(..count as usize);
        }
        if stopping && output.is_empty() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_backs_off_to_thirty_seconds_and_resets_on_wake() {
        let mut delay = INITIAL_RETRY_MS;
        for expected in [6_000, 12_000, 24_000, 30_000, 30_000] {
            delay = next_retry_ms(delay, false);
            assert_eq!(delay, expected);
        }
        assert_eq!(next_retry_ms(delay, true), INITIAL_RETRY_MS);
    }
}
