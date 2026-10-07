//! The logon session a block ran in. DPAPI and Credential Manager are expected to behave
//! differently in an interactive logon, which holds the password-derived keys, than in a
//! network logon such as a key-authenticated OpenSSH session, which holds no password. So
//! every report names the logon it ran in, and the blocks that touch either never refuse on
//! the logon: they make the call and record what happened beside the logon type, so block M
//! measures Windows rather than the probe.
//!
//! The naming is pure and tested on every system; the Windows reader in [`crate::win`] hands
//! it the `SECURITY_LOGON_TYPE` it read.

/// The kinds of logon that matter here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// At the console: Interactive 2, Unlock 7, CachedInteractive 11, CachedUnlock 13.
    Interactive,
    /// RDP: RemoteInteractive 10, CachedRemoteInteractive 12.
    RemoteInteractive,
    /// Network 3, which a key-authenticated OpenSSH session is expected to be: no password.
    Network,
    /// NetworkCleartext 8: a network logon given a password, as password-authenticated
    /// OpenSSH may be.
    NetworkCleartext,
    /// Batch 4, such as a task that runs whether or not the user is signed in.
    Batch,
    /// Service 5.
    Service,
    /// NewCredentials 9 (`runas /netonly`).
    NewCredentials,
    /// A type the probe did not expect.
    Other,
    /// The logon could not be read.
    Unknown,
}

/// The logon this process runs in, by Windows's `SECURITY_LOGON_TYPE` number when it was
/// read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogonSession {
    pub raw: Option<u32>,
}

impl LogonSession {
    pub fn from_security_logon_type(ty: u32) -> Self {
        LogonSession { raw: Some(ty) }
    }

    pub fn unknown() -> Self {
        LogonSession { raw: None }
    }

    pub fn kind(self) -> Kind {
        match self.raw {
            None => Kind::Unknown,
            Some(2 | 7 | 11 | 13) => Kind::Interactive,
            Some(10 | 12) => Kind::RemoteInteractive,
            Some(3) => Kind::Network,
            Some(8) => Kind::NetworkCleartext,
            Some(4) => Kind::Batch,
            Some(5) => Kind::Service,
            Some(9) => Kind::NewCredentials,
            Some(_) => Kind::Other,
        }
    }

    /// A stable word for the report.
    pub fn as_str(self) -> String {
        match self.kind() {
            Kind::Interactive => "interactive".into(),
            Kind::RemoteInteractive => "remote_interactive".into(),
            Kind::Network => "network".into(),
            Kind::NetworkCleartext => "network_cleartext".into(),
            Kind::Batch => "batch".into(),
            Kind::Service => "service".into(),
            Kind::NewCredentials => "new_credentials".into(),
            Kind::Other => format!("other_{}", self.raw.unwrap_or_default()),
            Kind::Unknown => "unknown".into(),
        }
    }

    /// Whether this logon is expected to reach the user's DPAPI master key and Credential
    /// Manager: yes at the console and over RDP, no in a plain network logon, and not known
    /// for the rest. It is an expectation printed beside a result, never a reason to refuse.
    pub fn expects_user_keys(self) -> Option<bool> {
        match self.kind() {
            Kind::Interactive | Kind::RemoteInteractive => Some(true),
            Kind::Network => Some(false),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(ty: u32) -> LogonSession {
        LogonSession::from_security_logon_type(ty)
    }

    #[test]
    fn the_console_kinds_are_interactive_cached_ones_included() {
        for ty in [2, 7, 11, 13] {
            assert_eq!(of(ty).kind(), Kind::Interactive, "type {ty}");
            assert_eq!(of(ty).expects_user_keys(), Some(true));
        }
    }

    #[test]
    fn rdp_and_cached_rdp_are_remote_interactive() {
        for ty in [10, 12] {
            assert_eq!(of(ty).kind(), Kind::RemoteInteractive, "type {ty}");
            assert_eq!(of(ty).expects_user_keys(), Some(true));
        }
    }

    #[test]
    fn a_network_logon_is_expected_not_to_reach_the_keys() {
        assert_eq!(of(3).kind(), Kind::Network);
        assert_eq!(of(3).expects_user_keys(), Some(false));
    }

    #[test]
    fn cleartext_batch_and_service_are_left_to_measure() {
        // A password-authenticated SSH session and a stored-password task may well reach the
        // keys; the probe records what happens rather than guessing.
        for ty in [8, 4, 5, 9] {
            assert_eq!(of(ty).expects_user_keys(), None, "type {ty}");
        }
        assert_eq!(of(8).as_str(), "network_cleartext");
        assert_eq!(of(4).as_str(), "batch");
    }

    #[test]
    fn an_unexpected_or_unread_type_is_carried_through() {
        assert_eq!(of(99).as_str(), "other_99");
        assert_eq!(of(99).expects_user_keys(), None);
        assert_eq!(LogonSession::unknown().as_str(), "unknown");
        assert_eq!(LogonSession::unknown().expects_user_keys(), None);
    }
}
