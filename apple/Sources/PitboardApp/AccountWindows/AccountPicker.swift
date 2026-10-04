import PitboardSites
import SwiftUI

/// Asks which account's window opens a link shared from another app, in the manner of Photos'
/// Choose Library: a list, Cancel, and a default button named for what it does.
///
/// Every link from outside waits here until somebody chooses, even with one account, so no
/// page can open an account's window by sharing a link with pitboard.
struct AccountPicker: View {
    /// The scene's id, named once so the scene and the code that opens it cannot drift apart.
    static let id = "open-link"

    let windows: AccountWindows
    @State private var chosen: UUID?
    /// Whether Open answers yet. Each link waits a moment before it can be opened, so a Return
    /// typed for another app as the picker came forward opens nothing.
    @State private var armed = false
    @Environment(\.openWindow) private var openWindow
    @Environment(\.dismissWindow) private var dismissWindow

    /// What the picker shows for the link waiting, if one is.
    private var state: PickerState? {
        windows.inbox.arrival.map { arrival in
            PickerState(
                arrival.link, status: windows.model.status, problem: windows.model.problem,
                lastChosen: windows.inbox.lastChosen)
        }
    }

    var body: some View {
        let state = state
        let choosing: Bool = {
            if case .choose? = state { return true }
            return false
        }()
        VStack(alignment: .leading, spacing: 12) {
            if let state {
                content(state)
            } else {
                ContentUnavailableView(
                    "No Link to Open", systemImage: Symbol.site,
                    description: Text(
                        "Share a \(Site.names(.or)) page with pitboard from your browser’s "
                            + "Share menu to open it as one of your accounts."))
                HStack {
                    Spacer()
                    Button("Close") { dismissWindow() }
                        .keyboardShortcut(.defaultAction)
                }
            }
        }
        .padding(20)
        .frame(width: 440)
        .appWindow(windows.presence)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("account-picker")
        // Armed from when the accounts to choose from appear, whether the link has just
        // arrived or the accounts were read only now.
        .task(id: Arming(arrival: windows.inbox.arrival?.id, choosing: choosing)) {
            armed = false
            try? await Task.sleep(for: Self.armingDelay)
            armed = !Task.isCancelled
        }
        .onChange(of: windows.inbox.arrival?.id) { before, now in
            if now == nil, before != nil {
                dismissWindow()
            } else if now != nil {
                // A link from another app: the picker comes to the front over it, with the
                // choice made afresh for the new link.
                chosen = nil
                windows.presence.comeForward()
            }
        }
        .onDisappear { windows.inbox.dismiss() }
    }

    @ViewBuilder private func content(_ state: PickerState) -> some View {
        switch state {
        case .reading:
            ProgressView("Reading accounts…")
                .frame(maxWidth: .infinity)
            buttons { cancelButton }
        case .readFailed(let problem):
            Text("Couldn’t Read Accounts").font(.headline)
            Text(problem).explanatory()
            buttons {
                cancelButton
                Button("Try Again") { Task { await windows.model.refresh(asked: true) } }
                    .keyboardShortcut(.defaultAction)
            }
        case .refused(let reason):
            Text("Can’t Open This Link").font(.headline)
            Text(reason).explanatory()
            buttons {
                Button("OK") { windows.inbox.dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
        case .noAccount(let link):
            Text("No \(link.site.name) Account").font(.headline)
            LinkLine(link: link)
            Text(
                "None of the accounts pitboard has opens \(link.site.name). Add one, and this "
                    + "link waits here until you choose it."
            )
            .explanatory()
            HStack {
                Button("Open in Browser") {
                    NSWorkspace.shared.open(link.url)
                    windows.inbox.dismiss()
                }
                Spacer()
                cancelButton
                Button("Add Account…") { windows.model.present(.add(provider: link.site.tool)) }
                    .keyboardShortcut(.defaultAction)
            }
        case .choose(let link, let accounts, let preferred):
            Text("Open this \(link.site.name) link as:").font(.headline)
            LinkLine(link: link)
            let selection = Binding {
                chosen ?? preferred
            } set: {
                chosen = $0
            }
            List(accounts, selection: selection) { account in
                AccountChoice(account: account, open: windows.sessions[account.store] != nil)
                    .tag(account.store)
            }
            .frame(minHeight: 96, idealHeight: 160, maxHeight: 360)
            .contextMenu(forSelectionType: UUID.self) { _ in
            } primaryAction: { stores in
                if armed, let store = stores.first { open(link, as: store, from: accounts) }
            }
            .accessibilityIdentifier("picker.accounts")
            buttons {
                cancelButton
                Button("Open") { open(link, as: chosen ?? preferred, from: accounts) }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!armed)
            }
        }
    }

    private var cancelButton: some View {
        Button("Cancel", role: .cancel) { windows.inbox.dismiss() }
            .keyboardShortcut(.cancelAction)
    }

    private func buttons<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        HStack {
            Spacer()
            content()
        }
    }

    /// How long a link waits before it can be opened.
    static let armingDelay = Duration.milliseconds(750)

    /// What restarts the wait: a link arriving, and the accounts to choose from appearing.
    private struct Arming: Equatable {
        let arrival: UUID?
        let choosing: Bool
    }

    private func open(_ link: SiteLink, as store: UUID, from accounts: [WindowAccount]) {
        guard let account = accounts.first(where: { $0.store == store }) else { return }
        windows.open(link, as: account)
        windows.inbox.chose(account)
        windows.presence.activate()
        openWindow(id: AccountWindowScene.id, value: account.store)
    }
}

/// The link waiting, on one line without its scheme, the whole of it in its help.
private struct LinkLine: View {
    let link: SiteLink

    var body: some View {
        let url = link.url.absoluteString
        Text(url.hasPrefix("https://") ? String(url.dropFirst("https://".count)) : url)
            .font(.callout.monospaced())
            .lineLimit(1)
            .truncationMode(.middle)
            .textSelection(.enabled)
            .help(url)
            .accessibilityLabel("Link")
            .accessibilityValue(url)
    }
}

/// An account the link can open as: its label and email, and whether its window is open,
/// where the link replaces what the window shows.
private struct AccountChoice: View {
    let account: WindowAccount
    let open: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(account.label)
            Text(open ? "\(account.email), window open" : account.email)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("picker.account.\(account.label)")
    }
}
