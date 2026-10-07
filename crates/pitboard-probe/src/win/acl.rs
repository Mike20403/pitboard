//! C1 and C2: who may reach the login folders, and what a protected DACL made from a token
//! gives. Every principal is named by its relation to the token (self, system,
//! administrators, other_account and so on), never by SID or name. Only security
//! descriptors are read, never a file's contents, and the login folders are read only in a
//! marked throwaway account.

use super::ffi::{self, Owned, Token};
use super::logon_now;
use crate::elevation::relation;
use crate::report::Report;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce,
    GetSecurityDescriptorControl, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SECURITY_ATTRIBUTES,
};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_ALWAYS, CreateFileW, DELETE, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ,
    FILE_GENERIC_WRITE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

const INHERIT_ONLY_ACE: u8 = 0x08;
const INHERITED_ACE: u8 = 0x10;
const SE_DACL_PROTECTED: u16 = 0x1000;
/// Rights that read or change a file's data, for "shared with others".
const DATA_RIGHTS: u32 = 0x0001 | 0x0002 | 0x0004 | 0x8000_0000 | 0x4000_0000 | 0x1000_0000;

/// A security descriptor read from a path, freed on drop.
struct Descriptor {
    sd: PSECURITY_DESCRIPTOR,
    owner: PSID,
    dacl: *mut ACL,
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        if !self.sd.is_null() {
            // SAFETY: `sd` is the LocalAlloc descriptor GetNamedSecurityInfoW allocated.
            unsafe {
                LocalFree(self.sd);
            }
        }
    }
}

fn read_descriptor(path: &Path) -> Result<Descriptor, u32> {
    let wide = ffi::wide(path);
    let mut d = Descriptor {
        sd: std::ptr::null_mut(),
        owner: std::ptr::null_mut(),
        dacl: std::ptr::null_mut(),
    };
    // SAFETY: `wide` is NUL-terminated; the out-params point into one descriptor freed when
    // `d` drops.
    let rc = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut d.owner,
            std::ptr::null_mut(),
            &mut d.dacl,
            std::ptr::null_mut(),
            &mut d.sd,
        )
    };
    if rc != 0 { Err(rc) } else { Ok(d) }
}

/// A path's owner as a SID string, for comparing only.
pub fn owner_sid(path: &Path) -> Result<String, u32> {
    let d = read_descriptor(path)?;
    ffi::sid_to_string(d.owner).ok_or_else(ffi::last_error)
}

/// A path's owner and every ACE of its DACL, each principal by relation.
pub fn describe_access(path: &Path, own: Option<&str>) -> Value {
    if !path.exists() {
        return json!({ "exists": false });
    }
    let d = match read_descriptor(path) {
        Ok(d) => d,
        Err(code) => return json!({ "exists": true, "error": code }),
    };
    let owner = ffi::sid_to_string(d.owner);
    let mut control = 0u16;
    let mut revision = 0u32;
    // SAFETY: `d.sd` is a valid descriptor.
    let protected = (unsafe { GetSecurityDescriptorControl(d.sd, &mut control, &mut revision) }
        != 0)
        .then_some(control & SE_DACL_PROTECTED != 0);
    let (aces, shared) = describe_dacl(d.dacl, own, owner.as_deref());
    json!({
        "exists": true,
        "owner": owner.as_deref().map(|o| relation(o, own)),
        "dacl_protected": protected,
        "aces": aces,
        "data_reachable_by_others": shared,
    })
}

/// The DACL's ACEs by relation, and whether any allow ACE that applies here gives data
/// rights to anyone but this token's user, the owner, SYSTEM or Administrators.
fn describe_dacl(dacl: *const ACL, own: Option<&str>, owner: Option<&str>) -> (Value, bool) {
    if dacl.is_null() {
        return (json!("null_dacl_grants_everyone_everything"), true);
    }
    // SAFETY: `dacl` is a valid ACL from GetNamedSecurityInfoW.
    let count = u32::from(unsafe { (*dacl).AceCount });
    let mut out = Vec::new();
    let mut shared = false;
    for i in 0..count {
        let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: `dacl` is valid and `i` < AceCount.
        if unsafe { GetAce(dacl, i, &mut ace) } == 0 || ace.is_null() {
            continue;
        }
        // SAFETY: every ACE begins with an ACE_HEADER.
        let header = unsafe { &*(ace as *const ACE_HEADER) };
        let kind = match header.AceType {
            0 => "allow",
            1 => "deny",
            _ => {
                out.push(json!({ "ace_type": header.AceType }));
                continue;
            }
        };
        // SAFETY: allow and deny ACEs share ACCESS_ALLOWED_ACE's layout; SidStart begins
        // the SID.
        let body = unsafe { &*(ace as *const ACCESS_ALLOWED_ACE) };
        let sid = ffi::sid_to_string(std::ptr::addr_of!(body.SidStart) as PSID);
        let rel = sid.as_deref().map_or("unreadable", |s| relation(s, own));
        let applies_here = header.AceFlags & INHERIT_ONLY_ACE == 0;
        if kind == "allow"
            && applies_here
            && body.Mask & DATA_RIGHTS != 0
            && !matches!(rel, "self" | "system" | "administrators")
            && sid.as_deref() != owner
        {
            shared = true;
        }
        out.push(json!({
            "kind": kind,
            "principal": rel,
            "mask": format!("0x{:08x}", body.Mask),
            "flags": format!("0x{:02x}", header.AceFlags),
            "inherited": header.AceFlags & INHERITED_ACE != 0,
            "inherit_only": !applies_here,
        }));
    }
    (Value::Array(out), shared)
}

pub fn acl(
    scratch: Option<&Path>,
    create: bool,
    keep: bool,
    open: Option<&Path>,
    remove: Option<&Path>,
) -> Report {
    let logon = logon_now();
    let own = Token::current().ok().and_then(|t| t.user_sid());
    let own = own.as_deref();
    let mut data = json!({});

    // C1: the login folders' access. The marker is required, since this reads the security
    // descriptors of folders a tool may have made.
    if !ffi::marker_present() {
        return Report::refused(
            "acl",
            logon,
            format!(
                "this account carries no {}; acl reads the login folders' access only in a \
                 throwaway account",
                crate::MARKER_FILE
            ),
        );
    }
    let mut folders: Vec<(&str, Option<PathBuf>)> = Vec::new();
    let profile = ffi::folder_profile();
    let lad = ffi::folder_local_app_data();
    folders.push(("profile", profile.clone()));
    folders.push(("dot_claude", profile.as_ref().map(|p| p.join(".claude"))));
    folders.push(("dot_codex", profile.as_ref().map(|p| p.join(".codex"))));
    folders.push(("local_app_data", lad.clone()));
    folders.push(("pitboard_home", lad.as_ref().map(|p| p.join("Pitboard"))));
    folders.push(("temp", Some(std::env::temp_dir())));
    folders.push(("scratch", scratch.map(Path::to_path_buf)));
    let mut read = serde_json::Map::new();
    for (label, path) in folders {
        if let Some(p) = path {
            read.insert(label.into(), describe_access(&p, own));
        }
    }
    data["folders"] = Value::Object(read);

    let needs_scratch = create || open.is_some() || remove.is_some();
    if !needs_scratch {
        return Report::ok("acl", logon, data);
    }
    let Some(scratch) = scratch else {
        return Report::refused("acl", logon, "--create, --open and --remove need --scratch");
    };
    if let Some(reason) = ffi::refuse_write(scratch) {
        return Report::refused("acl", logon, reason);
    }
    if create {
        data["created"] = match create_protected(scratch, own, keep) {
            Ok(v) => v,
            Err(e) => json!({ "error": e }),
        };
    }
    for (label, file) in [("opened", open), ("removed", remove)] {
        let Some(file) = file else { continue };
        let is_probe_file = file
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(crate::PROBE_PREFIX));
        if !is_probe_file || !ffi::lies_in(file, scratch) {
            return Report::refused(
                "acl",
                logon,
                "--open and --remove take a pitboard-probe-* file under --scratch",
            );
        }
        data[label] = if label == "opened" {
            try_opens(file)
        } else {
            match std::fs::remove_file(file) {
                Ok(()) => json!({ "removed": true }),
                Err(e) => json!({ "removed": false, "error": super::io_code(&e) }),
            }
        };
    }
    Report::ok("acl", logon, data)
}

/// C2: a file with a protected DACL granting this user, SYSTEM and Administrators, and no
/// owner set, so it gets the token's default owner. Its owner and access are read back.
fn create_protected(scratch: &Path, own: Option<&str>, keep: bool) -> Result<Value, String> {
    std::fs::create_dir_all(scratch).map_err(|e| e.to_string())?;
    let sid = own.ok_or("cannot read the token's user")?;
    let sddl = format!("D:P(A;;FA;;;{sid})(A;;FA;;;SY)(A;;FA;;;BA)");
    let sddl_w = ffi::wide(&sddl);
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: `sddl_w` is NUL-terminated; `sd` receives a LocalAlloc descriptor freed below.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl_w.as_ptr(),
            1,
            &mut sd,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 || sd.is_null() {
        return Err(format!("SDDL conversion failed: {}", ffi::last_error()));
    }
    let attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd,
        bInheritHandle: 0,
    };
    let file = scratch.join(format!("{}protected", crate::PROBE_PREFIX));
    let file_w = ffi::wide(&file);
    // SAFETY: `file_w` is NUL-terminated; `attrs` holds a valid descriptor.
    let handle = Owned::new(unsafe {
        CreateFileW(
            file_w.as_ptr(),
            FILE_GENERIC_WRITE,
            0,
            &attrs,
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    });
    let create_error = handle.is_none().then(ffi::last_error);
    drop(handle);
    // SAFETY: `sd` is the LocalAlloc descriptor from the conversion.
    unsafe {
        LocalFree(sd);
    }
    let read_back = describe_access(&file, own);
    if !keep {
        let _ = std::fs::remove_file(&file);
    }
    Ok(json!({
        "file": file.file_name().map(|n| n.to_string_lossy().into_owned()),
        "create_error": create_error,
        "read_back": read_back,
        "kept": keep,
    }))
}

/// Open `file` for reading, writing and deleting in turn, recording each error.
fn try_opens(file: &Path) -> Value {
    let w = ffi::wide(file);
    let mut out = serde_json::Map::new();
    for (label, access) in [
        ("read", FILE_GENERIC_READ),
        ("write", FILE_GENERIC_WRITE),
        ("delete", DELETE),
    ] {
        // SAFETY: `w` is NUL-terminated; the handle is closed when dropped.
        let h = Owned::new(unsafe {
            CreateFileW(
                w.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        });
        out.insert(
            label.into(),
            match h {
                Some(_) => json!({ "ok": true }),
                None => json!({ "ok": false, "error": ffi::last_error() }),
            },
        );
    }
    Value::Object(out)
}
