import SwiftUI
import UIKit

/**
 * The web's colors (packages/web/src/routes/layout.css) for the native screens: plain greys, a
 * near-black primary (a light grey in the dark theme), and a profile's greys tinted with its
 * avatar's hue, as its pages are on the web. The values are generated (Palettes.swift); each color
 * here follows the light or dark theme. A screen reads the open profile's from the environment.
 */
struct Palette {
	/// A color as `0xRRGGBB` in sRGB, and how opaque.
	struct Shade {
		let rgb: UInt32
		let opacity: Double

		init(_ rgb: UInt32, _ opacity: Double = 1) {
			self.rgb = rgb
			self.opacity = opacity
		}

		var uiColor: UIColor {
			UIColor(
				red: CGFloat((rgb >> 16) & 0xFF) / 255,
				green: CGFloat((rgb >> 8) & 0xFF) / 255,
				blue: CGFloat(rgb & 0xFF) / 255,
				alpha: opacity
			)
		}
	}

	/// The variables of layout.css, by the same names.
	struct Colors {
		var background: Shade
		var foreground: Shade
		var card: Shade
		var popover: Shade
		var primary: Shade
		var primaryForeground: Shade
		var secondary: Shade
		var muted: Shade
		var mutedForeground: Shade
		var accent: Shade
		var destructive: Shade
		var border: Shade
		var input: Shade
		var ring: Shade
		var sidebar: Shade
		var sidebarAccent: Shade
		var sidebarBorder: Shade
		var bubble: Shade
		var composer: Shade
		var warning: Shade
	}

	let light: Colors
	let dark: Colors

	/// A profile's, by its avatar; the plain ones without one.
	static func of(avatar: String?) -> Palette {
		avatar.flatMap { tinted[$0] } ?? plain
	}

	/// One of them, as a color that follows the theme.
	func color(_ shade: KeyPath<Colors, Shade>) -> Color {
		Color(uiColor: uiColor(shade))
	}

	func uiColor(_ shade: KeyPath<Colors, Shade>) -> UIColor {
		let (light, dark) = (light[keyPath: shade].uiColor, dark[keyPath: shade].uiColor)
		return UIColor { $0.userInterfaceStyle == .dark ? dark : light }
	}

	/// The page.
	var background: Color { color(\.background) }
	/// Text.
	var foreground: Color { color(\.foreground) }
	/// Cards with a border, a step above the page.
	var card: Color { color(\.card) }
	/// Buttons that matter most, links, and what's picked: near black, or a light grey in the dark.
	var primary: Color { color(\.primary) }
	/// Text on `primary`.
	var primaryForeground: Color { color(\.primaryForeground) }
	/// Tiles and rows on the page.
	var muted: Color { color(\.muted) }
	/// Text that says less: hints, times, counts.
	var mutedForeground: Color { color(\.mutedForeground) }
	/// What's under the pointer or pressed.
	var accent: Color { color(\.accent) }
	var destructive: Color { color(\.destructive) }
	/// Hairlines around cards, chips and the composer.
	var border: Color { color(\.border) }
	var sidebar: Color { color(\.sidebar) }
	/// The open row in the sidebar.
	var sidebarAccent: Color { color(\.sidebarAccent) }
	/// A person's message.
	var bubble: Color { color(\.bubble) }
	var composer: Color { color(\.composer) }
	var warning: Color { color(\.warning) }
}

private struct PaletteKey: EnvironmentKey {
	static let defaultValue = Palette.plain
}

extension EnvironmentValues {
	/// The open profile's colors (MainView sets them).
	var palette: Palette {
		get { self[PaletteKey.self] }
		set { self[PaletteKey.self] = newValue }
	}
}

extension View {
	/// A list or form as the web's pages: the page's color behind it. Its rows, as the web's tiles,
	/// take `muted` (`listRowBackground`).
	func pageBackground(_ palette: Palette) -> some View {
		scrollContentBackground(.hidden).background(palette.background)
	}
}
