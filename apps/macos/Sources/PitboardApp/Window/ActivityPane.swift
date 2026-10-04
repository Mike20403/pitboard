import PitboardKit
import SwiftUI

/// Everything Pitboard has changed, newest first, from the log it keeps: every switch,
/// enrolment, rename, forget and renewal, whichever front end asked for it.
struct ActivityPane: View {
    let machine: MachineModel

    /// A line of the log, numbered. The log itself has no id: two changes can share a
    /// timestamp, a verb and a subject, and rows that claim the same identity make a Table
    /// drop all but one of them.
    struct Line: Identifiable {
        let id: Int
        let change: Change
        let date: Date?
    }

    private var lines: [Line] {
        machine.changes.enumerated().map {
            Line(id: $0.offset, change: $0.element, date: changeDate($0.element.at))
        }
    }

    var body: some View {
        Table(lines) {
            TableColumn("Date") { line in
                Group {
                    if let date = line.date {
                        Text(date.formatted(date: .abbreviated, time: .shortened))
                    } else {
                        Text(line.change.at)
                    }
                }
                .monospacedDigit()
            }
            .width(min: 120, ideal: 150)
            TableColumn("Change") { line in
                Text(changeVerb(line.change.verb))
            }
            .width(min: 70, ideal: 90)
            TableColumn("Account") { line in
                Text(line.change.subject).lineLimit(1).truncationMode(.middle)
            }
            TableColumn("Result") { line in
                Text(changeOutcome(line.change.outcome))
                    .foregroundStyle(line.change.outcome == "ok" ? .secondary : .primary)
            }
            .width(min: 60, ideal: 90)
            TableColumn("Asked By") { line in
                Text(changeCaller(line.change.caller)).foregroundStyle(.secondary)
            }
            .width(min: 80, ideal: 110)
        }
        .overlay {
            if machine.changes.isEmpty {
                ContentUnavailableView(
                    "No Activity",
                    systemImage: Symbol.activity,
                    description: Text("Pitboard lists every change it makes here."))
            }
        }
        .navigationTitle("Activity")
        .toolbar {
            Button {
                Task { await machine.readChanges() }
            } label: {
                Label("Refresh", systemImage: Symbol.refresh)
            }
            .help("Read the log again")
        }
        .focusedSceneValue(
            \.refresh,
            RefreshCommand(title: "Refresh", disabled: false) {
                Task { await machine.readChanges() }
            }
        )
        .task { await machine.readChanges() }
    }
}
