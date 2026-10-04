//! The person signed in, the POSIX way.

/// The name in the passwd database for this user id, as Node's `os.userInfo()` reads it.
pub(crate) fn login_name() -> Option<String> {
    let mut buffer = [0_i8; 1024];
    // SAFETY: an all-zero `passwd` is a valid one to hand to getpwuid_r, which fills it.
    let mut passwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: getpwuid_r writes into buffers this call owns and reports through `found`,
    // which is null when there is no entry. The name it points at lives in `buffer`, which
    // outlives the copy made below.
    let code = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &raw mut passwd,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &raw mut found,
        )
    };
    if code != 0 || found.is_null() || passwd.pw_name.is_null() {
        return None;
    }
    // SAFETY: pw_name points into `buffer` and is NUL-terminated, as getpwuid_r guarantees
    // when it reports an entry.
    let name = unsafe { std::ffi::CStr::from_ptr(passwd.pw_name) };
    name.to_str().ok().map(str::to_owned)
}
