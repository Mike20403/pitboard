import Foundation
import PitboardKit
import Testing

@testable import PitboardApp

private func link(_ text: String) -> Result<SiteLink, LinkRefusal> {
    Result { try SiteLink(text) }.mapError { $0 as? LinkRefusal ?? .Unreadable }
}

private let accounts = status([
    account("work", signedIn: true, uuid: "w"), account("personal", uuid: "p"),
    account("main", of: "codex", signedIn: true, uuid: "m"),
    account("spare", of: "codex", uuid: "s"),
])

private func store(_ site: Site, _ uuid: String) -> UUID {
    storeID(site: site, accountUuid: uuid)
}

/// Every link from outside waits for a choice, and the choice offered is the account chosen
/// last for the site, else the one in use, else the first.
@Test func thePickerOffersTheSitesAccountsWithOneChosen() throws {
    let claude = try SiteLink("https://claude.ai/chat/x")
    guard
        case .choose(let shown, let offered, let chosen) = PickerState(
            .success(claude), status: accounts, problem: nil, lastChosen: [:])
    else {
        Issue.record("two claude.ai accounts to choose from")
        return
    }
    #expect(shown == claude)
    #expect(offered.map(\.label) == ["work", "personal"], "only claude.ai's accounts")
    #expect(chosen == store(.claude, "w"), "the account in use")

    let again = PickerState(
        .success(claude), status: accounts, problem: nil,
        lastChosen: [Site.claude.host: store(.claude, "p")])
    #expect(again == .choose(claude, offered, chosen: store(.claude, "p")))

    let stale = PickerState(
        .success(claude), status: accounts, problem: nil,
        lastChosen: [Site.claude.host: store(.chatGPT, "m")])
    #expect(
        stale == .choose(claude, offered, chosen: store(.claude, "w")),
        "an account of another site is never chosen")
}

@Test func thePickerSaysWhatStandsInTheWay() throws {
    let claude = try SiteLink("https://claude.ai/")
    #expect(
        PickerState(
            link("https://example.com/"), status: accounts, problem: nil, lastChosen: [:])
            == .refused(linkRefusalReason(refusal: .NotASite(host: "example.com"))))
    #expect(
        PickerState(.success(claude), status: nil, problem: nil, lastChosen: [:]) == .reading)
    #expect(
        PickerState(.success(claude), status: nil, problem: "No network.", lastChosen: [:])
            == .readFailed("No network."))
    #expect(
        PickerState(
            .success(claude), status: status([account("main", of: "codex", uuid: "m")]),
            problem: nil, lastChosen: [:])
            == .noAccount(claude))
}

/// A link is read as strictly as a stranger's, and a second one replaces the first.
@MainActor
@Test func theInboxTakesPitboardLinksOfItsOwnScheme() throws {
    let inbox = LinkInbox(scheme: "pitboard-debug")
    #expect(inbox.arrival == nil)

    inbox.receive(URL(string: "pitboard-debug://open?url=https%3A%2F%2Fclaude.ai%2Fnew")!)
    let first = try #require(inbox.arrival)
    #expect(try first.link.get().url == "https://claude.ai/new")

    inbox.receive(URL(string: "pitboard://open?url=https%3A%2F%2Fclaude.ai%2Fnew")!)
    let second = try #require(inbox.arrival)
    #expect(second != first, "shown as new")
    #expect(second.link == .failure(.Unreadable), "another build's scheme")

    inbox.dismiss()
    #expect(inbox.arrival == nil)
}

@MainActor
@Test func choosingAnAccountRemembersItForTheSite() throws {
    let inbox = LinkInbox(scheme: "pitboard")
    inbox.receive(URL(string: "pitboard://open?url=https%3A%2F%2Fchatgpt.com%2F")!)
    let spare = try #require(windowAccounts(in: accounts).first { $0.label == "spare" })
    inbox.chose(spare)
    #expect(inbox.arrival == nil)
    #expect(inbox.lastChosen == [Site.chatGPT.host: spare.store])
}
