//! D1-D3 and M: DPAPI with the vault's flags (CurrentUser scope, no UI, entropy binding a
//! constant to the park's name), on a dummy secret only. `round-trip` seals and unseals in
//! memory and times both; `seal` keeps the sealed bytes in a scratch file; `unseal` opens a
//! kept file and says whether it gave back the dummy, so a password change, an
//! administrator reset, a task or another token can be tried between the two. The calls
//! are made in any logon, and the report names the logon beside the result. Neither the
//! sealed bytes nor the dummy are ever printed. `round-trip` and `seal` write, so they need
//! the throwaway marker; `unseal` only reads a `pitboard-probe-*` file in a scratch folder
//! that passes the guard, so a restricted token that cannot see the marker can be measured.

use super::ffi;
use super::logon_now;
use crate::cli::DpapiAction;
use crate::report::Report;
use serde_json::json;
use std::path::Path;
use std::time::Instant;

use windows_sys::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};

/// The dummy every seal protects. Never a real secret.
const DUMMY: &[u8] = b"pitboard-probe: a dummy park, never a real token";
/// The vault's shape of entropy: a constant, then the park's name.
const CONSTANT: &[u8] = b"pitboard-probe-vault-v1";
const PARK: &str = "pitboard-probe-park";

fn entropy(service: &str) -> Vec<u8> {
    let mut e = CONSTANT.to_vec();
    e.extend_from_slice(service.as_bytes());
    e
}

fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    }
}

/// Take a LocalAlloc'd blob's bytes and free it.
fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    // SAFETY: on success `out.pbData` is `out.cbData` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    // SAFETY: `out.pbData` is a LocalAlloc buffer the call handed over.
    unsafe {
        windows_sys::Win32::Foundation::LocalFree(out.pbData.cast());
    }
    bytes
}

fn seal(secret: &[u8], service: &str) -> Result<Vec<u8>, u32> {
    let ent = entropy(service);
    let (input, ent_blob) = (blob(secret), blob(&ent));
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: the blobs point at live buffers; `out` receives a LocalAlloc buffer.
    let ok = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            &ent_blob,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    };
    if ok == 0 {
        Err(ffi::last_error())
    } else {
        Ok(take(out))
    }
}

fn unseal(sealed: &[u8], service: &str) -> Result<Vec<u8>, u32> {
    let ent = entropy(service);
    let (input, ent_blob) = (blob(sealed), blob(&ent));
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: the blobs point at live buffers; `out` receives a LocalAlloc buffer.
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            &ent_blob,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    };
    if ok == 0 {
        Err(ffi::last_error())
    } else {
        Ok(take(out))
    }
}

pub fn dpapi(scratch: &Path, action: DpapiAction, file: &str) -> Report {
    let logon = logon_now();
    // unseal only reads a file the probe sealed earlier, so it needs the scratch check and
    // not the marker: a restricted token (Codex's sandbox, D2b) may not see the profile's
    // marker, and is exactly the token D2b measures.
    let path = match action {
        DpapiAction::Unseal => ffi::scratch_file_to_read(scratch, file),
        DpapiAction::RoundTrip | DpapiAction::Seal => ffi::scratch_file(scratch, file),
    };
    let path = match path {
        Ok(p) => p,
        Err(reason) => return Report::refused("dpapi", logon, reason),
    };
    if action != DpapiAction::Unseal
        && let Err(e) = std::fs::create_dir_all(scratch)
    {
        return Report::refused(
            "dpapi",
            logon,
            format!("cannot make the scratch folder: {e}"),
        );
    }
    let data = match action {
        DpapiAction::RoundTrip => {
            let mut seal_us = Vec::new();
            let mut unseal_us = Vec::new();
            let mut first_error = None;
            let mut round_trip_ok = true;
            let mut wrong_entropy_refused = None;
            for _ in 0..20 {
                let t = Instant::now();
                let sealed = match seal(DUMMY, PARK) {
                    Ok(s) => s,
                    Err(code) => {
                        first_error = Some(json!({ "seal_error": code }));
                        round_trip_ok = false;
                        break;
                    }
                };
                seal_us.push(t.elapsed().as_micros() as u64);
                let t = Instant::now();
                let opened = unseal(&sealed, PARK);
                unseal_us.push(t.elapsed().as_micros() as u64);
                if opened.as_deref() != Ok(DUMMY) {
                    round_trip_ok = false;
                    first_error.get_or_insert(json!({ "unseal_error": opened.err() }));
                }
                wrong_entropy_refused
                    .get_or_insert(unseal(&sealed, "pitboard-probe-other").is_err());
            }
            json!({
                "action": "round-trip",
                "round_trip_ok": round_trip_ok,
                "wrong_entropy_refused": wrong_entropy_refused,
                "error": first_error,
                "seal_micros": seal_us,
                "unseal_micros": unseal_us,
            })
        }
        DpapiAction::Seal => match seal(DUMMY, PARK) {
            Ok(sealed) => match std::fs::write(&path, &sealed) {
                Ok(()) => {
                    json!({ "action": "seal", "sealed": true, "sealed_bytes": sealed.len(), "file": file })
                }
                Err(e) => json!({ "action": "seal", "sealed": true, "write_error": e.to_string() }),
            },
            Err(code) => json!({ "action": "seal", "sealed": false, "error": code }),
        },
        DpapiAction::Unseal => match std::fs::read(&path) {
            Ok(sealed) => {
                let t = Instant::now();
                let opened = unseal(&sealed, PARK);
                json!({
                    "action": "unseal",
                    "file": file,
                    "unsealed": opened.is_ok(),
                    "gave_back_the_dummy": opened.as_deref() == Ok(DUMMY),
                    "error": opened.err(),
                    "unseal_micros": t.elapsed().as_micros() as u64,
                })
            }
            Err(e) => json!({ "action": "unseal", "file_read_error": e.to_string() }),
        },
    };
    Report::ok("dpapi", logon, data)
}
