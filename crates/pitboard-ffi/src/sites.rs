//! The sites an account's window opens, and which links from outside it opens, as
//! `pitboard-sites` decides them, for both apps. Each type is declared here, apart from that
//! crate's own, since the C# generator cannot use a type from another crate.
//!
//! None reads a clock, a file or the keychain: each answers at once, on any thread.

use pitboard_core::host::{OS, Os};
use std::fmt;

/// A website Pitboard opens in an account's window: whose accounts it serves, which hosts are
/// its own, where its sign-in goes, and what its window says about signing in.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct Site {
    /// The site's own host, in lower case: where its windows start and what links to it are
    /// opened on.
    pub host: String,
    /// The site as a sentence names it: its host, so a window on chatgpt.com is never
    /// confused with OpenAI's ChatGPT app.
    pub name: String,
    /// The tool whose enrolled accounts get this site's windows, as a `Tool`'s `code`.
    pub provider: String,
    /// Other hosts that send a browser to `host` with the path kept, taken as `host`. Each is
    /// matched exactly: a subdomain of one is not one.
    pub aliases: Vec<String>,
    /// Every host whose links the site opens: `host`, then `aliases`.
    pub hosts: Vec<String>,
    /// Hosts the site's own sign-in leaves for and comes back from. A window's page loads
    /// them as it loads the site, and one asked for in a new window opens in a sign-in window
    /// that shares the account's store. A link from outside to one opens nothing.
    pub sign_in_hosts: Vec<String>,
    /// Hosts a window refuses as its page or a new window, and never hands to the browser.
    pub blocked_hosts: Vec<String>,
    /// Paths that sign in whoever a link belongs to, as the lower-case segments they start
    /// with.
    pub sign_in_paths: Vec<Vec<String>>,
    /// Hashed into the store id of each of the site's windows. It never changes once a site
    /// has shipped: a change would leave every window of the site without its data.
    pub store_name: String,
    /// How to sign in to the site in a window, as the steps its sign-in page shows, and what
    /// of them a window on this system cannot do.
    pub sign_in_steps: String,
    /// What the blocked hosts leave unusable in the site's window.
    pub blocked_services: String,
}

impl From<&pitboard_sites::Site> for Site {
    fn from(site: &pitboard_sites::Site) -> Site {
        let owned = |hosts: &[&str]| hosts.iter().map(|&host| host.to_owned()).collect();
        Site {
            host: site.host.into(),
            name: site.name().into(),
            provider: site.provider.into(),
            aliases: owned(site.aliases),
            hosts: site.hosts().map(str::to_owned).collect(),
            sign_in_hosts: owned(site.sign_in_hosts),
            blocked_hosts: owned(site.blocked_hosts),
            sign_in_paths: site.sign_in_paths.iter().map(|path| owned(path)).collect(),
            store_name: site.store_name.into(),
            sign_in_steps: sign_in_steps(OS, site),
            blocked_services: site.blocked_services.into(),
        }
    }
}

/// How to sign in to `site` in a window on `os`: the steps its sign-in page shows, then a
/// passkey's, where the site offers one and a window on `os` cannot use it.
fn sign_in_steps(os: Os, site: &pitboard_sites::Site) -> String {
    let passkeys = match os {
        // WebKit gives a page a passkey only in an app macOS lets act as a browser, and
        // Pitboard is not one.
        Os::MacOs => false,
        // No app runs on Linux, and nothing was measured there, so none is promised.
        Os::Linux => false,
    };
    if site.passkeys && !passkeys {
        format!(
            "{} Passkeys do not work in this window.",
            site.sign_in_steps
        )
    } else {
        site.sign_in_steps.to_owned()
    }
}

/// A link one of the sites opens, checked: the site, and the address its window loads.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct SiteLink {
    pub site: Site,
    /// The link on the site's own host over `https`, with its path, query and fragment.
    pub url: String,
}

impl From<pitboard_sites::SiteLink> for SiteLink {
    fn from(link: pitboard_sites::SiteLink) -> SiteLink {
        SiteLink {
            site: link.site().into(),
            url: link.url().into(),
        }
    }
}

/// Why a link from outside is not opened. `link_refusal_reason` says it in a sentence.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Error)]
pub enum LinkRefusal {
    /// Nothing that reads as a link to a web page.
    NoLink,
    /// A link to a host none of the sites is, with the host when there is one to name.
    NotASite { host: Option<String> },
    /// A site's sign-in link, which would sign a window in as whoever it is for. The site is
    /// named by its host, as `Site.host` names it.
    SignInLink { host: String },
    /// Longer than any of the sites' links is.
    TooLong,
    /// A Pitboard link this version does not read: another scheme, another request, or a
    /// link inside it that was not encoded.
    Unreadable,
}

impl From<pitboard_sites::LinkRefusal> for LinkRefusal {
    fn from(refusal: pitboard_sites::LinkRefusal) -> LinkRefusal {
        use pitboard_sites::LinkRefusal as Sites;
        match refusal {
            Sites::NoLink => LinkRefusal::NoLink,
            Sites::NotASite { host } => LinkRefusal::NotASite { host },
            Sites::SignInLink(site) => LinkRefusal::SignInLink {
                host: site.host.into(),
            },
            Sites::TooLong => LinkRefusal::TooLong,
            Sites::Unreadable => LinkRefusal::Unreadable,
        }
    }
}

impl From<&LinkRefusal> for pitboard_sites::LinkRefusal {
    fn from(refusal: &LinkRefusal) -> pitboard_sites::LinkRefusal {
        use pitboard_sites::LinkRefusal as Sites;
        match refusal {
            LinkRefusal::NoLink => Sites::NoLink,
            LinkRefusal::NotASite { host } => Sites::NotASite { host: host.clone() },
            // Only a site's own sign-in link is refused as one; a refusal made up elsewhere
            // may name a host no site has, and is said as a link to that host.
            LinkRefusal::SignInLink { host } => pitboard_sites::Site::serving(host).map_or_else(
                || Sites::NotASite {
                    host: Some(host.clone()),
                },
                Sites::SignInLink,
            ),
            LinkRefusal::TooLong => Sites::TooLong,
            LinkRefusal::Unreadable => Sites::Unreadable,
        }
    }
}

impl fmt::Display for LinkRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        pitboard_sites::LinkRefusal::from(self).fmt(f)
    }
}

impl std::error::Error for LinkRefusal {}

/// How a list of names is joined in a sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Conjunction {
    /// "claude.ai and chatgpt.com".
    And,
    /// "claude.ai or chatgpt.com".
    Or,
}

/// Every site, in the order a listing shows them.
#[uniffi::export]
pub fn sites() -> Vec<Site> {
    pitboard_sites::ALL
        .iter()
        .map(|&site| site.into())
        .collect()
}

/// The sites whose windows the accounts of `provider`, a `Tool`'s `code`, get, in the order
/// a listing shows them. None for a tool that has no site.
#[uniffi::export]
pub fn sites_for(provider: String) -> Vec<Site> {
    pitboard_sites::Site::of_provider(&provider)
        .map(Site::from)
        .collect()
}

/// The sites' names as a sentence lists them: "claude.ai and chatgpt.com".
#[uniffi::export]
pub fn site_names(conjunction: Conjunction) -> String {
    pitboard_sites::Site::names(match conjunction {
        Conjunction::And => pitboard_sites::Conjunction::And,
        Conjunction::Or => pitboard_sites::Conjunction::Or,
    })
}

/// `text` as a link one of the sites opens, or why it is not one. Spaces around it are
/// dropped, a bare host of a site is taken as `https`, and `http` as `https`. Only a site's
/// own host or one of its aliases is accepted, with no port and no user information, and
/// never a site's sign-in link.
#[uniffi::export]
pub fn site_link(text: String) -> Result<SiteLink, LinkRefusal> {
    Ok(pitboard_sites::SiteLink::parse(&text)?.into())
}

/// The link a Pitboard link of `scheme`, `<scheme>://open?url=<link>`, carries, or why it
/// carries none this build opens. Strict, since anything on the machine can open one: the
/// link it carries must be encoded, and is then checked as `site_link` checks one.
#[uniffi::export]
pub fn read_pitboard_link(text: String, scheme: String) -> Result<SiteLink, LinkRefusal> {
    Ok(pitboard_sites::read_pitboard_link(&text, &scheme)?.into())
}

/// The Pitboard link of `scheme` that asks the app to open `text`, once `text` is checked as
/// `site_link` checks a link: the link it reads as, with everything but the unreserved
/// characters percent-encoded.
#[uniffi::export]
pub fn pitboard_link(text: String, scheme: String) -> Result<String, LinkRefusal> {
    let link = pitboard_sites::SiteLink::parse(&text)?;
    Ok(pitboard_sites::pitboard_link(&link, &scheme))
}

/// Why a link from outside is not opened, in the sentence the Share extension and the
/// account picker show.
#[uniffi::export]
pub fn link_refusal_reason(refusal: LinkRefusal) -> String {
    refusal.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitboard_core::provider::ProviderId;

    /// Each site serves the accounts of a tool the core knows, by the code every `provider`
    /// field names it with: pitboard-sites holds nothing of the core to check that itself.
    #[test]
    fn every_site_serves_a_tool_the_core_knows() {
        for site in sites() {
            let tool = ProviderId::parse(&site.provider).expect("a tool the core knows");
            assert_eq!(tool.code(), site.provider);
        }
        let claude = sites_for("claude".into());
        assert_eq!(
            claude.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["claude.ai"]
        );
        assert_eq!(
            sites_for("codex".into())[0].hosts,
            [
                "chatgpt.com",
                "chat.openai.com",
                "www.chatgpt.com",
                "chat.com"
            ]
        );
        assert!(sites_for("gemini".into()).is_empty());
        assert_eq!(site_names(Conjunction::Or), "claude.ai or chatgpt.com");
    }

    /// The steps chatgpt.com's window gives, as `Site.swift` said them before they were Rust:
    /// a passkey works in no window on a Mac. Whether one does is the system's to say, so
    /// pitboard-sites holds the steps without it.
    #[test]
    fn a_window_says_a_passkey_does_not_work_where_it_does_not() {
        for os in [Os::MacOs, Os::Linux] {
            assert_eq!(
                sign_in_steps(os, &pitboard_sites::CHATGPT),
                "Enter your email address, then its password or the code chatgpt.com emails \
                 you, or click Continue with Microsoft or Continue with Apple. Passkeys do not \
                 work in this window."
            );
            assert_eq!(
                sign_in_steps(os, &pitboard_sites::CLAUDE),
                "Click Continue with email, open the email on your phone, tap its link, then \
                 enter here the code claude.ai shows there.",
                "claude.ai's steps say nothing of a passkey"
            );
        }
        assert_eq!(
            sites_for("codex".into())[0].sign_in_steps,
            sign_in_steps(OS, &pitboard_sites::CHATGPT)
        );
    }

    /// The bindings check a link and read a Pitboard link as pitboard-sites does, and say a
    /// refusal in its sentence.
    #[test]
    fn a_link_crosses_the_bindings_as_pitboard_sites_reads_it() {
        let link = site_link(" chat.openai.com/c/x?y=1#z ".into()).expect("a site's link");
        assert_eq!(link.url, "https://chatgpt.com/c/x?y=1#z");
        assert_eq!(link.site.host, "chatgpt.com");
        let written = pitboard_link(link.url.clone(), "pitboard".into()).expect("a site's link");
        assert_eq!(
            written,
            "pitboard://open?url=https%3A%2F%2Fchatgpt.com%2Fc%2Fx%3Fy%3D1%23z"
        );
        assert_eq!(read_pitboard_link(written, "pitboard".into()), Ok(link));

        let refused = site_link("https://claude.ai/magic-link#a".into()).expect_err("sign-in");
        assert_eq!(
            refused,
            LinkRefusal::SignInLink {
                host: "claude.ai".into()
            }
        );
        assert_eq!(
            link_refusal_reason(refused),
            "Pitboard doesn’t open claude.ai sign-in links from outside: one would sign the \
             window in as whoever the link belongs to. Sign in inside the account’s claude.ai \
             window."
        );
        assert_eq!(
            read_pitboard_link("pitboard://open?url=a&url=b".into(), "pitboard".into()),
            Err(LinkRefusal::Unreadable)
        );
        assert_eq!(
            link_refusal_reason(LinkRefusal::NotASite {
                host: Some("example.com".into())
            }),
            "Pitboard opens claude.ai and chatgpt.com links only. This link is on example.com."
        );
        assert_eq!(
            link_refusal_reason(LinkRefusal::SignInLink {
                host: "example.com".into()
            }),
            "Pitboard opens claude.ai and chatgpt.com links only. This link is on example.com.",
            "a host no site has"
        );
    }
}
