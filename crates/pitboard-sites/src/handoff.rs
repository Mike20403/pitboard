//! The Pitboard link, `<scheme>://open?url=<link>`, in which a link reaches the app: from the
//! macOS Share extension, and from anything else on the machine that opens one.
//!
//! Anything can open a Pitboard link, so the app reads one as strictly as it reads a link typed
//! by a stranger, and opens nothing until the person chooses an account. The format is the
//! contract between the Share extension and the app it came in, which a release may be updated
//! around separately, so it does not change.

use crate::address::{self, is_unreserved, push_escape};
use crate::link::{LinkRefusal, SiteLink};

/// The Pitboard link of `scheme` that asks the app to open `link`, with everything but the
/// unreserved characters percent-encoded. That is a subset of what JavaScript's
/// `encodeURIComponent` leaves bare, so a link either of them builds reads back the same.
pub fn pitboard_link(link: &SiteLink, scheme: &str) -> String {
    let mut out = format!("{scheme}://open?url=");
    for b in link.url().bytes() {
        if is_unreserved(b) {
            out.push(char::from(b));
        } else {
            push_escape(&mut out, b);
        }
    }
    out
}

/// The link a Pitboard link of `scheme` carries, or why it carries none this build opens.
///
/// Strict on purpose. A Pitboard link written without encoding the link it carries,
/// `pitboard://open?url=https://claude.ai/x?a=1&b=2#c`, reads as a shorter link, another item
/// and a fragment, and opening the first part would open the wrong page without a word. The
/// scheme and the host `open` are matched in any case; nothing may name a user or a port, the
/// path is empty or `/`, there is no fragment, and the query is one item, `url`, which is not
/// empty. What it carries is then checked as any link from outside is.
pub fn read_pitboard_link(text: &str, scheme: &str) -> Result<SiteLink, LinkRefusal> {
    let parts = address::parse(text).ok_or(LinkRefusal::Unreadable)?;
    let authority = parts.authority.as_ref().ok_or(LinkRefusal::Unreadable)?;
    let path = parts.decoded_path();
    let readable = parts.scheme.to_lowercase() == scheme.to_lowercase()
        && authority
            .host
            .as_deref()
            .is_some_and(|host| host.to_lowercase() == "open")
        && (path.is_empty() || path == "/")
        && !authority.user_info
        && authority.port.is_none()
        && parts.fragment.is_none();
    if !readable {
        return Err(LinkRefusal::Unreadable);
    }
    let items = address::query_items(parts.query.as_deref().ok_or(LinkRefusal::Unreadable)?);
    match items.as_slice() {
        [(name, Some(carried))] if name == "url" && !carried.is_empty() => SiteLink::parse(carried),
        _ => Err(LinkRefusal::Unreadable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site::CLAUDE;

    fn link(text: &str) -> SiteLink {
        SiteLink::parse(text).expect("a site's link")
    }

    /// The link a Pitboard link `text` of `pitboard` carries, as its address, or why it
    /// carries none.
    fn carried(text: &str) -> Result<String, LinkRefusal> {
        read_pitboard_link(text, "pitboard").map(|link| link.url().to_owned())
    }

    // The vectors of the Swift HandoffTests, each with the answer it had there.

    /// The format is the contract between the Share extension and the app it came in, so one
    /// link is pinned exactly: everything but the unreserved characters is encoded, a space as
    /// `%20` and a plus as `%2B`, as JavaScript's `encodeURIComponent` encodes them.
    #[test]
    fn a_pitboard_link_carries_one_encoded_link() {
        assert_eq!(
            pitboard_link(
                &link("https://claude.ai/public/artifacts/0e5a?x=1&y=a%20b+c#frag"),
                "pitboard"
            ),
            "pitboard://open?url=https%3A%2F%2Fclaude.ai%2Fpublic%2Fartifacts%2F0e5a\
             %3Fx%3D1%26y%3Da%2520b%2Bc%23frag"
        );
        assert!(
            pitboard_link(&link("https://claude.ai/"), "pitboard-debug")
                .starts_with("pitboard-debug://")
        );
    }

    #[test]
    fn handing_a_link_over_gives_the_same_link() {
        let long = format!("https://claude.ai/{}", "a".repeat(8100));
        for original in [
            "https://claude.ai/new",
            "https://claude.ai/public/artifacts/0e5a?x=1&y=a%20b+c#frag",
            "https://claude.ai/chat/x?q=100%25&r=a=b",
            "https://claude.ai/x?a='()!*'",
            "https://claude.ai/chat/%C3%BCber?q=%E6%97%A5",
            "https://claude.ai/x#one#two",
            "https://chatgpt.com/c/abc",
            &long,
        ] {
            let shared = link(original);
            assert_eq!(
                carried(&pitboard_link(&shared, "pitboard")).as_deref(),
                Ok(shared.url()),
                "{original}"
            );
        }
    }

    /// A link is read as Foundation reads it: a second `#` in a fragment is escaped, which is
    /// the same page.
    #[test]
    fn a_link_is_handed_over_as_foundation_reads_it() {
        assert_eq!(
            link("https://claude.ai/x#one#two").url(),
            "https://claude.ai/x#one%23two"
        );
    }

    /// An alias is handed over as the site's own host, which is what the window opens.
    #[test]
    fn an_alias_is_handed_over_as_the_sites_host() {
        let built = pitboard_link(&link("https://chat.openai.com/c/x"), "pitboard");
        assert_eq!(carried(&built).as_deref(), Ok("https://chatgpt.com/c/x"));
    }

    /// Anything can open a Pitboard link, so one carrying a link no site opens is refused for
    /// the same reason the link itself would be.
    #[test]
    fn what_a_pitboard_link_carries_is_checked_as_a_link_from_outside() {
        assert_eq!(
            carried("pitboard://open?url=https%3A%2F%2Fexample.com%2F"),
            Err(LinkRefusal::NotASite {
                host: Some("example.com".into())
            })
        );
        assert_eq!(
            carried("pitboard://open?url=https%3A%2F%2Fclaude.ai%2Fmagic-link%23a"),
            Err(LinkRefusal::SignInLink(&CLAUDE))
        );
        assert_eq!(
            carried(&format!("pitboard://open?url={}", "a".repeat(8200))),
            Err(LinkRefusal::TooLong)
        );
    }

    #[test]
    fn a_link_for_another_scheme_or_request_is_unreadable() {
        let inside = "url=https%3A%2F%2Fclaude.ai%2F";
        for text in [
            format!("pitboard-debug://open?{inside}"),
            format!("https://open?{inside}"),
            format!("pitboard:open?{inside}"),
            format!("pitboard://someone@open?{inside}"),
            format!("pitboard://open:8080?{inside}"),
            format!("pitboard://close?{inside}"),
            format!("pitboard://open/x?{inside}"),
        ] {
            assert_eq!(carried(&text), Err(LinkRefusal::Unreadable), "{text}");
        }
        assert_eq!(
            carried(&format!("PITBOARD://OPEN/?{inside}")).as_deref(),
            Ok("https://claude.ai/")
        );
        assert_eq!(
            read_pitboard_link(&format!("pitboard-debug://open?{inside}"), "pitboard-debug")
                .map(|link| link.url().to_owned())
                .as_deref(),
            Ok("https://claude.ai/")
        );
    }

    /// A link written by hand without encoding reads as a shorter link, another item and a
    /// fragment. Opening the first part would open the wrong page without a word.
    #[test]
    fn an_ambiguous_link_is_unreadable() {
        for text in [
            "pitboard://open?url=https://claude.ai/x?a=1&b=2#c",
            "pitboard://open?url=https://claude.ai/x#c",
            "pitboard://open?url=a&url=b",
            "pitboard://open?url=https%3A%2F%2Fclaude.ai%2F&also=1",
            "pitboard://open?url=",
            "pitboard://open?link=https%3A%2F%2Fclaude.ai%2F",
            "pitboard://open?",
            "pitboard://open",
        ] {
            assert_eq!(carried(text), Err(LinkRefusal::Unreadable), "{text}");
        }
    }

    // Where Foundation and the WHATWG URL standard read a Pitboard link differently, it is
    // read as Foundation read it. Each answer here is the Swift Handoff's, measured on macOS
    // 27; the Windows app reads one from its command line with the same rule.

    /// A `+` is a `+`: a space only in a form, which a Pitboard link is not.
    #[test]
    fn a_plus_stays_a_plus() {
        assert_eq!(
            carried("pitboard://open?url=https%3A%2F%2Fclaude.ai%2F%3Fa%3Db+c").as_deref(),
            Ok("https://claude.ai/?a=b+c")
        );
        assert_eq!(
            carried("pitboard://open?url=+claude.ai"),
            Err(LinkRefusal::NoLink)
        );
    }

    #[test]
    fn a_pitboard_link_is_read_as_foundation_reads_it() {
        let opens = |text: &str| carried(text).map_err(|refusal| format!("{text}: {refusal:?}"));
        for text in [
            "pitboard://open?%75rl=https%3A%2F%2Fclaude.ai%2F",
            "pitboard://OPEN?url=claude.ai",
            "pitboard://op%65n?url=claude.ai",
            "pitboard://open:?url=claude.ai",
            "pitboard://open?url=%20claude.ai%20",
        ] {
            assert_eq!(opens(text).as_deref(), Ok("https://claude.ai/"));
        }
        assert_eq!(
            carried("pitboard://open?url=claude.ai%23x").as_deref(),
            Ok("https://claude.ai/#x")
        );
        for text in [
            "pitboard://open?URL=https%3A%2F%2Fclaude.ai%2F",
            "pitboard://open?url=https%3A%2F%2Fclaude.ai%2F%FF",
            "pitboard://open?url",
            "pitboard://open?=x",
            "pitboard://open//?url=claude.ai",
            "pitboard://open?url=claude.ai&",
            "pitboard://open?&url=claude.ai",
            "pitboard://@open?url=claude.ai",
            "pitboard://open?url=claude.ai#",
            "pitboard:///open?url=claude.ai",
            "pitboard://OPEN.?url=claude.ai",
            "pitboard://op en?url=claude.ai",
        ] {
            assert_eq!(carried(text), Err(LinkRefusal::Unreadable), "{text}");
        }
        for text in [
            "pitboard://open?url=%",
            "pitboard://open?url==claude.ai",
            "pitboard://open?url=a=b",
        ] {
            assert_eq!(carried(text), Err(LinkRefusal::NoLink), "{text}");
        }
    }

    /// A user or a port is refused whatever it holds, and so is a path whose bytes are not
    /// UTF-8. Foundation reads none of the three where it cannot decode or hold it, so the
    /// Swift Handoff read each of these as `pitboard://open?url=claude.ai`.
    #[test]
    fn a_user_a_port_or_a_path_counts_whatever_bytes_it_holds() {
        for text in [
            "pitboard://%FF@open?url=claude.ai",
            "pitboard://open:99999999999999999999?url=claude.ai",
            "pitboard://open/%FF?url=claude.ai",
        ] {
            assert_eq!(carried(text), Err(LinkRefusal::Unreadable), "{text}");
        }
    }

    /// What a Pitboard link carries is compared by its text, not by grapheme cluster, as any
    /// link from outside is. The Swift Handoff, measured on macOS 27, opened the sign-in link
    /// and found no link in the bare host.
    #[test]
    fn a_carried_link_is_compared_by_its_scalars() {
        assert_eq!(
            carried("pitboard://open?url=https%3A%2F%2Fclaude.ai%2Fmagic-link%2F%CC%81"),
            Err(LinkRefusal::SignInLink(&CLAUDE))
        );
        assert_eq!(
            carried("pitboard://open?url=claude.ai%2Fmagic-link%2F%E2%80%8D"),
            Err(LinkRefusal::SignInLink(&CLAUDE))
        );
        assert_eq!(
            carried("pitboard://open?url=chat.com%2F%CC%81").as_deref(),
            Ok("https://chatgpt.com/%CC%81")
        );
    }
}
