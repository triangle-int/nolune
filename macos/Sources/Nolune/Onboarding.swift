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

	init() {
		music = Music()
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
	 * Space, to the song: stars come out, one of them draws the wordmark, its three dots type,
	 * then fly out as the eight avatar colors and orbit it, pooling into a glow behind the welcome
	 * as the song lifts. About 24 seconds, as on the web; a click or Esc skips it.
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
		music.play()
		let start = Date()
		introStart = start
		sky.stage = .stars

		func at(_ seconds: TimeInterval) async -> Bool {
			let wait = start.addingTimeInterval(seconds).timeIntervalSinceNow
			if wait > 0 { try? await Task.sleep(nanoseconds: UInt64(wait * 1_000_000_000)) }
			return phase == .intro && !skipped
		}

		// The dots break loose as eight colors, and circle the wordmark like planets.
		guard await at(IntroTiming.orbit) else { return }
		sky.origins = layout.dotCenters
		sky.center = layout.center
		sky.stage = .orbit

		// They pool into a glow as the wordmark fades.
		guard await at(IntroTiming.aurora) else { return }
		sky.stage = .aurora

		// Space gives way to the page as the song lifts.
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
		go(.account)
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

/// The intro's beats, in seconds from its start, as the web times them to the song.
enum IntroTiming {
	/// A star at the middle brightens and draws the letters.
	static let draw: TimeInterval = 9
	static let drawLength: TimeInterval = 2.8
	/// The three dots type.
	static let typing = draw + drawLength
	static let beat: TimeInterval = 0.62
	static let gap: TimeInterval = 0.18
	/// They break loose and orbit.
	static let orbit = typing + 2 * beat + 2 * gap
	/// They pool into a glow; the wordmark fades.
	static let aurora = orbit + 7
	static let dawn = aurora + 2.4
	static let welcome = dawn + 0.8
}
