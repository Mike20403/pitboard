import CryptoKit
import Foundation
import PitboardKit

/// An enrolled account that has a window on a site: what the menus, the account picker and
/// the window itself say about it, and the store that keeps its sign-in.
struct WindowAccount: Hashable, Identifiable {
    let site: Site
    let label: String
    let email: String
    /// The window's store, which is also the window's value: one window per account.
    let store: UUID
    let inUse: Bool
    /// What the window is titled: the label, or the label and the site when an account on
    /// another site has the same label, so the Window menu tells the two apart.
    let title: String

    var id: UUID { store }
}

/// Pitboard's namespace for the windows' stores. It never changes, and neither does the name
/// hashed in it: a change would leave every window without its data, and the next sweep would
/// delete that data. A golden test pins both.
let storeNamespace = UUID(uuidString: "674b09f3-8d37-4e48-a361-5af2a6856773")!

/// A version 5 UUID (RFC 9562, SHA-1) of `<store name>:<account id>` in Pitboard's namespace,
/// for the window of the account `accountUuid` names on `site`.
///
/// Derived rather than stored, so there is nothing to keep in step with the accounts: a
/// rename keeps the window's sign-in, and a store no enrolled account derives is one the app
/// can find. A version 5 UUID is never the nil UUID, which WebKit refuses as an identifier.
/// The account id is not secret; the hash keeps it out of folder names and saved windows.
func storeID(site: Site, accountUuid: String) -> UUID {
    let namespace = withUnsafeBytes(of: storeNamespace.uuid) { Array($0) }
    let name = Array("\(site.storeName):\(accountUuid.lowercased())".utf8)
    var bytes = Array(Insecure.SHA1.hash(data: namespace + name).prefix(16))
    bytes[6] = (bytes[6] & 0x0F) | 0x50
    bytes[8] = (bytes[8] & 0x3F) | 0x80
    return UUID(
        uuid: (
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
        ))
}

/// Every enrolled account's window, in the core's order and then the sites' order: what the
/// menus, the shortcut menu and the picker offer, so they cannot disagree.
///
/// An account has a window when its tool has a site, it has a label and an account id, and
/// Pitboard can place it. A login Pitboard has no name for, or a Codex API key, has none.
func windowAccounts(in status: Status?) -> [WindowAccount] {
    let eligible = (status?.accounts ?? []).filter { account in
        account.label != nil && !account.unplaced && !account.accountUuid.isEmpty
    }
    let placed = eligible.flatMap { account in
        sitesFor(provider: account.provider).map { (site: $0, account: account) }
    }
    return placed.map { site, account in
        let label = account.label ?? ""
        let shared = placed.contains { $0.site != site && $0.account.label == label }
        return WindowAccount(
            site: site, label: label, email: account.email,
            store: storeID(site: site, accountUuid: account.accountUuid),
            inUse: account.signedIn, title: shared ? "\(label) (\(site.name))" : label)
    }
}

/// Every window `account` has, one for each site of its tool. The site is checked as well
/// as the store: two tools' accounts can share an account id.
func siteWindows(of account: Account, in status: Status?) -> [WindowAccount] {
    windowAccounts(in: status).filter { window in
        sitesFor(provider: account.provider).contains(window.site)
            && window.store == storeID(site: window.site, accountUuid: account.accountUuid)
    }
}

/// How a menu offers one site's windows: an item naming the only account, and a submenu of
/// titles for several. A submenu of one item is what the platform asks not to make.
enum SiteMenu: Hashable, Identifiable {
    case one(WindowAccount)
    case several(Site, [WindowAccount])

    var site: Site {
        switch self {
        case .one(let account): account.site
        case .several(let site, _): site
        }
    }

    var id: String { site.host }

    /// The item's title, or the submenu's.
    var title: String {
        switch self {
        case .one(let account): "Open \(account.site.name) as \(account.label)"
        case .several(let site, _): "Open \(site.name)"
        }
    }
}

/// One menu entry per site that has an account with a window, in the sites' order.
func siteMenus(in status: Status?) -> [SiteMenu] {
    let accounts = windowAccounts(in: status)
    return sites().compactMap { site in
        let mine = accounts.filter { $0.site == site }
        switch mine.count {
        case 0: return nil
        case 1: return .one(mine[0])
        default: return .several(site, mine)
        }
    }
}

/// What the forget alert says. Forgetting an account that has a window deletes what that
/// window keeps too, and a person deciding should know.
func forgetMessage(for account: Account, in status: Status?) -> String {
    let sites = siteWindows(of: account, in: status).map(\.site.name)
    guard !sites.isEmpty else {
        return "Pitboard deletes the login it parked for this account. Using it again needs a "
            + "sign-in in your browser."
    }
    return "Pitboard deletes the login it parked for this account, and everything its "
        + "\(sites.formatted(.list(type: .and))) window keeps on this Mac, its sign-in "
        + "included. Using it again needs a sign-in in your browser."
}
