//! Small wrappers over the Windows calls the blocks share: wide strings, the last error,
//! owned handles, the known folders, tokens and SIDs, the redactor, and the write guard in
//! its Windows form.

use crate::elevation::{self, ElevationType, TokenFacts};
use crate::guard::{self, RealProfile};
use crate::redact::Redactor;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, PSID, TOKEN_ELEVATION,
    TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL, TOKEN_OWNER, TOKEN_QUERY, TOKEN_USER,
    TokenElevation, TokenElevationType, TokenIntegrityLevel, TokenOwner, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{GetLongPathNameW, GetShortPathNameW};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::System::WindowsProgramming::GetUserNameW;
use windows_sys::Win32::UI::Shell::{
    FOLDERID_LocalAppData, FOLDERID_Profile, FOLDERID_ProgramData, SHGetKnownFolderPath,
};
use windows_sys::core::{GUID, PWSTR};

/// A NUL-terminated UTF-16 copy of `s`.
pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

/// A string from a NUL-terminated UTF-16 pointer.
///
/// # Safety
/// `ptr` is null, or NUL-terminated and valid for reads up to and including the NUL.
pub unsafe fn from_wide_ptr(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    // SAFETY: the caller promises a NUL-terminated run; we stop at the NUL.
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `ptr` is valid for `len` reads, per the caller.
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    OsString::from_wide(slice).to_string_lossy().into_owned()
}

/// A string from a UTF-16 buffer, up to its first NUL.
pub fn from_wide_buf(buf: &[u16]) -> String {
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    OsString::from_wide(&buf[..len])
        .to_string_lossy()
        .into_owned()
}

/// The last Win32 error.
pub fn last_error() -> u32 {
    // SAFETY: GetLastError reads thread-local state and is always safe to call.
    unsafe { GetLastError() }
}

/// A handle closed on drop. Null and `INVALID_HANDLE_VALUE` are never held.
pub struct Owned(HANDLE);

impl Owned {
    /// Take ownership of `h`, or `None` (with the last error still set) when it is null or
    /// invalid.
    pub fn new(h: HANDLE) -> Option<Owned> {
        (!h.is_null() && h != INVALID_HANDLE_VALUE).then_some(Owned(h))
    }

    pub fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a handle this value owns.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// A known folder as a path.
fn known_folder(id: &GUID) -> Option<PathBuf> {
    let mut out: PWSTR = std::ptr::null_mut();
    // SAFETY: `id` is a valid GUID; `out` receives a CoTaskMem string freed below.
    let hr = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut out) };
    if hr < 0 || out.is_null() {
        return None;
    }
    // SAFETY: `out` is a NUL-terminated string the shell allocated.
    let s = unsafe { from_wide_ptr(out) };
    // SAFETY: `out` was allocated by SHGetKnownFolderPath, and is freed with CoTaskMemFree.
    unsafe { windows_sys::Win32::System::Com::CoTaskMemFree(out as *const core::ffi::c_void) };
    Some(PathBuf::from(s))
}

pub fn folder_profile() -> Option<PathBuf> {
    known_folder(&FOLDERID_Profile)
}

pub fn folder_local_app_data() -> Option<PathBuf> {
    known_folder(&FOLDERID_LocalAppData)
}

pub fn folder_program_data() -> Option<PathBuf> {
    known_folder(&FOLDERID_ProgramData)
}

pub fn real_profile() -> Option<RealProfile> {
    Some(RealProfile {
        profile: folder_profile()?,
        local_app_data: folder_local_app_data()?,
    })
}

/// Whether the account carries the throwaway marker in its real profile folder.
pub fn marker_present() -> bool {
    folder_profile()
        .map(|p| p.join(crate::MARKER_FILE).is_file())
        .unwrap_or(false)
}

/// The reason to refuse writing under `scratch`, or `None` to go ahead: the account must
/// carry the throwaway marker, and `scratch` must pass [`scratch_check`].
pub fn refuse_write(scratch: &Path) -> Option<String> {
    if !marker_present() {
        return Some(format!(
            "this account carries no {} in its profile folder; the probe writes only in a \
             throwaway account the owner marked",
            crate::MARKER_FILE
        ));
    }
    scratch_check(scratch).err()
}

/// Whether `path` is safe to use as a scratch folder, in every form Windows could open it
/// by: made absolute against the working folder, and with its longest existing part
/// resolved through 8.3 names, junctions and links. Each form is compared with the real
/// login folders, as given and as resolved.
pub fn scratch_check(path: &Path) -> Result<(), String> {
    let profile =
        real_profile().ok_or("cannot read the real profile to check the scratch path; refusing")?;
    let absolute = std::path::absolute(path)
        .map_err(|e| format!("cannot make the scratch path absolute ({e}); refusing"))?;
    let resolved = resolve_existing(&absolute);
    let mut roots = profile.forbidden_roots();
    for root in profile.forbidden_roots() {
        if let Ok(c) = std::fs::canonicalize(&root) {
            roots.push(c);
        }
    }
    for form in [&absolute, &resolved] {
        if !guard::scratch_is_safe(form, &roots) {
            return Err(
                "the scratch path is relative, keeps a '..', or lies (in some spelling) inside a \
                 real login folder (.claude, .codex or %LOCALAPPDATA%\\Pitboard); refusing"
                    .into(),
            );
        }
    }
    Ok(())
}

/// `path` with its longest existing ancestor canonicalized and the rest appended as given.
fn resolve_existing(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut rest: Vec<OsString> = Vec::new();
    loop {
        if let Ok(c) = std::fs::canonicalize(&existing) {
            let mut out = c;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (
            existing.file_name().map(OsStr::to_os_string),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// A file named `name` (a plain file name) inside `scratch`, once `scratch` passes the write
/// guard.
pub fn scratch_file(scratch: &Path, name: &str) -> Result<PathBuf, String> {
    if !guard::is_plain_file_name(name) {
        return Err(format!("{name:?} is not a plain file name"));
    }
    if let Some(reason) = refuse_write(scratch) {
        return Err(reason);
    }
    Ok(scratch.join(name))
}

/// Whether `file` lies directly or deeper inside `scratch`, with no `..`.
pub fn lies_in(file: &Path, scratch: &Path) -> bool {
    let (Ok(f), Ok(s)) = (std::path::absolute(file), std::path::absolute(scratch)) else {
        return false;
    };
    !f.components().any(|c| matches!(c, Component::ParentDir)) && f != s && guard::is_within(&f, &s)
}

/// The 8.3 short form of an existing path, when the volume keeps one.
pub fn short_path(path: &Path) -> Option<String> {
    let w = wide(path);
    let mut buf = vec![0u16; 1024];
    // SAFETY: `w` is NUL-terminated; `buf` holds 1024 units.
    let n = unsafe { GetShortPathNameW(w.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    (n > 0 && (n as usize) < buf.len()).then(|| from_wide_buf(&buf))
}

/// The long form of an existing path.
pub fn long_path(path: &Path) -> Option<String> {
    let w = wide(path);
    let mut buf = vec![0u16; 1024];
    // SAFETY: `w` is NUL-terminated; `buf` holds 1024 units.
    let n = unsafe { GetLongPathNameW(w.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    (n > 0 && (n as usize) < buf.len()).then(|| from_wide_buf(&buf))
}

/// The account name Windows reports, for the redactor alone; it is never printed.
pub fn account_name() -> Option<String> {
    let mut buf = vec![0u16; 512];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` holds `len` units.
    let ok = unsafe { GetUserNameW(buf.as_mut_ptr(), &mut len) };
    (ok != 0).then(|| from_wide_buf(&buf))
}

/// The redactor for this account: the profile folder in its long, short and canonical
/// forms, and the account's name in each form it takes.
pub fn redactor() -> Redactor {
    let mut profiles = Vec::new();
    let mut names = Vec::new();
    if let Some(p) = folder_profile() {
        profiles.push(p.display().to_string());
        if let Some(s) = short_path(&p) {
            names.extend(
                Path::new(&s)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned()),
            );
            profiles.push(s);
        }
        if let Ok(c) = std::fs::canonicalize(&p) {
            profiles.push(c.display().to_string());
        }
        names.extend(p.file_name().map(|n| n.to_string_lossy().into_owned()));
    }
    names.extend(account_name());
    if let Ok(n) = std::env::var("USERNAME") {
        names.push(n);
    }
    Redactor::new(&profiles, &names)
}

/// A token handle, closed on drop, and what can be read from it.
pub struct Token {
    handle: HANDLE,
}

impl Token {
    /// This process's token, for reading.
    pub fn current() -> Result<Token, u32> {
        let mut handle: HANDLE = std::ptr::null_mut();
        // SAFETY: GetCurrentProcess is a pseudo-handle; OpenProcessToken fills `handle`.
        let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) };
        if ok == 0 {
            return Err(last_error());
        }
        Ok(Token { handle })
    }

    /// A token this value will close.
    pub fn owned(handle: HANDLE) -> Token {
        Token { handle }
    }

    pub fn raw(&self) -> HANDLE {
        self.handle
    }

    /// Call `read` with a class's information, in a buffer sized by a first call and kept
    /// alive while `read` runs, since the structures in it point into it.
    fn with_info<R>(
        &self,
        class: TOKEN_INFORMATION_CLASS,
        read: impl FnOnce(*const u8) -> R,
    ) -> Option<R> {
        let mut len = 0u32;
        // SAFETY: a sizing call with a null buffer returns the needed length in `len`.
        unsafe {
            GetTokenInformation(self.handle, class, std::ptr::null_mut(), 0, &mut len);
        }
        if len == 0 {
            return None;
        }
        // u64s, so the buffer is aligned for the structures read out of it.
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        // SAFETY: `buf` holds at least `len` bytes, the size the first call asked for.
        let ok = unsafe {
            GetTokenInformation(self.handle, class, buf.as_mut_ptr().cast(), len, &mut len)
        };
        (ok != 0).then(|| read(buf.as_ptr().cast::<u8>()))
    }

    pub fn is_elevated(&self) -> Option<bool> {
        let mut info = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        // SAFETY: `info` is sized for TOKEN_ELEVATION.
        let ok = unsafe {
            GetTokenInformation(
                self.handle,
                TokenElevation,
                (&mut info as *mut TOKEN_ELEVATION).cast(),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            )
        };
        (ok != 0).then_some(info.TokenIsElevated != 0)
    }

    /// `TokenElevationType`'s raw value (1 default, 2 full, 3 limited).
    pub fn elevation_type(&self) -> Option<u32> {
        let mut ty = 0i32;
        let mut len = 0u32;
        // SAFETY: `ty` is a 4-byte int, the size this class returns.
        let ok = unsafe {
            GetTokenInformation(
                self.handle,
                TokenElevationType,
                (&mut ty as *mut i32).cast(),
                std::mem::size_of::<i32>() as u32,
                &mut len,
            )
        };
        (ok != 0).then_some(ty as u32)
    }

    /// The token's user, as a SID string, for comparing only.
    pub fn user_sid(&self) -> Option<String> {
        // SAFETY: the buffer begins with a TOKEN_USER whose SID points inside the buffer,
        // which lives while the closure runs.
        self.with_info(TokenUser, |p| unsafe {
            sid_to_string((*(p as *const TOKEN_USER)).User.Sid)
        })
        .flatten()
    }

    /// The owner new objects get by default, as a SID string, for comparing only.
    pub fn default_owner_sid(&self) -> Option<String> {
        // SAFETY: the buffer begins with a TOKEN_OWNER whose SID points inside the buffer.
        self.with_info(TokenOwner, |p| unsafe {
            sid_to_string((*(p as *const TOKEN_OWNER)).Owner)
        })
        .flatten()
    }

    /// The integrity level's RID: the last sub-authority of the label's SID.
    pub fn integrity_rid(&self) -> Option<u32> {
        self.with_info(TokenIntegrityLevel, |p| {
            // SAFETY: the buffer begins with a TOKEN_MANDATORY_LABEL whose SID lies inside the
            // buffer; the OS accessors read its sub-authorities.
            unsafe {
                let sid = (*(p as *const TOKEN_MANDATORY_LABEL)).Label.Sid;
                let count = *GetSidSubAuthorityCount(sid);
                (count > 0).then(|| *GetSidSubAuthority(sid, u32::from(count) - 1))
            }
        })
        .flatten()
    }

    /// Everything the elevation reading needs.
    pub fn facts(&self) -> TokenFacts {
        TokenFacts {
            elevation_type: self.elevation_type().map(ElevationType::from_raw),
            is_elevated: self.is_elevated(),
            user_is_service: self.user_sid().map(|s| elevation::is_service_sid(&s)),
            integrity_rid: self.integrity_rid(),
        }
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: an owned token handle, closed once.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

/// A SID pointer's string form. The pointer must be a valid SID or null.
#[allow(
    clippy::not_unsafe_ptr_arg_deref,
    reason = "the SID is only handed to ConvertSidToStringSidW, which validates it; callers \
              pass a SID the OS gave them"
)]
pub fn sid_to_string(sid: PSID) -> Option<String> {
    if sid.is_null() {
        return None;
    }
    let mut out: PWSTR = std::ptr::null_mut();
    // SAFETY: `sid` is a valid SID; `out` receives a LocalAlloc string.
    let ok = unsafe { ConvertSidToStringSidW(sid, &mut out) };
    if ok == 0 || out.is_null() {
        return None;
    }
    // SAFETY: `out` is a NUL-terminated LocalAlloc string.
    let s = unsafe { from_wide_ptr(out) };
    // SAFETY: `out` came from ConvertSidToStringSidW and is freed with LocalFree.
    unsafe {
        LocalFree(out as *mut core::ffi::c_void);
    }
    Some(s)
}

/// A FILETIME as 100-nanosecond ticks since 1601.
pub fn filetime_ticks(ft: windows_sys::Win32::Foundation::FILETIME) -> u64 {
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}

/// FILETIME ticks as milliseconds since the Unix epoch.
pub fn ticks_to_unix_ms(ticks: u64) -> i64 {
    (ticks as i64 - 116_444_736_000_000_000) / 10_000
}

/// A process started by [`spawn`], with its handle.
pub struct Child {
    pub process: Owned,
    pub pid: u32,
}

impl Child {
    /// Wait up to `ms` milliseconds; the exit code, or `None` if it is still running.
    pub fn wait_ms(&self, ms: u32) -> Option<u32> {
        use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
        // SAFETY: an open process handle.
        let w = unsafe { WaitForSingleObject(self.process.raw(), ms) };
        if w != 0 {
            return None;
        }
        let mut code = 0u32;
        // SAFETY: an open process handle; `code` is a valid out-param.
        let ok = unsafe { GetExitCodeProcess(self.process.raw(), &mut code) };
        (ok != 0).then_some(code)
    }
}

/// One argument quoted the way the C runtime and `CommandLineToArgvW` split a command line.
pub fn quote_arg(arg: &OsStr) -> Vec<u16> {
    let units: Vec<u16> = arg.encode_wide().collect();
    let needs = units.is_empty()
        || units
            .iter()
            .any(|&u| u == u16::from(b' ') || u == u16::from(b'\t') || u == u16::from(b'"'));
    if !needs {
        return units;
    }
    let mut out = vec![u16::from(b'"')];
    let mut backslashes = 0usize;
    for &u in &units {
        if u == u16::from(b'\\') {
            backslashes += 1;
        } else {
            if u == u16::from(b'"') {
                out.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes + 1));
            }
            backslashes = 0;
        }
        out.push(u);
    }
    out.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes));
    out.push(u16::from(b'"'));
    out
}

/// Start `exe` with `args` through CreateProcessW, or CreateProcessAsUserW with `token`,
/// with `flags` and, when given, a whole environment block (`CREATE_UNICODE_ENVIRONMENT` is
/// added). No standard handles are passed: a child that must answer writes `--out`.
pub fn spawn(
    token: Option<HANDLE>,
    exe: &Path,
    args: &[&OsStr],
    flags: u32,
    env: Option<&[u16]>,
) -> Result<Child, u32> {
    use windows_sys::Win32::System::Threading::{
        CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, CreateProcessW, PROCESS_INFORMATION,
        STARTUPINFOW,
    };
    let app = wide(exe);
    let mut line = quote_arg(exe.as_os_str());
    for a in args {
        line.push(u16::from(b' '));
        line.extend(quote_arg(a));
    }
    line.push(0);
    let si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();
    let flags = if env.is_some() {
        flags | CREATE_UNICODE_ENVIRONMENT
    } else {
        flags
    };
    let env_ptr = env.map_or(std::ptr::null(), |e| e.as_ptr().cast::<core::ffi::c_void>());
    // SAFETY: `app` and `line` are NUL-terminated and `line` is mutable as the call needs;
    // `si` is sized; `env`, when given, is a double-NUL-terminated block that outlives the
    // call; `pi` receives two handles, one closed here and one owned by the result.
    let ok = unsafe {
        match token {
            Some(t) => CreateProcessAsUserW(
                t,
                app.as_ptr(),
                line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                flags,
                env_ptr,
                std::ptr::null(),
                &si,
                &mut pi,
            ),
            None => CreateProcessW(
                app.as_ptr(),
                line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                flags,
                env_ptr,
                std::ptr::null(),
                &si,
                &mut pi,
            ),
        }
    };
    if ok == 0 {
        return Err(last_error());
    }
    drop(Owned::new(pi.hThread));
    Ok(Child {
        process: Owned::new(pi.hProcess).ok_or_else(last_error)?,
        pid: pi.dwProcessId,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The guard refuses the real login folders in the spellings Windows opens them by. It
    /// reads the known folders and writes nothing.
    #[test]
    fn the_scratch_check_sees_through_spellings_of_the_real_login_folders() {
        let profile = folder_profile().expect("the profile folder");
        let lad = folder_local_app_data().expect("the local app data folder");
        for bad in [
            profile.join(".claude"),
            profile.join(".claude").join("x"),
            profile.join(".codex").join("y"),
            lad.join("Pitboard").join("z"),
            PathBuf::from(format!(r"\\?\{}", profile.join(".claude").display())),
            PathBuf::from(format!("{}.", profile.join(".claude").display())),
            PathBuf::from(format!("{} ", profile.join(".codex").display())),
        ] {
            assert!(
                scratch_check(&bad).is_err(),
                "{} should be refused",
                bad.display()
            );
        }
        if let Some(short) = short_path(&profile) {
            let bad = Path::new(&short).join(".claude").join("x");
            assert!(
                scratch_check(&bad).is_err(),
                "{} should be refused",
                bad.display()
            );
        }
        let ok = std::env::temp_dir().join("pitboard-probe-guard-test");
        assert!(scratch_check(&ok).is_ok(), "{}", ok.display());
    }

    #[test]
    fn a_relative_path_is_judged_where_it_resolves() {
        let cwd = std::env::current_dir().unwrap();
        let profile = folder_profile().unwrap();
        // The test runs in the crate's folder, not the profile, so `.claude` here is not the
        // real one; from the profile folder it would be, and is refused there by resolving.
        assert_eq!(
            scratch_check(Path::new(".claude")).is_ok(),
            !guard::is_within(&cwd.join(".claude"), &profile.join(".claude"))
        );
    }

    #[test]
    fn arguments_are_quoted_as_the_c_runtime_splits_them() {
        let q = |s: &str| String::from_utf16(&quote_arg(OsStr::new(s))).unwrap();
        assert_eq!(q("plain"), "plain");
        assert_eq!(q("two words"), "\"two words\"");
        assert_eq!(q(""), "\"\"");
        assert_eq!(q(r#"a"b"#), r#""a\"b""#);
        assert_eq!(q(r"C:\dir with space\"), r#""C:\dir with space\\""#);
    }
}
