//! G8: a sign-in started the way W19 will start one, from a parent with no window: the
//! program runs with `CREATE_NO_WINDOW` and piped standard input, output and error. What it
//! prints is passed on to this process's standard error, so the owner sees the prompts; what
//! the owner types is passed to it line by line with the chosen ending. Neither is kept or
//! put in the report, which holds counts, whether a URL was printed, whether a browser
//! started, and the exit code.
//!
//! It refuses unless the account is marked and `CLAUDE_CONFIG_DIR` and `CODEX_HOME` are both
//! set to folders that pass the scratch check, so a sign-in it starts can only land in a
//! scratch home; and it starts only a tool's program.

use super::{ffi, logon_now};
use crate::cli::Newline;
use crate::images;
use crate::report::Report;
use serde_json::json;
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Instant;

use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

fn browsers() -> BTreeSet<u32> {
    super::processes::snapshot()
        .map(|all| {
            all.into_iter()
                .filter(|e| images::is_browser_image(&e.image))
                .map(|e| e.pid)
                .collect()
        })
        .unwrap_or_default()
}

pub fn windowless(scratch: &Path, newline: Newline, program: &[String]) -> Report {
    let logon = logon_now();
    if let Some(reason) = ffi::refuse_write(scratch) {
        return Report::refused("windowless", logon, reason);
    }
    for var in ["CLAUDE_CONFIG_DIR", "CODEX_HOME"] {
        match std::env::var_os(var) {
            Some(v) if !v.is_empty() => {
                if let Err(reason) = ffi::scratch_check(Path::new(&v)) {
                    return Report::refused("windowless", logon, format!("{var}: {reason}"));
                }
            }
            _ => {
                return Report::refused(
                    "windowless",
                    logon,
                    format!("{var} must be set to a scratch folder before a sign-in is started"),
                );
            }
        }
    }
    let Some(first) = program.first() else {
        return Report::refused("windowless", logon, "name a program after --");
    };
    let base = Path::new(first)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let image = if base.to_lowercase().ends_with(".exe") {
        base.clone()
    } else {
        format!("{base}.exe")
    };
    let tool = image.to_lowercase();
    if !(tool.starts_with("claude") || tool.starts_with("codex")) || !images::is_tool_image(&image)
    {
        return Report::refused(
            "windowless",
            logon,
            "windowless starts only claude or codex",
        );
    }

    let before = browsers();
    let started = Instant::now();
    let mut child = match Command::new(first)
        .args(&program[1..])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return Report::refused("windowless", logon, format!("cannot start it: {e}")),
    };

    let url_seen = Arc::new(AtomicBool::new(false));
    let pump = |stream: Box<dyn std::io::Read + Send>| {
        let url_seen = Arc::clone(&url_seen);
        std::thread::spawn(move || {
            let mut lines = 0u32;
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                lines += 1;
                if line.contains("http://") || line.contains("https://") {
                    url_seen.store(true, Ordering::Relaxed);
                }
                eprintln!("{line}");
            }
            lines
        })
    };
    let out = pump(Box::new(child.stdout.take().expect("piped")));
    let err = pump(Box::new(child.stderr.take().expect("piped")));

    let forwarded = Arc::new(AtomicU32::new(0));
    let mut stdin = child.stdin.take().expect("piped");
    let ending = match newline {
        Newline::Lf => "\n",
        Newline::Crlf => "\r\n",
    };
    {
        let forwarded = Arc::clone(&forwarded);
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                if stdin
                    .write_all(format!("{line}{ending}").as_bytes())
                    .is_err()
                    || stdin.flush().is_err()
                {
                    break;
                }
                forwarded.fetch_add(1, Ordering::Relaxed);
            }
        });
    }

    let status = child.wait();
    let stdout_lines = out.join().unwrap_or(0);
    let stderr_lines = err.join().unwrap_or(0);
    let new_browsers = browsers().difference(&before).count();
    Report::ok(
        "windowless",
        logon,
        json!({
            "program": image,
            "newline": format!("{newline:?}"),
            "exit": status.ok().and_then(|s| s.code()),
            "stdout_lines": stdout_lines,
            "stderr_lines": stderr_lines,
            "printed_a_url": url_seen.load(Ordering::Relaxed),
            "lines_forwarded": forwarded.load(Ordering::Relaxed),
            "browser_processes_started": new_browsers,
            "elapsed_ms": started.elapsed().as_millis() as u64,
        }),
    )
}
