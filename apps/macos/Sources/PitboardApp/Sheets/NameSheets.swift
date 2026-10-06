import PitboardKit
import SwiftUI

/// Names the login signed in now, so Pitboard can park it, with no browser since the login
/// is already there; or gives an enrolled account a new name, which keeps its parked login
/// and its place, inside its own tool.
///
/// What it says, its default button's words, whether that can be pressed and what it saves
/// are the model's: the button offers what `nameToSave` would save, and the model saves by
/// the same rule.
struct NameSheet: View {
    let model: AppModel
    let sheet: Sheet
    /// What saving `name` asks of the model.
    let saving: (String) -> Intent
    @State private var name: String
    @FocusState private var focused: Bool
    @Environment(\.dismiss) private var dismiss

    init(model: AppModel, sheet: Sheet, saving: @escaping (String) -> Intent) {
        self.model = model
        self.sheet = sheet
        self.saving = saving
        _name = State(initialValue: model.sheetText?.name ?? "")
    }

    var body: some View {
        let text = model.sheetText
        // A name being saved cannot be withdrawn, so there is nothing to cancel.
        let busy = text?.saving ?? false
        SheetLayout(title: text?.title ?? "", message: text?.message ?? "") {
            Section {
                TextField("Name", text: $name, prompt: Text(text?.prompt ?? ""))
                    .accessibilityIdentifier("sheet.name")
                    .focused($focused)
                    .onSubmit(save)
            }
            if let failure = model.sheetFailure {
                SheetFailure(failure: failure)
            }
        } buttons: {
            Button("Cancel", role: .cancel) { dismiss() }
                .keyboardShortcut(.cancelAction)
                .disabled(busy)
            Button(text?.confirm ?? "", action: save)
                .keyboardShortcut(.defaultAction)
                .disabled(nameToSave(sheet: sheet, typed: name) == nil || busy)
        }
        .onAppear { focused = true }
        .interactiveDismissDisabled(busy)
    }

    private func save() {
        guard let name = nameToSave(sheet: sheet, typed: name), model.sheetText?.saving != true
        else { return }
        model.send(saving(name))
    }
}
