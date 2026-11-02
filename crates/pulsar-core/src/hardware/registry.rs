use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD,
    RRF_RT_REG_SZ, RegGetValueW,
};
use windows::core::HSTRING;

fn raw(
    hive: HKEY,
    path: &str,
    name: &str,
    flags: windows::Win32::System::Registry::REG_ROUTINE_FLAGS,
) -> Option<Vec<u8>> {
    let (path, name) = (HSTRING::from(path), HSTRING::from(name));
    let mut size = 0u32;
    unsafe { RegGetValueW(hive, &path, &name, flags, None, None, Some(&mut size)) }
        .ok()
        .ok()?;
    let mut data = vec![0u8; size as usize];
    unsafe {
        RegGetValueW(
            hive,
            &path,
            &name,
            flags,
            None,
            Some(data.as_mut_ptr().cast()),
            Some(&mut size),
        )
    }
    .ok()
    .ok()?;
    data.truncate(size as usize);
    Some(data)
}

pub fn string(path: &str, name: &str) -> Option<String> {
    string_in(HKEY_LOCAL_MACHINE, path, name)
}

pub fn user_string(path: &str, name: &str) -> Option<String> {
    string_in(HKEY_CURRENT_USER, path, name)
}

fn string_in(hive: HKEY, path: &str, name: &str) -> Option<String> {
    let data = raw(hive, path, name, RRF_RT_REG_SZ)?;
    let wide: Vec<u16> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&c| u16::from_le_bytes(c))
        .take_while(|&c| c != 0)
        .collect();
    let s = String::from_utf16_lossy(&wide).trim().to_string();
    (!s.is_empty()).then_some(s)
}

pub fn dword(path: &str, name: &str) -> Option<u32> {
    let data = raw(HKEY_LOCAL_MACHINE, path, name, RRF_RT_REG_DWORD)?;
    Some(u32::from_le_bytes(data.get(..4)?.try_into().ok()?))
}

pub fn binary(path: &str, name: &str) -> Option<Vec<u8>> {
    raw(HKEY_LOCAL_MACHINE, path, name, RRF_RT_REG_BINARY)
}
