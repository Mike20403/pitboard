import Foundation
import PitboardSites

/// Something an account window has to say, in the bar above its page, until it is
/// dismissed or the next one replaces it.
struct WindowNote: Equatable, Identifiable {
    enum Kind: Equatable {
        /// The window opened for the first time: how to sign in to its site.
        case signIn
        /// The policy stopped a page of Google's: its sign-in, its single sign-on, or the
        /// connection of one of its apps.
        case googleRefused
        /// The site sent a page to the browser without a click.
        case openedInBrowser
        /// A page asked for a link to another app, which a window never opens.
        case otherApp(scheme: String)
    }

    let kind: Kind
    let text: String
    /// Tells two notes of the same kind apart, so saying one again shows it again.
    let id = UUID()

    init(_ kind: Kind, for account: WindowAccount) {
        self.kind = kind
        let site = account.site
        switch kind {
        case .signIn:
            text =
                "Sign in to \(site.name) as \(account.email). Google’s sign-in does not work "
                + "inside apps. \(site.signInSteps)"
        case .googleRefused:
            text =
                "Google does not allow its pages inside apps, so Pitboard stopped it. Sign in "
                + "as \(account.email) another way. \(site.signInSteps) \(site.blockedServices)"
        case .openedInBrowser:
            text =
                "\(site.name) opened a page in your browser. Anything you connect there goes to "
                + "the \(site.name) account your browser is signed in to, which may not be "
                + "“\(account.label)”."
        case .otherApp(let scheme):
            text = "This window doesn’t open links to other apps, such as this \(scheme): link."
        }
    }

    static func == (lhs: WindowNote, rhs: WindowNote) -> Bool { lhs.id == rhs.id }
}
