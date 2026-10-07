//! G2-G4, H4 and M: Credential Manager. `credman-names` asks for each family's prefix alone
//! (never the whole vault), keeps only what [`crate::names`] calls reportable, and prints
//! names, or in a marked account names and attributes, never a blob. `credman-item` writes,
//! reads and deletes one `pitboard-probe-*` item of the probe's own, so block M can see
//! whether Credential Manager works in a logon at all. Every call is made in any logon, and
//! its error is recorded beside the logon.

use super::ffi;
use super::logon_now;
use crate::cli::{ItemAction, Persist};
use crate::hashes;
use crate::names::{self, ENUMERATION_FILTERS, Kind, UNLISTABLE_FORMS};
use crate::report::Report;
use serde_json::{Value, json};
use std::path::PathBuf;

use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_ENTERPRISE, CRED_PERSIST_LOCAL_MACHINE, CRED_PERSIST_SESSION, CRED_TYPE_GENERIC,
    CREDENTIALW, CredDeleteW, CredEnumerateW, CredFree, CredReadW, CredWriteW,
};

const ERROR_NOT_FOUND: u32 = 1168;

/// What may be printed of one item. The blob's size is read, the blob never is.
fn describe(cred: &CREDENTIALW, details: bool, r: &crate::redact::Redactor) -> Value {
    // SAFETY: TargetName is a NUL-terminated string in the item.
    let target = unsafe { ffi::from_wide_ptr(cred.TargetName) };
    let kind = names::classify(&target);
    let mut v = json!({
        "target": target,
        "kind": match kind {
            Kind::Live(_) => "live",
            Kind::CiTest => "citest",
            Kind::Probe => "probe",
            Kind::Other => "other",
        },
        "family": match kind { Kind::Live(f) => Some(f.as_str()), _ => None },
    });
    if details {
        // SAFETY: UserName and Comment are null or NUL-terminated strings in the item.
        let (user, comment) = unsafe {
            (
                ffi::from_wide_ptr(cred.UserName),
                ffi::from_wide_ptr(cred.Comment),
            )
        };
        let attributes: Vec<Value> = (0..cred.AttributeCount as usize)
            .map(|i| {
                // SAFETY: `Attributes` holds `AttributeCount` entries.
                let a = unsafe { &*cred.Attributes.add(i) };
                // SAFETY: Keyword is a NUL-terminated string in the item.
                let keyword = unsafe { ffi::from_wide_ptr(a.Keyword) };
                json!({ "keyword": keyword, "value_bytes": a.ValueSize, "flags": a.Flags })
            })
            .collect();
        v["type"] = json!(cred.Type);
        v["persist"] = json!(match cred.Persist {
            CRED_PERSIST_SESSION => "session",
            CRED_PERSIST_LOCAL_MACHINE => "local_machine",
            CRED_PERSIST_ENTERPRISE => "enterprise",
            _ => "other",
        });
        v["flags"] = json!(cred.Flags);
        // An address-shaped user name is a sign-in's, so only its shape is printed.
        v["user_name"] = json!((!cred.UserName.is_null()).then(|| {
            if user.contains('@') {
                "<address>".to_string()
            } else {
                r.redact(&user)
            }
        }));
        v["user_name_chars"] = json!(user.chars().count());
        v["comment"] = json!((!cred.Comment.is_null()).then(|| r.redact(&comment)));
        v["attributes"] = json!(attributes);
        v["blob_bytes"] = json!(cred.CredentialBlobSize);
        v["last_written_ms"] = json!(ffi::ticks_to_unix_ms(ffi::filetime_ticks(cred.LastWritten)));
    }
    v
}

pub fn names(leak_check: bool, config_dirs: &[PathBuf]) -> Report {
    let logon = logon_now();
    let details = ffi::marker_present();
    let r = ffi::redactor();
    let candidates: Vec<(String, Vec<hashes::Candidate>)> = config_dirs
        .iter()
        .map(|d| {
            let canonical = std::fs::canonicalize(d)
                .ok()
                .map(|c| c.display().to_string());
            (
                r.redact(&d.display().to_string()),
                hashes::candidates(&d.display().to_string(), canonical.as_deref()),
            )
        })
        .collect();

    let mut items = Vec::new();
    let mut leaks = Vec::new();
    let mut filter_errors = serde_json::Map::new();
    for filter in ENUMERATION_FILTERS {
        let w = ffi::wide(filter);
        let mut count = 0u32;
        let mut creds: *mut *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: a prefix filter; the out-params receive a count and a CredFree-owned array.
        let ok = unsafe { CredEnumerateW(w.as_ptr(), 0, &mut count, &mut creds) };
        if ok == 0 {
            let code = ffi::last_error();
            if code != ERROR_NOT_FOUND {
                filter_errors.insert(filter.into(), json!(code));
            }
            continue;
        }
        for i in 0..count as usize {
            // SAFETY: `creds` is an array of `count` pointers from CredEnumerateW.
            let cred = unsafe { &**creds.add(i) };
            // SAFETY: TargetName is a NUL-terminated string in the item.
            let target = unsafe { ffi::from_wide_ptr(cred.TargetName) };
            let kind = names::classify(&target);
            if !kind.is_reportable() {
                continue;
            }
            if kind.is_leak() {
                leaks.push(target.clone());
            }
            let mut v = describe(cred, details, &r);
            if !candidates.is_empty() {
                v["hash_matches"] = json!(candidates
                    .iter()
                    .map(|(dir, c)| json!({ "dir": dir, "labels": hashes::matching_labels(&target, c) }))
                    .collect::<Vec<_>>());
            }
            items.push(v);
        }
        // SAFETY: `creds` was allocated by CredEnumerateW.
        unsafe { CredFree(creds.cast()) };
    }

    if leak_check && (!leaks.is_empty() || !filter_errors.is_empty()) {
        return Report::refused(
            "credman-names",
            logon,
            format!(
                "leak check: {} live or test item(s) present, {} famil(ies) unreadable: {:?} {:?}",
                leaks.len(),
                filter_errors.len(),
                leaks,
                filter_errors
            ),
        );
    }
    Report::ok(
        "credman-names",
        logon,
        json!({
            "filters": ENUMERATION_FILTERS,
            "filter_errors": filter_errors,
            "details_shown": details,
            "items": items,
            "not_listable_by_prefix": UNLISTABLE_FORMS,
        }),
    )
}

/// The dummy blob of `n` bytes the probe writes: a pattern, never a secret.
fn dummy(n: u32) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

pub fn item(action: ItemAction, name: &str, persist: Persist, blob_bytes: u32) -> Report {
    let logon = logon_now();
    if !names::is_probe_item_name(name) {
        return Report::refused(
            "credman-item",
            logon,
            "--name must be a pitboard-probe-* target",
        );
    }
    if !ffi::marker_present() {
        return Report::refused(
            "credman-item",
            logon,
            format!("this account carries no {}; refusing", crate::MARKER_FILE),
        );
    }
    let target = ffi::wide(name);
    let data = match action {
        ItemAction::Write => {
            let mut blob = dummy(blob_bytes.min(2560));
            let mut user = ffi::wide("pitboard-probe-user");
            let mut comment = ffi::wide("pitboard-probe item");
            let mut target_mut = target.clone();
            let cred = CREDENTIALW {
                Type: CRED_TYPE_GENERIC,
                TargetName: target_mut.as_mut_ptr(),
                Comment: comment.as_mut_ptr(),
                CredentialBlobSize: blob.len() as u32,
                CredentialBlob: blob.as_mut_ptr(),
                Persist: match persist {
                    Persist::Session => CRED_PERSIST_SESSION,
                    Persist::LocalMachine => CRED_PERSIST_LOCAL_MACHINE,
                    Persist::Enterprise => CRED_PERSIST_ENTERPRISE,
                },
                UserName: user.as_mut_ptr(),
                ..Default::default()
            };
            // SAFETY: every pointer in `cred` points at a live buffer.
            let ok = unsafe { CredWriteW(&cred, 0) };
            json!({ "action": "write", "written": ok != 0, "error": (ok == 0).then(ffi::last_error), "blob_bytes": blob.len() })
        }
        ItemAction::Read => {
            let mut out: *mut CREDENTIALW = std::ptr::null_mut();
            // SAFETY: `target` is NUL-terminated; `out` receives a CredFree-owned item.
            let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut out) };
            if ok == 0 {
                json!({ "action": "read", "found": false, "error": ffi::last_error() })
            } else {
                // SAFETY: on success `out` is a valid item; its blob is compared, not kept.
                let v = unsafe {
                    let cred = &*out;
                    let blob = std::slice::from_raw_parts(
                        cred.CredentialBlob,
                        cred.CredentialBlobSize as usize,
                    );
                    let matches = blob == dummy(cred.CredentialBlobSize).as_slice();
                    let mut v = describe(cred, true, &ffi::redactor());
                    v["blob_is_the_dummy"] = json!(matches);
                    v
                };
                // SAFETY: `out` was allocated by CredReadW.
                unsafe { CredFree(out.cast()) };
                json!({ "action": "read", "found": true, "item": v })
            }
        }
        ItemAction::Delete => {
            // SAFETY: `target` is NUL-terminated.
            let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
            json!({ "action": "delete", "deleted": ok != 0, "error": (ok == 0).then(ffi::last_error) })
        }
    };
    Report::ok("credman-item", logon, data)
}
