//! Managed preferences: what a configuration profile an administrator installed forces for a
//! program, which macOS keeps apart from the person's own defaults.
//!
//! Read the way Codex 0.160.0 reads its own, in `codex-rs/config/src/loader/macos.rs`
//! (`codex_managed_preferences` in its register): a value counts only while
//! `CFPreferencesAppValueIsForced` says a profile forces it, and must be a string.

use crate::host::administered::Administered;
use core_foundation::base::{CFType, TCFType};
use core_foundation::string::CFString;
use core_foundation_sys::preferences::{CFPreferencesAppValueIsForced, CFPreferencesCopyAppValue};

/// The text a configuration profile forces `key` of `domain` to: unset where nothing forces
/// it, whatever the person's own defaults hold, and unreadable where what is forced is not a
/// string.
///
/// Whether it is forced is asked before the value is copied and again after, as Codex asks:
/// `CFPreferencesCopyAppValue` also searches the person's own defaults, and the calls are not
/// one snapshot, so a profile removed in between would hand back the person's value.
pub(super) fn forced(domain: &str, key: &str) -> Administered {
    let (key, application) = (CFString::new(key), CFString::new(domain));
    let is_forced = || {
        // SAFETY: both are CFStrings that live past the call, which only reads them.
        unsafe {
            CFPreferencesAppValueIsForced(
                key.as_concrete_TypeRef(),
                application.as_concrete_TypeRef(),
            ) != 0
        }
    };
    if !is_forced() {
        return Administered::Unset;
    }
    // SAFETY: both are CFStrings that live past the call, which only reads them.
    let copied = unsafe {
        CFPreferencesCopyAppValue(key.as_concrete_TypeRef(), application.as_concrete_TypeRef())
    };
    if copied.is_null() {
        return Administered::Unset;
    }
    // SAFETY: CFPreferencesCopyAppValue follows the create rule, so the value it returned is
    // this caller's to release, which a CFType made under the create rule does when dropped.
    let value = unsafe { CFType::wrap_under_create_rule(copied) };
    if !is_forced() {
        return Administered::Unset;
    }
    match value.downcast::<CFString>() {
        Some(text) => Administered::Set(text.to_string()),
        None => Administered::Unreadable("is forced, and is not a string".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A domain no profile forces anything in reads as unset, from the real preferences: the
    /// person's own defaults never count as an administrator's.
    #[test]
    fn nothing_forced_is_unset() {
        assert_eq!(
            forced(
                "com.usepitboard.Pitboard.never-managed",
                "config_toml_base64"
            ),
            Administered::Unset
        );
    }
}
