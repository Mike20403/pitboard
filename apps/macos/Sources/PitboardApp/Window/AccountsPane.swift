import AppKit
import PitboardKit
import SwiftUI

/// Every account, a section per tool, with what Pitboard has to say above them.
struct AccountsPane: View {
    let model: AppModel
    let windows: AccountWindows
    @State private var selection: String?
    /// A question the model asks before something that cannot be undone, and what to send
    /// once it is answered.
    @State private var asking: Asking?
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        content
            .navigationTitle("Accounts")
            .navigationSubtitle(model.updatedWindow)
            .toolbar {
                ToolbarItemGroup {
                    Button {
                        model.send(.refresh(asked: true))
                    } label: {
                        Label("Refresh", systemImage: Symbol.refresh)
                    }
                    .help("Read every account’s usage again")
                    .disabled(model.reading)
                    // Command-N is the window's own command, so it works from every pane.
                    Button {
                        model.send(.presentSheet(sheet: .add(provider: nil)))
                    } label: {
                        Label("Add Account", systemImage: Symbol.add)
                    }
                    .help("Sign in to another account and park its login")
                }
            }
            .focusedSceneValue(
                \.refresh,
                RefreshCommand(title: "Refresh", disabled: model.reading) {
                    model.send(.refresh(asked: true))
                }
            )
            .task { model.send(.paneShown(pane: .accounts)) }
            .alert(
                asking?.question.title ?? "",
                isPresented: Binding(
                    get: { asking != nil }, set: { if !$0 { asking = nil } }),
                presenting: asking
            ) { asked in
                Button(asked.question.confirm, role: .destructive) { model.send(asked.intent) }
                Button("Cancel", role: .cancel) {}
            } message: { asked in
                Text(asked.question.message)
            }
    }

    @ViewBuilder private var content: some View {
        switch model.accountsShown {
        case .noTool(let title, let detail, let linkTitle, let link):
            ContentUnavailableView {
                Label(title, systemImage: Symbol.terminal)
            } description: {
                Text(detail)
            } actions: {
                if let link = URL(string: link) {
                    Link(linkTitle, destination: link)
                        .buttonStyle(.borderedProminent)
                }
            }
        case .readFailed(let title, let detail, let retry):
            // Nothing to list, and the read said why: that is the thing to say, in full,
            // with a way to try again, and not a spinner that never stops.
            ContentUnavailableView {
                Label(title, systemImage: Severity.error.symbol)
            } description: {
                Text(detail)
            } actions: {
                Button(retry.title) { model.send(retry.intent) }
                    .disabled(!retry.enabled)
            }
        case .noAccounts(let title, let detail, let add):
            ContentUnavailableView {
                Label(title, systemImage: Symbol.accounts)
            } description: {
                Text(detail)
            } actions: {
                Button(add.title) { model.send(add.intent) }
                    .buttonStyle(.borderedProminent)
                    .disabled(!add.enabled)
            }
        case .reading(let title):
            ProgressView(title)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .list:
            list
        }
    }

    private var list: some View {
        List(selection: $selection) {
            ForEach(model.notices, id: \.id) { notice in
                NoticeRow(notice: notice, perform: perform)
                    .selectionDisabled()
            }
            if let step = model.setup {
                SetupTip(step: step, footing: model.footing) { model.send($0) }
            }
            ForEach(model.sections, id: \.id) { section in
                // A heading per tool once there is more than one, and no section at all
                // before: a section with no heading still takes a heading's room.
                if let heading = section.heading {
                    Section(heading) { rows(of: section) }
                } else {
                    rows(of: section)
                }
            }
        }
        .listStyle(.inset)
        .contextMenu(forSelectionType: String.self) { ids in
            if let item = ids.first.flatMap(model.item) {
                menu(for: item)
            }
        } primaryAction: { ids in
            if let action = ids.first.flatMap(model.item)?.action {
                model.send(action.intent)
            }
        }
        .onDeleteCommand {
            if let item = selection.flatMap(model.item) { forget(item) }
        }
    }

    private func rows(of section: AccountSection) -> some View {
        ForEach(section.accounts, id: \.id) { item in
            AccountRow(item: item) { model.send($0) }
                .tag(item.id)
        }
    }

    // MARK: - What can be done to an account

    /// The account's own menu: what the model offers to do to it, each held back where the
    /// model says, then its windows, then the app's own Copy Email Address, and forgetting it
    /// last, where the model offers that.
    @ViewBuilder private func menu(for item: AccountItem) -> some View {
        ForEach(item.offers, id: \.title) { offer in
            Button(offer.title) { perform(offer) }
                .disabled(!offer.enabled)
        }
        if !item.windows.isEmpty {
            Divider()
            ForEach(item.windows, id: \.window.store) { offer in
                Button(offer.title) {
                    windows.presence.activate()
                    openWindow(id: AccountWindowScene.id, value: offer.window.id)
                }
            }
        }
        if !item.email.isEmpty {
            Divider()
            Button("Copy Email Address") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(item.email, forType: .string)
            }
        }
        if let forget = item.forget {
            Divider()
            Button(forget.title, role: .destructive) { perform(forget) }
                .disabled(!forget.enabled)
        }
    }

    /// Forgetting the account, where the model offers it, as its menu does: the Delete key's
    /// answer in the list.
    private func forget(_ item: AccountItem) {
        if let forget = item.forget, forget.enabled { perform(forget) }
    }

    /// Asks the question an offer comes with first, where it has one, and otherwise sends it.
    private func perform(_ offer: ItemOffer) {
        if let question = offer.confirm {
            asking = Asking(question: question, intent: offer.intent)
        } else {
            model.send(offer.intent)
        }
    }

    private func perform(_ action: NoticeAction) {
        if let question = action.confirm {
            asking = Asking(question: question, intent: action.intent)
        } else {
            model.send(action.intent)
        }
    }
}

/// A question asked first, and what answering yes sends.
private struct Asking: Equatable {
    let question: Question
    let intent: Intent
}

/// The one next thing to do on a machine that is not set up yet, above its accounts, with
/// its symbol: a name for an account that has none, or a second account to switch to.
private struct SetupTip: View {
    let step: SetupStep
    let footing: Footing
    let perform: (Intent) -> Void

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Design.iconSpacing) {
            Image(systemName: symbol)
                .foregroundStyle(.tint)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: Design.rowSpacing) {
                Text(step.title).fontWeight(.medium)
                Text(step.detail).explanatory()
                HStack {
                    ForEach(Array(step.actions.enumerated()), id: \.offset) { index, choice in
                        if index == 0 {
                            Button(choice.title) { perform(choice.intent) }
                                .buttonStyle(.borderedProminent)
                                .disabled(!choice.enabled)
                        } else {
                            Button(choice.title) { perform(choice.intent) }
                                .disabled(!choice.enabled)
                        }
                    }
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.vertical, 4)
        .selectionDisabled()
    }

    private var symbol: String {
        if case .unnamed = footing { return "tag" }
        return Symbol.switchAccount
    }
}
