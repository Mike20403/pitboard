import PitboardKit
import SwiftUI

/// Adds an account, or signs in again to one whose parked login can no longer be used,
/// through the tool's own sign-in, which the model runs.
///
/// Both tools open the browser themselves and finish through their own callback, so the
/// sheet shows what the tool is doing and offers the address it printed in case the browser
/// did not open. The code field appears only for a tool that reads one, only once it asks,
/// and again once Claude Code has refused the code typed back. The sheet stays over the
/// window while the browser is in front, which a menu could not: it closed the moment the
/// browser came forward.
struct SignInSheet: View {
    let model: AppModel
    let sheet: Sheet
    @State private var provider: String
    @State private var name: String
    @State private var code = ""
    /// The sign-in this sheet showed last, and what it said, kept while the sheet closes
    /// after it finishes, when the model has already let it go, so the sheet does not flash
    /// its form on the way out.
    @State private var shown: (signIn: RunningSignIn, text: SigningInText)?
    @FocusState private var nameFocused: Bool
    @FocusState private var codeFocused: Bool
    @Environment(\.dismiss) private var dismiss

    init(model: AppModel, sheet: Sheet) {
        self.model = model
        self.sheet = sheet
        // Chosen before the first frame, not after it: a picker drawn with a selection none
        // of its items has logs that it is invalid, and can draw with nothing selected.
        _provider = State(initialValue: model.sheetText?.tool ?? "")
        _name = State(initialValue: model.sheetText?.name ?? "")
    }

    /// The model runs one sign-in at a time, so a sign-in sheet shows the one running,
    /// whichever sheet started it.
    var body: some View {
        Group {
            if let signingIn = model.signingIn, let text = model.signingInText {
                progress(signingIn, text)
            } else if let shown {
                progress(shown.signIn, shown.text)
            } else {
                form
            }
        }
        .onChange(of: model.signingIn, initial: true) { _, running in
            if let running, let text = model.signingInText {
                shown = (running, text)
            } else if model.sheet != nil {
                // Over, and the sheet stays: it failed, or it was stopped. Back to the form,
                // with what went wrong and the name still in it.
                shown = nil
            }
        }
    }

    // MARK: - Before it starts

    @ViewBuilder private var form: some View {
        let text = model.sheetText
        let tools = text?.tools ?? []
        SheetLayout(
            title: text?.title ?? "",
            message: tools.first { $0.code == provider }?.message ?? text?.message ?? ""
        ) {
            Section {
                if !tools.isEmpty {
                    Picker("Tool", selection: $provider) {
                        ForEach(tools, id: \.code) { tool in
                            Text(tool.name).tag(tool.code)
                        }
                    }
                    .accessibilityIdentifier("sheet.tool")
                }
                if let account = text?.account {
                    LabeledContent("Account", value: account)
                } else {
                    TextField("Name", text: $name, prompt: Text(text?.prompt ?? ""))
                        .accessibilityIdentifier("sheet.name")
                        .focused($nameFocused)
                        .onSubmit(start)
                }
            } footer: {
                if let missing = text?.notOffered {
                    // Said rather than left out without a word, which read as Pitboard not
                    // handling the tool at all.
                    Text(missing).footnote()
                }
            }
            if let failure = model.sheetFailure {
                SheetFailure(failure: failure)
            }
        } buttons: {
            Button("Cancel", role: .cancel) { dismiss() }
                .keyboardShortcut(.cancelAction)
            Button("Sign In", action: start)
                .keyboardShortcut(.defaultAction)
                .disabled(
                    nameToSave(sheet: sheet, typed: name) == nil || model.signingIn != nil)
        }
        .onAppear { nameFocused = text?.account == nil }
    }

    private func start() {
        guard let name = nameToSave(sheet: sheet, typed: name), model.signingIn == nil,
            shown == nil
        else { return }
        model.send(.signIn(provider: provider, name: name))
    }

    // MARK: - While it runs

    private func progress(_ signingIn: RunningSignIn, _ text: SigningInText) -> some View {
        SheetLayout(title: text.title, message: text.message) {
            Section {
                LabeledContent {
                    ProgressView().controlSize(.small)
                } label: {
                    Text("Waiting for your browser")
                }
                if let url = signingIn.url.flatMap(URL.init(string:)) {
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
                    .focused($codeFocused)
                    .onSubmit(send)
                    // Appears once the tool asks, with the code on the clipboard to paste.
                    .onAppear { codeFocused = true }
                    if let refused = text.refused {
                        Text(refused).explanatory()
                    }
                } footer: {
                    Text(text.codeNote).footnote()
                }
            }
        } buttons: {
            Button("Cancel", role: .cancel) {
                model.send(.cancelSignIn)
                shown = nil
                dismiss()
            }
            .keyboardShortcut(.cancelAction)
            if signingIn.wantsCode {
                Button("Submit Code", action: send)
                    .keyboardShortcut(.defaultAction)
                    .disabled(code.isEmpty)
            }
        }
    }

    /// The model takes the code without the white space around it, and types it back only
    /// where the tool asks for one.
    private func send() {
        guard !code.isEmpty else { return }
        model.send(.pasteCode(code: code))
        code = ""
    }
}
