import Foundation

/// How the app keeps itself up to date, as the menu and the settings see it.
///
/// The updater is Sparkle, which the app target links and this library does not: its tests
/// and its previews then need no framework that only a signed bundle can load. A build from
/// a clone carries no update key, and there the updater is not available at all.
@MainActor
public protocol Updates: AnyObject, Observable {
    /// Whether this build can update itself.
    var available: Bool { get }
    /// The version waiting to be installed, once a scheduled check has found one.
    var waiting: String? { get }
    var checksAutomatically: Bool { get set }
    var installsAutomatically: Bool { get set }
    /// Shows the updater's own window: what it finds, what changed, and its install button.
    func check()
}

/// For a build that cannot update itself.
@MainActor
@Observable
public final class NoUpdates: Updates {
    public init() {}
    public var available: Bool { false }
    public var waiting: String? { nil }
    public var checksAutomatically = false
    public var installsAutomatically = false
    public func check() {}
}
