//! Background subscription to HID Consumer Control reports only.
//! No keyboard hook, conventional media-key message, or WM_APPCOMMAND is used.
use crate::{media_diagnostics, pnp_windows};
use nothingx_desktop::media::{Command, HardwareCommand, HardwareSource};
use std::{
    collections::{BTreeMap, BTreeSet},
    mem::offset_of,
    ptr::{null, null_mut},
    time::Instant,
};
use windows_sys::Win32::{
    Devices::{
        DeviceAndDriverInstallation::{
            CM_DEVCAP_REMOVABLE, CM_DRP_CAPABILITIES, CM_Get_DevNode_Registry_PropertyW,
            CM_Get_Device_IDW, CM_Get_Parent, CR_SUCCESS, HDEVINFO, SP_DEVICE_INTERFACE_DATA,
            SP_DEVINFO_DATA, SetupDiCreateDeviceInfoList, SetupDiDestroyDeviceInfoList,
            SetupDiGetDeviceInterfaceDetailW, SetupDiOpenDeviceInterfaceW,
        },
        HumanInterfaceDevice::{HIDP_STATUS_SUCCESS, HidP_GetUsages, HidP_Input},
    },
    Foundation::{ERROR_INSUFFICIENT_BUFFER, GetLastError, HANDLE, HWND, INVALID_HANDLE_VALUE},
    UI::Input::*,
};

const PAGE: u16 = 0x0c;
const COLLECTION: u16 = 0x01;

struct Collection {
    source: HardwareSource,
    preparsed: Vec<usize>,
    last: BTreeMap<u8, BTreeSet<u16>>,
}

#[derive(Default)]
pub struct ConsumerInput {
    registration: Option<(u64, String)>,
    devices: BTreeMap<usize, Collection>,
    sequence: u64,
}

impl ConsumerInput {
    pub fn register(&mut self, hwnd: HWND, generation: u64, address: &str) {
        if !register(hwnd, RIDEV_INPUTSINK | RIDEV_DEVNOTIFY) {
            media_diagnostics::record(format_args!("raw input registration failed"));
            return;
        }
        self.registration = Some((generation, address.to_ascii_uppercase()));
        self.enumerate();
    }

    pub fn unregister(&mut self) {
        if self.registration.take().is_some() {
            register(null_mut(), RIDEV_REMOVE);
        }
        self.devices.clear();
    }

    pub fn registered(&self) -> bool {
        self.registration.is_some()
    }

    pub fn generation(&mut self, generation: u64) {
        if let Some((current, _)) = &mut self.registration {
            *current = generation;
        }
    }

    pub fn enumerate(&mut self) {
        let Some((_, address)) = &self.registration else {
            return;
        };
        let mut present = BTreeSet::new();
        for item in raw_devices() {
            if item.dwType != RIM_TYPEHID {
                continue;
            }
            let handle = item.hDevice as usize;
            present.insert(handle);
            if let std::collections::btree_map::Entry::Vacant(entry) = self.devices.entry(handle)
                && let Some(collection) = collection(item.hDevice, address)
            {
                entry.insert(collection);
            }
        }
        self.devices.retain(|handle, _| present.contains(handle));
        media_diagnostics::record(format_args!(
            "raw input enumerated: paired={} external_usb={}",
            self.devices
                .values()
                .filter(|item| item.source == HardwareSource::PairedHeadset)
                .count(),
            self.devices
                .values()
                .filter(|item| item.source == HardwareSource::RemovableUsb)
                .count(),
        ));
    }

    pub fn report(&mut self, handle: HRAWINPUT, at: Instant) -> Option<(u64, HardwareCommand)> {
        let (generation, address) = self.registration.as_ref()?;
        let mut source = INPUT_MESSAGE_SOURCE::default();
        if unsafe { GetCurrentInputMessageSource(&mut source) } != 0
            && matches!(source.originId, IMO_INJECTED | IMO_SYSTEM)
        {
            return None;
        }
        let header_size = size_of::<RAWINPUTHEADER>() as u32;
        let mut header = RAWINPUTHEADER::default();
        let mut length = header_size;
        if unsafe {
            GetRawInputData(
                handle,
                RID_HEADER,
                (&mut header as *mut RAWINPUTHEADER).cast(),
                &mut length,
                header_size,
            )
        } != header_size
            || header.dwType != RIM_TYPEHID
        {
            return None;
        }
        if header.hDevice.is_null() {
            return None;
        }
        // Some devnode arrivals race the first WM_INPUT. Resolve the sender
        // directly when possible, still requiring the same PnP proof.
        let device_key = header.hDevice as usize;
        let device = match self.devices.entry(device_key) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(collection(header.hDevice, address)?)
            }
        };
        let mut length = header.dwSize;
        if length < header_size + 9 || length > 16384 {
            return None;
        }
        // Raw Input requires DWORD alignment. The report starts eight bytes
        // into RAWHID, after its report size and count.
        let mut storage = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
        let read = unsafe {
            GetRawInputData(
                handle,
                RID_INPUT,
                storage.as_mut_ptr().cast(),
                &mut length,
                header_size,
            )
        };
        if read != length || read < header_size + 9 || read > header.dwSize {
            return None;
        }
        let bytes = unsafe {
            std::slice::from_raw_parts_mut(storage.as_mut_ptr().cast::<u8>(), read as usize)
        };
        let offset = offset_of!(RAWINPUT, data);
        let size = u32::from_ne_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?) as usize;
        let count =
            u32::from_ne_bytes(bytes.get(offset + 4..offset + 8)?.try_into().ok()?) as usize;
        if size == 0 || count == 0 {
            return None;
        }
        let end = (offset + 8).checked_add(size.checked_mul(count)?)?;
        let reports = bytes.get_mut(offset + 8..end)?;
        let mut command = None;
        let mut ambiguous = false;
        for report in reports.chunks_exact_mut(size) {
            let mut list = [0u16; 64];
            let mut count = list.len() as u32;
            let status = unsafe {
                HidP_GetUsages(
                    HidP_Input,
                    PAGE,
                    0,
                    list.as_mut_ptr(),
                    &mut count,
                    device.preparsed.as_ptr() as isize,
                    report.as_mut_ptr(),
                    size as u32,
                )
            };
            if status != HIDP_STATUS_SUCCESS || count > list.len() as u32 {
                continue;
            }
            let next: BTreeSet<_> = list[..count as usize]
                .iter()
                .copied()
                .filter(|usage| Command::from_consumer_usage(*usage).is_some())
                .collect();
            let previous = device.last.entry(report[0]).or_default();
            if next.len() > 1 {
                ambiguous = true;
            }
            for usage in next.difference(previous) {
                if command.replace(*usage).is_some() {
                    ambiguous = true;
                }
            }
            *previous = next;
        }
        if ambiguous {
            return None;
        }
        let action = Command::from_consumer_usage(command?)?;
        self.sequence = self.sequence.wrapping_add(1);
        Some((
            *generation,
            HardwareCommand {
                action,
                source: device.source,
                sequence: self.sequence,
                at,
            },
        ))
    }
}

pub fn inventory(address: &str) -> (usize, usize, usize) {
    let (mut consumer, mut paired, mut usb) = (0, 0, 0);
    for device in raw_devices()
        .into_iter()
        .filter(|device| device.dwType == RIM_TYPEHID)
    {
        if consumer_info(device.hDevice) {
            consumer += 1;
        }
        match collection(device.hDevice, address).map(|item| item.source) {
            Some(HardwareSource::PairedHeadset) => paired += 1,
            Some(HardwareSource::RemovableUsb) => usb += 1,
            None => {}
        }
    }
    (consumer, paired, usb)
}

fn raw_devices() -> Vec<RAWINPUTDEVICELIST> {
    let unit = size_of::<RAWINPUTDEVICELIST>() as u32;
    let mut count = 0;
    if unsafe { GetRawInputDeviceList(null_mut(), &mut count, unit) } != 0
        || count == 0
        || count > 4096
    {
        return Vec::new();
    }
    let mut entries = vec![RAWINPUTDEVICELIST::default(); count as usize];
    let found = unsafe { GetRawInputDeviceList(entries.as_mut_ptr(), &mut count, unit) };
    if found > entries.len() as u32 {
        return Vec::new();
    }
    entries.truncate(found as usize);
    entries
}

fn register(hwnd_target: HWND, dw_flags: u32) -> bool {
    let device = RAWINPUTDEVICE {
        usUsagePage: PAGE,
        usUsage: COLLECTION,
        dwFlags: dw_flags,
        hwndTarget: hwnd_target,
    };
    unsafe { RegisterRawInputDevices(&device, 1, size_of::<RAWINPUTDEVICE>() as u32) != 0 }
}

fn consumer_info(handle: HANDLE) -> bool {
    if handle.is_null() {
        return false;
    }
    let mut info = RID_DEVICE_INFO {
        cbSize: size_of::<RID_DEVICE_INFO>() as u32,
        ..Default::default()
    };
    let mut size = info.cbSize;
    if unsafe {
        GetRawInputDeviceInfoW(
            handle,
            RIDI_DEVICEINFO,
            (&mut info as *mut RID_DEVICE_INFO).cast(),
            &mut size,
        )
    } == u32::MAX
        || info.dwType != RIM_TYPEHID
    {
        return false;
    }
    let hid = unsafe { info.Anonymous.hid };
    (hid.usUsagePage, hid.usUsage) == (PAGE, COLLECTION)
}

fn collection(handle: HANDLE, address: &str) -> Option<Collection> {
    if !consumer_info(handle) {
        return None;
    }
    let source = hardware_source(handle, address)?;
    let mut size = 0;
    if unsafe { GetRawInputDeviceInfoW(handle, RIDI_PREPARSEDDATA, null_mut(), &mut size) } != 0
        || size == 0
        || size > 65536
    {
        return None;
    }
    let mut preparsed = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetRawInputDeviceInfoW(
            handle,
            RIDI_PREPARSEDDATA,
            preparsed.as_mut_ptr().cast(),
            &mut size,
        )
    } == u32::MAX
    {
        return None;
    }
    Some(Collection {
        source,
        preparsed,
        last: BTreeMap::new(),
    })
}

fn device_node(handle: HANDLE) -> Option<u32> {
    let mut size = 0;
    if unsafe { GetRawInputDeviceInfoW(handle, RIDI_DEVICENAME, null_mut(), &mut size) } != 0
        || size == 0
        || size > 4096
    {
        return None;
    }
    let mut name = vec![0u32; (size as usize + 1).div_ceil(2)];
    if unsafe {
        GetRawInputDeviceInfoW(handle, RIDI_DEVICENAME, name.as_mut_ptr().cast(), &mut size)
    } == u32::MAX
    {
        return None;
    }
    struct DeviceSet(HDEVINFO);
    impl Drop for DeviceSet {
        fn drop(&mut self) {
            unsafe {
                SetupDiDestroyDeviceInfoList(self.0);
            }
        }
    }
    let set = unsafe { SetupDiCreateDeviceInfoList(null(), null_mut()) };
    if set == INVALID_HANDLE_VALUE as HDEVINFO {
        return None;
    }
    let set = DeviceSet(set);
    let mut interface = SP_DEVICE_INTERFACE_DATA {
        cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
        ..Default::default()
    };
    if unsafe { SetupDiOpenDeviceInterfaceW(set.0, name.as_ptr().cast(), 0, &mut interface) } == 0 {
        return None;
    }
    let mut device = SP_DEVINFO_DATA {
        cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
        ..Default::default()
    };
    let mut required = 0;
    // SetupAPI fills DevInst even when an interface path buffer isn't supplied.
    // The reported insufficient buffer concerns the path, which Raw Input
    // already supplied. A device property query on the interface wouldn't
    // reliably retrieve the owning devnode.
    unsafe {
        let result = SetupDiGetDeviceInterfaceDetailW(
            set.0,
            &interface,
            null_mut(),
            0,
            &mut required,
            &mut device,
        );
        (result == 0
            && GetLastError() == ERROR_INSUFFICIENT_BUFFER
            && required != 0
            && device.DevInst != 0)
            .then_some(device.DevInst)
    }
}

fn hardware_source(handle: HANDLE, address: &str) -> Option<HardwareSource> {
    let mut node = device_node(handle)?;
    let interface_node = node;
    let mut instance = [0u16; 512];
    let mut ancestry = Vec::new();
    for _ in 0..24 {
        if unsafe { CM_Get_Device_IDW(node, instance.as_mut_ptr(), instance.len() as u32, 0) }
            != CR_SUCCESS
        {
            return None;
        }
        let end = instance.iter().position(|&unit| unit == 0)?;
        let id = String::from_utf16_lossy(&instance[..end]).to_ascii_uppercase();
        let mut caps = 0u32;
        let mut size = size_of_val(&caps) as u32;
        let removable = unsafe {
            CM_Get_DevNode_Registry_PropertyW(
                node,
                CM_DRP_CAPABILITIES,
                null_mut(),
                (&mut caps as *mut u32).cast(),
                &mut size,
                0,
            )
        } == CR_SUCCESS
            && size as usize == size_of_val(&caps)
            && caps & CM_DEVCAP_REMOVABLE != 0;
        ancestry.push((id, removable));
        let mut parent = 0;
        if unsafe { CM_Get_Parent(&mut parent, node, 0) } != CR_SUCCESS {
            break;
        }
        node = parent;
    }
    let source = classify(
        ancestry
            .iter()
            .map(|(id, removable)| (id.as_str(), *removable)),
        address,
    );
    source.or_else(|| {
        let bluetooth = ancestry.iter().any(|(node, _)| {
            node.starts_with("BTHLEDEVICE\\")
                || node.starts_with("BTHENUM\\")
                || node.starts_with("BTHLE\\")
        });
        let belongs_to_other = ancestry.iter().any(|(node, _)| {
            pnp_windows::bluetooth_address(node).is_some_and(|found| found != address)
        });
        if !bluetooth || belongs_to_other {
            return None;
        }
        let headset = pnp_windows::headset_container(address)?;
        let input = pnp_windows::container(interface_node)?;
        pnp_windows::same_container(&headset, &input).then_some(HardwareSource::PairedHeadset)
    })
}

fn classify<'a>(
    ancestry: impl IntoIterator<Item = (&'a str, bool)>,
    address: &str,
) -> Option<HardwareSource> {
    let mut removable_usb = false;
    let mut root_hub = false;
    let mut bluetooth = false;
    for (node, removable) in ancestry {
        bluetooth |= ["BTHENUM\\", "BTHLE\\", "BTHLEDEVICE\\", "BTHHFENUM\\"]
            .iter()
            .any(|prefix| node.starts_with(prefix));
        if let Some(addr) = pnp_windows::bluetooth_address(node) {
            return (address.len() == 12 && addr == address)
                .then_some(HardwareSource::PairedHeadset);
        }
        removable_usb |= node.starts_with("USB\\VID_") && removable;
        root_hub |= node.starts_with("USB\\ROOT_HUB");
        if !bluetooth && removable_usb && root_hub && node.starts_with("PCI\\VEN_") {
            return Some(HardwareSource::RemovableUsb);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setupapi_resolves_a_present_raw_input_hid_without_registering_or_reading_input() {
        let devices: Vec<_> = raw_devices()
            .into_iter()
            .filter(|item| item.dwType == RIM_TYPEHID && !item.hDevice.is_null())
            .collect();
        if devices.is_empty() {
            eprintln!("No HID collections are visible to this terminal session.");
            return;
        }
        assert!(
            devices
                .iter()
                .any(|item| device_node(item.hDevice).is_some())
        );
        eprintln!("Resolved a Raw Input HID interface to its SetupAPI devnode.");
    }

    #[test]
    fn source_requires_the_paired_address_or_a_removable_usb_device_under_a_physical_controller() {
        let address = "001122334455";
        let classify_nodes = |nodes: &[(&str, bool)]| classify(nodes.iter().copied(), address);
        assert_eq!(
            classify_nodes(&[("HID\\FAKE", false), ("BTHLE\\DEV_001122334455\\1", true)]),
            Some(HardwareSource::PairedHeadset)
        );
        assert_eq!(
            classify_nodes(&[("BTHENUM\\DEV_001122334456\\1", true)]),
            None
        );
        assert_eq!(
            classify_nodes(&[("HID\\FAKE", true), ("ROOT\\VIRTUAL", true)]),
            None
        );
        assert_eq!(
            classify_nodes(&[
                ("USB\\VID_1234", true),
                ("USB\\ROOT_HUB30", false),
                ("PCI\\VEN_1234", false)
            ]),
            Some(HardwareSource::RemovableUsb)
        );
        assert_eq!(
            classify_nodes(&[
                ("USB\\VID_1234", false),
                ("USB\\ROOT_HUB30", false),
                ("PCI\\VEN_1234", false)
            ]),
            None
        );
        assert_eq!(
            classify_nodes(&[("USB\\VID_1234", true), ("ROOT\\VIRTUAL", true)]),
            None
        );
    }
}
