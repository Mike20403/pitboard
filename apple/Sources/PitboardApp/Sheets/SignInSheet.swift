import PitboardKit
import SwiftUI

/// Adds an account, or signs in again to one whose parked login can no longer be used,
/// through the tool's own sign-in.
///
/// Both tools open the browser themselves and finish through their own callback, so the
/// sheet shows what the tool is doing and offers the address it printed in case the browser
/// did not open. The code field appears only for a tool that reads one, and only once it
/// asks. The sheet stays over the window while the browser is in front, which a menu could
/// not: it closed the moment the browser came forward.
struct SignInSheet: View {
    let model: AppModel
    /// The account being signed in to again, or nil for a new one.
    let again: String?
    @State private var provider: String
    @State private var name: String
    @State private var code = ""
    @State private var failure: ActionFailure?
    /// Whether this sheet started the sign-in the model is running.
    @State private var started = false
    @FocusState private var nameFocused: Bool
    @Environment(\.dismiss) private var dismiss

    init(model: AppModel, provider: String?, again: String?) {
        self.model = model
        self.again = again
        // Chosen before the first frame, not after it: a picker drawn with a selection none
        // of its items has logs that it is invalid, and can draw with nothing selected.
        _provider = State(initialValue: model.provider(for: .add(provider: provider)))
        _name = State(initialValue: again ?? "")
    }

    var body: some View {
        if started, let signingIn = model.signingIn {
            progress(signingIn)
        } else {
            form
        }
    }

    // MARK: - Before it starts

    private var form: some View {
        SheetLayout(
            title: again.map { "Sign In to \($0) Again" } ?? "Add Account",
            message: again == nil
                ? "pitboard opens \(toolName)’s own sign-in in your browser. Sign in as the "
                    + "account you’re adding, and pitboard parks its login beside the one "
                    + "in use."
                : "pitboard opens \(toolName)’s own sign-in in your browser. Sign in as "
                    + "\(again ?? "") to give pitboard a new login for it."
        ) {
            Section {
                if again == nil, model.addable.count > 1 {
                    Picker("Tool", selection: $provider) {
                        ForEach(model.addable, id: \.code) { tool in
                            Text(tool.name).tag(tool.code)
                        }
                    }
                }
                if let again {
                    LabeledContent("Account", value: again)
                } else {
                    TextField("Name", text: $name, prompt: Text("work"))
                        .accessibilityIdentifier("sheet.name")
                        .focused($nameFocused)
                        .onSubmit(start)
                }
            } footer: {
                if again == nil, let missing = model.notOffered {
                    // Said rather than left out without a word, which read as pitboard not
                    // handling the tool at all.
                    Text(missing).footnote()
                }
            }
            if let failure {
                SheetFailure(failure: failure)
            }
        } buttons: {
            Button("Cancel", role: .cancel) { dismiss() }
                .keyboardShortcut(.cancelAction)
            Button("Sign In", action: start)
                .keyboardShortcut(.defaultAction)
                .disabled(trimmed(name).isEmpty || model.signingIn != nil)
        }
        .onAppear { nameFocused = again == nil }
    }

    private func start() {
        let name = trimmed(name)
        guard !name.isEmpty, model.signingIn == nil else { return }
        failure = nil
        started = true
        Task {
            failure = await model.signIn(name, for: provider)
            started = false
        }
    }

    // MARK: - While it runs

    private func progress(_ signingIn: SigningIn) -> some View {
        SheetLayout(
            title: "Signing In to \(signingIn.tool)",
            message: "Finish signing in as \(signingIn.label) in your browser. This closes "
                + "once \(signingIn.tool) says you’re in."
        ) {
            Section {
                LabeledContent {
                    ProgressView().controlSize(.small)
                } label: {
                    Text("Waiting for your browser")
                }
                if let url = signingIn.url {
                    LabeledContent("Browser didn’t open?") {
                        Link("Open Sign-In Page", destination: url)
                            .help(url.absoluteString)
                    }
                }
            }
            if signingIn.wantsCode {
                Section {
                    TextField(
                        "Code", text: $code, prompt: Text("Paste the code from your browser")
                    )
                    .accessibilityIdentifier("sheet.code")
                    .onSubmit(send)
                } footer: {
                    Text(
                        "\(signingIn.tool) asks for this only when your browser couldn’t reach it."
                    )
                    .footnote()
                }
            }
        } buttons: {
            Button("Cancel", role: .cancel) {
                model.cancelSignIn()
                dismiss()
            }
            .keyboardShortcut(.cancelAction)
            if signingIn.wantsCode {
                Button("Submit Code", action: send)
                    .keyboardShortcut(.defaultAction)
                    .disabled(trimmed(code).isEmpty)
            }
        }
    }

    private func send() {
        let typed = trimmed(code)
        guard !typed.isEmpty else { return }
        model.paste(typed)
        code = ""
    }

    private var toolName: String { model.tool(provider)?.name ?? provider }
}
