//! E4 and the runner facts' console: each manifest variant started each way, each child
//! recording from inside whether it got a console and whether that console has a visible
//! window, written to its own `--out` file since some of them have no standard output. The
//! manifest each variant actually carries is read back from the program, so a build that
//! embedded nothing says so. Children's output never reaches this process's standard output.

use super::{ffi, logon_now, sibling_exe, take_child_report, task};
use crate::cli::{Outcome, TaskExec};
use crate::console::{Scenario, Variant, excerpt, spelling};
use crate::report::Report;
use serde_json::{Value, json};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use windows_sys::Win32::Foundation::FreeLibrary;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_TYPE_CHAR, FILE_TYPE_DISK, FILE_TYPE_PIPE, GetFileType,
};
use windows_sys::Win32::System::Console::{
    GetConsoleProcessList, GetConsoleWindow, GetStdHandle, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::LibraryLoader::{
    FindResourceW, LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE, LoadLibraryExW,
    LoadResource, LockResource, SizeofResource,
};
use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, DETACHED_PROCESS};
use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;

/// `RT_MANIFEST`, and the resource id of a program's own manifest.
const RT_MANIFEST: usize = 24;
const CREATEPROCESS_MANIFEST_RESOURCE_ID: usize = 1;

/// The manifest embedded in a program, as text.
pub fn embedded_manifest(exe: &Path) -> Option<String> {
    let w = ffi::wide(exe);
    // SAFETY: `w` is NUL-terminated; the module is mapped as data only and freed below.
    let module = unsafe {
        LoadLibraryExW(
            w.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        )
    };
    if module.is_null() {
        return None;
    }
    // SAFETY: `module` is loaded; the resource ids are integer ids passed as pointers, as
    // MAKEINTRESOURCE does; the resource's bytes live while the module is loaded.
    let text = unsafe {
        let res = FindResourceW(
            module,
            CREATEPROCESS_MANIFEST_RESOURCE_ID as *const u16,
            RT_MANIFEST as *const u16,
        );
        if res.is_null() {
            None
        } else {
            let size = SizeofResource(module, res) as usize;
            let data = LockResource(LoadResource(module, res));
            (!data.is_null()).then(|| {
                String::from_utf8_lossy(std::slice::from_raw_parts(data.cast::<u8>(), size))
                    .into_owned()
            })
        }
    };
    // SAFETY: `module` was loaded above.
    unsafe {
        FreeLibrary(module);
    }
    text
}

fn manifest_facts(exe: &Path, expected: crate::console::Spelling) -> Value {
    if !exe.is_file() {
        return json!({ "built": false });
    }
    match embedded_manifest(exe) {
        None => json!({ "built": true, "manifest_embedded": false }),
        Some(text) => {
            let found = spelling(&text);
            json!({
                "built": true,
                "manifest_embedded": true,
                "policy_spelling": found.as_str(),
                "as_expected": found == expected,
                "excerpt": excerpt(&text),
            })
        }
    }
}

pub fn console(scratch: &Path, with_task: bool) -> Report {
    let logon = logon_now();
    if let Some(reason) = ffi::refuse_write(scratch) {
        return Report::refused("console", logon, reason);
    }
    let _ = std::fs::create_dir_all(scratch);
    let launcher = sibling_exe("pitboard-probe");
    let pid = std::process::id();
    let mut variants = serde_json::Map::new();
    let mut matrix = Vec::new();
    for variant in Variant::ALL {
        let exe = sibling_exe(variant.bin_name());
        variants.insert(
            variant.as_str().into(),
            manifest_facts(&exe, variant.expected_spelling()),
        );
        if !exe.is_file() {
            continue;
        }
        for scenario in Scenario::ALL {
            let out = scratch.join(format!(
                "{}console-{}-{}-{pid}.json",
                crate::PROBE_PREFIX,
                variant.as_str(),
                scenario.as_str()
            ));
            let _ = std::fs::remove_file(&out);
            let started = match scenario {
                Scenario::NoConsoleToInherit => {
                    via_launcher(&launcher, &exe, DETACHED_PROCESS, &out)
                }
                Scenario::CreateNoWindowParent => {
                    via_launcher(&launcher, &exe, CREATE_NO_WINDOW, &out)
                }
                Scenario::CreateNoWindow => direct(&exe, CREATE_NO_WINDOW, &out),
                Scenario::DetachedProcess => direct(&exe, DETACHED_PROCESS, &out),
                Scenario::FromConsoleParent => direct(&exe, 0, &out),
                Scenario::FromTask if with_task => {
                    let exec = match variant {
                        Variant::Plain => TaskExec::Probe,
                        Variant::Detached => TaskExec::Detached,
                        Variant::DetachedAsmV1 => TaskExec::DetachedAsmv1,
                    };
                    let args = format!("--out \"{}\" console-child", out.display());
                    task::run_once(
                        &format!("console-{}", variant.as_str().replace('_', "-")),
                        exec,
                        &args,
                        scratch,
                        &out,
                    )
                }
                Scenario::FromTask => json!({ "run": false, "reason": "needs --with-task" }),
            };
            matrix.push(json!({
                "variant": variant.as_str(),
                "scenario": scenario.as_str(),
                "started": started,
                "child": take_child_report(&out).map(|r| r["data"].clone()),
            }));
        }
    }
    // SAFETY: GetConsoleWindow has no preconditions.
    let own = unsafe { GetConsoleWindow() };
    Report::ok(
        "console",
        logon,
        json!({
            "this_process_has_console_window": !own.is_null(),
            // SAFETY: `own` is a window handle or null, which IsWindowVisible accepts.
            "this_process_window_visible": !own.is_null() && unsafe { IsWindowVisible(own) } != 0,
            "variants": variants,
            "matrix": matrix,
            "note": "no_console_to_inherit is the scenario consoleAllocationPolicy changes: a \
                     plain program gets a console with a window, an honoured detached one gets \
                     no console",
        }),
    )
}

fn direct(exe: &Path, flags: u32, out: &Path) -> Value {
    let mut cmd = Command::new(exe);
    cmd.arg("--out")
        .arg(out)
        .arg("console-child")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if flags != 0 {
        cmd.creation_flags(flags);
    }
    match cmd.status() {
        Ok(s) => json!({ "ok": true, "exit": s.code() }),
        Err(e) => json!({ "ok": false, "error": super::io_code(&e) }),
    }
}

/// Start the plain probe with `launcher_flags` as a go-between, which starts `exe` with no
/// flags, so `exe`'s parent is a process with that console state.
fn via_launcher(launcher: &Path, exe: &Path, launcher_flags: u32, out: &Path) -> Value {
    let status = Command::new(launcher)
        .arg("console-launch")
        .arg("--exe")
        .arg(exe)
        .arg("--flags")
        .arg("0")
        .arg("--child-out")
        .arg(out)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(launcher_flags)
        .status();
    match status {
        Ok(s) => json!({ "ok": true, "launcher_exit": s.code() }),
        Err(e) => json!({ "ok": false, "error": super::io_code(&e) }),
    }
}

/// The go-between: start `exe` as `console-child` with `flags` and exit with its code.
pub fn console_launch(exe: &Path, flags: u32, child_out: &Path) -> Outcome {
    let mut cmd = Command::new(exe);
    cmd.arg("--out").arg(child_out).arg("console-child");
    if flags != 0 {
        cmd.creation_flags(flags);
    }
    match cmd.status() {
        Ok(s) => Outcome::Code(s.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(_) => Outcome::Code(101),
    }
}

/// What a console variant finds when it starts.
pub fn console_child() -> Report {
    let logon = logon_now();
    // SAFETY: GetConsoleWindow has no preconditions.
    let hwnd = unsafe { GetConsoleWindow() };
    let mut pids = [0u32; 16];
    // SAFETY: `pids` holds 16 entries.
    let attached = unsafe { GetConsoleProcessList(pids.as_mut_ptr(), pids.len() as u32) };
    // SAFETY: GetStdHandle has no preconditions; GetFileType accepts any handle value.
    let stdout_type = unsafe { GetFileType(GetStdHandle(STD_OUTPUT_HANDLE)) };
    Report::ok(
        "console-child",
        logon,
        json!({
            "program": std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())),
            "has_console": attached > 0,
            "processes_on_console": attached,
            "has_console_window": !hwnd.is_null(),
            // SAFETY: `hwnd` is a window handle or null.
            "console_window_visible": !hwnd.is_null() && unsafe { IsWindowVisible(hwnd) } != 0,
            "stdout_type": match stdout_type {
                FILE_TYPE_CHAR => "char",
                FILE_TYPE_DISK => "disk",
                FILE_TYPE_PIPE => "pipe",
                _ => "none_or_unknown",
            },
        }),
    )
}
