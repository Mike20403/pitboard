import Foundation
import PitboardKit

// Which accounts have a window, the store that keeps each one's data, what each window is
// titled, the menus' entries and what forgetting an account deletes are the core's rules:
// `windowAccounts`, `windowOfStore`, `windowsOf`, `siteMenus`, `forgetMessage` and `storeId`.
// What is left here is the store as WebKit and the window's scene take it.

extension WindowAccount: Identifiable {
    /// The window's store, as WebKit names a store and as the account windows' scene keeps a
    /// window's value: one window per account. The core derives it as a UUID, so it always
    /// reads as one.
    public var id: UUID { UUID(uuidString: store)! }
}
