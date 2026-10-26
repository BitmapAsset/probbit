# probbit Monitor for macOS

`probbit monitor STRAND --open` serves the monitor page on 127.0.0.1 and opens it in the browser. **probbit Monitor** is the
same page in its own Mac window: an app you open from Finder, the Dock or Launchpad, with the probbit binary inside it. The
page itself is the one described in [persona.md §5.8](persona.md#58-monitor-watch-an-individuals-inner-state); what it looks
like: [probbit-monitor.png](probbit-monitor.png).

## Get it

From the [release page](https://github.com/BitmapAsset/probbit/releases): `probbit-Monitor-<version>-macos.zip` and its
`.sha256`. Check it (`shasum -a 256 -c probbit-Monitor-<version>-macos.zip.sha256`), unzip it, move **probbit Monitor** to
Applications. It runs on macOS 13 or later, Apple silicon and Intel (one universal app). No installer, nothing else to get.

### The first launch: the app is not notarized

The app is signed ad-hoc (`codesign --sign -`), not with an Apple Developer ID, and it is not notarized: notarization needs a
paid Apple Developer account. macOS therefore refuses the first launch of a downloaded copy ("Apple could not verify ...").
Allow it once:

- macOS 15 and later: open the app once (it is refused), then System Settings → Privacy & Security → **Open Anyway**, and
  confirm.
- macOS 13 and 14: right-click (or Control-click) the app → **Open** → **Open**.
- From a terminal, instead: `xattr -dr com.apple.quarantine "/Applications/probbit Monitor.app"`.

After that it opens like any app. A copy you build yourself (below) has no quarantine mark and opens directly.

## Use it

- **Open a strand**: double-click a `.strand` file (the app registers the `dev.probbit.strand` type for the `.strand`
  extension), drag it onto the Dock icon, use File → Open… (⌘O), or "Open a strand…" in the first window. The dialog takes
  several files, or a folder.
- **Try the demo**: the first window's second button, or File → Try the Demo: the tutor's synthetic week
  (`probbit monitor --demo week`), no strand needed.
- **Several strands, a folder**: the app passes every file you open, and a folder as `--dir FOLDER`, to its probbit. A probbit
  that watches one strand at a time refuses that; the app then shows the first strand you chose (for a folder, its most
  recently changed `*.strand` file) and says so under the window title.
- The window title is the page's title. When the page's title starts with a glyph, the Dock icon shows that glyph as its badge.
- File → Open Recent lists what you opened; the app reopens the last selection when it starts with nothing else to open.
- View → Reload (⌘R) reloads the page; **Follow Latest** (⌘L) loads the page's root again, the latest event with no event
  held; Actual Size (⌘0) undoes a pinch zoom. Appearance (dark, light) follows the system, as the page does.
- From a terminal: `open -a "probbit Monitor" pip.strand`, or `open -a "probbit Monitor" --args --demo week`.

## What it runs

The app is a small Swift program (AppKit and WebKit, no third-party code, no Xcode project) in
`apps/macos/ProbbitMonitor/`. The bundle:

```text
probbit Monitor.app/Contents/
  MacOS/ProbbitMonitor     the window (universal: arm64, x86_64)
  MacOS/probbit            the engine, the same binary as the release archives (universal in the release zip)
  Resources/AppIcon.icns   made from docs/brand/logo.svg
  Info.plist               dev.probbit.monitor, version = the probbit version
```

On a strand the app runs `Contents/MacOS/probbit monitor <files> --serve --port 0`, reads the `http://127.0.0.1:PORT/` line
the engine prints, and loads that page in its window. Opening something else stops that engine and starts another.

**The engine never outlives the app.** It is started with `posix_spawn` and `POSIX_SPAWN_SETPGROUP` (a process group of its
own) and `POSIX_SPAWN_CLOEXEC_DEFAULT` (it inherits no descriptor of the app beyond stdin, stdout and stderr). Quitting the
app (⌘Q, closing the window, `SIGTERM`, `SIGINT`, `SIGHUP`) sends `SIGTERM` to the group, then `SIGKILL` after one second.
A crash or a `SIGKILL` of the app runs no code, so the group is led by a few lines of `/bin/sh` whose stdin is a pipe only the
app can write to: when the app ends, for any reason, the kernel closes that pipe, the shell's `read` returns, and it stops
the engine. CI checks both cases (`smoke.sh --quit term` and `--quit kill9`).

**Network and data.** The engine listens on 127.0.0.1 only (`probbit monitor` has no option to listen elsewhere). The window
loads only that address; a link on the page to anywhere else opens in your browser. The web view keeps no cookies, cache or
storage on disk. The app sends nothing anywhere, has no telemetry, no updater and no preferences window. It stores, in its
own user defaults (`dev.probbit.monitor`), the last selection, the recent list and the window frame.

## Build it

Needs the Xcode command-line tools (`xcode-select --install`: `swiftc`, `lipo`, `codesign`, `ditto`) and a probbit binary.

```sh
cargo build --release -p probbit-cli
apps/macos/ProbbitMonitor/build.sh                      # bundles target/release/probbit
apps/macos/ProbbitMonitor/build.sh --binary PATH        # or any probbit binary
apps/macos/ProbbitMonitor/smoke.sh "apps/macos/ProbbitMonitor/dist/probbit Monitor.app"   # launch, check, quit
```

`build.sh` compiles the shell for arm64 and x86_64 with `swiftc -O -warnings-as-errors`, joins them with `lipo`, assembles the
bundle, signs it ad-hoc (`codesign --verify --deep --strict` must pass), and writes
`dist/probbit-Monitor-<version>-macos.zip` and its `.sha256` (`--asset-version` names the zip, `--out` moves the output). The
zip carries no extended attributes, so any unzip tool gives a bundle whose signature verifies. `smoke.sh` launches the app,
checks that its engine answers `GET /` with 200 on 127.0.0.1 and nowhere else, quits it (`--quit term`, `osascript`, or
`kill9`) and checks that no engine process is left. `make-icon.sh` remakes `Resources/AppIcon.icns` from the logo with
headless Chrome; the `.icns` is committed, so a build needs no browser.

CI (`ci.yml`, job `macos-app`) runs `build.sh` around the probbit built from each commit, then `smoke.sh` twice. On a `v*` tag,
`release.yml` (job `macos-app`) joins the two macOS release binaries into a universal one with `lipo`, runs the same
`build.sh` and `smoke.sh`, and adds `probbit-Monitor-<tag>-macos.zip` and its `.sha256` to the draft release.

## Linux and Windows

There is no app for them: `probbit monitor STRAND --open` opens the same page in the browser, `--serve` prints its address.
