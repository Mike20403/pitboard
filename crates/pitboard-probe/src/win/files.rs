//! C3 to C6 and G6: rename routes, the replace loop and its readers, flushing a directory,
//! proper-lockfile's lock, LockFileEx, and replacing a file in a scratch folder. Every file
//! is a `pitboard-probe-*` one under a scratch folder that passed the write guard; G6's
//! target and source are `pitboard-probe-*` files the owner stages there, so a swap can
//! never replace a file the probe did not make.

use super::ffi::{self, Owned};
use super::{io_code, logon_now, take_child_report};
use crate::PROBE_PREFIX;
use crate::cli::{LfxMode, LockMode, ReaderShare, Route};
use crate::replace::{self, Budget, Disposition, ReadCheck};
use crate::report::Report;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{FILETIME, GENERIC_READ, GENERIC_WRITE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, DELETE, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_BACKUP_SEMANTICS, FILE_GENERIC_WRITE,
    FILE_RENAME_INFO, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
    FileRenameInfoEx, FlushFileBuffers, GetDriveTypeW, GetVolumeInformationByHandleW,
    LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, MOVEFILE_REPLACE_EXISTING,
    MOVEFILE_WRITE_THROUGH, MoveFileExW, OPEN_ALWAYS, OPEN_EXISTING, ReadFile,
    SetFileInformationByHandle, SetFileTime, UnlockFileEx,
};
use windows_sys::Win32::System::IO::OVERLAPPED;
use windows_sys::Win32::System::SystemServices::FILE_SUPPORTS_POSIX_UNLINK_RENAME;

/// `FILE_RENAME_REPLACE_IF_EXISTS` and `FILE_RENAME_POSIX_SEMANTICS` (ntifs.h), the flags
/// of `FILE_RENAME_INFO` under `FileRenameInfoEx`.
const FILE_RENAME_REPLACE_IF_EXISTS: u32 = 0x1;
const FILE_RENAME_POSIX_SEMANTICS: u32 = 0x2;

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

fn probe_file(scratch: &Path, stem: &str) -> PathBuf {
    scratch.join(format!("{PROBE_PREFIX}{stem}"))
}

fn open(path: &Path, access: u32, share: u32, disposition: u32, flags: u32) -> Result<Owned, u32> {
    let w = ffi::wide(path);
    // SAFETY: `w` is NUL-terminated; the handle is owned by the result.
    let h = unsafe {
        CreateFileW(
            w.as_ptr(),
            access,
            share,
            std::ptr::null(),
            disposition,
            flags,
            std::ptr::null_mut(),
        )
    };
    Owned::new(h).ok_or_else(ffi::last_error)
}

fn share_flags(share: ReaderShare) -> u32 {
    match share {
        ReaderShare::Delete | ReaderShare::None => {
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        }
        ReaderShare::NoDelete => FILE_SHARE_READ | FILE_SHARE_WRITE,
    }
}

/// `path` in the `\\?\` form a rename's target name takes.
fn verbatim(path: &Path) -> Vec<u16> {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let text = abs.display().to_string();
    let v = if text.starts_with(r"\\?\") {
        text
    } else if let Some(rest) = text.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{rest}")
    } else {
        format!(r"\\?\{text}")
    };
    std::ffi::OsStr::new(&v).encode_wide().collect()
}

/// Rename `src` over `dst` with `FileRenameInfoEx` and POSIX semantics. The buffer is laid
/// out as std's own rename lays it: the name at `FileName`'s offset, its length in bytes
/// without the NUL, and room for the NUL.
pub fn rename_posix(src: &Path, dst: &Path) -> Result<(), u32> {
    let h = open(
        src,
        DELETE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL,
    )?;
    let name = verbatim(dst);
    let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let name_bytes = name.len() * 2;
    let size = (offset + name_bytes + 2).max(std::mem::size_of::<FILE_RENAME_INFO>());
    // u64s, so the buffer is aligned for FILE_RENAME_INFO.
    let mut buf = vec![0u64; size.div_ceil(8)];
    let info = buf.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: `buf` holds `size` zeroed bytes, aligned for FILE_RENAME_INFO; the header
    // fields are written in place and the name is copied to FileName's offset, within
    // `size`.
    unsafe {
        (*info).Anonymous.Flags = FILE_RENAME_REPLACE_IF_EXISTS | FILE_RENAME_POSIX_SEMANTICS;
        (*info).RootDirectory = std::ptr::null_mut();
        (*info).FileNameLength = name_bytes as u32;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            info.cast::<u8>().add(offset).cast::<u16>(),
            name.len(),
        );
    }
    // SAFETY: `h` is open for DELETE; `info` holds a well-formed FILE_RENAME_INFO of `size`
    // bytes.
    let ok =
        unsafe { SetFileInformationByHandle(h.raw(), FileRenameInfoEx, info.cast(), size as u32) };
    if ok == 0 {
        Err(ffi::last_error())
    } else {
        Ok(())
    }
}

/// `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`.
pub fn rename_movefile(src: &Path, dst: &Path) -> Result<(), u32> {
    let s = ffi::wide(src);
    let d = ffi::wide(dst);
    // SAFETY: both strings are NUL-terminated.
    let ok = unsafe {
        MoveFileExW(
            s.as_ptr(),
            d.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(ffi::last_error())
    } else {
        Ok(())
    }
}

fn rename(route: Route, src: &Path, dst: &Path) -> Result<(), u32> {
    match route {
        Route::Movefile => rename_movefile(src, dst),
        Route::Posix => rename_posix(src, dst),
    }
}

// --- C3: the volume ----------------------------------------------------------------------

pub fn volume(scratch: &Path) -> Report {
    if let Err(r) = guarded("volume", scratch) {
        return r;
    }
    let (flags, fs_name) = volume_info(scratch);
    let drive_type = drive_type(scratch);
    let pid = std::process::id();
    let mut routes = serde_json::Map::new();
    for (route, label) in [
        (Route::Posix, "posix_rename"),
        (Route::Movefile, "movefile"),
    ] {
        let mut by_hold = serde_json::Map::new();
        for (hold, hold_label) in [
            (None, "target_not_held"),
            (Some(ReaderShare::Delete), "target_held_sharing_delete"),
            (
                Some(ReaderShare::NoDelete),
                "target_held_not_sharing_delete",
            ),
        ] {
            let src = probe_file(scratch, &format!("vol-src-{pid}"));
            let dst = probe_file(scratch, &format!("vol-dst-{pid}"));
            let result =
                if std::fs::write(&src, b"s").is_err() || std::fs::write(&dst, b"d").is_err() {
                    json!({ "attempted": false, "reason": "could not write the pair" })
                } else {
                    let held = hold.map(|share| {
                        open(
                            &dst,
                            GENERIC_READ,
                            share_flags(share),
                            OPEN_EXISTING,
                            FILE_ATTRIBUTE_NORMAL,
                        )
                    });
                    let hold_error = held.as_ref().and_then(|h| h.as_ref().err().copied());
                    let r = rename(route, &src, &dst);
                    drop(held);
                    json!({
                        "attempted": true,
                        "ok": r.is_ok(),
                        "error": r.err(),
                        "hold_open_error": hold_error,
                    })
                };
            let _ = std::fs::remove_file(&src);
            let _ = std::fs::remove_file(&dst);
            by_hold.insert(hold_label.into(), result);
        }
        routes.insert(label.into(), Value::Object(by_hold));
    }
    Report::ok(
        "volume",
        logon_now(),
        json!({
            "file_system": fs_name,
            "drive_type": drive_type,
            "filesystem_flags": flags.map(|f| format!("0x{f:08x}")),
            "posix_unlink_rename_supported": flags.map(|f| f & FILE_SUPPORTS_POSIX_UNLINK_RENAME != 0),
            "routes": routes,
        }),
    )
}

fn volume_info(dir: &Path) -> (Option<u32>, Option<String>) {
    let Ok(h) = open(
        dir,
        FILE_GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS,
    ) else {
        return (None, None);
    };
    let mut flags = 0u32;
    let mut name = [0u16; 64];
    // SAFETY: `h` is an open directory handle; the out-params are sized; the volume name
    // buffer is not wanted.
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            h.raw(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut flags,
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    if ok == 0 {
        (None, None)
    } else {
        (Some(flags), Some(ffi::from_wide_buf(&name)))
    }
}

fn drive_type(dir: &Path) -> Value {
    let abs = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let root = abs
        .ancestors()
        .last()
        .map(|r| {
            let mut s = r.display().to_string();
            if !s.ends_with('\\') {
                s.push('\\');
            }
            s
        })
        .unwrap_or_default();
    let w = ffi::wide(&root);
    // SAFETY: `w` is NUL-terminated.
    let t = unsafe { GetDriveTypeW(w.as_ptr()) };
    json!(match t {
        2 => "removable",
        3 => "fixed",
        4 => "remote",
        5 => "cdrom",
        6 => "ramdisk",
        1 => "no_root_dir",
        _ => "unknown",
    })
}

// --- C4: the replace loop and its readers ---------------------------------------------------

fn target_of(scratch: &Path) -> PathBuf {
    probe_file(scratch, "replace-target")
}

fn done_of(scratch: &Path) -> PathBuf {
    probe_file(scratch, "replace-target.done")
}

#[derive(Default)]
struct ReaderStats {
    reads: u64,
    whole: u64,
    torn: u64,
    empty: u64,
    open_errors: BTreeMap<u32, u64>,
    read_errors: BTreeMap<u32, u64>,
}

impl ReaderStats {
    fn to_json(&self) -> Value {
        let codes = |m: &BTreeMap<u32, u64>| {
            m.iter()
                .map(|(k, v)| (k.to_string(), json!(v)))
                .collect::<serde_json::Map<_, _>>()
        };
        json!({
            "reads": self.reads,
            "whole": self.whole,
            "torn": self.torn,
            "empty": self.empty,
            "missing_by_open_error": codes(&self.open_errors),
            "read_errors_by_code": codes(&self.read_errors),
        })
    }
}

/// Open the target by path, read it whole and check it, as a tool that reads its login on
/// each use does.
fn read_once(target: &Path, share: u32, stats: &mut ReaderStats) {
    stats.reads += 1;
    let h = match open(
        target,
        GENERIC_READ,
        share,
        OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL,
    ) {
        Ok(h) => h,
        Err(code) => {
            *stats.open_errors.entry(code).or_default() += 1;
            return;
        }
    };
    let mut buf = vec![0u8; replace::RECORD_LEN * 2];
    let mut total = 0usize;
    loop {
        let mut n = 0u32;
        // SAFETY: `h` is open for reading; the slice past `total` is writable.
        let ok = unsafe {
            ReadFile(
                h.raw(),
                buf[total..].as_mut_ptr(),
                (buf.len() - total) as u32,
                &mut n,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            *stats.read_errors.entry(ffi::last_error()).or_default() += 1;
            return;
        }
        if n == 0 || total + n as usize >= buf.len() {
            total += n as usize;
            break;
        }
        total += n as usize;
    }
    match replace::check(&buf[..total]) {
        ReadCheck::Whole(_) => stats.whole += 1,
        ReadCheck::Torn => stats.torn += 1,
        ReadCheck::Empty => stats.empty += 1,
    }
}

pub fn replace_loop(scratch: &Path, rounds: u32, lead_seconds: u64, reader: ReaderShare) -> Report {
    if let Err(r) = guarded("replace-loop", scratch) {
        return r;
    }
    let target = target_of(scratch);
    let done = done_of(scratch);
    let _ = std::fs::remove_file(&done);
    if let Err(e) = std::fs::write(&target, replace::record(0)) {
        return Report::refused(
            "replace-loop",
            logon_now(),
            format!("cannot seed the target: {e}"),
        );
    }
    if lead_seconds > 0 {
        eprintln!("pitboard-probe: the target is ready; the loop starts in {lead_seconds} s");
        std::thread::sleep(Duration::from_secs(lead_seconds));
    }

    let stop = Arc::new(AtomicBool::new(false));
    let reader_thread = (reader != ReaderShare::None).then(|| {
        let stop = Arc::clone(&stop);
        let target = target.clone();
        let share = share_flags(reader);
        std::thread::spawn(move || {
            let mut stats = ReaderStats::default();
            while !stop.load(Ordering::Relaxed) {
                read_once(&target, share, &mut stats);
            }
            stats
        })
    });

    let budget = Budget::default();
    let mut errors: BTreeMap<u32, u64> = BTreeMap::new();
    let (mut retries, mut failures, mut done_rounds) = (0u64, 0u64, 0u64);
    let mut max_attempts = 0u32;
    let pid = std::process::id();
    let started = Instant::now();
    for round in 1..=rounds {
        let tmp = probe_file(scratch, &format!("replace-tmp-{pid}-{round}"));
        if std::fs::write(&tmp, replace::record(round)).is_err() {
            failures += 1;
            continue;
        }
        let mut attempt = 0u32;
        loop {
            let err = rename_movefile(&tmp, &target).err();
            if let Some(code) = err {
                *errors.entry(code).or_default() += 1;
            }
            match replace::classify(err) {
                Disposition::Done => {
                    done_rounds += 1;
                    break;
                }
                Disposition::Retry if attempt < budget.max_retries => {
                    retries += 1;
                    std::thread::sleep(Duration::from_millis(budget.backoff_ms(attempt)));
                    attempt += 1;
                }
                _ => {
                    failures += 1;
                    let _ = std::fs::remove_file(&tmp);
                    break;
                }
            }
        }
        max_attempts = max_attempts.max(attempt + 1);
    }
    let elapsed_ms = started.elapsed().as_millis() as u64;
    stop.store(true, Ordering::Relaxed);
    let in_process = reader_thread.and_then(|t| t.join().ok());
    // Tell readers in other processes the loop is over, give them a moment, then clean up.
    let _ = std::fs::write(&done, b"done");
    std::thread::sleep(Duration::from_secs(2));
    let _ = std::fs::remove_file(&target);
    let _ = std::fs::remove_file(&done);

    let errors: serde_json::Map<String, Value> = errors
        .into_iter()
        .map(|(k, v)| (k.to_string(), json!(v)))
        .collect();
    Report::ok(
        "replace-loop",
        logon_now(),
        json!({
            "rounds": rounds,
            "done": done_rounds,
            "retries": retries,
            "failures": failures,
            "max_attempts_in_a_round": max_attempts,
            "writer_errors_by_code": errors,
            "elapsed_ms": elapsed_ms,
            "in_process_reader_share": format!("{reader:?}"),
            "in_process_reader": in_process.map(|s| s.to_json()),
        }),
    )
}

pub fn replace_reader(scratch: &Path, seconds: u64, share: ReaderShare) -> Report {
    if let Err(r) = guarded("replace-reader", scratch) {
        return r;
    }
    let target = target_of(scratch);
    let done = done_of(scratch);
    let waited = Instant::now();
    while !target.exists() && waited.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(100));
    }
    if !target.exists() {
        return Report::refused(
            "replace-reader",
            logon_now(),
            "the replace loop's target did not appear within 60 s; start replace-loop first",
        );
    }
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut stats = ReaderStats::default();
    let flags = share_flags(share);
    while Instant::now() < deadline && !done.exists() {
        read_once(&target, flags, &mut stats);
    }
    Report::ok(
        "replace-reader",
        logon_now(),
        json!({ "share": format!("{share:?}"), "stats": stats.to_json() }),
    )
}

// --- C5: flushing a directory and mtime precision -----------------------------------------

pub fn flush_dir(scratch: &Path) -> Report {
    if let Err(r) = guarded("flush-dir", scratch) {
        return r;
    }
    let mut out = serde_json::Map::new();
    for (label, access) in [
        ("generic_write_handle", FILE_GENERIC_WRITE),
        ("write_attributes_handle", FILE_WRITE_ATTRIBUTES),
    ] {
        let v = match open(
            scratch,
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
        ) {
            Ok(h) => {
                // SAFETY: an open directory handle.
                let ok = unsafe { FlushFileBuffers(h.raw()) };
                json!({ "opened": true, "flushed": ok != 0, "flush_error": (ok == 0).then(ffi::last_error) })
            }
            Err(code) => json!({ "opened": false, "open_error": code }),
        };
        out.insert(label.into(), v);
    }
    Report::ok(
        "flush-dir",
        logon_now(),
        json!({ "directory_flush": out, "mtime": mtime_precision(scratch) }),
    )
}

/// The sub-second part of a few fresh files' mtimes, to show the file system's precision.
fn mtime_precision(scratch: &Path) -> Value {
    let mut nanos = Vec::new();
    for i in 0..5 {
        let p = probe_file(scratch, &format!("mtime-{i}"));
        if std::fs::write(&p, b"t").is_ok() {
            if let Some(n) = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.subsec_nanos())
            {
                nanos.push(n);
            }
            let _ = std::fs::remove_file(&p);
        }
        std::thread::sleep(Duration::from_millis(7));
    }
    json!({
        "subsec_nanos": nanos,
        "all_multiples_of_100ns": nanos.iter().all(|n| n % 100 == 0),
        "all_whole_seconds": nanos.iter().all(|n| *n == 0),
    })
}

// --- C5: proper-lockfile's lock ---------------------------------------------------------------

const STALE_MS: u64 = 15_000;
const HEARTBEAT_MS: u64 = 7_500;

pub fn lock(scratch: &Path, mode: LockMode, seconds: u64) -> Report {
    if let Err(r) = guarded("lock", scratch) {
        return r;
    }
    let target = probe_file(scratch, "locktarget");
    if !target.exists() {
        let _ = std::fs::write(&target, b"x");
    }
    let lockdir = probe_file(scratch, "locktarget.lock");
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let data = match mode {
        LockMode::Hold => {
            if let Err(e) = std::fs::create_dir(&lockdir) {
                return Report::ok(
                    "lock",
                    logon_now(),
                    json!({ "mode": "hold", "acquired": false, "error": io_code(&e),
                            "held_by_other": e.kind() == std::io::ErrorKind::AlreadyExists }),
                );
            }
            let (mut beats, mut lost, mut changed_by_other) = (0u32, false, false);
            let mut last_set: Option<u64> = None;
            while Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(HEARTBEAT_MS).min(
                    deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
                ));
                if !lockdir.is_dir() {
                    lost = true;
                    break;
                }
                if let (Some(set), Some(now)) = (last_set, mtime_ms(&lockdir))
                    && now != set
                {
                    changed_by_other = true;
                }
                match touch(&lockdir) {
                    Ok(()) => {
                        beats += 1;
                        last_set = mtime_ms(&lockdir);
                    }
                    Err(_) => {
                        lost = true;
                        break;
                    }
                }
            }
            let _ = std::fs::remove_dir(&lockdir);
            json!({
                "mode": "hold",
                "acquired": true,
                "held_seconds": seconds,
                "heartbeats": beats,
                "lock_dir_lost": lost,
                "mtime_changed_by_someone_else": changed_by_other,
            })
        }
        LockMode::Check => {
            let (mut held, mut free, mut stale) = (0u32, 0u32, 0u32);
            let mut max_age = 0u64;
            while Instant::now() < deadline {
                match std::fs::create_dir(&lockdir) {
                    Ok(()) => {
                        free += 1;
                        let _ = std::fs::remove_dir(&lockdir);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                        held += 1;
                        if let Some(age) = mtime_ms(&lockdir).map(|m| now_ms().saturating_sub(m)) {
                            max_age = max_age.max(age);
                            if age > STALE_MS {
                                stale += 1;
                            }
                        }
                    }
                    Err(_) => {}
                }
                std::thread::sleep(Duration::from_millis(1000));
            }
            json!({
                "mode": "check",
                "samples_held": held,
                "samples_free": free,
                "samples_stale": stale,
                "max_heartbeat_age_ms": max_age,
            })
        }
    };
    Report::ok("lock", logon_now(), data)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn mtime_ms(p: &Path) -> Option<u64> {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
}

/// Set a directory's mtime to now, as proper-lockfile's heartbeat does.
fn touch(dir: &Path) -> Result<(), u32> {
    let h = open(
        dir,
        FILE_WRITE_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS,
    )?;
    let mut now = FILETIME::default();
    // SAFETY: `now` is a valid out-param.
    unsafe { windows_sys::Win32::System::SystemInformation::GetSystemTimeAsFileTime(&mut now) };
    // SAFETY: `h` is open for FILE_WRITE_ATTRIBUTES; `now` is a valid FILETIME.
    let ok = unsafe { SetFileTime(h.raw(), std::ptr::null(), std::ptr::null(), &now) };
    if ok == 0 {
        Err(ffi::last_error())
    } else {
        Ok(())
    }
}

// --- C6: LockFileEx on a home's state.lock ------------------------------------------------

fn home_of(scratch: &Path) -> PathBuf {
    probe_file(scratch, "home")
}

pub fn lockfileex(scratch: &Path, mode: LfxMode, seconds: u64, share: ReaderShare) -> Report {
    if let Err(r) = guarded("lockfileex", scratch) {
        return r;
    }
    let home = home_of(scratch);
    let state = home.join("state.lock");
    let ready = home.join(format!("{PROBE_PREFIX}ready"));
    let data = match mode {
        LfxMode::Hold => {
            if let Err(e) = std::fs::create_dir_all(&home) {
                return Report::refused(
                    "lockfileex",
                    logon_now(),
                    format!("cannot make the home: {e}"),
                );
            }
            let h = match open(
                &state,
                GENERIC_READ | GENERIC_WRITE,
                share_flags(share),
                OPEN_ALWAYS,
                FILE_ATTRIBUTE_NORMAL,
            ) {
                Ok(h) => h,
                Err(code) => {
                    return Report::refused(
                        "lockfileex",
                        logon_now(),
                        format!("cannot open state.lock: {code}"),
                    );
                }
            };
            let _ = std::fs::write(
                home.join(format!("{PROBE_PREFIX}content")),
                b"a file beside the lock",
            );
            let mut ov = OVERLAPPED::default();
            // SAFETY: `h` is open for writing; `ov` is zeroed, so the range starts at 0; the
            // whole possible range is locked.
            let ok = unsafe {
                LockFileEx(
                    h.raw(),
                    LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                    0,
                    u32::MAX,
                    u32::MAX,
                    &mut ov,
                )
            };
            let err = (ok == 0).then(ffi::last_error);
            let _ = std::fs::write(&ready, b"ready");
            std::thread::sleep(Duration::from_secs(seconds));
            if ok != 0 {
                // SAFETY: the range locked above.
                unsafe { UnlockFileEx(h.raw(), 0, u32::MAX, u32::MAX, &mut ov) };
            }
            let _ = std::fs::remove_file(&ready);
            json!({ "mode": "hold", "locked": ok != 0, "lock_error": err, "range": "whole", "share": format!("{share:?}") })
        }
        LfxMode::Read => {
            let mut out = serde_json::Map::new();
            for (label, s) in [
                (
                    "sharing_all",
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ),
                ("sharing_read_only", FILE_SHARE_READ),
            ] {
                let v = match open(
                    &state,
                    GENERIC_READ,
                    s,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                ) {
                    Ok(h) => {
                        let mut buf = [0u8; 16];
                        let mut n = 0u32;
                        // SAFETY: `h` is open for reading; `buf` is writable.
                        let ok = unsafe {
                            ReadFile(
                                h.raw(),
                                buf.as_mut_ptr(),
                                buf.len() as u32,
                                &mut n,
                                std::ptr::null_mut(),
                            )
                        };
                        json!({ "opened": true, "read_ok": ok != 0, "read_error": (ok == 0).then(ffi::last_error), "bytes": n })
                    }
                    Err(code) => json!({ "opened": false, "open_error": code }),
                };
                out.insert(label.into(), v);
            }
            json!({ "mode": "read", "exists": state.exists(), "attempts": out })
        }
        LfxMode::RemoveHome => remove_home_while_held(scratch, &home, &ready, seconds, share),
    };
    Report::ok("lockfileex", logon_now(), data)
}

/// Start a holder of the home's lock in another process, remove the home while it holds,
/// and record what was left.
fn remove_home_while_held(
    scratch: &Path,
    home: &Path,
    ready: &Path,
    seconds: u64,
    share: ReaderShare,
) -> Value {
    let exe = std::env::current_exe().unwrap_or_default();
    let out = probe_file(scratch, &format!("lfx-holder-{}.json", std::process::id()));
    let share_word = match share {
        ReaderShare::NoDelete => "no-delete",
        _ => "delete",
    };
    let child = std::process::Command::new(&exe)
        .arg("--out")
        .arg(&out)
        .args([
            "lockfileex",
            "--mode",
            "hold",
            "--share",
            share_word,
            "--seconds",
        ])
        .arg(seconds.max(5).to_string())
        .arg("--scratch")
        .arg(scratch)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            return json!({ "mode": "remove-home", "holder_started": false, "error": e.to_string() });
        }
    };
    let waited = Instant::now();
    while !ready.exists() && waited.elapsed() < Duration::from_secs(15) {
        std::thread::sleep(Duration::from_millis(50));
    }
    let holder_ready = ready.exists();
    let removed = std::fs::remove_dir_all(home);
    let left_while_held = json!({
        "home_exists": home.exists(),
        "state_lock_exists": home.join("state.lock").exists(),
    });
    let _ = child.wait();
    let after = json!({
        "home_exists": home.exists(),
        "second_remove_ok": std::fs::remove_dir_all(home).is_ok() || !home.exists(),
    });
    json!({
        "mode": "remove-home",
        "holder_ready": holder_ready,
        "remove_dir_all_ok": removed.is_ok(),
        "remove_dir_all_error": removed.as_ref().err().map(io_code),
        "left_while_held": left_while_held,
        "after_holder_exited": after,
        "holder": take_child_report(&out),
    })
}

// --- G6: replace a file in a scratch folder --------------------------------------------------

pub fn swap(scratch: &Path, target: &str, source: &str, route: Route) -> Report {
    let logon = logon_now();
    let (target_path, source_path) = match (
        ffi::scratch_file(scratch, target),
        ffi::scratch_file(scratch, source),
    ) {
        (Ok(t), Ok(s)) => (t, s),
        (Err(r), _) | (_, Err(r)) => return Report::refused("swap", logon, r),
    };
    if !target_path.is_file() || !source_path.is_file() {
        return Report::refused(
            "swap",
            logon,
            "--target and --source must both be pitboard-probe-* files in --scratch",
        );
    }
    let tmp = probe_file(scratch, &format!("swap-{}.tmp", std::process::id()));
    if let Err(e) = std::fs::copy(&source_path, &tmp) {
        return Report::refused("swap", logon, format!("cannot stage the source: {e}"));
    }
    let budget = Budget::default();
    let mut errors: BTreeMap<u32, u64> = BTreeMap::new();
    let started = Instant::now();
    let mut attempt = 0u32;
    let ok = loop {
        let err = rename(route, &tmp, &target_path).err();
        if let Some(code) = err {
            *errors.entry(code).or_default() += 1;
        }
        match replace::classify(err) {
            Disposition::Done => break true,
            Disposition::Retry if attempt < budget.max_retries => {
                std::thread::sleep(Duration::from_millis(budget.backoff_ms(attempt)));
                attempt += 1;
            }
            _ => break false,
        }
    };
    if !ok {
        let _ = std::fs::remove_file(&tmp);
    }
    let errors: serde_json::Map<String, Value> = errors
        .into_iter()
        .map(|(k, v)| (k.to_string(), json!(v)))
        .collect();
    Report::ok(
        "swap",
        logon,
        json!({
            "route": format!("{route:?}"),
            "replaced": ok,
            "attempts": attempt + 1,
            "errors_by_code": errors,
            "elapsed_ms": started.elapsed().as_millis() as u64,
            "target_mtime_ms": mtime_ms(&target_path),
            "at_ms": now_ms(),
        }),
    )
}
