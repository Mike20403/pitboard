import Foundation
import PitboardKit
import PitboardSites

/// The link the Share extension handed over, waiting for the person to choose which
/// account's window opens it.
///
/// Anything on the Mac can open a Pitboard link, so nothing opens by itself: every link waits
/// here until somebody chooses an account and clicks Open, even with one account.
@MainActor
@Observable
final class LinkInbox {
    /// A link that arrived, or why what arrived is not one, each told apart from the one
    /// before so the picker shows a second arrival of the same link as new.
    struct Arrival: Equatable, Identifiable {
        let link: Result<SiteLink, LinkRefusal>
        let id = UUID()

        static func == (lhs: Arrival, rhs: Arrival) -> Bool { lhs.id == rhs.id }
    }

    /// What arrived and is waiting for an account, or nil.
    private(set) var arrival: Arrival?
    /// The account last chosen for each site, chosen first next time, as Safari opens a link
    /// in the profile used last. Kept for as long as the app runs.
    private(set) var lastChosen: [Site.ID: UUID] = [:]
    @ObservationIgnored private let scheme: String

    /// An inbox for Pitboard links of `scheme`, this build's.
    init(scheme: String) {
        self.scheme = scheme
    }

    /// Takes a Pitboard link the app was asked to open, replacing one still waiting.
    func receive(_ url: URL) {
        arrival = Arrival(
            link: Result { () throws(LinkRefusal) in
                try Handoff.link(in: url, scheme: scheme)
            })
    }

    /// The person chose `account` for the link waiting.
    func chose(_ account: WindowAccount) {
        lastChosen[account.site.id] = account.store
        arrival = nil
    }

    /// The person closed the picker without choosing.
    func dismiss() {
        arrival = nil
    }
}

/// What the account picker shows for a link, from the link and the accounts as last read.
enum PickerState: Equatable {
    /// The accounts are being read for the first time.
    case reading
    /// The accounts could not be read, for the reason given.
    case readFailed(String)
    /// What arrived is not opened, for the reason given.
    case refused(String)
    /// No enrolled account has a window on the link's site.
    case noAccount(SiteLink)
    /// The accounts with a window on the link's site, and the one chosen until the person
    /// chooses another.
    case choose(SiteLink, [WindowAccount], chosen: UUID)

    init(
        _ arrived: Result<SiteLink, LinkRefusal>, status: Status?, problem: String?,
        lastChosen: [Site.ID: UUID]
    ) {
        let link: SiteLink
        switch arrived {
        case .failure(let refusal):
            self = .refused(refusal.localizedDescription)
            return
        case .success(let accepted):
            link = accepted
        }
        guard let status else {
            self = problem.map(PickerState.readFailed) ?? .reading
            return
        }
        let accounts = windowAccounts(in: status).filter { $0.site == link.site }
        let mine = Set(accounts.map(\.store))
        // Chosen last for this site, else the account in use, else the first.
        let chosen =
            lastChosen[link.site.id].flatMap { mine.contains($0) ? $0 : nil }
            ?? accounts.first(where: \.inUse)?.store ?? accounts.first?.store
        guard let chosen else {
            self = .noAccount(link)
            return
        }
        self = .choose(link, accounts, chosen: chosen)
    }
}
