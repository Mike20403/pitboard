/// The keys pitboard keeps its own preferences under, named once so a view and the model
/// cannot drift apart on one.
enum DefaultsKey {
    /// Whether this app has ever shown anyone anything.
    static let hasBeenSeen = "hasBeenSeen"
    /// The tools somebody said "Not Now" to a second account for, by code.
    static let secondAccountDeclined = "secondAccountDeclined"
    /// What the menu bar item shows.
    static let menuBarShows = "menuBarShows"
}
