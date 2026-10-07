//! The runner facts: the safe blocks every Windows CI leg runs as the job's user, in one
//! report, and the symbolic-link check the workflow also runs as a fresh standard user. The
//! machine-wide policy states (Smart App Control, Safer, AppLocker, WDAC, Defender, Developer
//! Mode, the build's UBR) are read by the workflow beside this, not by the probe, which reads
//! no registry value it did not make.

use super::ffi;
use super::{console, credman, exelookup, files, homes, logon_now, tokens};
use crate::report::Report;
use serde_json::json;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn symlink(scratch: &Path) -> Report {
    let logon = logon_now();
    if let Some(reason) = ffi::refuse_write(scratch) {
        return Report::refused("symlink", logon, reason);
    }
    if let Err(e) = std::fs::create_dir_all(scratch) {
        return Report::refused(
            "symlink",
            logon,
            format!("cannot make the scratch folder: {e}"),
        );
    }
    let pid = std::process::id();
    let target = scratch.join(format!("{}symlink-target-{pid}.txt", crate::PROBE_PREFIX));
    let link = scratch.join(format!("{}symlink-{pid}.txt", crate::PROBE_PREFIX));
    let dir_target = scratch.join(format!("{}symlink-dir-target-{pid}", crate::PROBE_PREFIX));
    let dir_link = scratch.join(format!("{}symlink-dir-{pid}", crate::PROBE_PREFIX));
    let _ = std::fs::write(&target, b"t");
    let _ = std::fs::create_dir_all(&dir_target);
    let reading = ffi::Token::current()
        .ok()
        .map(|t| tokens::describe(&t)["reading"].clone());
    let file = std::os::windows::fs::symlink_file(&target, &link);
    let dir = std::os::windows::fs::symlink_dir(&dir_target, &dir_link);
    let _ = std::fs::remove_file(&link);
    let _ = std::fs::remove_dir(&dir_link);
    let _ = std::fs::remove_file(&target);
    let _ = std::fs::remove_dir(&dir_target);
    Report::ok(
        "symlink",
        logon,
        json!({
            "file_symlink": file.is_ok(),
            "file_symlink_error": file.err().map(|e| super::io_code(&e)),
            "dir_symlink": dir.is_ok(),
            "dir_symlink_error": dir.err().map(|e| super::io_code(&e)),
            "token_reading": reading,
        }),
    )
}

fn present(name: &str) -> bool {
    Command::new("where")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub fn runner_facts(scratch: &Path) -> Report {
    let logon = logon_now();
    if let Some(reason) = ffi::refuse_write(scratch) {
        return Report::refused("runner-facts", logon, reason);
    }
    if let Err(e) = std::fs::create_dir_all(scratch) {
        return Report::refused(
            "runner-facts",
            logon,
            format!("cannot make the scratch folder: {e}"),
        );
    }
    let temp_scratch = std::env::temp_dir().join(format!("{}volume", crate::PROBE_PREFIX));
    let volume_temp = files::volume(&temp_scratch).to_json();
    let _ = std::fs::remove_dir_all(&temp_scratch);
    let facts = json!({
        "logon": super::logon_block().to_json(),
        "tokens": tokens::tokens(Some(scratch)).to_json(),
        "safer": tokens::safer(true, Some(scratch)).to_json(),
        "homes": homes::homes(None, None).to_json(),
        "path_vars": homes::path_vars(scratch).to_json(),
        "volume_scratch": files::volume(scratch).to_json(),
        "volume_temp": volume_temp,
        "flush_dir": files::flush_dir(scratch).to_json(),
        "console": console::console(scratch, true).to_json(),
        "exe_lookup": exelookup::exe_lookup(scratch, "pitboard-probe-lookup").to_json(),
        "pe_imports": super::pe_imports(None, None).to_json(),
        "symlink": symlink(scratch).to_json(),
        "credman_names": credman::names(false, &[]).to_json(),
        "winget_present": present("winget"),
        "scoop_present": present("scoop"),
    });
    Report::ok("runner-facts", logon, facts)
}
