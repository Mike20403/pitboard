import PitboardKit
import SwiftUI

/// What pitboard finds about this Mac, the checks `pitboard doctor` makes, for when
/// something is wrong at machine level and a failed switch's one line is not enough to act
/// on.
struct MachinePane: View {
    let machine: MachineModel

    var body: some View {
        Form {
            Section {
                ForEach(machine.checks, id: \.code) { check in
                    CheckRow(check: check)
                }
            } header: {
                summary
            }
        }
        .formStyle(.grouped)
        .overlay {
            if machine.checks.isEmpty {
                ProgressView("Checking this Mac…")
            }
        }
        .navigationTitle("This Mac")
        .toolbar {
            Button {
                Task { await machine.diagnose() }
            } label: {
                Label("Check Again", systemImage: Symbol.refresh)
            }
            .help("Make every check again")
            .keyboardShortcut("r")
            .disabled(machine.checking)
        }
        .task {
            if machine.checks.isEmpty { await machine.diagnose() }
        }
    }

    @ViewBuilder private var summary: some View {
        let failing = machine.checks.filter { $0.level != .ok }.count
        if !machine.checks.isEmpty {
            Text(
                failing == 0
                    ? "Everything pitboard checks is in order."
                    : failing == 1
                        ? "One thing is worth looking at."
                        : "\(failing) things are worth looking at."
            )
        }
    }
}

/// One check: whether it passed, what it looked at, and what to do when it did not.
private struct CheckRow: View {
    let check: Check

    var body: some View {
        LabeledContent {
            Text(check.detail)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.trailing)
                .textSelection(.enabled)
        } label: {
            HStack(alignment: .firstTextBaseline, spacing: Design.iconSpacing) {
                Image(systemName: check.level.symbol)
                    .foregroundStyle(check.level.tint)
                    .accessibilityLabel(check.level.spoken)
                VStack(alignment: .leading, spacing: Design.lineSpacing) {
                    Text(check.name)
                    if check.level != .ok, !check.advice.isEmpty {
                        Text(check.advice).explanatory()
                    }
                }
            }
        }
    }
}
