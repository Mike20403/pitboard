import SwiftUI

/// Where the share stands, which the extension's view shows.
@MainActor
@Observable
final class ShareState {
    enum Phase: Equatable {
        /// Handing the link to Pitboard.
        case opening
        /// Not a link Pitboard opens, and why.
        case refused(String)
        /// Pitboard could not be opened, and why.
        case failed(String)
    }

    var phase = Phase.opening
}

/// What the share sheet shows while Pitboard is handed the link, or why it was not.
struct ShareView: View {
    let state: ShareState
    let done: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            switch state.phase {
            case .opening:
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text("Opening in Pitboard…")
                }
            case .refused(let reason):
                said("Can’t Open This Link", reason)
            case .failed(let reason):
                said("Couldn’t Open Pitboard", reason)
            }
            if state.phase != .opening {
                HStack {
                    Spacer()
                    Button("OK", action: done)
                        .keyboardShortcut(.defaultAction)
                }
            }
        }
        .padding(20)
        .frame(width: 360, alignment: .leading)
    }

    private func said(_ title: String, _ reason: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title)
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
            Text(reason)
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}
