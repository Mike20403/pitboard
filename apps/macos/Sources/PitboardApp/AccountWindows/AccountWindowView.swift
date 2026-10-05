import PitboardKit
import SwiftUI

/// An account's window on its site: titled with the account, the page below a toolbar to go
/// back, forward and reload, and what the window has to say above it.
///
/// A window for one site and one account, not a browser: no address field and no tabs of its
/// own. Links that leave the site go to the browser.
struct AccountWindowView: View {
    let windows: AccountWindows
    /// The account's store, the window's value; nil for a window macOS restored without one.
    let store: UUID?

    @State private var session: WebSession?
    @Environment(\.dismissWindow) private var dismissWindow

    private var account: WindowAccount? { store.flatMap(windows.account) }

    var body: some View {
        // A read found the account forgotten, or macOS restored a window for an account no
        // longer enrolled, or for none at all.
        let gone = store == nil || (windows.model.status != nil && account == nil)
        content
            .frame(minWidth: 480, minHeight: 360)
            .appWindow(windows.presence)
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("account-window")
            .task(id: account?.id) {
                guard let account else { return }
                session = windows.session(for: account)
            }
            .onChange(of: session?.page.url) { _, url in
                if let account { windows.remember(url, of: account) }
            }
            .onChange(of: gone, initial: true) { _, gone in
                if gone { dismissWindow() }
            }
            .onDisappear {
                if let store { windows.closed(store) }
            }
    }

    @ViewBuilder private var content: some View {
        if let session, let account {
            SessionView(windows: windows, session: session, account: account)
        } else if windows.model.status == nil, let problem = windows.model.problem {
            ContentUnavailableView {
                Label("Couldn’t Read Accounts", systemImage: Symbol.warning)
            } description: {
                Text(problem)
            } actions: {
                Button("Try Again") { Task { await windows.model.refresh(asked: true) } }
            }
            .navigationTitle("Account")
        } else {
            ProgressView("Reading accounts…")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .navigationTitle("Account")
        }
    }
}

/// An open account window's page, toolbar and commands.
private struct SessionView: View {
    let windows: AccountWindows
    @Bindable var session: WebSession
    let account: WindowAccount
    @State private var removingData = false
    @State private var showingDownloads = false

    var body: some View {
        let page = session.page
        let removal = removeDataAlert(account: account)
        VStack(spacing: 0) {
            if let note = session.note {
                NoteBar(note: note) { session.note = nil }
                Divider()
            }
            PageContent(page: page)
        }
        .navigationTitle(account.title)
        .navigationSubtitle(page.title.isEmpty ? account.site.name : page.title)
        .toolbar {
            ToolbarItemGroup(placement: .navigation) {
                Button("Back", systemImage: Symbol.back) { page.goBack() }
                    .help("Show the previous page")
                    .disabled(!page.canGoBack)
                Button("Forward", systemImage: Symbol.forward) { page.goForward() }
                    .help("Show the next page")
                    .disabled(!page.canGoForward)
            }
            ToolbarItemGroup(placement: .primaryAction) {
                if !windows.downloads.transfers(for: account.id).isEmpty {
                    Button("Downloads", systemImage: Symbol.downloads) {
                        showingDownloads.toggle()
                    }
                    .help("Show this window’s downloads")
                    .popover(isPresented: $showingDownloads, arrowEdge: .bottom) {
                        DownloadsList(downloads: windows.downloads, store: account.id)
                    }
                }
                if page.isLoading {
                    Button("Stop", systemImage: Symbol.stop) { page.stopLoading() }
                        .help("Stop loading this page")
                } else {
                    Button("Reload", systemImage: Symbol.refresh) { page.reload() }
                        .help("Load this page again")
                }
            }
        }
        .focusedSceneValue(session)
        .focusedSceneValue(
            \.refresh,
            RefreshCommand(title: "Reload Page", disabled: false) { page.reload() }
        )
        .onChange(of: session.removalAsked) { _, asked in
            if asked {
                session.removalAsked = false
                removingData = true
            }
        }
        .alert(removal.title, isPresented: $removingData) {
            Button("Remove", role: .destructive) {
                Task { await windows.removeWebsiteData(of: session) }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(removal.message)
        }
    }
}

/// What a window has to say, in a bar above its page, until it is dismissed.
private struct NoteBar: View {
    let note: WindowNote
    let dismiss: () -> Void

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Design.iconSpacing) {
            Image(systemName: note.kind == .signIn ? Symbol.signIn : Symbol.note)
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            Text(note.text)
                .explanatory()
                .textSelection(.enabled)
            Spacer(minLength: 0)
            Button("Dismiss", systemImage: "xmark", action: dismiss)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .help("Dismiss")
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(.bar)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("window.note")
        // Said when it appears, since it appears without the person doing anything here.
        .onAppear { AccessibilityNotification.Announcement(note.text).post() }
        .onChange(of: note) { _, said in
            AccessibilityNotification.Announcement(said.text).post()
        }
    }
}

/// An account window's downloads, newest first, each with its progress and what can be done
/// with it.
private struct DownloadsList: View {
    let downloads: DownloadCenter
    let store: UUID

    var body: some View {
        let transfers = downloads.transfers(for: store)
        let rows = VStack(alignment: .leading, spacing: 0) {
            ForEach(transfers) { transfer in
                TransferRow(transfer: transfer) { downloads.cancel(transfer) }
                    .padding(.horizontal, 12)
                Divider()
            }
        }
        VStack(alignment: .leading, spacing: 0) {
            // As tall as its rows, and scrolling once they would be taller than 300 points.
            ViewThatFits(in: .vertical) {
                rows
                ScrollView { rows }
            }
            .frame(width: 340)
            .frame(maxHeight: 300)
            HStack {
                Spacer()
                Button("Clear") { downloads.clearEnded(for: store) }
                    .disabled(transfers.allSatisfy(\.isRunning))
            }
            .padding(8)
        }
    }
}

private struct TransferRow: View {
    let transfer: Transfer
    let cancel: () -> Void

    var body: some View {
        HStack(spacing: Design.iconSpacing) {
            VStack(alignment: .leading, spacing: 2) {
                Text(transfer.name).lineLimit(1).truncationMode(.middle)
                switch transfer.state {
                case .starting, .running:
                    ProgressView(transfer.progress).labelsHidden()
                case .finished:
                    Text("Downloaded").font(.caption).foregroundStyle(.secondary)
                case .failed(let reason):
                    Text(reason).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                case .cancelled:
                    Text("Cancelled").font(.caption).foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: 0)
            if transfer.isRunning {
                Button("Cancel", systemImage: "xmark.circle.fill", action: cancel)
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
                    .help("Stop this download")
            } else if case .finished = transfer.state {
                Button("Show in Finder", systemImage: "magnifyingglass.circle.fill") {
                    transfer.reveal()
                }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .help("Show the file in Finder")
            }
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("download")
    }
}
