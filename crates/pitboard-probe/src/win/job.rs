//! F1 and F2: the Job Object. F1 starts a chain (the probe through the probe to a leaf, or a
//! `.cmd` through `node.exe` to the leaf, as an npm shim does), assigns its head to a job
//! right after starting it, and lets the leaf say whether it ended up in that job; over many
//! rounds this counts the grandchildren that escaped. F2 opens a local stand-in page from a
//! process in a job, then ends the job one way, and records whether the browser was in the
//! job and whether it outlived it. No real site is loaded and only browser images are named.

use super::ffi::{self, Owned};
use super::processes::{open_query, snapshot};
use super::{io_code, logon_now};
use crate::cli::{Chain, JobEnd, Outcome};
use crate::report::Report;
use crate::{guard, images};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    JobObjectBasicProcessIdList, OpenJobObjectW, QueryInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::SystemServices::JOB_OBJECT_QUERY;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, GetCurrentProcess, GetExitCodeProcess, OpenProcess, PROCESS_TERMINATE,
    TerminateProcess,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const IN_JOB: u8 = 10;
const NOT_IN_JOB: u8 = 11;
const NO_JOB: u8 = 12;
const STILL_ACTIVE: u32 = 259;

fn create_job(name: &str) -> Result<Owned, u32> {
    let w = ffi::wide(name);
    // SAFETY: `w` is NUL-terminated; the job is owned by the result.
    Owned::new(unsafe { CreateJobObjectW(std::ptr::null(), w.as_ptr()) })
        .ok_or_else(ffi::last_error)
}

fn in_job(process: HANDLE, job: HANDLE) -> Option<bool> {
    let mut r = 0;
    // SAFETY: both handles are valid or null (null asks about any job).
    (unsafe { IsProcessInJob(process, job, &mut r) } != 0).then_some(r != 0)
}

fn assign(job: &Owned, child: &std::process::Child) -> Result<(), u32> {
    // SAFETY: `job` and the child's process handle are valid.
    let ok = unsafe { AssignProcessToJobObject(job.raw(), child.as_raw_handle() as HANDLE) };
    if ok == 0 {
        Err(ffi::last_error())
    } else {
        Ok(())
    }
}

fn guarded(block: &'static str, scratch: &Path) -> Result<(), Report> {
    if let Some(reason) = ffi::refuse_write(scratch) {
        return Err(Report::refused(block, logon_now(), reason));
    }
    std::fs::create_dir_all(scratch).map_err(|e| {
        Report::refused(
            block,
            logon_now(),
            format!("cannot make the scratch folder: {e}"),
        )
    })
}

pub fn job(scratch: &Path, chain: Chain, rounds: u32) -> Report {
    if let Err(r) = guarded("job", scratch) {
        return r;
    }
    let probe = std::env::current_exe().unwrap_or_default();
    let name = format!("{}job-{}", crate::PROBE_PREFIX, std::process::id());
    let job = match create_job(&name) {
        Ok(j) => j,
        Err(code) => {
            return Report::refused(
                "job",
                logon_now(),
                format!("CreateJobObjectW failed: {code}"),
            );
        }
    };
    // SAFETY: a pseudo-handle for this process.
    let parent_in_a_job = in_job(unsafe { GetCurrentProcess() }, std::ptr::null_mut());

    let cmd_path = scratch.join(format!("{}chain.cmd", crate::PROBE_PREFIX));
    let mjs_path = scratch.join(format!("{}chain.mjs", crate::PROBE_PREFIX));
    if chain == Chain::CmdNodeExe {
        if !Command::new("where")
            .arg("node")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
        {
            return Report::refused(
                "job",
                logon_now(),
                "node.exe is not on PATH; the cmd-node-exe chain needs it",
            );
        }
        let _ = std::fs::write(
            &cmd_path,
            "@node \"%~dp0pitboard-probe-chain.mjs\" %*\r\n@exit /b %ERRORLEVEL%\r\n",
        );
        let _ = std::fs::write(
            &mjs_path,
            "import { spawnSync } from \"node:child_process\";\n\
             const [probe, jobName] = process.argv.slice(2);\n\
             const r = spawnSync(probe, [\"job-leaf\", \"--job-name\", jobName], { stdio: \"ignore\", windowsHide: true });\n\
             process.exit(r.status ?? 13);\n",
        );
    }

    let mut codes: BTreeMap<String, u32> = BTreeMap::new();
    let (mut assign_failed, mut in_job_n, mut escaped, mut unknown) = (0u32, 0u32, 0u32, 0u32);
    let mut assign_errors: BTreeMap<u32, u32> = BTreeMap::new();
    let started = Instant::now();
    for _ in 0..rounds {
        let mut cmd = match chain {
            Chain::Exe => {
                let mut c = Command::new(&probe);
                c.args(["job-mid", "--job-name", &name]);
                c
            }
            Chain::CmdNodeExe => {
                let mut c = Command::new("cmd.exe");
                c.raw_arg(format!(
                    "/d /s /c \"\"{}\" \"{}\" {}\"",
                    cmd_path.display(),
                    probe.display(),
                    name
                ));
                c
            }
        };
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                *codes
                    .entry(format!("spawn_error_{}", io_code(&e)))
                    .or_default() += 1;
                continue;
            }
        };
        if let Err(code) = assign(&job, &child) {
            assign_failed += 1;
            *assign_errors.entry(code).or_default() += 1;
        }
        let code = child.wait().ok().and_then(|s| s.code());
        *codes.entry(format!("{code:?}")).or_default() += 1;
        match code {
            Some(c) if c == i32::from(IN_JOB) => in_job_n += 1,
            Some(c) if c == i32::from(NOT_IN_JOB) => escaped += 1,
            _ => unknown += 1,
        }
    }
    let _ = std::fs::remove_file(&cmd_path);
    let _ = std::fs::remove_file(&mjs_path);
    Report::ok(
        "job",
        logon_now(),
        json!({
            "chain": format!("{chain:?}"),
            "rounds": rounds,
            "this_process_already_in_a_job": parent_in_a_job,
            "assign_failed": assign_failed,
            "assign_errors_by_code": assign_errors.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(),
            "leaf_in_the_job": in_job_n,
            "leaf_escaped": escaped,
            "leaf_unknown": unknown,
            "exit_codes": codes,
            "elapsed_ms": started.elapsed().as_millis() as u64,
        }),
    )
}

/// Exit code of a hidden helper given a job name or page that is not the probe's.
const NOT_THE_PROBES: u8 = 3;

/// The leaf: whether this process is in the named job, as its exit code.
pub fn job_leaf(job_name: &str) -> Outcome {
    if !guard::is_probe_object_name(job_name) {
        return Outcome::Code(NOT_THE_PROBES);
    }
    let w = ffi::wide(job_name);
    // SAFETY: `w` is NUL-terminated; the handle is owned below.
    let Some(job) = Owned::new(unsafe { OpenJobObjectW(JOB_OBJECT_QUERY, 0, w.as_ptr()) }) else {
        return Outcome::Code(NO_JOB);
    };
    // SAFETY: a pseudo-handle for this process.
    match in_job(unsafe { GetCurrentProcess() }, job.raw()) {
        Some(true) => Outcome::Code(IN_JOB),
        Some(false) => Outcome::Code(NOT_IN_JOB),
        None => Outcome::Code(NO_JOB),
    }
}

/// The middle of the exe chain: start the leaf and pass its answer on.
pub fn job_mid(job_name: &str) -> Outcome {
    if !guard::is_probe_object_name(job_name) {
        return Outcome::Code(NOT_THE_PROBES);
    }
    let probe = std::env::current_exe().unwrap_or_default();
    let status = Command::new(probe)
        .args(["job-leaf", "--job-name", job_name])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    Outcome::Code(
        status
            .ok()
            .and_then(|s| s.code())
            .map_or(13, |c| c.clamp(0, 255) as u8),
    )
}

/// F2's opener: once in the named job, open the page with the default browser. It opens
/// only the probe's own stand-in page, by an absolute path, so the shell's "open" can never
/// start a program or reach a site; and only in a job the probe named.
pub fn job_open_page(page: &Path, job_name: &str, linger_seconds: u64) -> Outcome {
    let is_standin = page
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case(crate::STANDIN_PAGE));
    if !guard::is_probe_object_name(job_name)
        || !is_standin
        || !guard::is_absolute_without_parent(page)
        || !page.is_file()
    {
        return Outcome::Code(NOT_THE_PROBES);
    }
    let w = ffi::wide(job_name);
    // SAFETY: `w` is NUL-terminated; the handle is owned below.
    let job = Owned::new(unsafe { OpenJobObjectW(JOB_OBJECT_QUERY, 0, w.as_ptr()) });
    let waited = Instant::now();
    loop {
        // SAFETY: a pseudo-handle for this process.
        let inside = job
            .as_ref()
            .and_then(|j| in_job(unsafe { GetCurrentProcess() }, j.raw()));
        if inside == Some(true) {
            break;
        }
        if waited.elapsed() > Duration::from_secs(3) {
            return Outcome::Code(2);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let verb = ffi::wide("open");
    let file = ffi::wide(page);
    // SAFETY: both strings are NUL-terminated; the other pointers may be null.
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    std::thread::sleep(Duration::from_secs(linger_seconds));
    Outcome::Code(if r as usize > 32 { 0 } else { 1 })
}

fn browsers() -> BTreeSet<u32> {
    snapshot()
        .map(|all| {
            all.into_iter()
                .filter(|e| images::is_browser_image(&e.image))
                .map(|e| e.pid)
                .collect()
        })
        .unwrap_or_default()
}

fn alive(pid: u32) -> bool {
    open_query(pid).is_ok_and(|h| {
        let mut code = 0u32;
        // SAFETY: an open process handle; `code` is a valid out-param.
        (unsafe { GetExitCodeProcess(h.raw(), &mut code) } != 0) && code == STILL_ACTIVE
    })
}

fn job_pids(job: &Owned) -> Vec<u32> {
    // Room for 256 pids after the list's two counts.
    let mut buf = vec![0usize; 2 + 256];
    let size = (buf.len() * std::mem::size_of::<usize>()) as u32;
    // SAFETY: `buf` holds `size` bytes, aligned for JOBOBJECT_BASIC_PROCESS_ID_LIST.
    let ok = unsafe {
        QueryInformationJobObject(
            job.raw(),
            JobObjectBasicProcessIdList,
            buf.as_mut_ptr().cast(),
            size,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Vec::new();
    }
    // SAFETY: on success `buf` begins with the list, followed by its ids.
    let list = unsafe { &*(buf.as_ptr() as *const JOBOBJECT_BASIC_PROCESS_ID_LIST) };
    let n = list.NumberOfProcessIdsInList as usize;
    let ids = std::ptr::addr_of!(list.ProcessIdList).cast::<usize>();
    // SAFETY: the list holds `n` ids, within `buf`.
    (0..n.min(256))
        .map(|i| unsafe { *ids.add(i) } as u32)
        .collect()
}

pub fn job_browser(scratch: &Path, end: JobEnd, wait_seconds: u64) -> Report {
    if let Err(r) = guarded("job-browser", scratch) {
        return r;
    }
    let page = scratch.join(crate::STANDIN_PAGE);
    let _ = std::fs::write(
        &page,
        "<!doctype html><title>pitboard-probe stand-in</title><p>A local stand-in page. No site is loaded.</p>",
    );
    let before = browsers();
    let name = format!("{}jobb-{}", crate::PROBE_PREFIX, std::process::id());
    let job = match create_job(&name) {
        Ok(j) => j,
        Err(code) => {
            return Report::refused(
                "job-browser",
                logon_now(),
                format!("CreateJobObjectW failed: {code}"),
            );
        }
    };
    let linger = if end == JobEnd::ParentExit {
        0
    } else {
        wait_seconds + 30
    };
    let mut opener = match Command::new(std::env::current_exe().unwrap_or_default())
        .arg("job-open-page")
        .arg("--page")
        .arg(&page)
        .args(["--job-name", &name, "--linger-seconds", &linger.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return Report::refused(
                "job-browser",
                logon_now(),
                format!("cannot start the opener: {e}"),
            );
        }
    };
    let assigned = assign(&job, &opener);
    std::thread::sleep(Duration::from_secs(wait_seconds));
    let new: Vec<u32> = browsers().difference(&before).copied().collect();
    let in_the_job: Vec<u32> = new
        .iter()
        .copied()
        .filter(|pid| {
            open_query(*pid)
                .ok()
                .and_then(|h| in_job(h.raw(), job.raw()))
                == Some(true)
        })
        .collect();

    let ended = match end {
        JobEnd::TerminateJob => {
            // SAFETY: `job` is a job handle this process made.
            json!({ "terminated": unsafe { TerminateJobObject(job.raw(), 1) } != 0 })
        }
        JobEnd::Allowlist => {
            let mut killed = 0u32;
            let images: BTreeMap<u32, String> = snapshot()
                .map(|all| all.into_iter().map(|e| (e.pid, e.image)).collect())
                .unwrap_or_default();
            for pid in job_pids(&job) {
                if images
                    .get(&pid)
                    .is_some_and(|i| i.to_lowercase().starts_with("pitboard-probe"))
                {
                    // SAFETY: OpenProcess for terminate; the handle is owned below.
                    if let Some(h) = Owned::new(unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) }) {
                        // SAFETY: an open process handle with terminate rights.
                        killed += u32::from(unsafe { TerminateProcess(h.raw(), 1) } != 0);
                    }
                }
            }
            json!({ "probe_processes_terminated": killed })
        }
        JobEnd::ParentExit => json!({ "opener_exit": opener.wait().ok().and_then(|s| s.code()) }),
    };
    let opener_exit = opener.wait().ok().and_then(|s| s.code());
    drop(job);
    std::thread::sleep(Duration::from_secs(3));
    let survived = new.iter().filter(|pid| alive(**pid)).count();
    Report::ok(
        "job-browser",
        logon_now(),
        json!({
            "end": format!("{end:?}"),
            "opener_assigned": assigned.is_ok(),
            "opener_exit": opener_exit,
            "browsers_running_before": before.len(),
            "browser_processes_started": new.len(),
            "browser_processes_in_the_job": in_the_job.len(),
            "ended": ended,
            "browser_processes_alive_after": survived,
            "note": "close the browser by hand afterwards; the page is a local file",
        }),
    )
}
