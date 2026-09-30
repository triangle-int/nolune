import AVFoundation

/**
 * The welcome's song and its one short sound, as src/lib/welcome/sounds.ts plays them: the song
 * carries the intro, then goes on much quieter under the questions.
 */
@MainActor
final class Music {
	/// The song's level under the questions.
	nonisolated static let under: Float = 0.15
	private nonisolated static let mutedKey = "sounds.muted"

	private let song: AVAudioPlayer?
	private var effects: [AVAudioPlayer] = []
	private var level: Float = 0

	var muted: Bool {
		didSet {
			UserDefaults.standard.set(muted, forKey: Music.mutedKey)
			apply(fade: 0.3)
		}
	}

	init() {
		muted = UserDefaults.standard.bool(forKey: Music.mutedKey)
		song = Music.player("music")
		song?.numberOfLoops = -1
		song?.volume = 0
		song?.prepareToPlay()
	}

	var isPlaying: Bool { song?.isPlaying ?? false }

	/// Plays the song from the start at `level`, rising in quickly.
	func play(level: Float = 1) {
		guard let song else { return }
		self.level = level
		song.currentTime = 0
		song.volume = 0
		song.play()
		apply(fade: 0.6)
	}

	/// The song goes on at `level`, over `fade` seconds.
	func duck(to level: Float, over fade: TimeInterval) {
		self.level = level
		apply(fade: fade)
	}

	/// Fades the song out and stops it.
	func stop(over fade: TimeInterval) {
		level = 0
		apply(fade: fade)
		DispatchQueue.main.asyncAfter(deadline: .now() + fade) { [weak self] in
			if self?.level == 0 { self?.song?.stop() }
		}
	}

	/// A short sound: `confirm` when a check passes.
	func play(effect name: String) {
		guard !muted, let player = Music.player(name) else { return }
		effects.removeAll { !$0.isPlaying }
		effects.append(player)
		player.play()
	}

	private func apply(fade: TimeInterval) {
		song?.setVolume(muted ? 0 : level, fadeDuration: fade)
	}

	private static func player(_ name: String) -> AVAudioPlayer? {
		guard let url = Bundle.main.url(forResource: name, withExtension: "mp3") else { return nil }
		return try? AVAudioPlayer(contentsOf: url)
	}
}
