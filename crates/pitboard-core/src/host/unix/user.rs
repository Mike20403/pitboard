//! The person signed in, the POSIX way.

use std::ffi::{CStr, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

/// The name in the passwd database for this user id, as Node's `os.userInfo()` reads it.
pub(crate) fn login_name() -> Option<String> {
    entry(|passwd| {
        text(passwd, passwd.pw_name)?
            .to_str()
            .ok()
            .map(str::to_owned)
    })
}

/// This user's home directory, as the passwd database names it: where a home is when the
/// environment names none, as Foundation finds it for an app.
pub(crate) fn home() -> Option<PathBuf> {
    entry(|passwd| path(passwd, passwd.pw_dir))
}

/// What `read` takes from this user's passwd entry, while the buffer its strings live in is
/// still there. `None` where there is no entry.
fn entry<T>(read: impl FnOnce(&libc::passwd) -> Option<T>) -> Option<T> {
    grown(
        first_size(),
        |passwd, buffer, found| {
            // SAFETY: getpwuid_r writes into buffers this call owns and reports through
            // `found`, which is null when there is no entry. The strings it points at live in
            // `buffer`, which `grown` keeps until `read` is done with the entry.
            unsafe {
                libc::getpwuid_r(
                    libc::getuid(),
                    passwd,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    found,
                )
            }
        },
        read,
    )
}

/// The size of buffer the system says an entry fits in, or 4096 bytes where it says none.
fn first_size() -> usize {
    // SAFETY: sysconf only reads a limit of the system's.
    let said = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    usize::try_from(said)
        .ok()
        .filter(|&size| size > 0)
        .unwrap_or(4096)
}

/// The largest buffer an entry is looked up with, so a lookup that never fits ends.
const LARGEST: usize = 1 << 20;

/// What `read` takes from the entry `lookup` finds, first with a buffer of `size` bytes and
/// then, each time it answers ERANGE, which says the entry did not fit, with one twice as
/// large, up to [`LARGEST`]. `None` where there is no entry, or the lookup failed.
fn grown<T>(
    mut size: usize,
    mut lookup: impl FnMut(
        &mut libc::passwd,
        &mut [libc::c_char],
        &mut *mut libc::passwd,
    ) -> libc::c_int,
    read: impl FnOnce(&libc::passwd) -> Option<T>,
) -> Option<T> {
    loop {
        let mut buffer: Vec<libc::c_char> = vec![0; size];
        // SAFETY: an all-zero `passwd` is a valid one to hand to getpwuid_r, which fills it.
        let mut passwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        match lookup(&mut passwd, &mut buffer, &mut found) {
            // The entry's strings point into `buffer`, which lives until `read` returns.
            0 if !found.is_null() => return read(&passwd),
            libc::ERANGE if size < LARGEST => size = size.saturating_mul(2).min(LARGEST),
            _ => return None,
        }
    }
}

/// One of `entry`'s strings, for as long as `entry` is borrowed, which the elided lifetime
/// ties it to. `None` where it is null.
fn text(entry: &libc::passwd, field: *const libc::c_char) -> Option<&CStr> {
    let _ = entry;
    // SAFETY: a field of an entry getpwuid_r reported is null or points into its buffer,
    // NUL-terminated, and that buffer outlives every borrow of the entry.
    (!field.is_null()).then(|| unsafe { CStr::from_ptr(field) })
}

/// One of `entry`'s strings as a path, whatever bytes it holds. `None` where it is null or
/// empty.
fn path(entry: &libc::passwd, field: *const libc::c_char) -> Option<PathBuf> {
    text(entry, field)
        .filter(|named| !named.is_empty())
        .map(|named| PathBuf::from(OsStr::from_bytes(named.to_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// A lookup standing in for getpwuid_r, whose entry has `home` for its home directory and
    /// which answers ERANGE, as getpwuid_r does, while the buffer is too small for it. It
    /// writes down the size of every buffer it is given.
    fn needing<'a>(
        home: &'a str,
        sizes: &'a mut Vec<usize>,
    ) -> impl FnMut(&mut libc::passwd, &mut [libc::c_char], &mut *mut libc::passwd) -> libc::c_int + 'a
    {
        move |passwd, buffer, found| {
            sizes.push(buffer.len());
            let bytes = [home.as_bytes(), b"\0"].concat();
            if buffer.len() < bytes.len() {
                return libc::ERANGE;
            }
            for (to, from) in buffer.iter_mut().zip(&bytes) {
                *to = libc::c_char::from_ne_bytes([*from]);
            }
            passwd.pw_dir = buffer.as_mut_ptr();
            *found = passwd;
            0
        }
    }

    /// An entry too large for the first buffer is looked up again with a larger one, as
    /// getpwuid_r asks by answering ERANGE, rather than taken for no entry: a home kept in a
    /// directory service can be longer than any one size given first.
    #[test]
    fn an_entry_too_large_for_the_first_buffer_is_looked_up_again() {
        let home = format!("/Users/{}", "x".repeat(5000));
        let mut sizes = Vec::new();
        let found = grown(4096, needing(&home, &mut sizes), |passwd| {
            path(passwd, passwd.pw_dir)
        });
        assert!(found.as_deref() == Some(Path::new(&home)), "{sizes:?}");
        assert_eq!(sizes, [4096, 8192]);
    }

    /// A lookup that never fits ends, at a buffer of a mebibyte.
    #[test]
    fn a_lookup_that_never_fits_ends() {
        let home = "x".repeat(LARGEST * 2);
        let mut sizes = Vec::new();
        let found = grown(4096, needing(&home, &mut sizes), |_| Some(()));
        assert_eq!(found, None);
        assert_eq!(sizes.first(), Some(&4096));
        assert_eq!(sizes.last(), Some(&LARGEST));
        assert_eq!(sizes.len(), 9, "{sizes:?}");
    }

    /// Every account the tests run as has a name and a home named from the root.
    #[test]
    fn this_account_has_a_name_and_a_home() {
        assert!(login_name().is_some_and(|name| !name.is_empty()));
        assert!(
            home().is_some_and(|home| home.is_absolute()),
            "{:?}",
            home()
        );
    }
}
