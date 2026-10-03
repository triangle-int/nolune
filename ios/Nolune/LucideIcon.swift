import SwiftUI

/**
 * A Lucide icon, as the web shows it: the one a command's step picked (by name, fetched from the
 * family's nolune once), or a suggestion's (its drawing given). Drawn with Lucide's 2-point
 * stroke on its 24-point grid, in the text's color and at the text's size. Until it has the
 * drawing, or for a name Lucide doesn't have, it's the SF Symbol `fallback`.
 */
struct LucideIcon: View {
	var name: String?
	var node: IconNode?
	var fallback = "terminal"
	@Environment(\.noluneClient) private var client
	@State private var fetched: IconNode?
	@ScaledMetric(relativeTo: .body) private var size: CGFloat = 16

	var body: some View {
		Group {
			if let icon = node ?? fetched {
				let path = Path(segments: icon.segments)
				Canvas { context, canvas in
					let scale = min(canvas.width, canvas.height) / 24
					context.scaleBy(x: scale, y: scale)
					context.stroke(path, with: .foreground, style: StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round))
				}
			} else {
				Image(systemName: fallback)
					.resizable()
					.scaledToFit()
			}
		}
		.frame(width: size, height: size)
		.accessibilityHidden(true)
		.task(id: name) {
			guard node == nil, let name, let client else { return }
			fetched = await Icons.shared.icon(name, from: client)
		}
	}
}

/// Lucide's drawings the app fetched, by name, for every icon on screen to share.
@MainActor
final class Icons {
	static let shared = Icons()
	private var found: [String: IconNode] = [:]
	private var fetching: [String: Task<IconNode?, Never>] = [:]

	func icon(_ name: String, from client: Client) async -> IconNode? {
		if let icon = found[name] { return icon }
		guard name.range(of: "^[a-z0-9-]{1,64}$", options: .regularExpression) != nil else { return nil }
		let task = fetching[name] ?? Task { try? await client.call("GET", "api/icons/\(name)", as: IconNode.self) }
		fetching[name] = task
		let icon = await task.value
		fetching[name] = nil
		if let icon { found[name] = icon }
		return icon
	}
}

extension Path {
	/// An outline read from SVG (SVG.swift).
	init(segments: [SVGSegment]) {
		self.init()
		for segment in segments {
			switch segment {
			case .move(let x, let y):
				move(to: CGPoint(x: x, y: y))
			case .line(let x, let y):
				addLine(to: CGPoint(x: x, y: y))
			case .curve(let x1, let y1, let x2, let y2, let x, let y):
				addCurve(to: CGPoint(x: x, y: y), control1: CGPoint(x: x1, y: y1), control2: CGPoint(x: x2, y: y2))
			case .close:
				closeSubpath()
			}
		}
	}
}

private struct NoluneClientKey: EnvironmentKey {
	static let defaultValue: Client? = nil
}

extension EnvironmentValues {
	/// The family's nolune, for views that fetch from it (LucideIcon).
	var noluneClient: Client? {
		get { self[NoluneClientKey.self] }
		set { self[NoluneClientKey.self] = newValue }
	}
}
