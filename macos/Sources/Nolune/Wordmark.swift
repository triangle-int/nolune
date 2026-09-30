import SwiftUI

/**
 * The wordmark, from packages/web/src/lib/assets/logo.svg: the letters' path and the three
 * trailing dots, in the logo's own units. Keep them in step with the SVG.
 */
enum Wordmark {
	static let size = CGSize(width: 2088, height: 343)

	struct Dot {
		var center: CGPoint
		var radius: CGFloat
		var opacity: Double
	}

	static let dots = [
		Dot(center: CGPoint(x: 1636.5, y: 264.5), radius: 78.5, opacity: 1),
		Dot(center: CGPoint(x: 1823, y: 264.5), radius: 78.5, opacity: 0.56),
		Dot(center: CGPoint(x: 2009.5, y: 264.5), radius: 78.5, opacity: 0.3)
	]

	static let letters: Path = SVGPath.parse(
		"M0 343L0 98L83.3 98L86.7 127.9Q98.5 110.7 115.6 101.4Q132.8 92.1 153.4 92.1Q183.3 92.1 202.9 104.1Q222.5 116.1 232 140.6Q241.6 165.1 241.6 202.9L241.6 343L153.4 343L153.4 217.6Q153.4 203.4 150.2 194.3Q147 185.2 140.9 180.3Q134.8 175.4 125.4 174.9Q106.8 174 97.5 183.3Q88.2 192.6 88.2 212.7L88.2 343ZM404.7 348.9Q365.5 348.9 335.4 332.5Q305.3 316.1 288.1 287.1Q271 258.2 271 220.5Q271 182.8 287.9 153.9Q304.8 125 334.9 108.5Q365.1 92.1 403.8 92.1Q443 92.1 473.1 108.5Q503.2 125 520.1 153.9Q537 182.8 537 220.5Q537 258.2 520.1 287.1Q503.2 316.1 473.3 332.5Q443.5 348.9 404.7 348.9ZM404.7 267.5Q417 267.5 426.3 261.4Q435.6 255.3 441 244.8Q446.4 234.2 446.4 220.5Q446.4 206.8 441 196.2Q435.6 185.7 426.1 179.6Q416.5 173.5 403.8 173.5Q391.5 173.5 382 179.6Q372.4 185.7 367 196.2Q361.6 206.8 361.6 220.5Q361.6 234.2 367 244.8Q372.4 255.3 382.2 261.4Q392 267.5 404.7 267.5ZM568.9 343L568.9 0L657.1 0L657.1 343ZM786.9 348.9Q757.1 348.9 737.5 336.9Q717.9 324.9 708.3 300.4Q698.7 275.9 698.7 238.1L698.7 98L786.9 98L786.9 223.4Q786.9 237.2 790.1 246.5Q793.3 255.8 799.4 260.7Q805.6 265.6 814.9 266.1Q833.5 266.6 842.8 257.3Q852.1 247.9 852.1 228.3L852.1 98L940.3 98L940.3 343L857 343L853.6 313.1Q841.8 330.3 824.7 339.6Q807.5 348.9 786.9 348.9ZM984.4 343L984.4 98L1067.7 98L1071.1 127.9Q1082.9 110.7 1100.1 101.4Q1117.2 92.1 1137.8 92.1Q1167.7 92.1 1187.3 104.1Q1206.9 116.1 1216.4 140.6Q1226 165.1 1226 202.9L1226 343L1137.8 343L1137.8 217.6Q1137.8 203.4 1134.6 194.3Q1131.4 185.2 1125.3 180.3Q1119.2 175.4 1109.9 174.9Q1091.2 174 1081.9 183.3Q1072.6 192.6 1072.6 212.7L1072.6 343ZM1391.6 348.9Q1351.9 348.9 1321 332.7Q1290.2 316.5 1272.8 287.6Q1255.4 258.7 1255.4 220.5Q1255.4 182.3 1272.5 153.4Q1289.7 124.5 1320.1 108.3Q1350.4 92.1 1390.1 92.1Q1427.9 92.1 1456.5 108.5Q1485.2 125 1501.6 158.3Q1518 191.6 1518 242.6L1346.5 242.6Q1349 260.7 1360 271.2Q1371 281.8 1389.1 281.8Q1405.3 281.8 1417.3 274.2Q1429.3 266.6 1432.3 255.3L1509.7 279.3Q1500.9 304.8 1481.5 320.2Q1462.2 335.7 1438.4 342.3Q1414.6 348.9 1391.6 348.9ZM1347 188.7L1428.4 188.7Q1425.9 174.9 1419.5 167.1Q1413.2 159.3 1404.8 156.1Q1396.5 152.9 1387.7 152.9Q1378.9 152.9 1370.5 156.1Q1362.2 159.3 1355.8 167.1Q1349.5 174.9 1347 188.7Z"
	)
}

/// Enough of SVG path syntax for the logo: absolute M, L, H, V, Q, C and Z.
enum SVGPath {
	private enum Token {
		case command(Character)
		case number(CGFloat)
	}

	static func parse(_ d: String) -> Path {
		var tokens: [Token] = []
		var digits = ""
		func endNumber() {
			if let value = Double(digits) { tokens.append(.number(CGFloat(value))) }
			digits = ""
		}
		for character in d {
			switch character {
			case "M", "L", "H", "V", "Q", "C", "Z":
				endNumber()
				tokens.append(.command(character))
			case " ", ",", "\n", "\t":
				endNumber()
			case "-":
				endNumber()
				digits = "-"
			default:
				digits.append(character)
			}
		}
		endNumber()

		var path = Path()
		var command: Character = "M"
		var current = CGPoint.zero
		var i = 0
		func number() -> CGFloat {
			guard i < tokens.count, case let .number(value) = tokens[i] else { return 0 }
			i += 1
			return value
		}
		func point() -> CGPoint {
			let x = number()
			return CGPoint(x: x, y: number())
		}
		while i < tokens.count {
			if case let .command(next) = tokens[i] {
				command = next
				i += 1
				if command == "Z" { path.closeSubpath() }
				continue
			}
			switch command {
			case "M":
				current = point()
				path.move(to: current)
				command = "L" // More pairs after a moveto are linetos.
			case "L":
				current = point()
				path.addLine(to: current)
			case "H":
				current = CGPoint(x: number(), y: current.y)
				path.addLine(to: current)
			case "V":
				current = CGPoint(x: current.x, y: number())
				path.addLine(to: current)
			case "Q":
				let control = point()
				current = point()
				path.addQuadCurve(to: current, control: control)
			case "C":
				let first = point()
				let second = point()
				current = point()
				path.addCurve(to: current, control1: first, control2: second)
			default:
				i += 1 // A stray number.
			}
		}
		return path
	}
}

/// The letters, filled, at a width; the dots are drawn by whoever animates them.
struct WordmarkLetters: Shape {
	func path(in rect: CGRect) -> Path {
		let scale = rect.width / Wordmark.size.width
		return Wordmark.letters.applying(
			CGAffineTransform(translationX: rect.minX, y: rect.minY).scaledBy(x: scale, y: scale)
		)
	}
}
