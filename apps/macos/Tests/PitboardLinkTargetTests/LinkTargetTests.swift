import Foundation
import PitboardLinkTarget
import Testing

/// A bundle in a folder of its own, whose Info.plist declares `scheme` under the key, or
/// nothing when it is nil. The folder is deleted when `body` returns.
private func withBundle(declaring scheme: String?, _ body: (Bundle) throws -> Void) throws {
    let folder = FileManager.default.temporaryDirectory
        .appending(path: "PitboardLinkTargetTests-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: folder) }
    let appex = folder.appending(path: "PitboardShare.appex")
    let contents = appex.appending(path: "Contents")
    try FileManager.default.createDirectory(at: contents, withIntermediateDirectories: true)
    var info: [String: Any] = [
        "CFBundleIdentifier": "com.usepitboard.tests.\(UUID().uuidString)"
    ]
    info[LinkTarget.schemeKey] = scheme
    try PropertyListSerialization.data(fromPropertyList: info, format: .xml, options: 0)
        .write(to: contents.appending(path: "Info.plist"))
    try body(try #require(Bundle(url: appex)))
}

@Test func theSchemeIsReadFromTheBundle() throws {
    #expect(LinkTarget.schemeKey == "PitboardURLScheme")
    try withBundle(declaring: "pitboard-debug") { bundle in
        #expect(LinkTarget.scheme(in: bundle) == "pitboard-debug")
    }
    try withBundle(declaring: nil) { bundle in
        #expect(LinkTarget.scheme(in: bundle) == nil)
    }
}

/// The app and its Share extension each declare their build's scheme under the key both
/// read it by, so the two never answer different schemes.
@Test func theAppAndItsExtensionDeclareTheSchemeUnderTheKey() throws {
    let macos = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    for plist in ["App/Info.plist", "ShareExtension/Info.plist"] {
        let data = try Data(contentsOf: macos.appending(path: plist))
        let info = try PropertyListSerialization.propertyList(from: data, format: nil)
        #expect(
            (info as? [String: Any])?[LinkTarget.schemeKey] as? String
                == "$(PITBOARD_URL_SCHEME)", "\(plist)")
    }
}

@Test func anExtensionFindsTheAppItIsIn() {
    let appex = URL(
        fileURLWithPath: "/Applications/Pitboard.app/Contents/PlugIns/PitboardShare.appex")
    #expect(LinkTarget.containingApp(of: appex)?.path == "/Applications/Pitboard.app")
    let debug = URL(
        fileURLWithPath:
            "/Users/x/DerivedData/Build/Products/Debug/Pitboard.app/Contents/PlugIns/"
            + "PitboardShare.appex")
    #expect(
        LinkTarget.containingApp(of: debug)?.path
            == "/Users/x/DerivedData/Build/Products/Debug/Pitboard.app")
    #expect(
        LinkTarget.containingApp(of: URL(fileURLWithPath: "/tmp/PitboardShare.appex")) == nil)
}
