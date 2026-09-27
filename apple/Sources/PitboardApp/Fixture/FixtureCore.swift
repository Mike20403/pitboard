#if DEBUG
    import Foundation
    import PitboardKit

    /// A core that answers from memory, the way the real one answers about a machine in the
    /// fixture's state, and changes that state the way the real one would: a switch moves
    /// who is in use, a sign-in adds an account, a forget drops one.
    ///
    /// Everything is under one lock, because the app calls it from whatever thread it likes.
    final class FixtureCore: Core, @unchecked Sendable {
        private let lock = NSLock()
        private let fixture: Fixture
        private var accounts: [Account]
        private var stuck: Bool
        private var changes: [Change] = []
        private var scheduled = false
        private var changedAt: Int64 = 1

        init(_ fixture: Fixture, now: Date = Date()) {
            self.fixture = fixture
            accounts = Self.accounts(for: fixture, now: now)
            stuck = fixture == .stuck
            changes = Self.history(now: now)
        }

        func tools() -> [Tool] { Self.tools }

        func installed() async -> [Tool] {
            fixture == .noClaudeCode ? [] : Self.tools
        }

        func searchPath() async -> String? { "/usr/bin:/bin" }

        func status(fresh: Bool) async throws -> Status {
            try lock.withLock {
                switch fixture {
                case .noClaudeCode:
                    throw PitboardError.Failed(
                        code: "claude_program_missing", cause: nil,
                        message: "`claude` is not on this machine. Install Claude Code and "
                            + "sign in to it once.",
                        warnings: [])
                case .readFailure:
                    throw PitboardError.Failed(
                        code: "unreachable", cause: nil,
                        message: "Anthropic could not be reached. The numbers shown are the "
                            + "last ones measured.",
                        warnings: [])
                case .stuck where stuck:
                    throw PitboardError.Failed(
                        code: "recovery_undetermined", cause: nil,
                        message: "A switch from work to personal was interrupted, and it "
                            + "cannot be finished until Anthropic answers.",
                        warnings: [])
                default:
                    return currentStatus()
                }
            }
        }

        func statusOffline() async throws -> Status {
            lock.withLock { currentStatus() }
        }

        func doctor() async -> Diagnosis {
            Diagnosis(
                checks: [
                    Check(
                        code: "claude_program", name: "Claude Code", level: .ok,
                        detail: "/opt/homebrew/bin/claude", advice: ""),
                    Check(
                        code: "keychain", name: "Keychain", level: .ok,
                        detail: "login keychain, unlocked", advice: ""),
                    Check(
                        code: "schedule", name: "Daily renewal", level: .warn,
                        detail: "not scheduled",
                        advice:
                            "Turn on daily renewal in pitboard's settings, so parked logins "
                            + "you leave alone do not expire."),
                ],
                healthy: true)
        }

        func switchTo(_ label: String) async throws -> Switched {
            try lock.withLock {
                guard let target = accounts.firstIndex(where: { $0.qualified == label }) else {
                    throw Self.failed("unknown_account", "There is no account called \(label).")
                }
                let provider = accounts[target].provider
                guard !accounts[target].signedIn else {
                    return Switched(
                        outcome: .alreadyActive(label: accounts[target].label ?? ""),
                        warnings: [])
                }
                let from = accounts.first { $0.provider == provider && $0.signedIn }
                for index in accounts.indices where accounts[index].provider == provider {
                    accounts[index] = with(
                        accounts[index], signedIn: index == target,
                        switchable: index != target)
                }
                record("switch", Self.typed(label))
                return Switched(
                    outcome: .switched(
                        provider: provider, from: from?.qualified ?? "",
                        to: accounts[target].qualified ?? label,
                        adoption: provider == "codex"
                            ? .restart(program: "codex") : .follows(withinSeconds: 45)),
                    warnings: [])
            }
        }

        func enrollCurrent(_ label: String) async throws -> Enrolled {
            try lock.withLock {
                let (provider, name) = Self.parts(of: label)
                guard
                    let index = accounts.firstIndex(where: {
                        $0.provider == provider && $0.signedIn && $0.label == nil
                    })
                else { throw Self.failed("nothing_signed_in", "Nobody is signed in to name.") }
                try requireUnused(name, of: provider)
                accounts[index] = with(accounts[index], label: name)
                record("enroll", Self.typed(label))
                return Enrolled(email: accounts[index].email, enrolled: .current, warnings: [])
            }
        }

        func forget(_ label: String) async throws -> Changed {
            try lock.withLock {
                guard let index = accounts.firstIndex(where: { $0.qualified == label }) else {
                    throw Self.failed("unknown_account", "There is no account called \(label).")
                }
                guard !accounts[index].signedIn else {
                    throw Self.failed(
                        "cannot_forget_active_account",
                        "\(accounts[index].label ?? label) is the account in use, so it "
                            + "cannot be forgotten. Switch to another account first.")
                }
                let email = accounts.remove(at: index).email
                record("forget", Self.typed(label))
                return Changed(email: email, warnings: [])
            }
        }

        func rename(_ from: String, to: String) async throws -> Changed {
            try lock.withLock {
                guard let index = accounts.firstIndex(where: { $0.qualified == from }) else {
                    throw Self.failed("unknown_account", "There is no account called \(from).")
                }
                try requireUnused(to, of: accounts[index].provider)
                accounts[index] = with(accounts[index], label: to)
                record("rename", "\(Self.typed(from)) -> \(to)")
                return Changed(email: accounts[index].email, warnings: [])
            }
        }

        func signIn(_ label: String) async throws -> SignIn {
            let (provider, name) = Self.parts(of: label)
            try lock.withLock {
                if !accounts.contains(where: { $0.provider == provider && $0.label == name }) {
                    try requireUnused(name, of: provider)
                }
            }
            return FixtureSignIn(provider: provider) { [weak self] in
                self?.signedIn(name, of: provider)
                    ?? Enrolled(
                        email: "", enrolled: .signedIn, warnings: [])
            }
        }

        func abandonRecovery() async throws -> Abandoned? {
            lock.withLock {
                guard stuck else { return nil }
                stuck = false
                record("abandon", "work -> personal")
                return Abandoned(from: "work", to: "personal", loginsKept: 2)
            }
        }

        func log(limit: UInt32) async -> [Change] {
            lock.withLock { Array(changes.suffix(Int(limit))) }
        }

        func renew() async -> [Renewed] {
            [Renewed(label: "work", provider: "claude", outcome: "renewed")]
        }

        func schedule() async -> Schedule {
            lock.withLock {
                scheduled
                    ? .installed(
                        path: "~/Library/LaunchAgents/com.usepitboard.renew.plist",
                        everySeconds: 86_400)
                    : .absent
            }
        }

        func scheduleInstall() async throws -> String {
            lock.withLock {
                scheduled = true
                return "~/Library/LaunchAgents/com.usepitboard.renew.plist"
            }
        }

        func scheduleUninstall() async throws -> Bool {
            lock.withLock {
                defer { scheduled = false }
                return scheduled
            }
        }

        func scheduleRepair() async throws -> Bool { false }

        func changedAt() async -> Int64 { lock.withLock { changedAt } }

        func readingsChangedAt() async -> Int64 { 1 }

        // MARK: Changing the state

        /// What finishing a sign-in enrols: a new account is parked beside the one in use,
        /// and one that was enrolled already has its parked login renewed.
        private func signedIn(_ name: String, of provider: String) -> Enrolled {
            lock.withLock {
                record("enroll", Self.typed("\(provider)/\(name)"))
                if let index = accounts.firstIndex(where: {
                    $0.provider == provider && $0.label == name
                }) {
                    accounts[index] = with(accounts[index], switchable: true, stale: nil)
                    return Enrolled(
                        email: accounts[index].email, enrolled: .renewed, warnings: [])
                }
                let email = "\(name)@example.com"
                accounts.append(
                    Self.account(
                        name, of: provider, email: email,
                        windows: [Self.window("five_hour", 3, length: 18_000, in: 18_000)]))
                return Enrolled(email: email, enrolled: .signedIn, warnings: [])
            }
        }

        private func requireUnused(_ name: String, of provider: String) throws {
            if accounts.contains(where: { $0.provider == provider && $0.label == name }) {
                throw Self.failed(
                    "label_taken", "There is already an account called \(name).")
            }
        }

        private func record(_ verb: String, _ subject: String) {
            changedAt += 1
            changes.append(
                Change(
                    at: Date().formatted(.iso8601), caller: "app", verb: verb,
                    subject: subject, outcome: "ok"))
        }

        /// A label as the log writes it: bare for Claude Code, with its tool for any other.
        private static func typed(_ label: String) -> String {
            label.hasPrefix("claude/") ? String(label.dropFirst("claude/".count)) : label
        }

        private func currentStatus() -> Status {
            Status(now: Int64(Date().timeIntervalSince1970), accounts: accounts, warnings: [])
        }

        // MARK: What each fixture starts with

        static let tools = [
            Tool(code: "claude", name: "Claude Code", program: "claude", service: "Anthropic"),
            Tool(code: "codex", name: "Codex", program: "codex", service: "OpenAI"),
        ]

        private static func accounts(for fixture: Fixture, now: Date) -> [Account] {
            let work = account(
                "work", email: "dana@work.example", signedIn: true,
                windows: [
                    window("five_hour", 42, length: 18_000, in: 7_800),
                    window("seven_day", 12, length: 604_800, in: 356_000),
                ])
            let personal = account(
                "personal", email: "dana@home.example",
                windows: [
                    window("five_hour", 100, length: 18_000, in: 4_800),
                    window("seven_day", 61, length: 604_800, in: 190_000),
                ],
                parkedFor: 11 * 86_400)
            switch fixture {
            case .twoTools:
                let old = account(
                    "old", email: "dana@old.example", switchable: false,
                    stale: "The parked login has expired. Sign in again to use this account.")
                let codexMain = account(
                    "main", of: "codex", email: "dana@work.example", signedIn: true,
                    windows: [
                        window("primary", 18, length: 18_000, in: 12_000),
                        window("secondary", 7, length: 604_800, in: 500_000),
                    ])
                let codexSpare = account(
                    "spare", of: "codex", email: "dana@home.example",
                    windows: [window("primary", 0, length: 18_000, in: 18_000)],
                    parkedFor: 20 * 86_400)
                return [work, personal, old, codexMain, codexSpare]
            case .oneTool, .readFailure, .stuck:
                return [work, personal]
            case .onlyOne:
                return [work]
            case .unnamed:
                return [account(nil, email: "dana@work.example", signedIn: true)]
            case .empty, .firstLaunch, .noClaudeCode:
                return []
            }
        }

        private static func history(now: Date) -> [Change] {
            [
                ("enroll", "work", "cli", 86_400 * 3),
                ("enroll", "personal", "app", 86_400 * 2),
                ("switch", "personal", "cli", 7_200), ("switch", "work", "app", 3_600),
            ].map { verb, subject, caller, ago in
                Change(
                    at: now.addingTimeInterval(-Double(ago)).formatted(.iso8601),
                    caller: caller, verb: verb, subject: subject, outcome: "ok")
            }
        }

        static func account(
            _ label: String?, of provider: String = "claude", email: String,
            signedIn: Bool = false, switchable: Bool? = nil, stale: String? = nil,
            windows: [PitboardBindings.Window] = [], parkedFor seconds: Int64? = nil
        ) -> Account {
            let now = Int64(Date().timeIntervalSince1970)
            return Account(
                id: "\(provider):\(email):\(label ?? "")", provider: provider, label: label,
                qualified: label.map { "\(provider)/\($0)" }, unplaced: false, email: email,
                accountUuid: email, signedIn: signedIn,
                switchable: switchable ?? !signedIn,
                parked: seconds.map {
                    Parked(
                        parkedAt: now - 3600, accessExpiresAt: nil, refreshExpiresAt: now + $0)
                },
                usage: windows.isEmpty
                    ? nil : Usage(source: .live, observedAt: now, windows: windows),
                stale: stale == nil ? nil : "parked_access_expired", staleExplanation: stale,
                lastsSeconds: signedIn && !windows.isEmpty ? 11_000 : nil,
                lastsBurning: signedIn)
        }

        static func window(
            _ kind: String, _ percent: Double, length: Int64, in seconds: Int64
        ) -> PitboardBindings.Window {
            PitboardBindings.Window(
                kind: kind, lengthSeconds: length, scope: nil, percent: percent,
                resetsAt: Int64(Date().timeIntervalSince1970) + seconds, severity: nil,
                isActive: true)
        }

        private func with(
            _ account: Account, label: String? = nil, signedIn: Bool? = nil,
            switchable: Bool? = nil, stale: String?? = .none
        ) -> Account {
            let label = label ?? account.label
            let explanation = stale.map { $0 } ?? account.staleExplanation
            return Account(
                id: account.id, provider: account.provider, label: label,
                qualified: label.map { "\(account.provider)/\($0)" },
                unplaced: account.unplaced,
                email: account.email, accountUuid: account.accountUuid,
                signedIn: signedIn ?? account.signedIn,
                switchable: switchable ?? account.switchable, parked: account.parked,
                usage: account.usage, stale: explanation == nil ? nil : account.stale,
                staleExplanation: explanation, lastsSeconds: account.lastsSeconds,
                lastsBurning: account.lastsBurning)
        }

        private static func parts(of label: String) -> (provider: String, name: String) {
            let parts = label.split(separator: "/", maxSplits: 1).map(String.init)
            return parts.count == 2 ? (parts[0], parts[1]) : ("claude", label)
        }

        private static func failed(_ code: String, _ message: String) -> PitboardError {
            .Failed(code: code, cause: nil, message: message, warnings: [])
        }
    }

    /// A sign-in that prints what the tool prints and finishes the way the tool does.
    ///
    /// Claude Code's asks for the code from the browser, as it does when its callback cannot
    /// be reached, and finishes once one is pasted; Codex's prints its address and finishes
    /// on its own a moment later.
    final class FixtureSignIn: SignIn, @unchecked Sendable {
        private let lock = NSLock()
        private let wake = DispatchSemaphore(value: 0)
        private var lines: [String]
        private var ended = false
        private var cancelled = false
        private let code: Bool
        private let enrol: @Sendable () -> Enrolled

        init(provider: String, enrol: @escaping @Sendable () -> Enrolled) {
            code = provider == "claude"
            self.enrol = enrol
            lines =
                code
                ? [
                    "Opening your browser to sign in…\n",
                    "If it did not open: https://claude.ai/oauth/authorize?fixture=1\n",
                    "Paste code here if prompted > ",
                ]
                : [
                    "Starting local login server on http://localhost:1455.\n",
                    "If your browser did not open, navigate to this URL to authenticate:\n",
                    "https://auth.openai.com/oauth/authorize?fixture=1\n",
                ]
            super.init(noHandle: NoHandle())
        }

        required init(unsafeFromHandle handle: UInt64) { fatalError("not from the core") }

        override func takesACode() -> Bool { code }

        override func nextLine() -> String? {
            let next: String? = lock.withLock { lines.isEmpty ? nil : lines.removeFirst() }
            if let next {
                Thread.sleep(forTimeInterval: 0.2)
                return next
            }
            // Codex finishes by itself; Claude Code waits for the code.
            if code {
                wake.wait()
            } else {
                _ = wake.wait(timeout: .now() + 1.5)
            }
            return nil
        }

        override func paste(line: String) throws {
            lock.withLock { ended = true }
            wake.signal()
        }

        override func finish() throws -> Enrolled {
            if lock.withLock({ cancelled }) {
                throw PitboardError.Failed(
                    code: "sign_in_cancelled", cause: nil, message: "The sign-in was stopped.",
                    warnings: [])
            }
            return enrol()
        }

        override func cancel() {
            lock.withLock { cancelled = true }
            wake.signal()
        }
    }
#endif
