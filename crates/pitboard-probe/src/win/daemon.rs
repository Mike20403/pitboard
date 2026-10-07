//! G9: Claude Code's supervisor daemon on Windows, in a scratch config folder of a marked
//! account. It reads `daemon.lock`'s field names and its pid, start and version fields, and
//! compares any process-start field with `GetProcessTimes` of that pid; lists the named
//! pipes whose names start `cc-daemon-` (no other application's pipe is named); and gives
//! `pipe.key`'s size and place, never its bytes. Every string read from the lock, the start
//! fields' values included, is redacted.

use super::ffi::{self, filetime_ticks, ticks_to_unix_ms};
use super::logon_now;
use super::processes::open_query;
use crate::images;
use crate::report::Report;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::Storage::FileSystem::{
    FindClose, FindFirstFileW, FindNextFileW, WIN32_FIND_DATAW,
};
use windows_sys::Win32::System::Threading::GetProcessTimes;

/// Fields read from `daemon.lock`, whose names are Claude Code's, as a list of name and value
/// rather than an object: two of its names could differ only in case, which a report's
/// object may not hold.
fn names_and_values(fields: serde_json::Map<String, Value>) -> Value {
    Value::Array(
        fields
            .into_iter()
            .map(|(name, value)| json!({ "name": name, "value": value }))
            .collect(),
    )
}

fn number(v: &Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn creation_ticks(pid: u32) -> Result<u64, u32> {
    let h = open_query(pid)?;
    let (mut c, mut e, mut k, mut u) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: an open process handle; four valid out-params.
    let ok = unsafe { GetProcessTimes(h.raw(), &mut c, &mut e, &mut k, &mut u) };
    if ok == 0 {
        Err(ffi::last_error())
    } else {
        Ok(filetime_ticks(c))
    }
}

fn daemon_pipes() -> Value {
    let pattern = ffi::wide(r"\\.\pipe\*");
    // SAFETY: WIN32_FIND_DATAW is plain data; all zeroes is valid.
    let mut data: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
    // SAFETY: `pattern` is NUL-terminated; `data` is a valid out-param.
    let h = unsafe { FindFirstFileW(pattern.as_ptr(), &mut data) };
    if h.is_null() || h == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        return json!({ "error": ffi::last_error() });
    }
    let mut names = Vec::new();
    let mut others = 0u32;
    loop {
        let name = ffi::from_wide_buf(&data.cFileName);
        if name.starts_with("cc-daemon-") {
            names.push(name);
        } else {
            others += 1;
        }
        // SAFETY: `h` is an open find handle; `data` is a valid out-param.
        if unsafe { FindNextFileW(h, &mut data) } == 0 {
            break;
        }
    }
    // SAFETY: `h` came from FindFirstFileW.
    unsafe { FindClose(h) };
    json!({ "cc_daemon_pipes": names, "other_pipes_counted_not_named": others })
}

fn find_named(dir: &Path, name: &str, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.filter_map(Result::ok) {
        let p = e.path();
        if e.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
            out.push(p.clone());
        }
        if depth > 0 && p.is_dir() {
            find_named(&p, name, depth - 1, out);
        }
    }
}

pub fn daemon(config_dir: &Path, pipe_key: Option<&Path>) -> Report {
    let logon = logon_now();
    if !ffi::marker_present() {
        return Report::refused(
            "daemon",
            logon,
            format!("this account carries no {}", crate::MARKER_FILE),
        );
    }
    if let Err(reason) = ffi::scratch_check(config_dir) {
        return Report::refused("daemon", logon, reason);
    }
    let r = ffi::redactor();
    let lock = match std::fs::read_to_string(config_dir.join("daemon.lock")) {
        Err(e) => json!({ "present": false, "error": e.to_string() }),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Err(_) => json!({ "present": true, "json": false, "bytes": text.len() }),
            Ok(v) => {
                let keys: Vec<String> = v
                    .as_object()
                    .map(|o| o.keys().cloned().collect())
                    .unwrap_or_default();
                let pid = v.get("pid").and_then(number).map(|p| p as u32);
                let start_fields: serde_json::Map<String, Value> = v
                    .as_object()
                    .map(|o| {
                        o.iter()
                            .filter(|(k, _)| k.to_lowercase().contains("start"))
                            .map(|(k, val)| (k.clone(), val.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                let process = pid.map(|pid| match creation_ticks(pid) {
                    Ok(ticks) => {
                        let comparisons: serde_json::Map<String, Value> = start_fields
                            .iter()
                            .filter_map(|(k, val)| number(val).map(|n| (k.clone(), n)))
                            .map(|(k, n)| {
                                (k, json!({
                                    "equals_creation_filetime": n == ticks,
                                    "equals_creation_unix_ms": n as i64 == ticks_to_unix_ms(ticks),
                                    "difference_from_filetime": n as i128 - ticks as i128,
                                }))
                            })
                            .collect();
                        let image = super::processes::snapshot().ok().and_then(|all| {
                            all.into_iter().find(|e| e.pid == pid).map(|e| e.image)
                        });
                        json!({
                            "running": true,
                            "image": image.as_deref().map(images::reportable),
                            "creation_filetime": ticks,
                            "creation_unix_ms": ticks_to_unix_ms(ticks),
                            "start_fields_against_creation": names_and_values(comparisons),
                        })
                    }
                    Err(code) => json!({ "running_or_readable": false, "open_error": code }),
                });
                let text_field = |k: &str| v.get(k).and_then(Value::as_str).map(|s| r.redact(s));
                json!({
                    "present": true,
                    "keys": keys,
                    "pid": pid,
                    "version": text_field("version"),
                    "origin": text_field("origin"),
                    "launch_target": text_field("launchTarget"),
                    "json_path": text_field("jsonPath"),
                    "start_fields": names_and_values(
                        start_fields
                            .iter()
                            .map(|(k, v)| (k.clone(), r.redact_json(v)))
                            .collect(),
                    ),
                    "process": process,
                })
            }
        },
    };
    let mut keys = Vec::new();
    match pipe_key {
        Some(p) if ffi::lies_in(p, config_dir) => keys.push(p.to_path_buf()),
        Some(_) => return Report::refused("daemon", logon, "--pipe-key must lie in --config-dir"),
        None => find_named(config_dir, "pipe.key", 3, &mut keys),
    }
    let keys: Vec<Value> = keys
        .iter()
        .map(|k| json!({ "path": r.redact(&k.display().to_string()), "bytes": std::fs::metadata(k).map(|m| m.len()).ok() }))
        .collect();
    Report::ok(
        "daemon",
        logon,
        json!({ "daemon_lock": lock, "pipes": daemon_pipes(), "pipe_keys": keys }),
    )
}
