import AppKit

/// Another app on this Mac, by its bundle id, that pitboard may quit and open again around a
/// switch: one that runs a tool for itself and keeps the tool's login in memory while it is
/// open, as ChatGPT does with Codex's.
///
/// A protocol so a test drives the whole quit, switch and reopen without touching a real
/// app. Only quitting the way Command-Q does and opening are offered: never a forced quit,
/// which would lose whatever the app had not saved.
@MainActor
public protocol AppControl: AnyObject {
    func isRunning(_ bundleID: String) -> Bool
    /// Asks the app to quit the way Command-Q does, which lets it ask about work in
    /// progress. Returns at once; the app may take a while, or decline.
    func requestQuit(_ bundleID: String)
    /// Opens the app the way Finder would.
    func open(_ bundleID: String)
}

extension AppControl {
    /// Asks the app to quit and waits until it has, for as long as `limit`. True once it is
    /// gone, or when it was not running; false when it is still running, as it was.
    func quit(
        _ bundleID: String, within limit: Duration,
        checkingEvery interval: Duration = .milliseconds(200)
    ) async -> Bool {
        guard isRunning(bundleID) else { return true }
        requestQuit(bundleID)
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: limit)
        while isRunning(bundleID) {
            guard clock.now < deadline else { return false }
            try? await Task.sleep(for: interval)
        }
        return true
    }
}

/// The apps running on this Mac, as macOS's own list of them has it.
@MainActor
public final class WorkspaceAppControl: AppControl {
    public init() {}

    private func running(_ bundleID: String) -> [NSRunningApplication] {
        NSRunningApplication.runningApplications(withBundleIdentifier: bundleID)
            .filter { !$0.isTerminated }
    }

    public func isRunning(_ bundleID: String) -> Bool { !running(bundleID).isEmpty }

    public func requestQuit(_ bundleID: String) {
        for app in running(bundleID) { app.terminate() }
    }

    public func open(_ bundleID: String) {
        guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID)
        else { return }
        NSWorkspace.shared.openApplication(at: url, configuration: .init())
    }
}
