//! F3 and the runner facts' exe-lookup: which stand-in `std::process::Command` starts for a
//! bare name. A copy of the probe, the "host", does each lookup from a folder of its own, so
//! "beside the program" means beside the host; each stand-in is another copy that answers
//! with the folder it sits in. Every file is a probe-named one in the probe's own folder
//! under the scratch folder, and the whole folder is removed at the end.

use super::{ffi, logon_now};
use crate::cli::Outcome;
use crate::exelookup::{CASES, CMD_ANSWER, Place, is_child_path_folder, is_probe_name};
use crate::report::Report;
use serde_json::json;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const HOST: &str = "pitboard-probe-lookup-host.exe";

pub fn exe_lookup(scratch: &Path, name: &str) -> Report {
    let logon = logon_now();
    if !is_probe_name(name) {
        return Report::refused(
            "exe-lookup",
            logon,
            "--name must be a pitboard-probe-* name of letters, digits and dashes",
        );
    }
    if let Some(reason) = ffi::refuse_write(scratch) {
        return Report::refused("exe-lookup", logon, reason);
    }
    let base = scratch.join(format!("{}lookup", crate::PROBE_PREFIX));
    if base.exists() {
        let _ = std::fs::remove_dir_all(&base);
    }
    let folder = |p: Place| base.join(p.folder());
    for p in Place::ALL {
        if let Err(e) = std::fs::create_dir_all(folder(p)) {
            return Report::refused("exe-lookup", logon, format!("cannot make the folders: {e}"));
        }
    }
    let probe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return Report::refused("exe-lookup", logon, format!("no current exe: {e}")),
    };
    let host = folder(Place::AppDir).join(HOST);
    if let Err(e) = std::fs::copy(&probe, &host) {
        return Report::refused("exe-lookup", logon, format!("cannot copy the host: {e}"));
    }
    let parent_path = {
        let mut p = OsString::from(folder(Place::ParentPath));
        p.push(";");
        if let Some(existing) = std::env::var_os("PATH") {
            p.push(existing);
        }
        p
    };

    let mut cases = serde_json::Map::new();
    for case in CASES {
        for p in Place::ALL {
            let _ = std::fs::remove_file(folder(p).join(format!("{name}.exe")));
            let _ = std::fs::remove_file(folder(p).join(format!("{name}.cmd")));
        }
        let mut placed = true;
        for p in case.exe_in {
            placed &= std::fs::copy(&probe, folder(*p).join(format!("{name}.exe"))).is_ok();
        }
        if case.cmd_on_parent_path {
            placed &= std::fs::write(
                folder(Place::ParentPath).join(format!("{name}.cmd")),
                format!("@echo {CMD_ANSWER}\r\n"),
            )
            .is_ok();
        }
        if !placed {
            cases.insert(case.label.into(), json!({ "placed": false }));
            continue;
        }
        let mut cmd = Command::new(&host);
        cmd.args(["exe-lookup-host", "--name", name])
            .env("PATH", &parent_path)
            .current_dir(folder(Place::WorkingDir))
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        if case.sets_child_path {
            cmd.arg("--child-path").arg(folder(Place::ChildPath));
        }
        let answer = match cmd.output() {
            Ok(o) => serde_json::from_slice::<serde_json::Value>(&o.stdout)
                .unwrap_or_else(|_| json!({ "unreadable_host_output": true })),
            Err(e) => json!({ "host_error": super::io_code(&e) }),
        };
        cases.insert(
            case.label.into(),
            json!({
                "exe_in": case.exe_in.iter().map(|p| p.folder()).collect::<Vec<_>>(),
                "cmd_on_parent_path": case.cmd_on_parent_path,
                "sets_child_path": case.sets_child_path,
                "result": answer,
            }),
        );
    }
    let _ = std::fs::remove_dir_all(&base);
    Report::ok(
        "exe-lookup",
        logon,
        json!({
            "name": name,
            "built_by": env!("PITBOARD_PROBE_RUSTC"),
            "cases": cases,
        }),
    )
}

/// The host: look `name` up as std's Command does, run it, and say which stand-in answered.
/// It takes only a probe name and the probe's own child-path folder, as `exe-lookup` does,
/// so it can never start a real program by a bare name.
pub fn host(name: &str, child_path: Option<&Path>) -> Outcome {
    if !is_probe_name(name) || child_path.is_some_and(|p| !is_child_path_folder(p)) {
        return Outcome::Line(
            json!({
                "ran": false,
                "refused": "--name must be a pitboard-probe-* name and --child-path the probe's child-path folder",
            })
            .to_string(),
        );
    }
    let mut cmd = Command::new(name);
    cmd.arg("exe-lookup-whoami")
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    if let Some(cp) = child_path {
        let mut p = OsString::from(cp);
        p.push(";");
        if let Some(existing) = std::env::var_os("PATH") {
            p.push(existing);
        }
        cmd.env("PATH", p);
    }
    let v = match cmd.output() {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let answered = if text == CMD_ANSWER {
                Some("cmd")
            } else {
                Place::from_folder(&text).map(Place::folder)
            };
            json!({ "ran": true, "answered_from": answered, "exit": o.status.code() })
        }
        Err(e) => {
            json!({ "ran": false, "error_kind": format!("{:?}", e.kind()), "error": e.raw_os_error() })
        }
    };
    Outcome::Line(v.to_string())
}

/// A stand-in: name the folder it sits in.
pub fn whoami() -> Outcome {
    let folder: Option<PathBuf> = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    Outcome::Line(
        folder
            .and_then(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default(),
    )
}
