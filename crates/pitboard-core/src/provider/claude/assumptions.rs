//! Every fact about Claude Code that Pitboard stands on, named and dated.
//!
//! The keychain item's name and how the slot is hashed from a directory, the five keys a
//! logout deletes, the write lock and its constants, the one write that skips the lock, the
//! 4032-byte ceiling on a `security` command, the thirty seconds a session caches a
//! credential for. All of it was read out of one build.
//!
//! None of it transfers. The next provider's equivalents have to be read out of its own
//! build the same way, into a list of its own, dated on its own schedule.
//!
//! See [`crate::assumptions`] for what an entry means and how a probe reads one.

use crate::assumptions::{Assumption, Platform};

/// The build every entry below was read from, unless it says otherwise.
pub const VERIFIED_AGAINST: &str = "2.1.284";

/// Facts about the keychain, which only the macOS build has code for. The Linux build has
/// none, so read from it they report the keychain gone, on every build from 2.1.278 on.
const MACOS_ONLY: &[&str] = &[
    "credential_service_name",
    "keychain_account_name",
    "keychain_write_route",
    "keychain_absence_codes",
];

/// Facts about Linux, read from the Linux build.
const LINUX_ONLY: &[&str] = &["no_keyring_off_macos"];

/// The systems whose builds a fact is read from: its own system's for the facts named
/// above, both for the rest.
pub fn read_on(name: &str) -> &'static [Platform] {
    if MACOS_ONLY.contains(&name) {
        &[Platform::MacOs]
    } else if LINUX_ONLY.contains(&name) {
        &[Platform::Linux]
    } else {
        Platform::ALL
    }
}

pub const ASSUMPTIONS: &[Assumption] = &[
    Assumption {
        name: "credential_service_name",
        fact: "the login lives in a keychain item named `Claude Code${OAUTH_FILE_SUFFIX}-credentials`, \
               with `-<first 8 hex of sha256 of the NFC-normalised storage directory>` appended for \
               any directory but the default",
        read_from: "the secure storage module's service name and slot derivation",
        verified_against: VERIFIED_AGAINST,
        depends: "slot.rs, and every read and write of the live credential",
        probe: &["-credentials", "OAUTH_FILE_SUFFIX"],
        absent: &[],
    },
    Assumption {
        name: "keychain_account_name",
        fact: "the keychain account is $USER when it matches /^[a-zA-Z0-9._-]+$/, and \
               `claude-code-user` when it does not",
        read_from: "the secure storage module's account name",
        verified_against: VERIFIED_AGAINST,
        depends: "slot::account_name",
        probe: &["claude-code-user"],
        absent: &[],
    },
    Assumption {
        name: "live_chain_order",
        fact: "on macOS the login is read from the keychain first and a plaintext \
               `.credentials.json` second, and a write moves it to the file only when the \
               keychain write fails for good; on Linux the file is the only store. The \
               successor backend replaces the fallback half and only for a caller that hands a \
               backend in, which an ordinary `claude` does not",
        read_from: "the keychain-with-plaintext-fallback store and `getSecureStorage`",
        verified_against: VERIFIED_AGAINST,
        depends: "store::resolve and everything that reads or writes through it",
        probe: &[".credentials.json", "-with-", "-fallback"],
        absent: &[],
    },
    Assumption {
        name: "keychain_write_route",
        fact: "a write gives `security -i` the line `add-generic-password -U -a \"<account>\" \
               -s \"<service>\" -X \"<hex>\"` on stdin while that line is at most 4032 bytes, \
               and the same command on the argument line above that; it never refuses or \
               splits a login. Each call has a 2 second timeout. A failed write is transient, \
               and skips the plaintext fallback, when it timed out or, from 2.1.281, when it \
               exited 36 after this process had seen the item; any other failure moves the \
               login to `.credentials.json`",
        read_from: "the keychain backend's update path and the fallback wrapper around it",
        verified_against: VERIFIED_AGAINST,
        depends: "host::macos::keychain::MAX_COMMAND_BYTES and the write path",
        probe: &[
            "exceeds security -i stdin limit; using argv",
            "primary_transient_skip_fallback",
            "keychain_locked_skip_fallback",
        ],
        absent: &[],
    },
    Assumption {
        name: "keychain_absence_codes",
        fact: "`find-generic-password -a <account> -w -s <service>`, with a 2 second timeout: \
               exit 0 with output is the login; exit 0 with nothing, exit 44, or output that is \
               not JSON is absent. Exit 36, a locked keychain, is absent to an ordinary read, a \
               failed read to a write once this process has seen the item \
               (`failureIfTransient`, from 2.1.281), and a failed read outright only when a \
               caller asks for that. Any other exit is a failed read. `show-keychain-info` \
               exiting 36 only adds an unlock hint. Pitboard reads 36 as unreadable on \
               purpose, the strict end of that",
        read_from: "the keychain backend's read path",
        verified_against: VERIFIED_AGAINST,
        depends: "host::macos::keychain::classify",
        probe: &[
            "failureIfTransient",
            "[keychain] readAsync failed; not caching a null",
            "show-keychain-info",
        ],
        absent: &[],
    },
    Assumption {
        name: "write_lock",
        fact: "every credential write takes proper-lockfile's directory lock at \
               `<storage dir>/.storage-write`, stale 15000ms, ten retries, 100ms to 1000ms of \
               backoff, and re-reads the credential inside the lock before changing it. A \
               re-read that fails abandons the write (`read_failed_skip_write`); from 2.1.281 a \
               locked keychain counts as a failed re-read once this process has seen the item, \
               where before it read as empty",
        read_from: "the secure storage module's write wrapper",
        verified_against: VERIFIED_AGAINST,
        depends: "lock.rs and the whole switch",
        probe: &[
            ".storage-write",
            "[secureStorage] write lock compromised: ",
            "read_failed_skip_write",
            "failureIfTransient",
        ],
        absent: &[],
    },
    Assumption {
        name: "logout_skips_the_lock",
        fact: "`/logout` retries for its own 7.5 seconds and then deletes the credential with \
               no lock held at all",
        read_from: "the secure storage module's already-locked escape hatch",
        verified_against: VERIFIED_AGAINST,
        depends: "the slot re-read in switch, which exists for this",
        probe: &["secureStorage.READ_FAILED"],
        absent: &[],
    },
    Assumption {
        name: "account_scoped_keys",
        fact: "signing in again deletes claudeAiOauth, organizationUuid, trustedDeviceToken, \
               enterpriseGateway and designOauth, and `/logout` clears the whole document but \
               writes back `coworkRemoteDevice`, so those five belong to the account",
        read_from: "the re-login prune and the logout path",
        verified_against: VERIFIED_AGAINST,
        depends: "switch::ACCOUNT_SCOPED, and what a park holds",
        probe: &[
            "claudeAiOauth",
            "organizationUuid",
            "trustedDeviceToken",
            "enterpriseGateway",
            "designOauth",
        ],
        absent: &[],
    },
    Assumption {
        name: "credential_cache",
        fact: "a running session serves the credential from a 30 second cache, so a swap is \
               picked up within about 33 seconds. The 30 seconds are unchanged in 2.1.284; its \
               re-checks after 1, 3 and 10 seconds, behind `tengu_streamed_thimble` from \
               2.1.281, can only shorten that",
        read_from: "the keychain backend's cache, and measured against a running session",
        // The 33 seconds were measured against a running 2.1.278; nothing later was run.
        verified_against: "2.1.278",
        depends: "switch::ADOPTION_CEILING_SECONDS",
        probe: &[],
        absent: &[],
    },
    Assumption {
        name: "config_file_location",
        fact: "a legacy `<config dir>/.config.json` wins when present; otherwise \
               `<$CLAUDE_CONFIG_DIR or $HOME>/.claude<suffix>.json`",
        read_from: "the config path resolution",
        verified_against: VERIFIED_AGAINST,
        depends: "claude::config_file",
        probe: &[".config.json", "CLAUDE_CONFIG_DIR"],
        absent: &[],
    },
    Assumption {
        name: "oauth_client",
        fact: "every login Claude Code stores was issued to client 9d1c250a-e61b-44d9-88ed-5944d1962f5e, \
               and a refresh answer without a refresh-token lifetime keeps the one it had",
        read_from: "the OAuth client id and the token refresh path",
        verified_against: VERIFIED_AGAINST,
        depends: "api::CLIENT_ID and park::renewed",
        probe: &["9d1c250a-e61b-44d9-88ed-5944d1962f5e"],
        absent: &[],
    },
    Assumption {
        name: "supervisor_daemon",
        fact: "a supervisor daemon outlives the session that started it, refreshes the login on \
               a timer of roughly eight hours, records itself in `<config dir>/daemon.lock`, and \
               writes through the same lock as everything else",
        read_from: "the daemon's auth scheduler and its lock file",
        verified_against: VERIFIED_AGAINST,
        depends: "daemon.rs, and the lock discipline the switch relies on",
        probe: &[
            "daemon.lock",
            "daemon.status.json",
            "auth: scheduling proactive refresh in ",
        ],
        absent: &[],
    },
    Assumption {
        name: "no_keyring_off_macos",
        fact: "Claude Code's code has no Secret Service, libsecret, gnome-keyring or KWallet \
               backend. Its storage backends are `keychain`, `plaintext` and `windows-credman`, \
               the last behind the `tengu_windows_credman` flag, and on Linux it uses \
               `plaintext` alone, so the login is the file. The `libsecret` in the Linux binary \
               belongs to the Bun runtime it ships in, behind `Bun.secrets`, which Claude \
               Code's code never calls",
        read_from: "the secure storage module's backend list and `getSecureStorage`, and the \
                    whole build searched for every Linux keyring name",
        verified_against: VERIFIED_AGAINST,
        depends: "host::linux, and Pitboard's claim that a parked login on Linux is no \
                  less protected than the live one",
        probe: &[
            "tengu_windows_credman",
            "CLAUDE_CODE_FORCE_WINDOWS_CREDMAN",
            r#"["keychain","plaintext","windows-credman"]"#,
        ],
        absent: &[
            "Bun.secrets",
            "org.freedesktop.secrets",
            "gnome-keyring",
            "SecretService",
        ],
    },
    Assumption {
        name: "sign_in_output",
        fact: "`claude auth login` writes `Opening browser to sign in…`, then `If the browser \
               didn't open, visit: <address>`, then `Paste code here if prompted > ` with no \
               newline, all to stdout and before it opens the browser, and from then on reads \
               a pasted `<code>#<state>` line from stdin. The address is the manual one: \
               `https`, on claude.com for a claude.ai login and platform.claude.com for a \
               Console one, coming back to platform.claude.com's page that shows the code. The \
               browser it opens goes to another address, which comes back to the loopback. \
               The address goes through a hyperlink helper, which writes it bare, or, where \
               its check says the terminal takes hyperlinks, as an OSC 8 hyperlink: \
               `ESC ] 8 ; ;`, the address, BEL, the address again as the link's text, bright \
               blue where colour is on, then `ESC ] 8 ; ;` and BEL. Piped, the check says yes \
               when `FORCE_HYPERLINK` is set to anything but 0 or nothing, which decides it \
               when set; otherwise when `NETLIFY` is set, `TERM_PROGRAM` or `LC_TERMINAL` is \
               ghostty, Hyper, kitty, alacritty, iTerm.app, iTerm2 or WarpTerminal, \
               `TERMINAL_EMULATOR` is JetBrains-JediTerm, `WT_SESSION` is set outside tmux, \
               `TERM_PROGRAM` is tmux 3.4 or later, or `TERM` contains kitty. A terminal \
               attached to a background session answers for the check instead, and \
               `auth login` has none",
        read_from: "the `auth login` command's OAuth flow and `startOAuthFlow`, the authorize \
                    address builder and its constants, the hyperlink helper the address is \
                    printed through with `assumeSupport`, and the supports-hyperlinks check \
                    it asks",
        // Read from a newer build than the rest of this register.
        verified_against: "2.1.289",
        depends: "provider::claude::engine's read_sign_in, provider::printed, which reads \
                  the hyperlink, and the address and code field the app's sign-in sheet \
                  offers",
        probe: &[
            "If the browser didn't open, visit: ",
            "Paste code here if prompted > ",
            r#"CLAUDE_AI_AUTHORIZE_URL:"https://"#,
            r#"CONSOLE_AUTHORIZE_URL:"https://"#,
            r#"MANUAL_REDIRECT_URL:"https://"#,
            // A sign-in address going through the hyperlink helper with `assumeSupport`, from
            // after the helper's name, which the minifier chooses. `auth login` makes one of
            // the three such calls in 2.1.289; 2.1.110 has none.
            "{assumeSupport:!0})}",
        ],
        absent: &[],
    },
    Assumption {
        name: "sign_in_takes_another_code",
        fact: "`claude auth login` reads each line typed back while it waits, trims it and \
               splits it at `#`. A line that is not `<code>#<state>` with both halves it \
               refuses by writing `Invalid code. Please make sure the full code was copied.` \
               and a newline to stderr, and it goes on reading in the same process, printing \
               no prompt again, so another line can be typed back. The first line with both \
               halves it takes, whatever its state half says. After that it still refuses a \
               line without both halves, and ignores one with them; a code the token exchange \
               then refuses ends the sign-in with `Login failed: ` on stderr and exit status 1",
        read_from: "the `auth login` command's handler for each line read from stdin, which \
                    trims and splits the line and refuses it before it looks at whether a code \
                    was taken, returns after the refusal and leaves the line reader open, and \
                    the OAuth service's `waitForAuthorizationCode` and \
                    `handleManualAuthCodeInput`, which takes a code only while its resolver is \
                    set, hands on the code half alone and clears the resolver",
        // Read on 2026-10-05 from the Windows arm64 build's sources and the macOS and Linux
        // builds of 2.1.289, all of which hold every literal below; 2.1.110 holds none.
        verified_against: "2.1.289",
        depends: "provider::claude::engine's refused_code, and the app model offering the \
                  code field again once Claude Code has refused a code",
        probe: &[
            // The refusal, then the handler returning rather than ending the process.
            "Invalid code. Please make sure the full code was copied.\n`);return}",
            "if(this.manualAuthCodeResolver)this.authorizationCodeReceived=!0",
        ],
        absent: &[],
    },
    Assumption {
        name: "install_places",
        fact: "the native installer puts its launcher at `~/.local/bin/claude`, and says so \
               when that directory is not on `PATH`; a global npm install puts `claude` in \
               npm's global `bin`, which is in Homebrew's prefix, `/opt/homebrew` or \
               `/usr/local`, or in `/usr/local` where nodejs.org's installer put Node, and \
               Claude Code tells it is one by `/node_modules/@anthropic-ai/` in the path the \
               running program resolves to; one whose path runs through a Homebrew \
               `Caskroom` is the Homebrew cask's",
        read_from: "the installation-method detection, which returns `npm-global` for an \
                    `execPath` holding `/node_modules/@anthropic-ai/` and lists \
                    `/opt/homebrew/bin` and `/usr/local/bin` among npm's places, \
                    `getHomebrewCaskName`, and the native install's PATH advice",
        // Read on 2026-10-05 from the macOS builds of 2.1.283 and 2.1.289 and the Linux build
        // of 2.1.289, all of which hold every literal below.
        verified_against: "2.1.289",
        depends: "claude::paths::install_places, where an app looks for `claude` after the \
                  login shell's PATH",
        probe: &[
            "Native installation exists but ~/.local/bin is not in your PATH",
            ".local/bin/claude",
            "/node_modules/@anthropic-ai/",
            "Detected Homebrew cask installation: ",
        ],
        absent: &[],
    },
    Assumption {
        name: "plaintext_credential_mode",
        fact: "the plaintext credential is written and then chmod'd to 0600, in its storage \
               directory, under the fixed name `.credentials.json`",
        read_from: "the plaintext backend's write path, which chmods 384 after writing",
        verified_against: VERIFIED_AGAINST,
        depends: "store::file, slot::CRED_FILE, and atomic::Perms::Secret matching what \
                  Claude Code itself writes",
        probe: &[
            ".credentials.json",
            "Warning: Storing credentials in plaintext.",
        ],
        absent: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// A name in either list that is not a fact would leave the fact it meant read from
    /// both systems, and the Linux build would report it gone.
    #[test]
    fn every_fact_named_for_one_system_is_in_the_register() {
        for name in MACOS_ONLY.iter().chain(LINUX_ONLY) {
            assert!(
                ASSUMPTIONS.iter().any(|a| a.name == *name),
                "{name} is not a fact"
            );
        }
    }
}
