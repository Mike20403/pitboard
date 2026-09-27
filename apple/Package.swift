// swift-tools-version: 6.2
import PackageDescription

// Everything the app does, as libraries its tests can load without starting the app. The app
// itself is Pitboard.xcodeproj, which links PitboardApp, adds Sparkle, and carries the UI
// tests.
let package = Package(
    name: "Pitboard",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "PitboardKit", targets: ["PitboardKit"]),
        .library(name: "PitboardApp", targets: ["PitboardApp"]),
    ],
    targets: [
        // Both built by scripts/build-xcframework.sh and not committed.
        .binaryTarget(name: "PitboardFFI", path: "PitboardFFI.xcframework"),
        .target(
            name: "PitboardBindings",
            dependencies: ["PitboardFFI"],
            // UniFFI's generated code does not yet meet Swift 6's strict concurrency checks.
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .target(name: "PitboardKit", dependencies: ["PitboardBindings"]),
        .target(name: "PitboardApp", dependencies: ["PitboardKit"]),
        .testTarget(name: "PitboardKitTests", dependencies: ["PitboardKit"]),
        .testTarget(name: "PitboardAppTests", dependencies: ["PitboardApp"]),
    ]
)
