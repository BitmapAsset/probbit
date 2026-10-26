// The engine process: the bundled `probbit monitor ... --serve --port 0`, run as a child of the app.
//
// How the child cannot outlive the app:
// - It is started with posix_spawn and POSIX_SPAWN_SETPGROUP, so it leads a new process group; quitting the app (menu, Cmd-Q,
//   SIGTERM, SIGINT, SIGHUP, last window closed) sends SIGTERM to that whole group, then SIGKILL after one second.
// - POSIX_SPAWN_CLOEXEC_DEFAULT: the child inherits only stdin, stdout and stderr, never another descriptor of the app.
// - A crash or a SIGKILL of the app runs no handler, so the group is led by a small /bin/sh guard rather than by probbit
//   itself: the guard's stdin is a pipe whose only write end lives in the app. When the app ends for any reason the kernel
//   closes that end, the guard's `read` returns, and the guard sends SIGTERM to probbit. No pid file, no polling.
import Foundation

/// The guard run by /bin/sh: $0 is the probbit binary, "$@" its arguments. probbit gets /dev/null as stdin and no fd 3.
private let guardScript = """
exec 3<&0 0</dev/null
"$0" "$@" 3<&- &
p=$!
{ read -r _ <&3; kill -TERM "$p"; } >/dev/null 2>&1 &
wait "$p"
"""

@MainActor
final class Engine {
    enum Event {
        case ready(URL)
        case failed(String)
    }

    private(set) var pid: pid_t = 0
    private var lifeline: Int32 = -1
    private var outBuf = Data()
    private var errTail = Data()
    private var url: URL?
    private var exitSource: DispatchSourceProcess?
    private var timeout: DispatchWorkItem?
    private var outHandle: FileHandle?
    private var errHandle: FileHandle?
    private var onEvent: ((Event) -> Void)?
    /// Bumped on every start, so output still queued from an earlier child is dropped
    private var generation = 0

    var isRunning: Bool { pid > 0 }

    /// Starts `binary args...`; `onEvent` gets `.ready` once with the page address, or `.failed` once (before or after ready).
    func start(binary: String, args: [String], onEvent: @escaping (Event) -> Void) {
        stop()
        generation += 1
        let gen = generation
        self.onEvent = onEvent
        outBuf = Data(); errTail = Data(); url = nil

        var inP: [Int32] = [-1, -1], outP: [Int32] = [-1, -1], errP: [Int32] = [-1, -1]
        guard pipe(&inP) == 0, pipe(&outP) == 0, pipe(&errP) == 0 else { return finish(.failed("cannot create pipes: \(String(cString: strerror(errno)))")) }
        for fd in [inP[1], outP[0], errP[0]] { _ = fcntl(fd, F_SETFD, FD_CLOEXEC) }

        var fa: posix_spawn_file_actions_t?
        posix_spawn_file_actions_init(&fa)
        posix_spawn_file_actions_adddup2(&fa, inP[0], 0)
        posix_spawn_file_actions_adddup2(&fa, outP[1], 1)
        posix_spawn_file_actions_adddup2(&fa, errP[1], 2)
        var attr: posix_spawnattr_t?
        posix_spawnattr_init(&attr)
        posix_spawnattr_setflags(&attr, Int16(POSIX_SPAWN_SETPGROUP | POSIX_SPAWN_CLOEXEC_DEFAULT | POSIX_SPAWN_SETSIGDEF | POSIX_SPAWN_SETSIGMASK))
        posix_spawnattr_setpgroup(&attr, 0)
        var none = sigset_t(), all = sigset_t()
        sigemptyset(&none); sigfillset(&all)
        posix_spawnattr_setsigmask(&attr, &none)
        posix_spawnattr_setsigdefault(&attr, &all)

        let argv = ["/bin/sh", "-c", guardScript, binary] + args
        var cargs: [UnsafeMutablePointer<CChar>?] = argv.map { strdup($0) } + [nil]
        var child: pid_t = 0
        let rc = posix_spawn(&child, "/bin/sh", &fa, &attr, &cargs, environ)
        cargs.forEach { free($0) }
        posix_spawn_file_actions_destroy(&fa)
        posix_spawnattr_destroy(&attr)
        close(inP[0]); close(outP[1]); close(errP[1])
        guard rc == 0 else {
            close(inP[1]); close(outP[0]); close(errP[0])
            return finish(.failed("cannot start \(binary): \(String(cString: strerror(rc)))"))
        }
        pid = child
        lifeline = inP[1]

        let out = FileHandle(fileDescriptor: outP[0], closeOnDealloc: true)
        let err = FileHandle(fileDescriptor: errP[0], closeOnDealloc: true)
        outHandle = out; errHandle = err
        out.readabilityHandler = { [weak self] h in
            let d = h.availableData
            if d.isEmpty { h.readabilityHandler = nil }
            DispatchQueue.main.async { MainActor.assumeIsolated { if self?.generation == gen { self?.gotOut(d) } } }
        }
        err.readabilityHandler = { [weak self] h in
            let d = h.availableData
            if d.isEmpty { h.readabilityHandler = nil }
            DispatchQueue.main.async { MainActor.assumeIsolated { if self?.generation == gen { self?.gotErr(d) } } }
        }

        let src = DispatchSource.makeProcessSource(identifier: child, eventMask: .exit, queue: .main)
        src.setEventHandler { [weak self] in MainActor.assumeIsolated { self?.exited(child) } }
        src.resume()
        exitSource = src

        let t = DispatchWorkItem { [weak self] in
            MainActor.assumeIsolated {
                guard let self, self.pid == child, self.url == nil else { return }
                self.stop()
                self.finish(.failed("probbit did not report a page address within 20 seconds" + self.stderrText(prefix: ":\n")))
            }
        }
        timeout = t
        DispatchQueue.main.asyncAfter(deadline: .now() + 20, execute: t)
    }

    /// SIGTERM to the whole process group (the guard, probbit, the guard's reader), SIGKILL if anything is left after 1 s.
    func stop() {
        timeout?.cancel(); timeout = nil
        exitSource?.cancel(); exitSource = nil
        outHandle?.readabilityHandler = nil; errHandle?.readabilityHandler = nil
        outHandle = nil; errHandle = nil
        if lifeline >= 0 { close(lifeline); lifeline = -1 }
        guard pid > 0 else { return }
        let p = pid
        pid = 0
        onEvent = nil
        kill(-p, SIGTERM)
        var status: Int32 = 0
        for _ in 0..<100 {
            if waitpid(p, &status, WNOHANG) != 0 { break }
            usleep(10_000)
        }
        if kill(-p, 0) == 0 { kill(-p, SIGKILL); _ = waitpid(p, &status, WNOHANG) }
    }

    private func gotOut(_ d: Data) {
        guard url == nil, !d.isEmpty else { return }
        outBuf.append(d)
        guard let nl = outBuf.firstIndex(of: 0x0A) else { return }
        let line = String(decoding: outBuf[..<nl], as: UTF8.self).trimmingCharacters(in: .whitespaces)
        // Only a loopback page is accepted: the app opens no other address.
        if let u = URL(string: line), u.scheme == "http", u.host == "127.0.0.1", u.port != nil {
            url = u
            timeout?.cancel(); timeout = nil
            onEvent?(.ready(u))
        } else {
            stop()
            finish(.failed("probbit printed an unexpected first line: \(line)"))
        }
    }

    private func gotErr(_ d: Data) {
        errTail.append(d)
        if errTail.count > 8192 { errTail = errTail.suffix(8192) }
    }

    private func exited(_ child: pid_t) {
        guard child == pid else { return }
        var status: Int32 = 0
        _ = waitpid(child, &status, 0)
        let how = (status & 0x7f) == 0 ? "exit status \((status >> 8) & 0xff)" : "signal \(status & 0x7f)"
        // The guard has ended; clear what is left of its group, and take what probbit wrote to stderr before it stopped.
        kill(-child, SIGTERM)
        if let h = errHandle {
            h.readabilityHandler = nil
            let fd = h.fileDescriptor
            _ = fcntl(fd, F_SETFL, fcntl(fd, F_GETFL) | O_NONBLOCK)
            var buf = [UInt8](repeating: 0, count: 4096)
            while true { let n = read(fd, &buf, buf.count); if n <= 0 { break }; gotErr(Data(buf[0..<n])) }
        }
        pid = 0
        let cb = onEvent
        stop()
        onEvent = cb
        // Output read just before the exit is still queued on the main queue: report after it.
        let gen = generation
        DispatchQueue.main.async { [weak self] in
            MainActor.assumeIsolated {
                guard let self, self.generation == gen else { return }
                self.finish(.failed("probbit stopped (\(how))" + self.stderrText(prefix: ":\n")))
            }
        }
    }

    private func stderrText(prefix: String) -> String {
        let s = String(decoding: errTail, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        return s.isEmpty ? "" : prefix + s
    }

    private func finish(_ e: Event) {
        let cb = onEvent
        onEvent = nil
        cb?(e)
    }
}
