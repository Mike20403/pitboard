#!/bin/sh
# Builds the Rust the macOS app links, universal, each library with its generated Swift
# bindings, for the Swift package in apps/macos. Outputs are build products and are not
# committed:
#
# - PitboardFFI.xcframework and Sources/PitboardBindings: Pitboard's core, which the app
#   links through PitboardKit.
# - PitboardShareFFI.xcframework and Sources/PitboardShareBindings: pitboard-share-ffi, the
#   check of a shared link, which the Share extension links and nothing else does.
#
# The two never meet in one binary: each library carries its own copy of Rust's standard
# library, and scripts/build-app.sh fails a build where they do.
set -eu

cd "$(dirname "$0")/../../.."
export MACOSX_DEPLOYMENT_TARGET=14.0
out=apps/macos/build
package=apps/macos
rm -rf "$out"

# build <crate> <module> <bindings>: the crate's static library for both kinds of Mac, as
# <module>.xcframework, with its Swift bindings in Sources/<bindings>. The crate's own
# uniffi.toml names the bindings' module. The headers sit in a folder named for the module,
# where Clang looks for its module map, so the two frameworks' maps never share a path when
# Xcode gathers their headers.
build() {
    crate=$1
    module=$2
    library=lib$(echo "$crate" | tr - _).a
    work=$out/$module
    generated=$package/Sources/$3
    rm -rf "$generated"
    mkdir -p "$work/bindings" "$work/headers/$module" "$generated"
    for target in aarch64-apple-darwin x86_64-apple-darwin; do
        cargo build --locked --release -p "$crate" --target "$target"
    done
    lipo -create \
        "target/aarch64-apple-darwin/release/$library" \
        "target/x86_64-apple-darwin/release/$library" \
        -output "$work/$library"

    cargo run --locked --release -p uniffi-bindgen-swift -- \
        "target/aarch64-apple-darwin/release/$library" "$work/bindings" \
        --swift-sources --headers --modulemap \
        --module-name "$module" --modulemap-filename module.modulemap
    mv "$work/bindings"/*.h "$work/bindings/module.modulemap" "$work/headers/$module/"
    mv "$work/bindings"/*.swift "$generated/"

    rm -rf "$package/$module.xcframework"
    xcodebuild -create-xcframework \
        -library "$work/$library" -headers "$work/headers" \
        -output "$package/$module.xcframework"
}

build pitboard-ffi PitboardFFI PitboardBindings
build pitboard-share-ffi PitboardShareFFI PitboardShareBindings
