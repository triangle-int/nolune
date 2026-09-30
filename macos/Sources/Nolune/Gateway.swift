import Foundation

/**
 * `Nolune --gateway`: the gateway's keeper, which the app starts as its child when it opens
 * (Service.swift). It runs the gateway, `node cli.js start`, as its own child and keeps it
 * running: again at once on SIGHUP (`nolune service restart`), again after a pause when it stops
 * by itself. It stops the gateway on SIGTERM, and when the app it came from is gone.
 *
 * As the app's child, the gateway and every command it runs count as the app for macOS: one Full
 * Disk Access switch, named nolune, covers them, and it stays on across updates and Node versions.
 */
enum Gateway {
	static func run() -> Never {
		let keeper = Keeper(runtime: .shared)
		keeper.begin()
		withExtendedLifetime(keeper) { dispatchMain() }
	}

	/// Where the keeper's pid is while it runs, for `nolune service restart|status`.
	static func pidFile(_ runtime: Runtime) -> URL {
		runtime.home.appendingPathComponent("gateway.pid")
	}
}

/// Everything happens on the main queue.
private final class Keeper {
	private let runtime: Runtime
	private let log: FileHandle
	private let pidFile: URL
	private let parent = getppid()
	/// The gateway's pid while it runs.
	private var child: pid_t?
	private var started = Date()
	private var pause: TimeInterval = 1
	private var stopping = false
	private var restarting = false
	private var sources: [DispatchSourceProtocol] = []

	init(runtime: Runtime) {
		self.runtime = runtime
		try? FileManager.default.createDirectory(at: runtime.home, withIntermediateDirectories: true)
		// The gateway starts in nolune's home.
		_ = FileManager.default.changeCurrentDirectoryPath(runtime.home.path)
		log = Keeper.openLog(runtime.logFile)
		pidFile = Gateway.pidFile(runtime)
	}

	func begin() {
		try? "\(getpid())\n".write(to: pidFile, atomically: true, encoding: .utf8)
		on(SIGTERM) { self.stop() }
		on(SIGINT) { self.stop() }
		on(SIGHUP) { self.restart() }
		// The app quit, or crashed: the gateway goes with it.
		let watch = DispatchSource.makeTimerSource(queue: .main)
		watch.schedule(deadline: .now() + 1, repeating: 1)
		watch.setEventHandler { if getppid() != self.parent { self.stop() } }
		watch.resume()
		sources.append(watch)
		launch()
	}

	private func launch() {
		let pid: pid_t
		do {
			pid = try spawn([runtime.node.path] + runtime.cli + ["start"])
		} catch {
			note("couldn't start the gateway: \(error.localizedDescription)")
			quit(1)
		}
		child = pid
		started = Date()
		Thread.detachNewThread {
			var status: Int32 = 0
			while waitpid(pid, &status, 0) == -1, errno == EINTR {}
			// Its exit code, or 128 and the signal that ended it, as a shell says.
			let code = status & 0x7F == 0 ? (status >> 8) & 0xFF : 128 + status & 0x7F
			DispatchQueue.main.async { self.ended(code) }
		}
	}

	/**
	 * Starts a program as a new one starts, with no signal blocked and the ones this keeper ignores
	 * back to what they do. Foundation's Process passes on the thread's: a dispatch queue's thread
	 * blocks them all, and a gateway that never gets SIGTERM can't be stopped, only killed.
	 */
	private func spawn(_ arguments: [String]) throws -> pid_t {
		#if canImport(Darwin)
		var actions: posix_spawn_file_actions_t?
		var attributes: posix_spawnattr_t?
		#else
		var actions = posix_spawn_file_actions_t()
		var attributes = posix_spawnattr_t()
		#endif
		posix_spawn_file_actions_init(&actions)
		posix_spawnattr_init(&attributes)
		defer {
			posix_spawn_file_actions_destroy(&actions)
			posix_spawnattr_destroy(&attributes)
		}
		posix_spawn_file_actions_addopen(&actions, 0, "/dev/null", O_RDONLY, 0)
		posix_spawn_file_actions_adddup2(&actions, log.fileDescriptor, 1)
		posix_spawn_file_actions_adddup2(&actions, log.fileDescriptor, 2)
		var none = sigset_t()
		sigemptyset(&none)
		var ours = sigset_t()
		sigemptyset(&ours)
		for sig in [SIGTERM, SIGINT, SIGHUP] { sigaddset(&ours, sig) }
		posix_spawnattr_setsigmask(&attributes, &none)
		posix_spawnattr_setsigdefault(&attributes, &ours)
		var flags = POSIX_SPAWN_SETSIGMASK | POSIX_SPAWN_SETSIGDEF
		#if canImport(Darwin)
		// None of the keeper's files open in the gateway but the three it's given.
		flags |= POSIX_SPAWN_CLOEXEC_DEFAULT
		#endif
		posix_spawnattr_setflags(&attributes, Int16(flags))

		let environment = runtime.environment().map { "\($0.key)=\($0.value)" }
		var pid = pid_t()
		let result = Keeper.withCStrings(arguments) { argv in
			Keeper.withCStrings(environment) { envp in
				posix_spawn(&pid, argv[0]!, &actions, &attributes, argv, envp)
			}
		}
		guard result == 0 else { throw POSIXError(POSIXErrorCode(rawValue: result) ?? .EINVAL) }
		return pid
	}

	private static func withCStrings<R>(_ strings: [String], _ body: ([UnsafeMutablePointer<CChar>?]) -> R) -> R {
		let pointers = strings.map { strdup($0) } + [nil]
		defer { pointers.forEach { free($0) } }
		return body(pointers)
	}

	private func ended(_ status: Int32) {
		child = nil
		if stopping { quit(status) }
		if restarting {
			restarting = false
			launch()
			return
		}
		// It stopped by itself: again after a pause, longer each time it soon stops again.
		pause = Date().timeIntervalSince(started) > 60 ? 1 : min(pause * 2, 30)
		note("the gateway stopped (\(status)); starting it again in \(Int(pause)) s")
		DispatchQueue.main.asyncAfter(deadline: .now() + pause) {
			if !self.stopping, self.child == nil { self.launch() }
		}
	}

	private func restart() {
		guard !stopping else { return }
		guard let child else { return launch() }
		restarting = true
		pause = 1
		end(child)
	}

	private func stop() {
		guard !stopping else { return }
		stopping = true
		guard let child else { quit(0) }
		end(child)
	}

	/// Asks the gateway to stop, which stops what it runs, and makes it after ten seconds.
	private func end(_ pid: pid_t) {
		kill(pid, SIGTERM)
		DispatchQueue.main.asyncAfter(deadline: .now() + 10) {
			// Not waited for yet, so the pid is still the gateway's.
			if self.child == pid { kill(pid, SIGKILL) }
		}
	}

	private func quit(_ status: Int32) -> Never {
		try? FileManager.default.removeItem(at: pidFile)
		exit(status)
	}

	private func on(_ sig: Int32, _ handler: @escaping () -> Void) {
		signal(sig, SIG_IGN)
		let source = DispatchSource.makeSignalSource(signal: sig, queue: .main)
		source.setEventHandler(handler: handler)
		source.resume()
		sources.append(source)
	}

	private func note(_ message: String) {
		log.write(Data("nolune: \(message)\n".utf8))
	}

	/// The gateway's log, where `nolune service logs` reads it, appended to.
	private static func openLog(_ url: URL) -> FileHandle {
		let manager = FileManager.default
		try? manager.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
		if !manager.fileExists(atPath: url.path) { _ = manager.createFile(atPath: url.path, contents: nil) }
		guard let handle = try? FileHandle(forWritingTo: url) else { return FileHandle.standardError }
		handle.seekToEndOfFile()
		return handle
	}
}
