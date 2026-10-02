import SwiftUI

/**
 * The sky (Sky.swift) is the macOS app's, which can draw its screens to PNGs for CI
 * (macos/Sources/Nolune/Snapshot.swift). The iOS app never does: here the sky always moves.
 */
enum Snapshot {
	static let clock: Double? = nil
	static let intro = false

	static func drawSky(_ posed: Sky, _ context: inout GraphicsContext, size: CGSize, clock: Double, intro: Bool) {}
}
