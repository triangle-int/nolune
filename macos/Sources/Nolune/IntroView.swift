import SwiftUI

/// Where the wordmark sits in the window, and where its dots are, in window points.
struct IntroLayout {
	let size: CGSize

	var width: Double { Double(size.width) * 0.5 }
	/// Window points per logo unit.
	var unit: Double { width / Double(Wordmark.size.width) }
	var height: Double { Double(Wordmark.size.height) * unit }
	var center: CGPoint { CGPoint(x: Double(size.width) / 2, y: Double(size.height) * 0.5) }
	var origin: CGPoint { CGPoint(x: Double(center.x) - width / 2, y: Double(center.y) - height / 2) }
	/// Where the star first brightens: the middle of the window.
	var spark: CGPoint { CGPoint(x: Double(size.width) / 2, y: Double(size.height) / 2) }

	func point(_ logo: CGPoint) -> CGPoint {
		CGPoint(x: Double(origin.x) + Double(logo.x) * unit, y: Double(origin.y) + Double(logo.y) * unit)
	}

	var dotCenters: [CGPoint] { Wordmark.dots.map { point($0.center) } }
}

/// The intro over the sky: the wordmark drawn by a star, its dots typing, and a way to skip.
struct IntroView: View {
	@ObservedObject var onboarding: Onboarding
	let layout: IntroLayout
	@State private var hintShown = false

	var body: some View {
		ZStack {
			if let start = onboarding.introStart {
				TimelineView(.animation) { timeline in
					Canvas { context, _ in
						IntroWordmark.draw(&context, layout: layout, t: timeline.date.timeIntervalSince(start))
					}
				}
			}
			VStack {
				Spacer()
				Text("Click anywhere to skip")
					.font(Theme.font(12))
					.foregroundStyle(Theme.muted.opacity(0.55))
					.padding(.bottom, 22)
					.opacity(hintShown ? 1 : 0)
			}
		}
		.contentShape(Rectangle())
		.onTapGesture { onboarding.skipIntro() }
		.onAppear {
			withAnimation(.easeInOut(duration: 1.2).delay(2)) { hintShown = true }
		}
	}
}

/**
 * The wordmark's part of the intro, as the web animates it (welcome/+page@.svelte): a star
 * brightens in the middle, sweeps across and the letters appear behind it, it lands as the first
 * of three dots, they bounce like someone typing, then burst into the planets.
 */
enum IntroWordmark {
	static func draw(_ context: inout GraphicsContext, layout: IntroLayout, t: Double) {
		guard t >= IntroTiming.draw else { return }
		var c = context
		// The wordmark leaves as the planets pool into the glow.
		if t >= IntroTiming.aurora {
			let k = cubicBezier(0.42, 0, 1, 1, (t - IntroTiming.aurora) / 1.4)
			if k >= 1 { return }
			let scale = 1 - 0.03 * k
			c.opacity = 1 - k
			c.translateBy(x: layout.center.x, y: Double(layout.center.y) - 16 * k)
			c.scaleBy(x: scale, y: scale)
			c.translateBy(x: -layout.center.x, y: -layout.center.y)
		}

		let unit = layout.unit
		let progress = (t - IntroTiming.draw) / IntroTiming.drawLength
		let rect = CGRect(x: layout.origin.x, y: layout.origin.y, width: layout.width, height: layout.height)
		let letters = WordmarkLetters().path(in: rect)
		if progress >= 1 {
			c.fill(letters, with: .color(.white))
		} else if progress >= 0.5 {
			// The letters appear behind the light as it sweeps across them.
			let sweep = cubicBezier(0.45, 0, 0.25, 1, (progress - 0.5) / 0.5)
			let shown = sweep * Double(Wordmark.dots[0].center.x) * unit
			var clipped = c
			clipped.clip(to: Path(CGRect(x: rect.minX, y: rect.minY - 40, width: shown, height: rect.height + 80)))
			clipped.fill(letters, with: .color(.white))
		}

		for (index, dot) in Wordmark.dots.enumerated() {
			let home = layout.point(dot.center)
			var center = home
			var scale = 1.0
			var opacity = dot.opacity
			var glow = 0.0
			if progress < 1 {
				// Only the first dot, the star, is out while it draws.
				guard index == 0 else { continue }
				(center, scale, glow) = star(progress, home: home, layout: layout)
			} else if t < IntroTiming.orbit {
				let bounce = typing(index, t: t - IntroTiming.typing)
				center = CGPoint(x: Double(home.x), y: Double(home.y) + bounce * unit)
			} else {
				// They break loose: the planets take over from here.
				let k = clamp((t - IntroTiming.orbit) / 0.25)
				if k >= 1 { continue }
				opacity *= 1 - k
				scale = 1 + 0.8 * k
			}
			let r = Double(dot.radius) * unit * scale
			if glow > 0 {
				let reach = r * 3.2
				var halo = c
				halo.opacity = glow
				halo.fill(
					Path(ellipseIn: CGRect(x: Double(center.x) - reach, y: Double(center.y) - reach, width: reach * 2, height: reach * 2)),
					with: .radialGradient(
						Gradient(colors: [.white.opacity(0.55), .white.opacity(0)]),
						center: center, startRadius: 0, endRadius: reach
					)
				)
			}
			var body = c
			body.opacity = opacity
			body.fill(
				Path(ellipseIn: CGRect(x: Double(center.x) - r, y: Double(center.y) - r, width: r * 2, height: r * 2)),
				with: .color(.white)
			)
		}
	}

	/// The star: brightens at the middle, moves to the letters' start, then sweeps to its place.
	private static func star(_ p: Double, home: CGPoint, layout: IntroLayout) -> (CGPoint, Double, Double) {
		let spark = layout.spark
		let start = CGPoint(x: Double(layout.origin.x) + 40 * layout.unit, y: Double(home.y))
		var position = spark
		var scale = 1.0
		if p < 0.22 {
			scale = 1.35 * cubicBezier(0.2, 0.8, 0.2, 1, p / 0.22)
		} else if p < 0.36 {
			scale = 1.35 - 0.35 * cubicBezier(0.42, 0, 0.58, 1, (p - 0.22) / 0.14)
		} else if p < 0.5 {
			position = mix(spark, start, cubicBezier(0.6, 0, 0.4, 1, (p - 0.36) / 0.14))
		} else {
			position = mix(start, home, cubicBezier(0.45, 0, 0.25, 1, (p - 0.5) / 0.5))
		}
		// Its glow: up as it brightens, holding through the sweep, gone as it lands.
		let glow: Double
		if p < 0.22 {
			glow = p / 0.22
		} else if p < 0.9 {
			glow = 1 - 0.3 * (p - 0.22) / 0.68
		} else {
			glow = 0.7 * (1 - (p - 0.9) / 0.1)
		}
		return (position, scale, glow)
	}

	/// Dot `index` bouncing, twice, a beat after the one before: its lift in logo units.
	private static func typing(_ index: Int, t: Double) -> Double {
		let local = t - Double(index) * IntroTiming.gap
		guard local >= 0, local < 2 * IntroTiming.beat else { return 0 }
		let phase = cubicBezier(0.42, 0, 0.58, 1, local.truncatingRemainder(dividingBy: IntroTiming.beat) / IntroTiming.beat)
		if phase < 0.3 { return -55 * phase / 0.3 }
		if phase < 0.6 { return -55 * (1 - (phase - 0.3) / 0.3) }
		return 0
	}

	private static func mix(_ a: CGPoint, _ b: CGPoint, _ k: Double) -> CGPoint {
		CGPoint(x: Double(a.x) + (Double(b.x) - Double(a.x)) * k, y: Double(a.y) + (Double(b.y) - Double(a.y)) * k)
	}
}
