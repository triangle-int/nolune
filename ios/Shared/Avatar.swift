import SwiftUI

/**
 * A profile's assistant, drawn as the web draws it (packages/core/src/avatars.ts): shapes in its
 * color on a 24-point grid, each with its holes knocked out so what's behind shows through, then
 * eyes for the glyphs whose eyes sit in a hole. The glyphs themselves are generated (Avatars.swift).
 */
struct AvatarGlyph {
	struct Shape {
		let path: Path
		let holes: Path?

		init(path: String, holes: String?) {
			self.path = Path(svg: path)
			self.holes = holes.map(Path.init(svg:))
		}
	}

	let shapes: [Shape]
	let eyes: Path?
	let light: Color
	let dark: Color

	init(shapes: [Shape], eyes: String?, light: Color, dark: Color) {
		self.shapes = shapes
		self.eyes = eyes.map(Path.init(svg:))
		self.light = light
		self.dark = dark
	}

	static func named(_ avatar: String) -> AvatarGlyph {
		all[avatar] ?? all["probe"]!
	}

	/// Its color in the light or dark theme, as the web's layout.css has it.
	func color(_ scheme: ColorScheme) -> Color {
		scheme == .dark ? dark : light
	}
}

struct AvatarView: View {
	let avatar: String
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		let glyph = AvatarGlyph.named(avatar)
		let color = glyph.color(colorScheme)
		Canvas { context, size in
			context.scaleBy(x: size.width / 24, y: size.height / 24)
			for shape in glyph.shapes {
				context.drawLayer { layer in
					layer.fill(shape.path, with: .color(color))
					if let holes = shape.holes {
						layer.blendMode = .destinationOut
						layer.fill(holes, with: .color(.black))
					}
				}
			}
			if let eyes = glyph.eyes {
				context.fill(eyes, with: .color(color))
			}
		}
		.aspectRatio(1, contentMode: .fit)
		.accessibilityHidden(true)
	}
}

extension Path {
	/// SVG path data with absolute M, L, C and Z only, as Avatars.swift has it.
	init(svg data: String) {
		self.init()
		var commands: [(Character, [CGFloat])] = []
		var letter: Character?
		var numbers: [CGFloat] = []
		var number = ""
		func endNumber() {
			if let value = Double(number) { numbers.append(CGFloat(value)) }
			number = ""
		}
		for character in data {
			if "MLCZ".contains(character) {
				endNumber()
				if let letter { commands.append((letter, numbers)) }
				letter = character
				numbers = []
			} else if character == " " || character == "," {
				endNumber()
			} else if character == "-", !number.isEmpty {
				endNumber()
				number = "-"
			} else {
				number.append(character)
			}
		}
		endNumber()
		if let letter { commands.append((letter, numbers)) }

		for (command, values) in commands {
			switch command {
			case "M", "L":
				var i = 0
				while i + 1 < values.count {
					let point = CGPoint(x: values[i], y: values[i + 1])
					if command == "M", i == 0 { move(to: point) } else { addLine(to: point) }
					i += 2
				}
			case "C":
				var i = 0
				while i + 5 < values.count {
					addCurve(
						to: CGPoint(x: values[i + 4], y: values[i + 5]),
						control1: CGPoint(x: values[i], y: values[i + 1]),
						control2: CGPoint(x: values[i + 2], y: values[i + 3])
					)
					i += 6
				}
			default:
				closeSubpath()
			}
		}
	}
}
