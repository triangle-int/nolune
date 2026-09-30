import SwiftUI

/// The window's size, and its middle, where the big bang happens.
struct IntroLayout {
	let size: CGSize

	var center: CGPoint { CGPoint(x: Double(size.width) / 2, y: Double(size.height) / 2) }
}

/// The intro over the sky: the big bang. A click or Esc skips it.
struct IntroView: View {
	@ObservedObject var onboarding: Onboarding
	let layout: IntroLayout

	var body: some View {
		let burstsOut = onboarding.burstsOut
		ZStack {
			if let clock = Snapshot.clock {
				Canvas { context, _ in
					BigBang.draw(&context, layout: layout, t: clock, burstsOut: burstsOut)
				}
			} else if let start = onboarding.introStart {
				TimelineView(.animation) { timeline in
					Canvas { context, _ in
						BigBang.draw(&context, layout: layout, t: timeline.date.timeIntervalSince(start), burstsOut: burstsOut)
					}
				}
			}
		}
		.contentShape(Rectangle())
		.onTapGesture { onboarding.skipIntro() }
	}
}

/**
 * The app's intro, short: the full one, with the wordmark and the song's first minute, is the
 * web welcome's, which comes next. In the dark a point of light gathers, trembling, and bursts: a
 * flash, a shock wave, and the stars fly out of it (Sky's `burst`), the eight colors after them.
 * Out over the desktop (Outburst.swift), the burst is drawn there, over the window and around it.
 */
enum BigBang {
	static let warm = Color(hex: 0xFFF1D6)
	/// The shock wave, and a fainter one in a color after it.
	private static let waves: [(lag: Double, color: Color, strength: Double)] = [
		(0, .white, 0.7),
		(0.18, Theme.avatars[3], 0.45)
	]

	/// `burstsOut`: the outburst draws the burst, out over the desktop; the window only the gathering.
	static func draw(_ context: inout GraphicsContext, layout: IntroLayout, t: Double, burstsOut: Bool = false) {
		let center = layout.center
		gather(&context, center: center, t: t)

		let since = t - IntroTiming.bang
		guard since >= 0, !burstsOut else { return }
		let extent = Double(max(layout.size.width, layout.size.height))
		shockWaves(&context, center: center, radius: extent * 0.85, length: 1.5, since: since)
		flash(&context, center: center, extent: extent, since: since)
	}

	/// A point of light, trembling and swelling until it bursts.
	private static func gather(_ context: inout GraphicsContext, center: CGPoint, t: Double) {
		guard t >= IntroTiming.gather, t < IntroTiming.bang else { return }
		let p = (t - IntroTiming.gather) / (IntroTiming.bang - IntroTiming.gather)
		let tremble: Double = 1 + 0.12 * p * sin(t * 47) * sin(t * 13)
		let r: Double = (1 + 5 * p * p) * tremble
		let reach: Double = r * (5 + 12 * p * p)
		var glow = context
		glow.opacity = 0.35 + 0.65 * p
		glow.fill(
			Path(ellipseIn: circle(center, reach)),
			with: .radialGradient(
				Gradient(colors: [warm, warm.opacity(0.3), warm.opacity(0)]),
				center: center, startRadius: 0, endRadius: reach
			)
		)
		glow.opacity = 0.6 + 0.4 * p
		glow.fill(Path(ellipseIn: circle(center, r)), with: .color(.white))
	}

	/// The shock waves, `since` the bang: out to `radius` over `length` seconds, fast then slowing.
	static func shockWaves(_ context: inout GraphicsContext, center: CGPoint, radius: Double, length: Double, since: Double) {
		for wave in waves {
			let s = (since - wave.lag) / length
			guard s > 0, s < 1 else { continue }
			let ring = Path(ellipseIn: circle(center, easeOut(s) * radius))
			var haze = context
			haze.opacity = wave.strength * (1 - s) * 0.25
			haze.stroke(ring, with: .color(wave.color), lineWidth: 4 + 14 * (1 - s))
			var edge = context
			edge.opacity = wave.strength * (1 - s)
			edge.stroke(ring, with: .color(wave.color), lineWidth: 1 + 5 * (1 - s))
		}
	}

	/// The flash, `since` the bang: all light for a moment, fading from the edges in, about `extent` wide.
	static func flash(_ context: inout GraphicsContext, center: CGPoint, extent: Double, since: Double) {
		let f = since / 0.8
		guard f >= 0, f < 1 else { return }
		var flash = context
		flash.opacity = pow(1 - f, 2)
		let reach: Double = extent * (0.35 + 1.1 * f)
		flash.fill(
			Path(ellipseIn: circle(center, reach)),
			with: .radialGradient(
				Gradient(colors: [.white, warm.opacity(0.85), warm.opacity(0)]),
				center: center, startRadius: 0, endRadius: reach
			)
		)
	}

	static func circle(_ center: CGPoint, _ r: Double) -> CGRect {
		CGRect(x: Double(center.x) - r, y: Double(center.y) - r, width: r * 2, height: r * 2)
	}
}
