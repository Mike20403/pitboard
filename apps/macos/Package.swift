// swift-tools-version: 6.2
import PackageDescription

// Everything the app does, as libraries its tests can load without starting the app. The app
// itself is Pitboard.xcodeproj, which links PitboardApp, adds Sparkle, and carries the UI
// tests and the Share extension. The extension links PitboardShareBindings and
// PitboardLinkTarget alone: it is sandboxed, and has no business with the core, its bindings
// or anything else the app links.
let package = Package(
    name: "Pitboard",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "PitboardKit", targets: ["PitboardKit"]),
        .library(name: "PitboardApp", targets: ["PitboardApp"]),
        .library(name: "PitboardShareBindings", targets: ["PitboardShareBindings"]),
        .library(name: "PitboardLinkTarget", targets: ["PitboardLinkTarget"]),
    ],
    targets: [
        // Each built with its bindings by scripts/build-xcframework.sh, and not committed.
        .binaryTarget(name: "PitboardFFI", path: "PitboardFFI.xcframework"),
        .binaryTarget(name: "PitboardShareFFI", path: "PitboardShareFFI.xcframework"),
        .target(
            name: "PitboardBindings",
            dependencies: ["PitboardFFI"],
            // UniFFI's generated code does not yet meet Swift 6's strict concurrency checks.
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        // The check of a shared link and the Pitboard link that hands it over, from Rust's
        // pitboard-sites, for the Share extension alone. Nothing here links it with
        // PitboardFFI: two Rust static libraries in one binary each bring their own standard
        // library. So no test target here uses it, since a SwiftPM may build every test
        // target into one bundle; pitboard-share-ffi's tests are Rust's.
        .target(
            name: "PitboardShareBindings",
            dependencies: ["PitboardShareFFI"],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .target(name: "PitboardKit", dependencies: ["PitboardBindings"]),
        // Where a Pitboard link goes: the scheme each build declares, and the app a Share
        // extension is inside. Foundation only, so the app and its extension both link it,
        // and a test target may too.
        .target(name: "PitboardLinkTarget"),
        .target(name: "PitboardApp", dependencies: ["PitboardKit", "PitboardLinkTarget"]),
        .testTarget(name: "PitboardKitTests", dependencies: ["PitboardKit"]),
        .testTarget(name: "PitboardLinkTargetTests", dependencies: ["PitboardLinkTarget"]),
        .testTarget(name: "PitboardAppTests", dependencies: ["PitboardApp"]),
    ]
)
