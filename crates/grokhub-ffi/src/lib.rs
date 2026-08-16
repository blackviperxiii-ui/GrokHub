//! C ABI so Android / Windows call the same brain. No second protocol.

use grokhub_core::{
    dedicated_imagine_model, dedicated_voice_model, forbidden_reason, make_pair_code,
    normalize_code, parse_slash, DEFAULT_PORT, HUB_KIND,
};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};

fn cstr(s: impl Into<String>) -> *mut c_char {
    CString::new(s.into()).unwrap_or_default().into_raw()
}

fn read(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

#[no_mangle]
pub extern "C" fn grokhub_hub_kind() -> *mut c_char {
    cstr(HUB_KIND)
}

#[no_mangle]
pub extern "C" fn grokhub_default_port() -> u16 {
    DEFAULT_PORT
}

#[no_mangle]
pub extern "C" fn grokhub_make_pair_code() -> *mut c_char {
    cstr(make_pair_code())
}

#[no_mangle]
pub extern "C" fn grokhub_normalize_code(code: *const c_char) -> *mut c_char {
    match read(code) {
        Some(s) => cstr(normalize_code(&s)),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn grokhub_imagine_model(user: *const c_char) -> *mut c_char {
    cstr(dedicated_imagine_model(&read(user).unwrap_or_default()))
}

#[no_mangle]
pub extern "C" fn grokhub_voice_model(user: *const c_char) -> *mut c_char {
    cstr(dedicated_voice_model(&read(user).unwrap_or_default()))
}

#[no_mangle]
pub extern "C" fn grokhub_forbidden(cmd: *const c_char) -> c_int {
    match read(cmd) {
        Some(s) if forbidden_reason(&s).is_some() => 1,
        _ => 0,
    }
}

#[no_mangle]
pub extern "C" fn grokhub_slash_kind(line: *const c_char) -> *mut c_char {
    let Some(s) = read(line) else {
        return std::ptr::null_mut();
    };
    let kind = match parse_slash(&s) {
        Some(s) => grokhub_core::slash_kind(&s),
        None => "none",
    };
    cstr(kind)
}

/// # Safety
/// `s` must come from a grokhub_* function that returned a CString.
#[no_mangle]
pub unsafe extern "C" fn grokhub_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn pair_and_limits_c_abi() {
        unsafe {
            let raw = grokhub_make_pair_code();
            let code = CStr::from_ptr(raw).to_str().unwrap().to_string();
            grokhub_string_free(raw);
            assert_eq!(code.len(), 7);
            assert_eq!(grokhub_default_port(), 18766);
            let img = grokhub_imagine_model(std::ptr::null());
            assert_eq!(CStr::from_ptr(img).to_str().unwrap(), "grok-imagine-image-2.0");
            grokhub_string_free(img);
            let chat = CString::new("grok-3-mini-fast").unwrap();
            let img2 = grokhub_imagine_model(chat.as_ptr());
            assert_eq!(CStr::from_ptr(img2).to_str().unwrap(), "grok-imagine-image-2.0");
            grokhub_string_free(img2);
            let voice = grokhub_voice_model(std::ptr::null());
            assert_eq!(CStr::from_ptr(voice).to_str().unwrap(), "grok-voice-think-fast-2.0");
            grokhub_string_free(voice);
            let cmd = CString::new("cat /etc/shadow").unwrap();
            assert_eq!(grokhub_forbidden(cmd.as_ptr()), 1);
            let sl = CString::new("/imagine a cabin").unwrap();
            let k = grokhub_slash_kind(sl.as_ptr());
            assert_eq!(CStr::from_ptr(k).to_str().unwrap(), "imagine");
            grokhub_string_free(k);
            let sl = CString::new("/update").unwrap();
            let k = grokhub_slash_kind(sl.as_ptr());
            assert_eq!(CStr::from_ptr(k).to_str().unwrap(), "update");
            grokhub_string_free(k);
        }
    }
}
