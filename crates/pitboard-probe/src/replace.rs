//! The replace-loop block (VM C4) and its readers. W16 will retry a sharing violation and an
//! access-denied with a bounded backoff, and fail any other error at once; C4's counts set
//! that budget. The classifier here is the rule the loop measures with, and the record
//! format lets a reader tell a whole file from a torn one: every replacement writes one
//! record, and a reader that finds anything but a whole record has seen a torn read.

/// `ERROR_ACCESS_DENIED`.
pub const ERROR_ACCESS_DENIED: u32 = 5;
/// `ERROR_SHARING_VIOLATION`.
pub const ERROR_SHARING_VIOLATION: u32 = 32;

/// What to do with the result of one rename attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Done,
    /// A transient collision with a reader or a scanner; wait and try again.
    Retry,
    /// Anything else; fail at once.
    Fail,
}

/// Classify a rename result: `None` on success, else the `GetLastError` value.
pub fn classify(error: Option<u32>) -> Disposition {
    match error {
        None => Disposition::Done,
        Some(ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION) => Disposition::Retry,
        Some(_) => Disposition::Fail,
    }
}

/// The retry budget the loop measures with. It only bounds the measurement, so a
/// pathological loop still ends; W16's own budget comes from the counts.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub max_retries: u32,
    pub min_backoff_ms: u64,
    pub max_backoff_ms: u64,
}

impl Default for Budget {
    fn default() -> Self {
        Budget {
            max_retries: 50,
            min_backoff_ms: 1,
            max_backoff_ms: 50,
        }
    }
}

impl Budget {
    /// The backoff before attempt `attempt` (0-based), doubling from the minimum to the
    /// maximum.
    pub fn backoff_ms(&self, attempt: u32) -> u64 {
        let shifted = self
            .min_backoff_ms
            .saturating_mul(1u64.checked_shl(attempt).unwrap_or(u64::MAX));
        shifted.min(self.max_backoff_ms)
    }
}

/// The length of every record, near the size of a login file.
pub const RECORD_LEN: usize = 4096;
const MAGIC: &[u8; 4] = b"PBRL";

/// The record a replacement of round `round` writes: a magic, the round, a filler derived
/// from the round, and a checksum over all of it.
pub fn record(round: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(RECORD_LEN);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&round.to_le_bytes());
    let fill = (round % 251) as u8;
    out.resize(RECORD_LEN - 4, fill);
    let sum = checksum(&out);
    out.extend_from_slice(&sum.to_le_bytes());
    out
}

/// What a reader found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadCheck {
    /// A whole record of this round.
    Whole(u32),
    /// Nothing: the file was empty.
    Empty,
    /// Anything else: a short, long or mixed read.
    Torn,
}

impl ReadCheck {
    pub fn as_str(self) -> &'static str {
        match self {
            ReadCheck::Whole(_) => "whole",
            ReadCheck::Empty => "empty",
            ReadCheck::Torn => "torn",
        }
    }
}

/// Check what a reader read against the record format.
pub fn check(bytes: &[u8]) -> ReadCheck {
    if bytes.is_empty() {
        return ReadCheck::Empty;
    }
    if bytes.len() != RECORD_LEN || &bytes[..4] != MAGIC {
        return ReadCheck::Torn;
    }
    let (body, tail) = bytes.split_at(RECORD_LEN - 4);
    let sum = u32::from_le_bytes(tail.try_into().expect("four bytes"));
    if checksum(body) != sum {
        return ReadCheck::Torn;
    }
    let round = u32::from_le_bytes(body[4..8].try_into().expect("four bytes"));
    let fill = (round % 251) as u8;
    if body[8..].iter().any(|b| *b != fill) {
        return ReadCheck::Torn;
    }
    ReadCheck::Whole(round)
}

/// FNV-1a, enough to see a mixed read.
fn checksum(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5u32, |h, b| {
        (h ^ u32::from(*b)).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_is_done_and_sharing_or_access_denied_retry() {
        assert_eq!(classify(None), Disposition::Done);
        assert_eq!(classify(Some(ERROR_SHARING_VIOLATION)), Disposition::Retry);
        assert_eq!(classify(Some(ERROR_ACCESS_DENIED)), Disposition::Retry);
        for e in [2u32, 3, 183, 1920] {
            assert_eq!(classify(Some(e)), Disposition::Fail, "error {e}");
        }
    }

    #[test]
    fn the_backoff_doubles_and_is_capped() {
        let b = Budget {
            max_retries: 10,
            min_backoff_ms: 2,
            max_backoff_ms: 16,
        };
        assert_eq!(
            [0, 1, 2, 3, 4, 60].map(|a| b.backoff_ms(a)),
            [2, 4, 8, 16, 16, 16]
        );
    }

    #[test]
    fn a_whole_record_reads_back_with_its_round() {
        let r = record(1234);
        assert_eq!(r.len(), RECORD_LEN);
        assert_eq!(check(&r), ReadCheck::Whole(1234));
    }

    #[test]
    fn a_short_mixed_or_empty_read_is_told_apart() {
        assert_eq!(check(&[]), ReadCheck::Empty);
        let r = record(7);
        assert_eq!(check(&r[..100]), ReadCheck::Torn);
        let mut mixed = record(7);
        mixed[2000..].copy_from_slice(&record(8)[2000..]);
        assert_eq!(check(&mixed), ReadCheck::Torn);
        let mut flipped = record(7);
        flipped[10] ^= 1;
        assert_eq!(check(&flipped), ReadCheck::Torn);
    }
}
