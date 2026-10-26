// probbit Monitor: one window around the page `probbit monitor --serve` draws. The page comes from the bundled probbit on
// 127.0.0.1; the app loads nothing else, keeps no web data on disk and sends nothing anywhere.
import AppKit
import UniformTypeIdentifiers
import WebKit

/// What the engine is asked to show
enum Source: Equatable {
    case demo(String)
    case paths([String])

    var label: String {
        switch self {
        case .demo(let k): return "demo \(k)"
        case .paths(let ps): return ps.map { ($0 as NSString).lastPathComponent }.joined(separator: ", ")
        }
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate, NSMenuDelegate, WKNavigationDelegate, WKUIDelegate {
    private var window: NSWindow!
    private var web: WKWebView!
    private var welcome: NSView!
    private var note: NSTextField!
    private let engine = Engine()
    private var page: URL?
    private var source: Source?
    private var titleWatch: NSKeyValueObservation?
    private var signals: [DispatchSourceSignal] = []
    private var started = false
    /// Files opened from Finder or the Dock at launch arrive before the window exists
    private var pending: Source?
    private let recentMenu = NSMenu(title: "Open Recent")
    private let defaults = UserDefaults.standard
    private static let maxRecent = 10

    private var binary: String { Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/probbit").path }

    // MARK: launch and quit

    func applicationDidFinishLaunching(_ n: Notification) {
        for s in [SIGTERM, SIGINT, SIGHUP] {
            signal(s, SIG_IGN)
            let src = DispatchSource.makeSignalSource(signal: s, queue: .main)
            src.setEventHandler { NSApp.terminate(nil) }
            src.resume()
            signals.append(src)
        }
        buildMenu()
        buildWindow()
        // Files opened from Finder or the Dock arrive before this; the command line and the remembered selection come next.
        DispatchQueue.main.async { [self] in
            guard !started else { return }
            if let s = pending { show(s) }
            else if let s = commandLineSource() { show(s) }
            else if let last = defaults.stringArray(forKey: "last"), !last.isEmpty, last.allSatisfy(FileManager.default.fileExists) { show(.paths(last)) }
            else { showWelcome(nil) }
        }
    }

    func applicationWillTerminate(_ n: Notification) { engine.stop() }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool { true }

    func application(_ application: NSApplication, open urls: [URL]) {
        let ps = urls.filter(\.isFileURL).map(\.path)
        guard !ps.isEmpty else { return }
        if window == nil { pending = .paths(ps) } else { show(.paths(ps)) }
    }

    /// `--demo [week|drives]`, or strand paths, given on the command line (`open -a "probbit Monitor" --args --demo week`)
    private func commandLineSource() -> Source? {
        let a = Array(CommandLine.arguments.dropFirst())
        if let i = a.firstIndex(of: "--demo") {
            let k = i + 1 < a.count && ["week", "drives"].contains(a[i + 1]) ? a[i + 1] : "week"
            return .demo(k)
        }
        let ps = a.filter { !$0.hasPrefix("-") && FileManager.default.fileExists(atPath: $0) }
            .map { URL(fileURLWithPath: $0).standardizedFileURL.path }
        return ps.isEmpty ? nil : .paths(ps)
    }

    // MARK: the engine

    /// The arguments for `probbit`. `single` is the form every probbit since 0.8 accepts: one strand. The full form passes
    /// every file, and a folder as `--dir`, for a probbit that watches several strands at once.
    private func arguments(_ s: Source, single: Bool) -> [String]? {
        switch s {
        case .demo(let k): return ["monitor", "--demo", k, "--serve", "--port", "0"]
        case .paths(let ps):
            var files: [String] = [], dirs: [String] = []
            for p in ps {
                var isDir: ObjCBool = false
                if FileManager.default.fileExists(atPath: p, isDirectory: &isDir) { if isDir.boolValue { dirs.append(p) } else { files.append(p) } }
            }
            if single {
                guard let one = files.first ?? dirs.lazy.compactMap(newestStrand).first else { return nil }
                return ["monitor", one, "--serve", "--port", "0"]
            }
            return ["monitor"] + files + dirs.flatMap { ["--dir", $0] } + ["--serve", "--port", "0"]
        }
    }

    /// Needs the fallback: more than one strand, or a folder
    private func isMulti(_ s: Source) -> Bool {
        guard case .paths(let ps) = s else { return false }
        if ps.count > 1 { return true }
        var isDir: ObjCBool = false
        return FileManager.default.fileExists(atPath: ps[0], isDirectory: &isDir) && isDir.boolValue
    }

    /// The most recently changed `*.strand` file in a folder
    private func newestStrand(in dir: String) -> String? {
        let fm = FileManager.default
        let u = URL(fileURLWithPath: dir)
        guard let items = try? fm.contentsOfDirectory(at: u, includingPropertiesForKeys: [.contentModificationDateKey], options: [.skipsHiddenFiles]) else { return nil }
        let strands = items.filter { $0.pathExtension == "strand" }
        func date(_ x: URL) -> Date { (try? x.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast }
        return strands.max { date($0) < date($1) }?.path
    }

    private func show(_ s: Source) {
        started = true
        source = s
        page = nil
        window.subtitle = s.label
        if case .paths(let ps) = s {
            defaults.set(ps, forKey: "last")
            remember(ps)
        }
        launch(s, single: !isMulti(s))
    }

    private func launch(_ s: Source, single: Bool) {
        guard FileManager.default.isExecutableFile(atPath: binary) else { return showWelcome("This copy of the app has no probbit engine inside it (\(binary)).") }
        // The full form first; a probbit that takes one strand at a time refuses it, and the first strand is shown alone.
        let full = !single
        guard let args = arguments(s, single: single) else { return showWelcome("No .strand file found in \(s.label).") }
        engine.start(binary: binary, args: args) { [weak self] ev in
            guard let self, self.source == s else { return }
            switch ev {
            case .ready(let u):
                self.page = u
                if !full && self.isMulti(s) { self.window.subtitle = s.label + " (this probbit shows one strand at a time)" }
                self.welcome.isHidden = true
                self.web.isHidden = false
                self.web.load(URLRequest(url: u))
            case .failed(let why):
                if full && self.page == nil { self.launch(s, single: true) } else { self.page = nil; self.showWelcome(why) }
            }
        }
    }

    // MARK: window, page, first-run view

    private func buildWindow() {
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1280, height: 800),
                          styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView], backing: .buffered, defer: false)
        window.title = "probbit Monitor"
        window.minSize = NSSize(width: 560, height: 420)
        window.delegate = self
        window.isReleasedWhenClosed = false
        window.center()
        window.setFrameAutosaveName("main")
        window.titlebarAppearsTransparent = false

        let cfg = WKWebViewConfiguration()
        cfg.websiteDataStore = .nonPersistent()
        cfg.preferences.javaScriptCanOpenWindowsAutomatically = false
        web = WKWebView(frame: .zero, configuration: cfg)
        web.navigationDelegate = self
        web.uiDelegate = self
        web.allowsMagnification = true
        web.isHidden = true
        titleWatch = web.observe(\.title, options: [.new]) { [weak self] w, _ in
            let t = w.title ?? ""
            MainActor.assumeIsolated { self?.pageTitle(t) }
        }

        welcome = buildWelcome()
        let root = NSView()
        for v in [web!, welcome!] {
            v.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(v)
        }
        NSLayoutConstraint.activate([
            web.leadingAnchor.constraint(equalTo: root.leadingAnchor), web.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            web.topAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor), web.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            welcome.centerXAnchor.constraint(equalTo: root.centerXAnchor), welcome.centerYAnchor.constraint(equalTo: root.centerYAnchor),
            welcome.widthAnchor.constraint(lessThanOrEqualToConstant: 520),
            welcome.leadingAnchor.constraint(greaterThanOrEqualTo: root.leadingAnchor, constant: 24),
        ])
        window.contentView = root
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    /// The first-run view: what the app does and two ways in. Also shown, with the reason, when the engine stops.
    private func buildWelcome() -> NSView {
        let icon = NSImageView(image: NSApp.applicationIconImage)
        icon.imageScaling = .scaleProportionallyUpOrDown
        icon.widthAnchor.constraint(equalToConstant: 96).isActive = true
        icon.heightAnchor.constraint(equalToConstant: 96).isActive = true
        let head = NSTextField(labelWithString: "probbit Monitor")
        head.font = .systemFont(ofSize: 22, weight: .semibold)
        let body = NSTextField(wrappingLabelWithString: "Open a strand file, or a folder of strands, to watch an individual's state as probbit replays it. Or play the synthetic demo week.")
        body.alignment = .center
        body.textColor = .secondaryLabelColor
        note = NSTextField(wrappingLabelWithString: "")
        note.isSelectable = true
        note.font = .monospacedSystemFont(ofSize: 11, weight: .regular)
        note.textColor = .systemRed
        note.isHidden = true
        let open = NSButton(title: "Open a strand…", target: self, action: #selector(openDocument(_:)))
        open.keyEquivalent = "\r"
        let demo = NSButton(title: "Try the demo", target: self, action: #selector(tryDemo(_:)))
        let buttons = NSStackView(views: [open, demo])
        buttons.spacing = 12
        let stack = NSStackView(views: [icon, head, body, buttons, note])
        stack.orientation = .vertical
        stack.alignment = .centerX
        stack.spacing = 14
        stack.setCustomSpacing(22, after: body)
        return stack
    }

    private func showWelcome(_ why: String?) {
        web.isHidden = true
        web.loadHTMLString("", baseURL: nil)
        welcome.isHidden = false
        note.stringValue = why ?? ""
        note.isHidden = why == nil
        if why == nil { window.subtitle = "" }
        window.title = "probbit Monitor"
        NSApp.dockTile.badgeLabel = nil
    }

    /// The window title is the page's title; the Dock badge is its leading glyph when the title starts with one
    private func pageTitle(_ t: String) {
        guard !web.isHidden else { return }
        window.title = t.isEmpty ? "probbit Monitor" : t
        let first = t.split(separator: " ", maxSplits: 1).first.map(String.init) ?? ""
        let glyph = first.unicodeScalars.first.map { $0.properties.isEmoji && !$0.isASCII } ?? false
        NSApp.dockTile.badgeLabel = glyph ? first : nil
    }

    func windowWillClose(_ n: Notification) { engine.stop() }

    // Only the engine's own page is loaded in the window. A link the user clicks to anywhere else opens in their browser.
    func webView(_ w: WKWebView, decidePolicyFor a: WKNavigationAction, decisionHandler: @escaping @MainActor (WKNavigationActionPolicy) -> Void) {
        guard let u = a.request.url else { return decisionHandler(.cancel) }
        if ["about", "data", "blob"].contains(u.scheme ?? "") || (u.host == "127.0.0.1" && u.port == page?.port) { return decisionHandler(.allow) }
        if a.navigationType == .linkActivated, ["http", "https"].contains(u.scheme ?? "") { NSWorkspace.shared.open(u) }
        decisionHandler(.cancel)
    }

    func webView(_ w: WKWebView, createWebViewWith c: WKWebViewConfiguration, for a: WKNavigationAction, windowFeatures f: WKWindowFeatures) -> WKWebView? {
        if let u = a.request.url, ["http", "https"].contains(u.scheme ?? ""), !(u.host == "127.0.0.1" && u.port == page?.port) { NSWorkspace.shared.open(u) }
        return nil
    }

    // MARK: menu actions

    @objc func openDocument(_ sender: Any?) {
        let p = NSOpenPanel()
        p.message = "Choose one or more .strand files, or a folder of strands"
        p.canChooseFiles = true
        p.canChooseDirectories = true
        p.allowsMultipleSelection = true
        p.allowedContentTypes = [UTType(filenameExtension: "strand") ?? .data, .folder]
        p.beginSheetModal(for: window) { [weak self] r in
            guard r == .OK else { return }
            let ps = p.urls.map(\.path)
            MainActor.assumeIsolated { if !ps.isEmpty { self?.show(.paths(ps)) } }
        }
    }

    @objc func tryDemo(_ sender: Any?) { show(.demo("week")) }

    @objc func reloadPage(_ sender: Any?) { if page != nil { web.reload() } }

    /// Back to the page's root: the latest event, nothing held
    @objc func followLatest(_ sender: Any?) { if let u = page { web.load(URLRequest(url: u)) } }

    @objc func actualSize(_ sender: Any?) { web.pageZoom = 1; web.magnification = 1 }

    @objc func openRecent(_ sender: NSMenuItem) {
        guard let ps = sender.representedObject as? [String] else { return }
        let gone = ps.filter { !FileManager.default.fileExists(atPath: $0) }
        if gone.isEmpty { show(.paths(ps)) } else { showWelcome("Not found: \(gone.joined(separator: ", "))") }
    }

    @objc func clearRecent(_ sender: Any?) {
        defaults.removeObject(forKey: "recent")
        NSDocumentController.shared.clearRecentDocuments(nil)
    }

    private func remember(_ ps: [String]) {
        var r = (defaults.array(forKey: "recent") as? [[String]]) ?? []
        r.removeAll { $0 == ps }
        r.insert(ps, at: 0)
        defaults.set(Array(r.prefix(Self.maxRecent)), forKey: "recent")
        if ps.count == 1 { NSDocumentController.shared.noteNewRecentDocumentURL(URL(fileURLWithPath: ps[0])) }
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        guard menu === recentMenu else { return }
        menu.removeAllItems()
        let r = (defaults.array(forKey: "recent") as? [[String]]) ?? []
        for ps in r {
            let title = ps.map { ($0 as NSString).lastPathComponent }.joined(separator: ", ")
            let it = NSMenuItem(title: title, action: #selector(openRecent(_:)), keyEquivalent: "")
            it.representedObject = ps
            it.toolTip = ps.joined(separator: "\n")
            menu.addItem(it)
        }
        if !r.isEmpty { menu.addItem(.separator()) }
        let clear = NSMenuItem(title: "Clear Menu", action: #selector(clearRecent(_:)), keyEquivalent: "")
        clear.isEnabled = !r.isEmpty
        menu.addItem(clear)
    }

    // MARK: menus

    private func buildMenu() {
        let main = NSMenu()
        func sub(_ title: String, _ items: [NSMenuItem]) -> NSMenu {
            let m = NSMenu(title: title)
            items.forEach(m.addItem)
            let host = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            host.submenu = m
            main.addItem(host)
            return m
        }
        func item(_ t: String, _ a: Selector?, _ k: String = "", _ mods: NSEvent.ModifierFlags = .command) -> NSMenuItem {
            let i = NSMenuItem(title: t, action: a, keyEquivalent: k)
            i.keyEquivalentModifierMask = mods
            return i
        }
        let name = "probbit Monitor"
        _ = sub(name, [
            item("About \(name)", #selector(NSApplication.orderFrontStandardAboutPanel(_:))), .separator(),
            item("Hide \(name)", #selector(NSApplication.hide(_:)), "h"),
            item("Hide Others", #selector(NSApplication.hideOtherApplications(_:)), "h", [.command, .option]),
            item("Show All", #selector(NSApplication.unhideAllApplications(_:))), .separator(),
            item("Quit \(name)", #selector(NSApplication.terminate(_:)), "q"),
        ])
        let recent = NSMenuItem(title: "Open Recent", action: nil, keyEquivalent: "")
        recentMenu.delegate = self
        recent.submenu = recentMenu
        _ = sub("File", [
            item("Open…", #selector(openDocument(_:)), "o"), recent, .separator(),
            item("Try the Demo", #selector(tryDemo(_:))), .separator(),
            item("Close", #selector(NSWindow.performClose(_:)), "w"),
        ])
        _ = sub("Edit", [
            item("Cut", #selector(NSText.cut(_:)), "x"), item("Copy", #selector(NSText.copy(_:)), "c"),
            item("Paste", #selector(NSText.paste(_:)), "v"), item("Select All", #selector(NSText.selectAll(_:)), "a"),
        ])
        _ = sub("View", [
            item("Reload", #selector(reloadPage(_:)), "r"),
            item("Follow Latest", #selector(followLatest(_:)), "l"),
            item("Actual Size", #selector(actualSize(_:)), "0"), .separator(),
            item("Enter Full Screen", #selector(NSWindow.toggleFullScreen(_:)), "f", [.command, .control]),
        ])
        let win = sub("Window", [
            item("Minimize", #selector(NSWindow.performMiniaturize(_:)), "m"),
            item("Zoom", #selector(NSWindow.performZoom(_:))),
        ])
        NSApp.mainMenu = main
        NSApp.windowsMenu = win
    }
}
