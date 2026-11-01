//! The crate's one foreign call.
#![allow(unsafe_code)]

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

/// The bytes the current user may still write on the volume holding `path`.
/// `path` must exist.
pub fn free_bytes(path: &Path) -> io::Result<u64> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0u64;
    // SAFETY: `wide` is NUL-terminated and outlives the call, `available` is a valid
    // out-pointer, and the other two out-parameters are documented as optional.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(available)
}
