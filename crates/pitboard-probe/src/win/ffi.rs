//! Small wrappers over the Windows calls the blocks share: wide strings, the last error,
//! owned handles, the known folders, tokens and SIDs, the redactor, and the write guard in
//! its Windows form.

use crate::elevation::{self, ElevationType, TokenFacts};
use crate::guard::{self, RealProfile};
use crate::redact::Redactor;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, PSID, TOKEN_DUPLICATE,
    TOKEN_ELEVATION, TOKEN_IMPERSONATE, TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL,
    TOKEN_OWNER, TOKEN_QUERY, TOKEN_USER, TokenElevation, TokenElevationType, TokenIntegrityLevel,
    TokenOwner, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
    GetLongPathNameW, GetShortPathNameW, OPEN_EXISTING,
};
use windows_sys::Win32::System::SystemInformation::{
    ComputerNameDnsFullyQualified, ComputerNameDnsHostname, ComputerNameNetBIOS, GetComputerNameExW,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::System::WindowsProgramming::GetUserNameW;
use windows_sys::Win32::UI::Shell::{
    FOLDERID_LocalAppData, FOLDERID_Profile, FOLDERID_ProgramData, GetUserProfileDirectoryW,
    KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath,
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

/// A known folder as a path, asked the way a tool asks for it: no token, so `%USERPROFILE%`
/// in its registry value expands from this process's environment, and verified to exist.
/// What the homes block measures; the guard asks with [`configured_folder`].
fn known_folder(id: &GUID) -> Option<PathBuf> {
    let mut out: PWSTR = std::ptr::null_mut();
    // SAFETY: `id` is a valid GUID; `out` receives a CoTaskMem string freed in
    // `take_folder`.
    let hr = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut out) };
    take_folder(hr, out).ok()
}

/// A known folder's configured path, for the guard: not verified to exist, since a folder
/// this token cannot open, or one not made yet, is still a place to refuse; and asked with
/// `token` when one is given, so `%USERPROFILE%` in its registry value expands from the
/// account's own profile rather than from an environment this process was handed. The
/// error is the HRESULT.
fn configured_folder(id: &GUID, token: HANDLE) -> Result<PathBuf, i32> {
    let mut out: PWSTR = std::ptr::null_mut();
    // SAFETY: `id` is a valid GUID; `token` is null or a token opened for query and
    // impersonation, as the call asks; `out` receives a CoTaskMem string freed in
    // `take_folder`.
    let hr = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DONT_VERIFY as u32, token, &mut out) };
    take_folder(hr, out)
}

/// The path SHGetKnownFolderPath handed back, its buffer freed whether the call succeeded or
/// not, as the call's documentation asks.
fn take_folder(hr: i32, out: PWSTR) -> Result<PathBuf, i32> {
    // SAFETY: `out` is null or a NUL-terminated string the shell allocated.
    let s = (hr >= 0 && !out.is_null()).then(|| unsafe { from_wide_ptr(out) });
    // SAFETY: `out` is null or was allocated by SHGetKnownFolderPath; CoTaskMemFree takes
    // either.
    unsafe { windows_sys::Win32::System::Com::CoTaskMemFree(out as *const core::ffi::c_void) };
    s.map(PathBuf::from).ok_or(hr)
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

/// `GetUserProfileDirectoryW` of `token`: the profile folder Windows keeps for its user.
pub fn user_profile_directory(token: &Token) -> Result<PathBuf, u32> {
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `token` is open for query; `buf` holds `len` units.
    let ok = unsafe { GetUserProfileDirectoryW(token.raw(), buf.as_mut_ptr(), &mut len) };
    if ok == 0 {
        Err(last_error())
    } else {
        Ok(PathBuf::from(from_wide_buf(&buf)))
    }
}

/// This process's token, opened as SHGetKnownFolderPath asks a token to be.
fn folder_token() -> Result<Token, u32> {
    let mut handle: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess is a pseudo-handle; OpenProcessToken fills `handle`.
    let ok = unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_IMPERSONATE | TOKEN_DUPLICATE,
            &mut handle,
        )
    };
    if ok == 0 {
        Err(last_error())
    } else {
        Ok(Token::owned(handle))
    }
}

/// The profile and local app data folders Windows names for this account, by every route
/// the guard asks: `FOLDERID_Profile` and `FOLDERID_LocalAppData` with the token and without
/// one, and `GetUserProfileDirectoryW`. Each failure is kept, by route and code, for the
/// refusal; no path is.
struct WindowsFolders {
    profiles: Vec<PathBuf>,
    local_app_data: Vec<PathBuf>,
    failures: Vec<String>,
}

fn windows_folders() -> WindowsFolders {
    let mut f = WindowsFolders {
        profiles: Vec::new(),
        local_app_data: Vec::new(),
        failures: Vec::new(),
    };
    let hex = |hr: i32| format!("0x{:08x}", hr as u32);
    match folder_token() {
        Ok(token) => {
            match configured_folder(&FOLDERID_Profile, token.raw()) {
                Ok(p) => f.profiles.push(p),
                Err(hr) => f
                    .failures
                    .push(format!("FOLDERID_Profile with the token {}", hex(hr))),
            }
            match user_profile_directory(&token) {
                Ok(p) => f.profiles.push(p),
                Err(code) => f.failures.push(format!("GetUserProfileDirectoryW {code}")),
            }
            match configured_folder(&FOLDERID_LocalAppData, token.raw()) {
                Ok(p) => f.local_app_data.push(p),
                Err(hr) => f
                    .failures
                    .push(format!("FOLDERID_LocalAppData with the token {}", hex(hr))),
            }
        }
        Err(code) => f.failures.push(format!("the process token {code}")),
    }
    match configured_folder(&FOLDERID_Profile, std::ptr::null_mut()) {
        Ok(p) => f.profiles.push(p),
        Err(hr) => f.failures.push(format!("FOLDERID_Profile {}", hex(hr))),
    }
    match configured_folder(&FOLDERID_LocalAppData, std::ptr::null_mut()) {
        Ok(p) => f.local_app_data.push(p),
        Err(hr) => f
            .failures
            .push(format!("FOLDERID_LocalAppData {}", hex(hr))),
    }
    f
}

/// Every place this account's real logins may be, for the guard: what Windows names (see
/// [`windows_folders`]) and, when they are absolute, `%USERPROFILE%` and `%LOCALAPPDATA%`,
/// which a tool started from here follows. Refuses only when Windows names no profile, or no
/// local app data, by any route: then the guard cannot know where the logins are.
pub fn real_profile() -> Result<RealProfile, String> {
    let WindowsFolders {
        mut profiles,
        mut local_app_data,
        failures,
    } = windows_folders();
    if profiles.is_empty() || local_app_data.is_empty() {
        return Err(format!(
            "cannot read the real profile to check the scratch path ({}); refusing",
            failures.join(", ")
        ));
    }
    for (var, into) in [
        ("USERPROFILE", &mut profiles),
        ("LOCALAPPDATA", &mut local_app_data),
    ] {
        if let Some(v) = std::env::var_os(var) {
            let p = PathBuf::from(v);
            if guard::is_absolute_without_parent(&p) {
                into.push(p);
            }
        }
    }
    Ok(RealProfile {
        profiles,
        local_app_data,
    })
}

/// Whether the account carries the throwaway marker: in every profile folder Windows names
/// for it, and in at least one.
pub fn marker_present() -> bool {
    let profiles = windows_folders().profiles;
    !profiles.is_empty()
        && profiles
            .iter()
            .all(|p| p.join(crate::MARKER_FILE).is_file())
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

/// This machine's own names (NetBIOS, DNS host and fully qualified), by which a UNC path
/// may reach its own disks.
pub fn this_machine_names() -> Vec<String> {
    [
        ComputerNameNetBIOS,
        ComputerNameDnsHostname,
        ComputerNameDnsFullyQualified,
    ]
    .into_iter()
    .filter_map(|format| {
        let mut buf = vec![0u16; 512];
        let mut len = buf.len() as u32;
        // SAFETY: `buf` holds `len` units.
        let ok = unsafe { GetComputerNameExW(format, buf.as_mut_ptr(), &mut len) };
        (ok != 0).then(|| from_wide_buf(&buf))
    })
    .filter(|n| !n.is_empty())
    .collect()
}

/// Whether `path` is safe to use as a scratch folder, or as a file the probe writes, in every
/// form Windows could open it by. As given it must be absolute with no `..`: nothing is
/// resolved against the working folder. Then it is compared as given, made absolute, and
/// with its longest existing part resolved through 8.3 names, junctions and links, with the
/// real login folders and files as given and as resolved ([`guard::scratch_is_safe`]); a
/// UNC path to this machine is refused as written; and every existing ancestor is compared
/// by identity with those folders and files and their parents.
pub fn scratch_check(path: &Path) -> Result<(), String> {
    if !guard::is_absolute_without_parent(path) {
        return Err(
            "the scratch path must be absolute, with no '..' and no ':' past its drive; \
             refusing"
                .into(),
        );
    }
    let profile = real_profile()?;
    let absolute = std::path::absolute(path)
        .map_err(|e| format!("cannot make the scratch path absolute ({e}); refusing"))?;
    let machine = this_machine_names();
    if [path, absolute.as_path()]
        .iter()
        .any(|form| guard::is_unc_to_this_machine(form, &machine))
    {
        return Err(
            "the scratch path is a UNC path to this machine; map a drive letter to the share \
             and give that, which the identity check judges; refusing"
                .into(),
        );
    }
    let resolved = resolve_existing(&absolute);
    let forbidden = profile.forbidden_roots();
    let mut roots = forbidden.clone();
    for root in &forbidden {
        if let Ok(c) = std::fs::canonicalize(root) {
            roots.push(c);
        }
    }
    for form in [path, absolute.as_path(), resolved.as_path()] {
        if !guard::scratch_is_safe(form, &roots) {
            return Err(
                "the scratch path names an administrative or hidden share, or lies (in some \
                 spelling) in a real login folder or file (.claude, .claude.json, .codex or \
                 %LOCALAPPDATA%\\Pitboard); refusing"
                    .into(),
            );
        }
    }
    if reaches_a_root_by_identity(&absolute, &forbidden) {
        return Err(
            "the scratch path reaches a real login folder or file by identity, through a \
             mapped drive, a share, a link or a junction; refusing"
                .into(),
        );
    }
    Ok(())
}

/// An existing file's or folder's identity: its volume's serial number and its file index,
/// read through a handle that follows links, so one object has one identity whatever path
/// reaches it.
fn file_identity(path: &Path) -> Option<(u32, u64)> {
    let w = wide(path);
    // SAFETY: `w` is NUL-terminated; backup semantics opens a folder too; the handle is
    // owned below.
    let handle = Owned::new(unsafe {
        CreateFileW(
            w.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    })?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: an open handle; `info` is a valid out-param.
    let ok = unsafe { GetFileInformationByHandle(handle.raw(), &mut info) };
    (ok != 0).then(|| {
        (
            info.dwVolumeSerialNumber,
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        )
    })
}

/// Whether an existing ancestor of `path` (itself included) is one of `roots`, or is a
/// root's parent with the root's name next in `path`, by identity
/// ([`guard::identity_refuses`]).
fn reaches_a_root_by_identity(path: &Path, roots: &[PathBuf]) -> bool {
    let root_ids: Vec<guard::RootIdentity<(u32, u64)>> = roots
        .iter()
        .filter_map(|r| {
            Some(guard::RootIdentity {
                root: file_identity(r),
                parent: r.parent().and_then(file_identity),
                name: r.file_name()?.to_string_lossy().into_owned(),
            })
        })
        .collect();
    let mut ancestors = Vec::new();
    let mut below: Vec<String> = Vec::new();
    let mut at = Some(path);
    while let Some(p) = at {
        if let Some(id) = file_identity(p) {
            ancestors.push(guard::Ancestor {
                id,
                below: below.clone(),
            });
        }
        let Some(name) = p.file_name() else { break };
        below.insert(0, name.to_string_lossy().into_owned());
        at = p.parent();
    }
    guard::identity_refuses(&ancestors, &root_ids)
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

/// A file named `name` inside `scratch`, for the probe to write: `name` carries the probe's
/// prefix, the account is marked, `scratch` passes the write guard, and so does the file
/// itself, so a link or a hard link there to a login file is refused too.
pub fn scratch_file(scratch: &Path, name: &str) -> Result<PathBuf, String> {
    if !guard::is_probe_file_name(name) {
        return Err(format!(
            "{name:?} is not a plain pitboard-probe-* file name; the probe writes only files \
             it names itself"
        ));
    }
    if let Some(reason) = refuse_write(scratch) {
        return Err(reason);
    }
    let path = scratch.join(name);
    scratch_check(&path)?;
    Ok(path)
}

/// A file named `name` inside `scratch`, for the probe to read only: as [`scratch_file`], but
/// without the marker, so a token that cannot see the profile's marker (Codex's sandbox)
/// can still read a file the probe wrote there.
pub fn scratch_file_to_read(scratch: &Path, name: &str) -> Result<PathBuf, String> {
    if !guard::is_probe_file_name(name) {
        return Err(format!(
            "{name:?} is not a plain pitboard-probe-* file name"
        ));
    }
    scratch_check(scratch)?;
    let path = scratch.join(name);
    scratch_check(&path)?;
    Ok(path)
}

/// Whether `file` lies inside `scratch`, deeper than `scratch` itself, both absolute as given
/// with no `..`.
pub fn lies_in(file: &Path, scratch: &Path) -> bool {
    guard::is_absolute_without_parent(file)
        && guard::is_absolute_without_parent(scratch)
        && guard::is_within(file, scratch)
        && !guard::is_within(scratch, file)
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

/// The redactor for this account: every profile folder Windows names for it, in its long,
/// short and canonical forms, and the account's name in each form it takes.
pub fn redactor() -> Redactor {
    let mut profiles = Vec::new();
    let mut names = Vec::new();
    for p in folder_profile()
        .into_iter()
        .chain(windows_folders().profiles)
    {
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
        // The profile without its drive, for the administrative share's spellings.
        let tail = profile.display().to_string()[3..].to_string();
        for bad in [
            profile.join(".claude"),
            profile.join(".claude").join("x"),
            profile.join(".claude.json"),
            profile.join(".codex").join("y"),
            lad.join("Pitboard").join("z"),
            PathBuf::from(format!(r"\\?\{}", profile.join(".claude").display())),
            PathBuf::from(format!("{}.", profile.join(".claude").display())),
            PathBuf::from(format!("{} ", profile.join(".codex").display())),
            PathBuf::from(format!(
                r"{}\.claude::$INDEX_ALLOCATION\x",
                profile.display()
            )),
            PathBuf::from(format!(r"\\localhost\C$\{tail}\.claude\x")),
            PathBuf::from(format!(r"\\127.0.0.1\c$\{tail}\.codex")),
            PathBuf::from(format!(r"\\?\UNC\localhost\C$\{tail}\.claude")),
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
    fn a_relative_path_or_a_parent_component_is_refused() {
        for bad in [
            PathBuf::from(".claude"),
            PathBuf::from(r"scratch\x"),
            PathBuf::from(r"\Users\x"),
            std::env::temp_dir().join("..").join("x"),
        ] {
            assert!(
                scratch_check(&bad).is_err(),
                "{} should be refused",
                bad.display()
            );
        }
    }

    /// An existing folder is the same folder by identity however the path reaches it, so a
    /// forbidden root is refused through any spelling. Made under the temp folder and
    /// removed again; the real profile is only read.
    #[test]
    fn the_identity_check_finds_a_root_and_its_missing_children() {
        let base =
            std::env::temp_dir().join(format!("pitboard-probe-guard-id-{}", std::process::id()));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let roots = [root.clone(), base.join("missing")];
        let spellings = [
            root.clone(),
            root.join("x"),
            PathBuf::from(format!("{}.", root.display())),
            std::fs::canonicalize(&root).unwrap().join("y"),
            base.join("MISSING").join("z"),
        ];
        let short = short_path(&root);
        for p in spellings
            .iter()
            .cloned()
            .chain(short.map(|s| PathBuf::from(s).join("w")))
        {
            assert!(
                reaches_a_root_by_identity(&p, &roots),
                "{} reaches a root",
                p.display()
            );
        }
        assert!(!reaches_a_root_by_identity(&base.join("other"), &roots));
        assert!(!reaches_a_root_by_identity(&base, &roots));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_file_the_probe_writes_carries_its_prefix() {
        let temp = std::env::temp_dir();
        assert!(scratch_file(&temp, ".claude.json").is_err());
        assert!(scratch_file_to_read(&temp, "auth.json").is_err());
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
