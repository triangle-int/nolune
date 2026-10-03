import CoreText
import SwiftUI

/**
 * The web app's dark theme (packages/web/src/routes/layout.css), which the onboarding always wears.
 * The iOS app's first screens wear it too (ios/), so this and Sky.swift build for both.
 */
enum Theme {
	/// Deep space behind the intro: almost black, a little blue (IntroSky's `SPACE`).
	static let space = Color(red: 4 / 255, green: 6 / 255, blue: 14 / 255)
	static let background = Color(hex: 0x212121)
	static let foreground = Color(hex: 0xECECEC)
	static let muted = Color(hex: 0xAFAFAF)
	static let card = Color(hex: 0x2A2A2A)
	static let border = Color.white.opacity(0.1)
	static let input = Color.white.opacity(0.15)
	static let primaryForeground = Color(hex: 0x0D0D0D)

	/// The eight avatars' colors, in `AVATARS` order (packages/core/src/avatars.ts), dark theme.
	static let avatars: [Color] = ([
		0x70E0C4, // probe
		0xFE9042, // campfire
		0xFEE57F, // lantern
		0xA293FD, // planet
		0x85EBBD, // quantum
		0x8ED2FD, // comet
		0xFCEDD2, // moon
		0xFD767B // satellite
	] as [UInt32]).map { Color(hex: $0) }

	// MARK: Type

	/// Figtree, the web app's typeface: a variable font, set by weight on its `wght` axis.
	private static var figtree: CTFontDescriptor?

	static func registerFonts() {
		guard let url = Bundle.main.url(forResource: "Figtree", withExtension: "ttf") else { return }
		CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
		let descriptors = CTFontManagerCreateFontDescriptorsFromURL(url as CFURL) as? [CTFontDescriptor]
		figtree = descriptors?.first
	}

	/// Figtree at `size` and `weight` (100–900); the system font without it.
	static func font(_ size: CGFloat, weight: CGFloat = 400) -> Font {
		guard let figtree else { return .system(size: size, weight: systemWeight(weight)) }
		let wght = NSNumber(value: 0x7767_6874) as CFNumber // 'wght'
		let descriptor = CTFontDescriptorCreateCopyWithVariation(figtree, wght, weight)
		return Font(CTFontCreateWithFontDescriptor(descriptor, size, nil))
	}

	static func mono(_ size: CGFloat) -> Font {
		.system(size: size, weight: .regular, design: .monospaced)
	}

	private static func systemWeight(_ weight: CGFloat) -> Font.Weight {
		switch weight {
		case ..<350: return .light
		case ..<450: return .regular
		case ..<550: return .medium
		case ..<650: return .semibold
		default: return .bold
		}
	}
}

extension Color {
	init(hex: UInt32, opacity: Double = 1) {
		self.init(
			.sRGB,
			red: Double((hex >> 16) & 0xFF) / 255,
			green: Double((hex >> 8) & 0xFF) / 255,
			blue: Double(hex & 0xFF) / 255,
			opacity: opacity
		)
	}
}

// MARK: Controls, as the web welcome draws them

/// The off-white pill: the welcome's main button.
struct PillButtonStyle: ButtonStyle {
	var large = false
	@Environment(\.isEnabled) private var isEnabled

	func makeBody(configuration: Configuration) -> some View {
		configuration.label
			.font(Theme.font(large ? 16 : 14, weight: 500))
			.foregroundStyle(Theme.primaryForeground)
			.padding(.horizontal, large ? 30 : 22)
			.frame(height: large ? 46 : 40)
			.background(Capsule().fill(Theme.foreground))
			.opacity(isEnabled ? (configuration.isPressed ? 0.85 : 1) : 0.5)
			.scaleEffect(configuration.isPressed ? 0.98 : 1)
			.animation(.easeOut(duration: 0.15), value: configuration.isPressed)
			.contentShape(Capsule())
	}
}

/// A quieter pill, outlined.
struct OutlineButtonStyle: ButtonStyle {
	func makeBody(configuration: Configuration) -> some View {
		configuration.label
			.font(Theme.font(13, weight: 500))
			.foregroundStyle(Theme.foreground)
			.padding(.horizontal, 18)
			.frame(height: 34)
			.background(Capsule().fill(Color.white.opacity(configuration.isPressed ? 0.1 : 0.04)))
			.overlay(Capsule().strokeBorder(Theme.input))
			.contentShape(Capsule())
	}
}

/// Plain gray text that reads as a link.
struct QuietLinkStyle: ButtonStyle {
	func makeBody(configuration: Configuration) -> some View {
		configuration.label
			.font(Theme.font(12))
			.foregroundStyle(Theme.muted.opacity(configuration.isPressed ? 0.6 : 1))
			.contentShape(Rectangle())
	}
}

/// A text field on the welcome: dark, rounded, with a hairline border.
struct WelcomeField: View {
	let placeholder: String
	@Binding var text: String
	var monospaced = false
	/// A password's, which shows dots.
	var secure = false

	var body: some View {
		field
			.textFieldStyle(.plain)
			.font(monospaced ? Theme.mono(14) : Theme.font(15))
			.foregroundStyle(Theme.foreground)
			.padding(.horizontal, 16)
			.frame(height: 44)
			.background(RoundedRectangle(cornerRadius: 12).fill(Theme.card.opacity(0.7)))
			.overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Theme.input))
	}

	@ViewBuilder private var field: some View {
		let prompt = Text(placeholder).foregroundColor(Theme.muted.opacity(0.7))
		if secure {
			SecureField("", text: $text, prompt: prompt)
		} else {
			TextField("", text: $text, prompt: prompt)
		}
	}
}

/// The welcome's progress: a short bar per step, lit up to the current one.
struct StepBars: View {
	let count: Int
	let lit: Int

	var body: some View {
		HStack(spacing: 6) {
			ForEach(0..<count, id: \.self) { i in
				Capsule()
					.fill(i < lit ? Theme.foreground : Theme.foreground.opacity(0.15))
					.frame(width: 32, height: 4)
			}
		}
		.animation(.easeInOut(duration: 0.5), value: lit)
	}
}
