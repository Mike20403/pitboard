import PitboardKit
import UserNotifications

/// An account has run out, and another account of the same tool has room.
struct Advice {
    /// The tool both accounts are for, as a `Tool`'s code.
    let provider: String
    /// The tool's name, when accounts of more than one tool are shown and a sentence has to
    /// say which one it is about. Nil otherwise, so a machine with one tool reads as before.
    let tool: String?
    /// The account that ran out, and the window it ran out of.
    let ran: String
    let window: Limit
    /// The account offered instead, and what it has left in the same kind of window.
    let use: String
    let left: Int
    /// What to switch to: `use` with its tool, which names one account whatever else is
    /// enrolled. Two tools can each have a `work`.
    let switchTo: String

    /// Nothing to say unless an account in use has exhausted a window that has not been
    /// mentioned yet and an account of the same tool that can be switched to now has room in
    /// the same kind of window. At most one piece of advice per tool, in the order `tools`
    /// lists them.
    ///
    /// Only ever the same tool: a Codex account with room left is no help to somebody whose
    /// Claude Code account has run out, and a switch between them is not a switch at all.
    /// Weekly limits are per account, so the comparison is like for like.
    static func about(
        _ status: Status, tools: [Tool] = [], unless told: [String: Int64]
    ) -> [Advice] {
        let providers = inOrder(status.accounts.map(\.provider), by: tools)
        return providers.compactMap { provider in
            let mine = status.accounts.filter { $0.provider == provider }
            guard let current = mine.first(where: { $0.signedIn && $0.label != nil }),
                let ran = current.label
            else { return nil }
            for window in current.usage?.windows ?? [] where window.percent >= 100 {
                if let at = told[key(provider, ran, window)],
                    sameReset(between: at, and: window.resetsAt ?? 0)
                {
                    continue
                }
                guard let spare = spare(like: window, among: mine) else { continue }
                return Advice(
                    provider: provider,
                    tool: providers.count > 1
                        ? tools.first { $0.code == provider }?.name ?? provider : nil,
                    ran: ran, window: window, use: spare.use,
                    left: spare.left, switchTo: spare.switchTo)
            }
            return nil
        }
    }

    /// The account of the same tool with the most left of `window`'s kind that can be
    /// switched to now, and what it has left. None when none has any.
    private static func spare(
        like window: Limit, among mine: [Account]
    ) -> (use: String, left: Int, switchTo: String)? {
        let spare =
            mine
            .filter { $0.switchable && $0.qualified != nil }
            .min { used($0, like: window) < used($1, like: window) }
        guard let spare, let use = spare.label, let switchTo = spare.qualified,
            used(spare, like: window) < 100
        else { return nil }
        return (use, 100 - Int(used(spare, like: window).rounded()), switchTo)
    }

    /// This advice as `status` bears it out now: nil once the account that ran out has room
    /// again or is no longer in use, and otherwise offering the best account there is now.
    /// The one offered before may have been forgotten, renamed, expired or run out itself,
    /// and a menu item offering it would fail when chosen.
    func renewed(in status: Status, tools: [Tool] = []) -> Advice? {
        guard holds(in: status),
            let spare = Self.spare(
                like: window, among: status.accounts.filter { $0.provider == provider })
        else { return nil }
        return Advice(
            provider: provider, tool: tool, ran: ran, window: window, use: spare.use,
            left: spare.left, switchTo: spare.switchTo)
    }

    /// This advice about `label` of its tool, as it reads once that account is `name`.
    func renaming(_ label: String, to name: String) -> Advice {
        Advice(
            provider: provider, tool: tool, ran: ran == label ? name : ran, window: window,
            use: use == label ? name : use, left: left,
            switchTo: switchTo == qualified(label, for: provider)
                ? qualified(name, for: provider) : switchTo)
    }

    /// Whether `status` still bears this out: the account that ran out is still the one in
    /// use, and the same window of it is still spent.
    func holds(in status: Status) -> Bool {
        status.accounts.contains { account in
            account.provider == provider && account.label == ran && account.signedIn
                && (account.usage?.windows ?? []).contains {
                    $0.kind == window.kind && $0.scope == window.scope && $0.percent >= 100
                        && sameReset(between: $0.resetsAt ?? 0, and: window.resetsAt ?? 0)
                }
        }
    }

    /// What an account has used of the same window, counting a window it does not report
    /// as spent: an account whose limits are unknown is not one to recommend.
    private static func used(_ account: Account, like window: Limit) -> Double {
        account.usage?.windows.first { $0.kind == window.kind && $0.scope == window.scope }?
            .percent ?? 100
    }

    /// What `Notifier.told` is keyed by: the tool, the account and the window. Keyed by
    /// window alone, one tool's exhausted five hours would silence another's.
    static func key(_ provider: String, _ label: String, _ window: Limit) -> String {
        [provider, label, window.kind, window.scope ?? ""].joined(separator: "/")
    }

    var key: String { Self.key(provider, ran, window) }

    /// How a person names the window that ran out.
    var limit: String { limitName(limit: window) }

    /// The panel's sentence about it.
    var said: String {
        let about = tool.map { "\($0): " } ?? ""
        return "\(about)\(ran) has none of its \(limit) limit left. "
            + "\(use) has \(left)% of its own left."
    }

    var notification: UNNotificationContent {
        let content = UNMutableNotificationContent()
        content.title = "\(ran) has no \(limit) limit left"
        if let tool { content.subtitle = tool }
        content.body = "\(use) has \(left)% of its own left."
        content.categoryIdentifier = Notifier.category
        content.userInfo = ["label": switchTo]
        return content
    }
}
