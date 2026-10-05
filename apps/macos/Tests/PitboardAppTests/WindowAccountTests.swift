import Foundation
import PitboardKit
import Testing

@testable import PitboardApp

private func only(_ account: Account) -> WindowAccount? {
    windowAccounts(in: status([account])).first
}

// MARK: - Which store

/// A window's store is derived from its account, so it can never change once released: the
/// sweep would delete every window's data.
@Test func anEnrolledClaudeAccountHasAStoreDerivedFromItsAccountId() {
    func claude(_ id: String) -> UUID { storeID(site: .claude, accountUuid: id) }
    #expect(
        claude("4f3c2a10-8b7e-4d2a-9c1e-5a6b7c8d9e0f").uuidString.lowercased()
            == "7e15c34f-69ec-55b4-9542-f1c1fe3d7085")
    #expect(
        claude("dana@work.example").uuidString.lowercased()
            == "323d12fb-2c52-55b5-baec-df74fb60bc24",
        "the fixture's account id")
    #expect(
        claude("4F3C2A10-8B7E-4D2A-9C1E-5A6B7C8D9E0F")
            == claude("4f3c2a10-8b7e-4d2a-9c1e-5a6b7c8d9e0f"))
    let id = claude("anything")
    let bytes = withUnsafeBytes(of: id.uuid) { Array($0) }
    #expect(bytes[6] >> 4 == 5, "version 5")
    #expect(bytes[8] >> 6 == 0b10, "the RFC's variant")
    #expect(id != UUID(uuid: (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)))
    #expect(storeNamespace.uuidString.lowercased() == "674b09f3-8d37-4e48-a361-5af2a6856773")

    let work = only(account("work", uuid: "4f3c2a10-8b7e-4d2a-9c1e-5a6b7c8d9e0f"))
    #expect(work?.store.uuidString.lowercased() == "7e15c34f-69ec-55b4-9542-f1c1fe3d7085")
    #expect(work?.site == .claude)
}

/// A Codex account's window is on chatgpt.com, and its store is named apart from any Claude
/// Code account's, even one with the same id.
@Test func anEnrolledCodexAccountHasAChatGPTWindowOfItsOwn() throws {
    let main = try #require(only(account("main", of: "codex", uuid: "team_user-1")))
    #expect(main.site == .chatGPT)
    #expect(main.label == "main")
    #expect(
        main.store.uuidString.lowercased() == "8ff9e0e6-a7e2-53e2-a594-6e53ddadd38a",
        "the name hashed is codex:<account id>, which never changes once shipped")
    #expect(
        storeID(site: .chatGPT, accountUuid: "dana")
            != storeID(site: .claude, accountUuid: "dana"))
}

@Test func aRenameKeepsTheStore() {
    let before = only(account("work", uuid: "4f3c2a10"))
    let after = only(account("office", uuid: "4f3c2a10"))
    #expect(before != nil)
    #expect(before?.store == after?.store)
}

@Test func unnamedUnplacedAndUnknownToolsHaveNoWindow() {
    #expect(only(account(nil, signedIn: true, uuid: "dana")) == nil)
    #expect(only(account(nil, of: "codex", signedIn: true, uuid: "dana")) == nil)
    #expect(only(unplaced(of: "claude", signedIn: true)) == nil)
    #expect(only(unplaced(of: "codex", signedIn: true)) == nil, "an API key login")
    #expect(only(account("work", uuid: "")) == nil)
    #expect(only(account("x", of: "gemini")) == nil, "a tool with no site")
    #expect(windowAccounts(in: nil).isEmpty)
}

// MARK: - Titles and menus

/// The Window menu shows a window's title alone, so one label on two sites says which.
@Test func aLabelOnTwoSitesSaysWhichSiteInTheTitle() {
    let windows = windowAccounts(
        in: status([
            account("work", uuid: "a"), account("work", of: "codex", uuid: "b"),
            account("home", uuid: "c"),
        ]))
    #expect(windows.map(\.title) == ["work (claude.ai)", "work (chatgpt.com)", "home"])
    #expect(windows.map(\.label) == ["work", "work", "home"])
}

@Test func aSiteWithOneAccountIsOneItemAndWithSeveralASubmenu() {
    let menus = siteMenus(
        in: status([
            account("work", uuid: "a"), account("main", of: "codex", uuid: "b"),
            account("spare", of: "codex", uuid: "c"), account(nil, uuid: "d"),
        ]))
    #expect(menus.map(\.title) == ["Open claude.ai as work", "Open chatgpt.com"])
    guard case .several(let site, let accounts) = menus.last else {
        Issue.record("chatgpt.com has two accounts")
        return
    }
    #expect(site == .chatGPT)
    #expect(accounts.map(\.label) == ["main", "spare"])
    #expect(siteMenus(in: status([account(nil, uuid: "d")])).isEmpty)
}

/// Forgetting an account that has a window deletes what that window keeps too, and a person
/// deciding should know.
@Test func theForgetAlertNamesTheWindowsData() {
    let work = account("work", uuid: "a")
    #expect(
        forgetMessage(for: work, in: status([work]))
            == "Pitboard deletes the login it parked for this account, and everything its "
            + "claude.ai window keeps on this Mac, its sign-in included. Using it again needs a "
            + "sign-in in your browser.")
    let api = unplaced(of: "codex")
    #expect(
        forgetMessage(for: api, in: status([api]))
            == "Pitboard deletes the login it parked for this account. Using it again needs a "
            + "sign-in in your browser.")
}
