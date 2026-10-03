import Foundation

/**
 * Lucide's icons as the gateway sends them (`GET /api/icons/<name>`, and a suggestion's `icon`):
 * an SVG's elements on a 24-point grid, drawn with a 2-point stroke. Read here into lines and
 * curves, so the app draws the same icons as the web (LucideIcon.swift).
 */
struct IconNode: Decodable, Hashable {
	struct Element: Hashable {
		let tag: String
		let attributes: [String: String]
	}

	let elements: [Element]

	init(elements: [Element]) {
		self.elements = elements
	}

	/// `[["path", {"d": "…"}], ["circle", {"cx": "12", …}], …]`.
	init(from decoder: Decoder) throws {
		var list = try decoder.unkeyedContainer()
		var elements: [Element] = []
		while !list.isAtEnd {
			var pair = try list.nestedUnkeyedContainer()
			let tag = try pair.decode(String.self)
			let attributes = try pair.decode([String: AttributeValue].self).mapValues(\.text)
			elements.append(Element(tag: tag, attributes: attributes))
		}
		self.elements = elements
	}

	/// The icon's outline, as moves, lines and curves.
	var segments: [SVGSegment] {
		elements.flatMap(SVGSegment.shape)
	}

	/// An attribute's value, a number or a string in the JSON.
	private struct AttributeValue: Decodable {
		let text: String

		init(from decoder: Decoder) throws {
			let value = try decoder.singleValueContainer()
			if let number = try? value.decode(Double.self) {
				text = String(number)
			} else {
				text = try value.decode(String.self)
			}
		}
	}
}

/// A piece of an outline, in the icon's own points.
enum SVGSegment: Equatable {
	case move(x: Double, y: Double)
	case line(x: Double, y: Double)
	case curve(x1: Double, y1: Double, x2: Double, y2: Double, x: Double, y: Double)
	case close

	/// An element's outline: a path, or a circle, ellipse, rectangle, line, polyline or polygon.
	static func shape(_ element: IconNode.Element) -> [SVGSegment] {
		let a = element.attributes
		func number(_ name: String) -> Double { Double(a[name] ?? "") ?? 0 }
		switch element.tag {
		case "path":
			return path(a["d"] ?? "")
		case "circle":
			return ellipse(number("cx"), number("cy"), number("r"), number("r"))
		case "ellipse":
			return ellipse(number("cx"), number("cy"), number("rx"), number("ry"))
		case "line":
			return [.move(x: number("x1"), y: number("y1")), .line(x: number("x2"), y: number("y2"))]
		case "polyline", "polygon":
			var points = Numbers(a["points"] ?? "")
			let values = points.all()
			var segments: [SVGSegment] = []
			for index in stride(from: 0, to: values.count - 1, by: 2) {
				let (x, y) = (values[index], values[index + 1])
				segments.append(segments.isEmpty ? .move(x: x, y: y) : .line(x: x, y: y))
			}
			if element.tag == "polygon", !segments.isEmpty { segments.append(.close) }
			return segments
		case "rect":
			let (x, y, width, height) = (number("x"), number("y"), number("width"), number("height"))
			var rx = a["rx"].flatMap(Double.init) ?? a["ry"].flatMap(Double.init) ?? 0
			var ry = a["ry"].flatMap(Double.init) ?? rx
			rx = min(rx, width / 2)
			ry = min(ry, height / 2)
			if rx == 0 || ry == 0 {
				return [
					.move(x: x, y: y), .line(x: x + width, y: y), .line(x: x + width, y: y + height),
					.line(x: x, y: y + height), .close
				]
			}
			return path(
				"M\(x + rx) \(y)H\(x + width - rx)A\(rx) \(ry) 0 0 1 \(x + width) \(y + ry)V\(y + height - ry)"
					+ "A\(rx) \(ry) 0 0 1 \(x + width - rx) \(y + height)H\(x + rx)A\(rx) \(ry) 0 0 1 \(x) \(y + height - ry)"
					+ "V\(y + ry)A\(rx) \(ry) 0 0 1 \(x + rx) \(y)Z"
			)
		default:
			return []
		}
	}

	private static func ellipse(_ cx: Double, _ cy: Double, _ rx: Double, _ ry: Double) -> [SVGSegment] {
		path("M\(cx - rx) \(cy)A\(rx) \(ry) 0 1 0 \(cx + rx) \(cy)A\(rx) \(ry) 0 1 0 \(cx - rx) \(cy)Z")
	}

	/// SVG path data, every command: absolute and relative moves, lines, curves and arcs.
	static func path(_ data: String) -> [SVGSegment] {
		var numbers = Numbers(data)
		var segments: [SVGSegment] = []
		var (x, y) = (0.0, 0.0)
		var (startX, startY) = (0.0, 0.0)
		/// The last curve's second control point, for a smooth one after it (`S`, `T`).
		var (controlX, controlY) = (0.0, 0.0)
		var previous: Character = " "
		var command: Character?

		while true {
			let next: Character
			if let read = numbers.command() {
				next = read
			} else if let last = command, last != "Z", last != "z", numbers.hasNumber {
				// More numbers after a move are lines; after anything else, the same command again.
				next = last == "M" ? "L" : last == "m" ? "l" : last
			} else {
				break
			}
			let relative = next.isLowercase
			let (ox, oy) = relative ? (x, y) : (0, 0)
			var smooth = false
			switch next.uppercased() {
			case "M":
				guard let px = numbers.number(), let py = numbers.number() else { return segments }
				(x, y) = (ox + px, oy + py)
				(startX, startY) = (x, y)
				segments.append(.move(x: x, y: y))
			case "L":
				guard let px = numbers.number(), let py = numbers.number() else { return segments }
				(x, y) = (ox + px, oy + py)
				segments.append(.line(x: x, y: y))
			case "H":
				guard let px = numbers.number() else { return segments }
				x = ox + px
				segments.append(.line(x: x, y: y))
			case "V":
				guard let py = numbers.number() else { return segments }
				y = oy + py
				segments.append(.line(x: x, y: y))
			case "C", "S":
				var (x1, y1) = (2 * x - controlX, 2 * y - controlY)
				if next.uppercased() == "C" {
					guard let a = numbers.number(), let b = numbers.number() else { return segments }
					(x1, y1) = (ox + a, oy + b)
				} else if !"CcSs".contains(previous) {
					(x1, y1) = (x, y)
				}
				guard let c = numbers.number(), let d = numbers.number(), let e = numbers.number(), let f = numbers.number()
				else { return segments }
				let (x2, y2) = (ox + c, oy + d)
				(x, y) = (ox + e, oy + f)
				segments.append(.curve(x1: x1, y1: y1, x2: x2, y2: y2, x: x, y: y))
				(controlX, controlY) = (x2, y2)
				smooth = true
			case "Q", "T":
				var (qx, qy) = (2 * x - controlX, 2 * y - controlY)
				if next.uppercased() == "Q" {
					guard let a = numbers.number(), let b = numbers.number() else { return segments }
					(qx, qy) = (ox + a, oy + b)
				} else if !"QqTt".contains(previous) {
					(qx, qy) = (x, y)
				}
				guard let c = numbers.number(), let d = numbers.number() else { return segments }
				let (endX, endY) = (ox + c, oy + d)
				// A quadratic curve, as the cubic that draws it.
				segments.append(
					.curve(
						x1: x + 2 / 3 * (qx - x), y1: y + 2 / 3 * (qy - y),
						x2: endX + 2 / 3 * (qx - endX), y2: endY + 2 / 3 * (qy - endY),
						x: endX, y: endY
					)
				)
				(x, y) = (endX, endY)
				(controlX, controlY) = (qx, qy)
				smooth = true
			case "A":
				guard let rx = numbers.number(), let ry = numbers.number(), let rotation = numbers.number(),
					let large = numbers.flag(), let sweep = numbers.flag(), let px = numbers.number(), let py = numbers.number()
				else { return segments }
				let (endX, endY) = (ox + px, oy + py)
				for c in arc(from: (x, y), rx, ry, rotation, large, sweep, to: (endX, endY)) {
					segments.append(.curve(x1: c[0], y1: c[1], x2: c[2], y2: c[3], x: c[4], y: c[5]))
				}
				(x, y) = (endX, endY)
			case "Z":
				segments.append(.close)
				(x, y) = (startX, startY)
			default:
				return segments
			}
			if !smooth { (controlX, controlY) = (x, y) }
			previous = next
			command = next
		}
		return segments
	}

	/**
	 * An elliptical arc as cubic curves, a quarter turn at most each (the SVG spec's appendix
	 * F.6.5, as `arcToCubics` in packages/core/src/avatars-ios.ts).
	 */
	static func arc(
		from start: (Double, Double),
		_ rxIn: Double,
		_ ryIn: Double,
		_ rotation: Double,
		_ large: Bool,
		_ sweep: Bool,
		to end: (Double, Double)
	) -> [[Double]] {
		let (x1, y1) = start
		let (x2, y2) = end
		if x1 == x2 && y1 == y2 { return [] }
		var (rx, ry) = (abs(rxIn), abs(ryIn))
		if rx == 0 || ry == 0 { return [[x1, y1, x2, y2, x2, y2]] }
		let phi = rotation * .pi / 180
		let (cosine, sine) = (cos(phi), sin(phi))
		let (dx, dy) = ((x1 - x2) / 2, (y1 - y2) / 2)
		let x1p = cosine * dx + sine * dy
		let y1p = -sine * dx + cosine * dy
		let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry)
		if lambda > 1 { (rx, ry) = (rx * lambda.squareRoot(), ry * lambda.squareRoot()) }
		let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p
		let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p
		let coef = (large != sweep ? 1.0 : -1.0) * max(0, num / den).squareRoot()
		let cxp = coef * rx * y1p / ry
		let cyp = -coef * ry * x1p / rx
		let cx = cosine * cxp - sine * cyp + (x1 + x2) / 2
		let cy = sine * cxp + cosine * cyp + (y1 + y2) / 2
		func angle(_ ux: Double, _ uy: Double, _ vx: Double, _ vy: Double) -> Double {
			let dot = (ux * vx + uy * vy) / ((ux * ux + uy * uy).squareRoot() * (vx * vx + vy * vy).squareRoot())
			let a = acos(min(1, max(-1, dot)))
			return ux * vy - uy * vx < 0 ? -a : a
		}
		let theta = angle(1, 0, (x1p - cxp) / rx, (y1p - cyp) / ry)
		var delta = angle((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry)
		if !sweep && delta > 0 { delta -= 2 * .pi }
		if sweep && delta < 0 { delta += 2 * .pi }
		let count = max(1, Int((abs(delta) / (.pi / 2) - 1e-9).rounded(.up)))
		let step = delta / Double(count)
		let t = 4.0 / 3.0 * tan(step / 4)
		func point(_ a: Double) -> (Double, Double) {
			(cx + rx * cos(a) * cosine - ry * sin(a) * sine, cy + rx * cos(a) * sine + ry * sin(a) * cosine)
		}
		func tangent(_ a: Double) -> (Double, Double) {
			(-rx * sin(a) * cosine - ry * cos(a) * sine, -rx * sin(a) * sine + ry * cos(a) * cosine)
		}
		return (0..<count).map { index in
			let (a1, a2) = (theta + Double(index) * step, theta + Double(index + 1) * step)
			let (p1, p2) = (point(a1), point(a2))
			let (d1, d2) = (tangent(a1), tangent(a2))
			return [p1.0 + t * d1.0, p1.1 + t * d1.1, p2.0 - t * d2.0, p2.1 - t * d2.1, p2.0, p2.1]
		}
	}
}

/// SVG's numbers and commands, as path data and point lists write them.
private struct Numbers {
	private let characters: [Character]
	private var index = 0

	init(_ text: String) {
		characters = Array(text)
	}

	private mutating func skipSeparators() {
		while index < characters.count, characters[index] == " " || characters[index] == "," || characters[index].isNewline || characters[index] == "\t" {
			index += 1
		}
	}

	/// The next command letter, if one comes next.
	mutating func command() -> Character? {
		skipSeparators()
		guard index < characters.count, characters[index].isLetter, characters[index] != "e", characters[index] != "E" else {
			return nil
		}
		defer { index += 1 }
		return characters[index]
	}

	var hasNumber: Bool {
		mutating get {
			skipSeparators()
			guard index < characters.count else { return false }
			let c = characters[index]
			return c.isNumber || c == "-" || c == "+" || c == "."
		}
	}

	/// An arc's flag: a single 0 or 1, which may run into what follows (`a1 1 0 01.5 2`).
	mutating func flag() -> Bool? {
		skipSeparators()
		guard index < characters.count, characters[index] == "0" || characters[index] == "1" else { return nil }
		defer { index += 1 }
		return characters[index] == "1"
	}

	/// The next number: `-1.5e-3`, `.5`, and `1.5.5` as 1.5 and .5.
	mutating func number() -> Double? {
		skipSeparators()
		var text = ""
		if index < characters.count, characters[index] == "-" || characters[index] == "+" {
			text.append(characters[index])
			index += 1
		}
		var dot = false
		var exponent = false
		while index < characters.count {
			let c = characters[index]
			if c.isNumber {
				text.append(c)
			} else if c == ".", !dot, !exponent {
				dot = true
				text.append(c)
			} else if c == "e" || c == "E", !exponent, !text.isEmpty {
				exponent = true
				text.append(c)
				if index + 1 < characters.count, characters[index + 1] == "-" || characters[index + 1] == "+" {
					index += 1
					text.append(characters[index])
				}
			} else {
				break
			}
			index += 1
		}
		return Double(text)
	}

	mutating func all() -> [Double] {
		var values: [Double] = []
		while let value = number() { values.append(value) }
		return values
	}
}
