//! Identity shared by audio endpoint selection and Consumer Control input.
use windows_sys::{
    Win32::Devices::{
        DeviceAndDriverInstallation::{
            CM_GETIDLIST_FILTER_ENUMERATOR, CM_GETIDLIST_FILTER_PRESENT, CM_Get_DevNode_PropertyW,
            CM_Get_Device_ID_List_SizeW, CM_Get_Device_ID_ListW, CM_Locate_DevNodeW, CR_SUCCESS,
        },
        Properties::{DEVPKEY_Device_ContainerId, DEVPROP_TYPE_GUID},
    },
    core::GUID,
};

pub fn bluetooth_address(instance: &str) -> Option<&str> {
    let (bus, path) = instance.split_once('\\')?;
    if !matches!(bus, "BTHENUM" | "BTHLE") {
        return None;
    }
    let addr = path.split('\\').next()?.strip_prefix("DEV_")?;
    (addr.len() == 12 && addr.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(addr)
}

pub fn container(node: u32) -> Option<GUID> {
    let mut guid = GUID::default();
    let mut size = size_of::<GUID>() as u32;
    let mut kind = 0;
    let result = unsafe {
        CM_Get_DevNode_PropertyW(
            node,
            &DEVPKEY_Device_ContainerId,
            &mut kind,
            (&mut guid as *mut GUID).cast(),
            &mut size,
            0,
        )
    };
    (result == CR_SUCCESS
        && kind == DEVPROP_TYPE_GUID
        && size == size_of::<GUID>() as u32
        && guid.data1 != 0
        && guid.data1 != u32::MAX)
        .then_some(guid)
}

pub fn headset_container(address: &str) -> Option<GUID> {
    for bus in ["BTHENUM", "BTHLE"] {
        let bus: Vec<_> = bus.encode_utf16().chain([0]).collect();
        let flags = CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT;
        let mut size = 0;
        if unsafe { CM_Get_Device_ID_List_SizeW(&mut size, bus.as_ptr(), flags) } != CR_SUCCESS
            || !(2..=32768).contains(&size)
        {
            continue;
        }
        let mut list = vec![0; size as usize];
        if unsafe { CM_Get_Device_ID_ListW(bus.as_ptr(), list.as_mut_ptr(), size, flags) }
            != CR_SUCCESS
        {
            continue;
        }
        for id in list.split(|&unit| unit == 0).filter(|id| !id.is_empty()) {
            let name = String::from_utf16_lossy(id).to_ascii_uppercase();
            if !bluetooth_address(&name).is_some_and(|found| found.eq_ignore_ascii_case(address)) {
                continue;
            }
            let mut path: Vec<_> = id.iter().copied().chain([0]).collect();
            let mut node = 0;
            if unsafe { CM_Locate_DevNodeW(&mut node, path.as_mut_ptr(), 0) } == CR_SUCCESS
                && let Some(guid) = container(node)
            {
                return Some(guid);
            }
        }
    }
    None
}

pub fn same_container(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}
