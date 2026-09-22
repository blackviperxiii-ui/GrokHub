//! User-only DACL for Windows secret files. Unix uses mode 0600; this is the match.

#![cfg(windows)]

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, OwnedHandle, RawHandle};
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{
    LocalFree, BOOL, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
    SetNamedSecurityInfoW, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    GetSecurityDescriptorDacl, ACL, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL};

/// Protected DACL: Local System + owner, full access. No Everyone / Users inherit.
pub(crate) const USER_ONLY_SDDL: &str = "D:P(A;;FA;;;SY)(A;;FA;;;OW)";

/// Leftover from `File::create` / a loose folder ACL — Everyone can read.
#[cfg(test)]
pub(crate) const WORLD_READ_SDDL: &str = "D:P(A;;FR;;;WD)(A;;FA;;;OW)";

const SDDL_REVISION_1: u32 = 1;

struct LocalMem(*mut core::ffi::c_void);

impl Drop for LocalMem {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

unsafe fn wide_to_string(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *p.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
}

fn parse_sddl(sddl: &str) -> io::Result<LocalMem> {
    let w = wide(sddl);
    let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            w.as_ptr(),
            SDDL_REVISION_1,
            &mut sd,
            ptr::null_mut(),
        )
    };
    if ok == 0 || sd.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(LocalMem(sd))
}

fn dacl_ptr(sd: PSECURITY_DESCRIPTOR) -> io::Result<*mut ACL> {
    let mut present: BOOL = 0;
    let mut defaulted: BOOL = 0;
    let mut dacl: *mut ACL = ptr::null_mut();
    let ok = unsafe { GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    if present == 0 || dacl.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "security descriptor has a NULL DACL",
        ));
    }
    Ok(dacl)
}

pub(crate) fn create_user_only(path: &Path) -> io::Result<fs::File> {
    // `CreateFileW` CREATE_ALWAYS on an existing inode keeps that inode's DACL.
    // Drop any leftover temp first, then CREATE_NEW with the user-only SD.
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let sd = parse_sddl(USER_ONLY_SDDL)?;
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    let w = wide(path.as_os_str());
    let handle: HANDLE = unsafe {
        CreateFileW(
            w.as_ptr(),
            GENERIC_WRITE,
            0,
            &sa,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // Safety: CreateFileW returned an owned handle.
    let owned = unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) };
    Ok(fs::File::from(owned))
}

pub(crate) fn apply_sddl(path: &Path, sddl: &str) -> io::Result<()> {
    let sd = parse_sddl(sddl)?;
    let dacl = dacl_ptr(sd.0)?;
    let w = wide(path.as_os_str());
    let err = unsafe {
        SetNamedSecurityInfoW(
            w.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null_mut(),
        )
    };
    if err != 0 {
        return Err(io::Error::from_raw_os_error(err as i32));
    }
    Ok(())
}

pub(crate) fn restrict_user_only(path: &Path) {
    let _ = apply_sddl(path, USER_ONLY_SDDL);
}

pub(crate) fn file_sddl(path: &Path) -> io::Result<String> {
    let w = wide(path.as_os_str());
    let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
    let mut dacl: *mut ACL = ptr::null_mut();
    let err = unsafe {
        GetNamedSecurityInfoW(
            w.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut sd,
        )
    };
    if err != 0 {
        return Err(io::Error::from_raw_os_error(err as i32));
    }
    let held = LocalMem(sd);
    dacl_ptr(held.0)?;
    let mut text: *mut u16 = ptr::null_mut();
    let ok = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            held.0,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            ptr::null_mut(),
        )
    };
    if ok == 0 || text.is_null() {
        return Err(io::Error::last_os_error());
    }
    let text_mem = LocalMem(text as *mut core::ffi::c_void);
    Ok(unsafe { wide_to_string(text_mem.0 as *const u16) })
}

pub(crate) fn is_user_only(path: &Path) -> bool {
    match file_sddl(path) {
        Ok(sddl) => !crate::config::sddl_allows_world(&sddl),
        Err(_) => false,
    }
}
