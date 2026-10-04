import AppKit
import Foundation
import WebKit

/// One file an account window is downloading, or has downloaded, into the downloads folder.
@MainActor
@Observable
final class Transfer: Identifiable {
    enum State: Equatable {
        /// Waiting for a destination, or for the person to say it may go ahead.
        case starting
        case running
        case finished(URL)
        case failed(String)
        case cancelled
    }

    let id = UUID()
    /// The account window's store, whose window lists it.
    let store: UUID
    private(set) var name: String
    private(set) var state = State.starting
    /// Where the file goes, once it is decided.
    private(set) var destination: URL?
    /// WebKit's progress for the download, with the bytes written so far.
    @ObservationIgnored let progress: Progress
    @ObservationIgnored fileprivate let download: WKDownload

    init(download: WKDownload, store: UUID, name: String) {
        self.download = download
        self.store = store
        self.name = name
        progress = download.progress
    }

    var isRunning: Bool { state == .starting || state == .running }

    /// Shows the downloaded file in Finder.
    func reveal() {
        guard case .finished(let file) = state else { return }
        NSWorkspace.shared.activateFileViewerSelecting([file])
    }

    fileprivate func started(at destination: URL) {
        guard state == .starting else { return }
        self.destination = destination
        name = destination.lastPathComponent
        state = .running
    }

    fileprivate func end(_ state: State) {
        guard isRunning else { return }
        self.state = state
    }
}

/// Every download the account windows start, from deciding where each goes to its end.
///
/// Downloads carry on after their window closes: the person asked for the file, not for the
/// window to stay open. WebKit quarantines each file itself, as a browser's are, and makes a
/// suggested name safe; what is left here is never overwriting a file and asking before a
/// frame or a page off the site saves one.
@MainActor
@Observable
final class DownloadCenter: NSObject, WKDownloadDelegate {
    private(set) var transfers: [Transfer] = []
    @ObservationIgnored private let folder: URL
    /// Destinations given to downloads still running, which no other may be given.
    @ObservationIgnored private var reserved: Set<URL> = []
    @ObservationIgnored private var asking: [ObjectIdentifier: (Transfer, Bool)] = [:]

    init(folder: URL) {
        self.folder = folder
    }

    /// The downloads still running, which quitting would stop.
    var running: [Transfer] { transfers.filter(\.isRunning) }

    /// The downloads of the account window on `store`, newest first.
    func transfers(for store: UUID) -> [Transfer] {
        transfers.filter { $0.store == store }.reversed()
    }

    /// Takes over `download`, which a page of `account`'s window started, asking the person
    /// first when `asking` says to.
    func start(_ download: WKDownload, for account: WindowAccount, asking: Bool) {
        let transfer = Transfer(
            download: download, store: account.store,
            name: download.originalRequest?.url?.lastPathComponent ?? "Download")
        transfers.append(transfer)
        self.asking[ObjectIdentifier(download)] = (transfer, asking)
        download.delegate = self
    }

    /// Takes away the downloads that have ended.
    func clearEnded(for store: UUID) {
        transfers.removeAll { $0.store == store && !$0.isRunning }
    }

    /// Stops every download running and deletes what each had written, as Pitboard quits.
    func stopAll() {
        for transfer in running {
            transfer.download.cancel()
            if let file = transfer.destination { try? FileManager.default.removeItem(at: file) }
            transfer.end(.cancelled)
        }
    }

    /// Stops `transfer`, frees its name, and deletes what it had written. WebKit says nothing
    /// more about a download the app cancels.
    func cancel(_ transfer: Transfer) {
        guard transfer.isRunning else { return }
        asking[ObjectIdentifier(transfer.download)] = nil
        let file = transfer.destination
        transfer.end(.cancelled)
        transfer.download.cancel { [weak self] _ in
            // WebKit does not say which thread this is called on.
            Task { @MainActor [weak self] in
                guard let file else { return }
                try? FileManager.default.removeItem(at: file)
                self?.reserved.remove(file)
            }
        }
    }

    // MARK: - WebKit's questions

    func download(
        _ download: WKDownload, decideDestinationUsing response: URLResponse,
        suggestedFilename: String
    ) async -> URL? {
        guard let (transfer, ask) = asking.removeValue(forKey: ObjectIdentifier(download))
        else {
            return nil
        }
        if ask {
            let host = Self.host(of: download)
            let allowed = await PageDialogs.allowDownload(
                suggestedFilename, from: host, in: download.webView?.window)
            guard allowed else {
                transfer.end(.cancelled)
                return nil
            }
        }
        // It may have been cancelled, or failed, while the question was up.
        guard transfer.state == .starting else { return nil }
        let destination = Self.destination(
            suggested: suggestedFilename, in: folder,
            taken: { [reserved] in reserved.contains($0) || Self.exists($0) })
        reserved.insert(destination)
        transfer.started(at: destination)
        return destination
    }

    func downloadDidFinish(_ download: WKDownload) {
        guard let transfer = transfer(of: download), let file = transfer.destination else {
            return
        }
        reserved.remove(file)
        transfer.end(.finished(file))
    }

    func download(_ download: WKDownload, didFailWithError error: Error, resumeData: Data?) {
        asking[ObjectIdentifier(download)] = nil
        guard let transfer = transfer(of: download) else { return }
        let cancelled = (error as NSError).code == NSURLErrorCancelled
        if let file = transfer.destination {
            // Part of a file under its real name would pass for the whole of it.
            if !cancelled { try? FileManager.default.removeItem(at: file) }
            reserved.remove(file)
        }
        transfer.end(cancelled ? .cancelled : .failed(error.localizedDescription))
    }

    private func transfer(of download: WKDownload) -> Transfer? {
        transfers.first { $0.progress === download.progress }
    }

    // MARK: - Names

    /// Where a download named `suggested` is saved in `folder`: the name, then `report 2.pdf`,
    /// `report 3.pdf` and so on while `taken` says a name is. WebKit wants a file that does
    /// not exist yet and cancels without a word otherwise, and two downloads can ask at once.
    /// WebKit has already made the name safe: measured on macOS 27, it turns
    /// `../../evil:name.txt` into `_.._evil_name.txt`.
    nonisolated static func destination(
        suggested: String, in folder: URL, taken: (URL) -> Bool
    ) -> URL {
        var name = (suggested as NSString).lastPathComponent
        if name.isEmpty || name == "/" { name = "Download" }
        let base = (name as NSString).deletingPathExtension
        let suffix = (name as NSString).pathExtension
        var candidate = folder.appendingPathComponent(name)
        var number = 2
        while taken(candidate) {
            let numbered = suffix.isEmpty ? "\(base) \(number)" : "\(base) \(number).\(suffix)"
            candidate = folder.appendingPathComponent(numbered)
            number += 1
        }
        return candidate
    }

    nonisolated private static func exists(_ url: URL) -> Bool {
        FileManager.default.fileExists(atPath: url.path)
    }

    /// The site a download comes from, as its question names it: the host of the frame that
    /// started it where macOS says, else of its address. Foundation reads no host from a
    /// `blob:` link, so the one inside it is read instead.
    static func host(of download: WKDownload) -> String? {
        // A download the app started has an empty frame, whose host is empty.
        if #available(macOS 15.2, *) {
            let host = download.originatingFrame.securityOrigin.host
            if !host.isEmpty { return host }
        }
        return host(of: download.originalRequest?.url)
    }

    nonisolated static func host(of url: URL?) -> String? {
        guard let url else { return nil }
        if url.scheme?.lowercased() == "blob" {
            return URL(string: String(url.absoluteString.dropFirst("blob:".count)))?.host
        }
        return url.host
    }
}
