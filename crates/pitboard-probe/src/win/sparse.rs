//! K1: the sparse identity. Registering the self-signed sparse package is the owner's
//! (`sparse/register.ps1`, in the VM only); the probe never registers a package. What the
//! probe does:
//!
//! - `identity`: whether this process has package identity, and the package family;
//! - `write`: from wherever it runs, write a file under `%LOCALAPPDATA%\Pitboard\
//!   pitboard-probe-k1`, rename it, read it back, and write and read a value under
//!   `HKCU\Software\pitboard-probe-k1`, each tagged with `--tag`, and leave them for `read`;
//! - `children`: from an identity process, start the plain probe and a stand-in `claude.exe`
//!   (a copy of the probe), each doing `write` with a tag of its own, and report whether each
//!   carried the identity;
//! - `read`: from any process, list the tags it can see in the real folder and key, and the
//!   tags in the package's own redirected folder;
//! - `clean`: remove the folder, the key, and `%LOCALAPPDATA%\Pitboard` itself when the
//!   probe made it and it is empty.
//!
//! The write target is `%LOCALAPPDATA%\Pitboard` by design, since that is the folder K1
//! tests, so the scratch refusal cannot apply to it; the throwaway marker is required
//! instead, and only `pitboard-probe-*` names are made inside it.

use super::ffi;
use super::{logon_now, sibling_exe, take_child_report};
use crate::cli::SparseAction;
use crate::report::Report;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

use windows_sys::Win32::Storage::Packaging::Appx::{
    GetCurrentApplicationUserModelId, GetCurrentPackageFamilyName,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegEnumValueW, RegGetValueW, RegOpenKeyExW,
    RegSetValueExW,
};

const APPMODEL_ERROR_NO_PACKAGE: u32 = 15700;
const KEY: &str = r"Software\pitboard-probe-k1";
const FOLDER: &str = "pitboard-probe-k1";
const MADE_RECORD: &str = "pitboard-probe-k1-made-pitboard-folder";

fn identity() -> Value {
    let mut buf = vec![0u16; 256];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` holds `len` units.
    let rc = unsafe { GetCurrentPackageFamilyName(&mut len, buf.as_mut_ptr()) };
    let mut aumid_len = 0u32;
    // SAFETY: a sizing call with a null buffer.
    let aumid = unsafe { GetCurrentApplicationUserModelId(&mut aumid_len, std::ptr::null_mut()) };
    json!({
        "has_package_identity": rc != APPMODEL_ERROR_NO_PACKAGE,
        "package_family": (rc == 0).then(|| ffi::from_wide_buf(&buf)),
        "has_application_user_model_id": aumid != APPMODEL_ERROR_NO_PACKAGE,
        "program": std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())),
    })
}

fn valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 40
        && tag
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn home() -> Option<PathBuf> {
    ffi::folder_local_app_data().map(|p| p.join("Pitboard"))
}

pub fn sparse(
    action: SparseAction,
    tag: &str,
    scratch: Option<&Path>,
    out: Option<&Path>,
) -> Report {
    let logon = logon_now();
    if action != SparseAction::Identity && action != SparseAction::Read && !ffi::marker_present() {
        return Report::refused(
            "sparse",
            logon,
            format!(
                "this account carries no {}; K1 writes under %LOCALAPPDATA%\\Pitboard and HKCU \
                 only in a throwaway account in the VM",
                crate::MARKER_FILE
            ),
        );
    }
    if !valid_tag(tag) {
        return Report::refused("sparse", logon, "--tag takes up to 40 of a-z, 0-9 and -");
    }
    let Some(home) = home() else {
        return Report::refused("sparse", logon, "cannot find %LOCALAPPDATA%");
    };
    let data = match action {
        SparseAction::Identity => {
            json!({ "identity": identity(), "report_to": out.map(|_| "--out") })
        }
        SparseAction::Write => write(&home, tag),
        SparseAction::Children => match scratch {
            None => return Report::refused("sparse", logon, "children needs --scratch"),
            Some(s) => match ffi::refuse_write(s) {
                Some(reason) => return Report::refused("sparse", logon, reason),
                None => children(s),
            },
        },
        SparseAction::Read => read(&home),
        SparseAction::Clean => clean(&home),
    };
    Report::ok("sparse", logon, data)
}

fn write(home: &Path, tag: &str) -> Value {
    let existed = home.is_dir();
    let folder = home.join(FOLDER);
    let mut file = json!({});
    match std::fs::create_dir_all(&folder) {
        Err(e) => file["error"] = json!(e.to_string()),
        Ok(()) => {
            if !existed && let Some(lad) = home.parent() {
                let _ = std::fs::write(lad.join(MADE_RECORD), b"made by pitboard-probe");
            }
            let tmp = folder.join(format!("{tag}.tmp"));
            let fin = folder.join(format!("{tag}.txt"));
            let wrote = std::fs::write(&tmp, tag.as_bytes()).is_ok();
            let renamed = wrote && std::fs::rename(&tmp, &fin).is_ok();
            let read_back = std::fs::read_to_string(&fin).ok();
            file = json!({
                "pitboard_folder_existed": existed,
                "wrote": wrote,
                "renamed": renamed,
                "read_back_matches": read_back.as_deref() == Some(tag),
            });
        }
    }
    json!({ "identity": identity(), "file": file, "registry": write_value(tag) })
}

fn write_value(tag: &str) -> Value {
    let key_w = ffi::wide(KEY);
    let mut hkey: HKEY = std::ptr::null_mut();
    // SAFETY: `key_w` is NUL-terminated; `hkey` receives a key closed below.
    let rc = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key_w.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_READ | KEY_WRITE,
            std::ptr::null(),
            &mut hkey,
            std::ptr::null_mut(),
        )
    };
    if rc != 0 {
        return json!({ "create_error": rc });
    }
    let name = ffi::wide(tag);
    let value = ffi::wide("written");
    // SAFETY: `hkey` is open for writing; `value` is a NUL-terminated UTF-16 string, passed
    // with its byte length.
    let set = unsafe {
        RegSetValueExW(
            hkey,
            name.as_ptr(),
            0,
            REG_SZ,
            value.as_ptr().cast(),
            (value.len() * 2) as u32,
        )
    };
    let mut buf = vec![0u16; 64];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: `hkey` is open for reading; `buf` holds `size` bytes.
    let got = unsafe {
        RegGetValueW(
            hkey,
            std::ptr::null(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    // SAFETY: `hkey` was opened above.
    unsafe { RegCloseKey(hkey) };
    json!({
        "set_error": (set != 0).then_some(set),
        "read_back_matches": got == 0 && ffi::from_wide_buf(&buf) == "written",
    })
}

fn children(scratch: &Path) -> Value {
    let pid = std::process::id();
    let plain = sibling_exe("pitboard-probe");
    let mut out = serde_json::Map::new();
    let run = |exe: &Path, tag: &str| -> Value {
        let report = scratch.join(format!("{}k1-{tag}-{pid}.json", crate::PROBE_PREFIX));
        let status = std::process::Command::new(exe)
            .arg("--out")
            .arg(&report)
            .args(["sparse", "--action", "write", "--tag", tag])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        json!({
            "exit": status.ok().and_then(|s| s.code()),
            "report": take_child_report(&report).map(|r| r["data"].clone()),
        })
    };
    out.insert("child_plain_probe".into(), run(&plain, "child-probe"));

    let dir = scratch.join(format!("{}standin", crate::PROBE_PREFIX));
    let claude = dir.join("claude.exe");
    let standin = if claude.exists() {
        json!({ "skipped": "a claude.exe is already in the stand-in folder; the probe never overwrites one" })
    } else {
        let _ = std::fs::create_dir_all(&dir);
        match std::fs::copy(&plain, &claude) {
            Ok(_) => {
                let v = run(&claude, "child-claude-standin");
                let _ = std::fs::remove_file(&claude);
                let _ = std::fs::remove_dir(&dir);
                v
            }
            Err(e) => json!({ "copy_error": e.to_string() }),
        }
    };
    out.insert("child_claude_standin".into(), standin);
    json!({ "identity": identity(), "children": out })
}

fn tags_in(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter_map(|e| {
                    e.file_name()
                        .to_str()
                        .and_then(|n| n.strip_suffix(".txt"))
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn values_in_key() -> Value {
    let key_w = ffi::wide(KEY);
    let mut hkey: HKEY = std::ptr::null_mut();
    // SAFETY: `key_w` is NUL-terminated; `hkey` receives a key closed below.
    let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key_w.as_ptr(), 0, KEY_READ, &mut hkey) };
    if rc != 0 {
        return json!({ "key_present": false, "error": rc });
    }
    let mut names = Vec::new();
    for i in 0..256u32 {
        let mut name = vec![0u16; 128];
        let mut len = name.len() as u32;
        // SAFETY: `hkey` is open; `name` holds `len` units; the data is not wanted.
        let rc = unsafe {
            RegEnumValueW(
                hkey,
                i,
                name.as_mut_ptr(),
                &mut len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if rc != 0 {
            break;
        }
        names.push(ffi::from_wide_buf(&name));
    }
    // SAFETY: `hkey` was opened above.
    unsafe { RegCloseKey(hkey) };
    json!({ "key_present": true, "tags": names })
}

fn read(home: &Path) -> Value {
    let lad = home.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut redirected = serde_json::Map::new();
    if let Ok(rd) = std::fs::read_dir(lad.join("Packages")) {
        for e in rd.filter_map(Result::ok) {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("PitboardProbe_") {
                let dir = e.path().join(r"LocalCache\Local\Pitboard").join(FOLDER);
                redirected.insert(n, json!(tags_in(&dir)));
            }
        }
    }
    json!({
        "identity": identity(),
        "real_folder_tags": tags_in(&home.join(FOLDER)),
        "real_hkcu": values_in_key(),
        "package_redirected_folder_tags": redirected,
    })
}

fn clean(home: &Path) -> Value {
    let folder = home.join(FOLDER);
    let removed_folder = !folder.exists() || std::fs::remove_dir_all(&folder).is_ok();
    let key_w = ffi::wide(KEY);
    // SAFETY: `key_w` is NUL-terminated; only the probe's own key and what is under it go.
    let key_rc = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key_w.as_ptr()) };
    let record = home.parent().map(|l| l.join(MADE_RECORD));
    let pitboard_removed = match &record {
        Some(r) if r.exists() => {
            let gone = std::fs::remove_dir(home).is_ok();
            if gone {
                let _ = std::fs::remove_file(r);
            }
            Some(gone)
        }
        _ => None,
    };
    json!({
        "k1_folder_removed": removed_folder,
        "hkcu_key_removed_or_absent": key_rc == 0 || key_rc == 2,
        "pitboard_folder_removed": pitboard_removed,
    })
}
