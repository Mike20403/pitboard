import SwiftUI

/// pitboard's one window, beside the menu rather than instead of it.
///
/// The menu is the glance and the switch. This is where the things that need room go: each
/// account with its limits drawn out and everything that can be done to it, what pitboard
/// has to say in full, everything it has changed, and what it finds about this Mac.
struct MainWindow: View {
    /// The scene's id, named once so the menu bar item and the scene cannot drift apart.
    static let id = "main"

    @Bindable var model: AppModel
    @SceneStorage("pane") private var pane = Pane.accounts

    enum Pane: String, CaseIterable, Identifiable {
        case accounts
        case activity
        case machine

        var id: String { rawValue }

        var title: String {
            switch self {
            case .accounts: "Accounts"
            case .activity: "Activity"
            case .machine: "This Mac"
            }
        }

        var symbol: String {
            switch self {
            case .accounts: Symbol.accounts
            case .activity: Symbol.activity
            case .machine: Symbol.machine
            }
        }
    }

    var body: some View {
        NavigationSplitView {
            List(Pane.allCases, selection: selection) { pane in
                Label(pane.title, systemImage: pane.symbol)
                    .tag(pane)
                    .accessibilityIdentifier("sidebar.\(pane.rawValue)")
            }
            .navigationSplitViewColumnWidth(min: 150, ideal: 170, max: 220)
        } detail: {
            switch pane {
            case .accounts: AccountsPane(model: model)
            case .activity: ActivityPane(machine: model.machine)
            case .machine: MachinePane(machine: model.machine)
            }
        }
        .frame(minWidth: 640, minHeight: 440)
        .sheet(item: $model.sheet) { sheet in
            AccountSheetView(model: model, sheet: sheet)
        }
        .failureAlert($model.presentedFailure)
        .onChange(of: model.sheet) {
            // A sheet asked for from the menu is about accounts, so it opens over them.
            if model.sheet != nil { pane = .accounts }
        }
    }

    /// The sidebar's selection. A list selects nothing when its selection is cleared, and a
    /// window with no pane shows nothing, so clearing it keeps the pane shown.
    private var selection: Binding<Pane?> {
        Binding(get: { pane }, set: { if let chosen = $0 { pane = chosen } })
    }
}

extension View {
    /// An alert for a failure somebody should hear about, gone once it is read.
    func failureAlert(_ failure: Binding<ActionFailure?>) -> some View {
        alert(
            failure.wrappedValue?.title ?? "",
            isPresented: Binding(
                get: { failure.wrappedValue != nil },
                set: { if !$0 { failure.wrappedValue = nil } }),
            presenting: failure.wrappedValue
        ) { _ in
            Button("OK") { failure.wrappedValue = nil }
        } message: { shown in
            Text(([shown.message] + shown.warnings.map(\.message)).joined(separator: "\n\n"))
        }
    }
}
