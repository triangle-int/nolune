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
		ZStack {
			if let clock = Snapshot.clock {
				Canvas { context, _ in
					BigBang.draw(&context, layout: layout, t: clock)
				}
			} else if let start = onboarding.introStart {
				TimelineView(.animation) { timeline in
					Canvas { context, _ in
						BigBang.draw(&context, layout: layout, t: timeline.date.timeIntervalSince(start))
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
 */
enum BigBang {
	private static let warm = Color(hex: 0xFFF1D6)

	static func draw(_ context: inout GraphicsContext, layout: IntroLayout, t: Double) {
		let center = layout.center
		let cx = Double(center.x)
		let cy = Double(center.y)
		let extent = Double(max(layout.size.width, layout.size.height))

		if t >= IntroTiming.gather, t < IntroTiming.bang {
			let p = (t - IntroTiming.gather) / (IntroTiming.bang - IntroTiming.gather)
			let tremble: Double = 1 + 0.12 * p * sin(t * 47) * sin(t * 13)
			let r: Double = (1 + 5 * p * p) * tremble
			let reach: Double = r * (5 + 12 * p * p)
			var glow = context
			glow.opacity = 0.35 + 0.65 * p
			glow.fill(
				Path(ellipseIn: circle(cx, cy, reach)),
				with: .radialGradient(
					Gradient(colors: [warm, warm.opacity(0.3), warm.opacity(0)]),
					center: center, startRadius: 0, endRadius: reach
				)
			)
			glow.opacity = 0.6 + 0.4 * p
			glow.fill(Path(ellipseIn: circle(cx, cy, r)), with: .color(.white))
		}

		let since = t - IntroTiming.bang
		guard since >= 0 else { return }

		// The shock wave, and a fainter one in a color after it.
		let waves: [(lag: Double, color: Color, strength: Double)] = [
			(0, .white, 0.7),
			(0.18, Theme.avatars[3], 0.45)
		]
		for wave in waves {
			let s = (since - wave.lag) / 1.5
			guard s > 0, s < 1 else { continue }
			let radius: Double = easeOut(s) * extent * 0.85
			var ring = context
			ring.opacity = wave.strength * (1 - s)
			ring.stroke(Path(ellipseIn: circle(cx, cy, radius)), with: .color(wave.color), lineWidth: 1 + 5 * (1 - s))
		}

		// The flash: all light for a moment, fading from the edges in.
		let f = since / 0.8
		if f < 1 {
			var flash = context
			flash.opacity = pow(1 - f, 2)
			let reach: Double = extent * (0.35 + 1.1 * f)
			flash.fill(
				Path(CGRect(origin: .zero, size: layout.size)),
				with: .radialGradient(
					Gradient(colors: [.white, warm.opacity(0.85), warm.opacity(0)]),
					center: center, startRadius: 0, endRadius: reach
				)
			)
		}
	}

	private static func circle(_ x: Double, _ y: Double, _ r: Double) -> CGRect {
		CGRect(x: x - r, y: y - r, width: r * 2, height: r * 2)
	}
}
