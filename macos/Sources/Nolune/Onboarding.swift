import AppKit
import SwiftUI

/**
 * The first run: the intro in space, then one question per screen, like the web welcome
 * (packages/web/src/routes/p/[slug]/welcome). Who's setting it up, Full Disk Access, and the gateway started
 * in the background.
 */
@MainActor
final class Onboarding: ObservableObject {
	nonisolated static let doneKey = "onboarded"
	private nonisolated static let phaseKey = "onboarding.phase"

	enum Phase: String {
		case intro, welcome, account, files, service
	}

	/// The steps after the welcome, for the bars at the top.
	nonisolated static let steps: [Phase] = [.account, .files, .service]

	enum ServiceState: Equatable {
		case starting
		case needsApproval
		case failed(String)
		case ready
	}

	@Published private(set) var phase: Phase
	/// When the intro's clock started; the wordmark is drawn from it.
	@Published private(set) var introStart: Date?
	@Published private(set) var skipped = false

	@Published var name = ""
	@Published var email = ""
	@Published var password = Onboarding.newPassword()
	@Published private(set) var working = false
	@Published private(set) var problem: String?

	@Published private(set) var granted = false
	@Published private(set) var service = ServiceState.starting

	let sky = Sky()
	let music: Music
	var onFinish: (() -> Void)?
	/// Whether there's an admin already (an npm install, or a run that stopped halfway), found
	/// while the intro plays: the account step is skipped then, before it shows.
	private var admin: Task<Bool, Never>?

	init() {
		music = Music()
		if !Snapshot.active {
			admin = Task { await Runtime.shared.people()?.contains(where: \.isAdmin) ?? false }
		}
		// Relaunched partway (System Settings' "Quit & Reopen" after Full Disk Access): back
		// where it was, without the intro.
		let saved = UserDefaults.standard.string(forKey: Onboarding.phaseKey).flatMap(Phase.init(rawValue:))
		if let saved, Onboarding.steps.contains(saved) {
			phase = saved
			skipped = true
			sky.still = true
			sky.space = false
			sky.stage = .aurora
		} else {
			phase = .intro
		}
	}

	private func go(_ next: Phase) {
		withAnimation(.easeInOut(duration: 0.45)) { phase = next }
		UserDefaults.standard.set(next.rawValue, forKey: Onboarding.phaseKey)
	}

	// MARK: Intro

	/**
	 * A big bang (IntroView.swift): a point of light gathers in the dark and bursts, the song
	 * starts, the stars fly out of it and the eight avatar colors after them, pooling into a glow
	 * behind the welcome. About four seconds; the full intro is the web welcome's, next. A click or
	 * Esc skips it.
	 */
	func startIntro(layout: IntroLayout) async {
		guard !Snapshot.active else { return }
		guard phase == .intro, introStart == nil else {
			if skipped, !music.isPlaying { music.play(level: Music.under) }
			return
		}
		if NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
			skipIntro()
			return
		}
		let start = Date()
		introStart = start

		func at(_ seconds: TimeInterval) async -> Bool {
			let wait = start.addingTimeInterval(seconds).timeIntervalSinceNow
			if wait > 0 { try? await Task.sleep(nanoseconds: UInt64(wait * 1_000_000_000)) }
			return phase == .intro && !skipped
		}

		// It bursts: the song starts, and the stars fly out of the flash.
		guard await at(IntroTiming.bang) else { return }
		music.play()
		sky.burst = true
		sky.stage = .stars

		// The colors follow and pool into a glow.
		guard await at(IntroTiming.colors) else { return }
		sky.center = layout.center
		sky.stage = .aurora

		// Space gives way to the page.
		guard await at(IntroTiming.dawn) else { return }
		sky.space = false
		guard await at(IntroTiming.welcome) else { return }
		go(.welcome)

		// From here the welcome waits; the song rings on a moment, then goes quiet.
		try? await Task.sleep(nanoseconds: 2_500_000_000)
		if phase == .welcome { music.duck(to: Music.under, over: 4) }
	}

	func skipIntro() {
		guard phase == .intro else { return }
		if music.isPlaying { music.duck(to: Music.under, over: 1.2) } else { music.play(level: Music.under) }
		skipped = true
		sky.still = true
		sky.space = false
		sky.stage = .aurora
		go(.welcome)
	}

	func begin() {
		music.duck(to: Music.under, over: 2.5)
		Task {
			let skip = await admin?.value ?? false
			if phase == .welcome { go(skip ? .files : .account) }
		}
	}

	// MARK: Account

	/// Set up already, from an npm install or a run that stopped halfway: skip to the next step.
	func checkAccount() async {
		guard !Snapshot.active, let people = await Runtime.shared.people(), people.contains(where: \.isAdmin) else { return }
		if phase == .account { go(.files) }
	}

	var canCreateAccount: Bool {
		!working && !name.trimmingCharacters(in: .whitespaces).isEmpty && email.contains("@")
			&& !password.isEmpty
	}

	func createAccount() async {
		guard canCreateAccount else { return }
		working = true
		problem = nil
		let runtime = Runtime.shared
		let origin = runtime.config.origin ?? runtime.localURL.absoluteString
		let output = await runtime.run([
			"setup",
			"--name", name.trimmingCharacters(in: .whitespaces),
			"--email", email.trimmingCharacters(in: .whitespaces),
			"--password", password,
			"--origin", origin
		])
		working = false
		if output.succeeded {
			go(.files)
		} else {
			problem = output.problem
		}
	}

	/// Like `generatePassword` in packages/core/src/users.ts: four groups of six, easy to read.
	nonisolated static func newPassword() -> String {
		let alphabet = Array("abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789")
		return (0..<4).map { _ in String((0..<6).map { _ in alphabet.randomElement()! }) }
			.joined(separator: "-")
	}

	// MARK: Files

	/// Waits for the switch: checks every second until it's on, or the step is left.
	func watchAccess() async {
		guard !Snapshot.active else { return }
		DiskAccess.appearInList()
		while phase == .files, !granted {
			if await DiskAccess.isGranted() {
				accessGranted()
				return
			}
			try? await Task.sleep(nanoseconds: 1_000_000_000)
		}
	}

	private func accessGranted() {
		withAnimation(.spring(response: 0.5, dampingFraction: 0.7)) { granted = true }
		sky.bloom = 1
		music.play(effect: "confirm")
		// Back from System Settings to see it.
		NSApp.activate(ignoringOtherApps: true)
		Task {
			try? await Task.sleep(nanoseconds: 2_200_000_000)
			if phase == .files { go(.service) }
		}
	}

	func skipAccess() {
		go(.service)
	}

	// MARK: Service

	func startService() async {
		guard !Snapshot.active else { return }
		service = .starting
		// Run from a checkout (`swift run`) there's no app to register: `nolune start` instead.
		guard Runtime.shared.isBundled else {
			finish()
			return
		}
		switch Service.start() {
		case .running:
			break
		case .needsApproval:
			service = .needsApproval
			while phase == .service, Service.agent.status == .requiresApproval {
				try? await Task.sleep(nanoseconds: 1_000_000_000)
			}
			guard phase == .service else { return }
			service = .starting
		case let .failed(message):
			service = .failed(message)
			return
		}
		if await Service.waitUntilUp() {
			finish()
		} else {
			service = .failed("It didn't answer. The log may say why.")
		}
	}

	private func finish() {
		withAnimation(.easeInOut(duration: 0.5)) { service = .ready }
		// The menu bar extra comes in now: "it lives up here".
		UserDefaults.standard.set(true, forKey: Onboarding.doneKey)
		UserDefaults.standard.removeObject(forKey: Onboarding.phaseKey)
		music.stop(over: 6)
	}

	/// For `--snapshot`: a screen as it would be, without getting there.
	func pose(_ phase: Phase, granted: Bool = false, service: ServiceState = .starting) {
		self.phase = phase
		self.granted = granted
		self.service = service
		if phase != .intro {
			sky.still = true
			sky.space = false
			sky.stage = .aurora
		}
	}

	func openNolune() {
		NSWorkspace.shared.open(Runtime.shared.origin)
		sky.stage = .gone
		music.stop(over: 1)
		onFinish?()
	}
}

/// The intro's beats, in seconds from its start.
enum IntroTiming {
	/// A point of light gathers in the middle.
	static let gather: TimeInterval = 0.3
	/// It bursts, and the stars fly out of it.
	static let bang: TimeInterval = 1.3
	/// The eight colors follow and pool into a glow.
	static let colors = bang + 0.6
	/// Space gives way to the page.
	static let dawn = bang + 2.3
	static let welcome = dawn + 0.8
}
