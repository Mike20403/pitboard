/// The keys Pitboard keeps its own preferences under, named once so a view and the model
/// cannot drift apart on one.
enum DefaultsKey {
    /// What the menu bar item shows.
    static let menuBarShows = "menuBarShows"
    /// The account windows' stores each Pitboard directory made, by the directory's path.
    static let webStores = "webStores"
    /// The page each account window was last on, by the Pitboard directory's path and the
    /// window's store.
    static let windowPages = "windowPages"

    /// Where this app kept what the model now keeps in `app.json`, read once to hand over.
    enum Earlier {
        /// Whether this app had ever shown anyone anything.
        static let hasBeenSeen = "hasBeenSeen"
        /// The tools somebody said "Not Now" to a second account for, by code.
        static let secondAccountDeclined = "secondAccountDeclined"
        /// "Not Now" said before there was a second tool, which was about Claude Code.
        static let hideSecondAccountNudge = "hideSecondAccountNudge"

        static let all = [hasBeenSeen, secondAccountDeclined, hideSecondAccountNudge]
    }
}
