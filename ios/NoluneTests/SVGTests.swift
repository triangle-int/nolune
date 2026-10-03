import XCTest
@testable import Nolune

/// Lucide's icons, read from the SVG the gateway sends (SVG.swift).
final class SVGTests: XCTestCase {
	func testPathData() {
		XCTAssertEqual(
			SVGSegment.path("M2 3h4v-1l1,1L1-1.5.5.5zm1 1 2 2"),
			[
				.move(x: 2, y: 3),
				.line(x: 6, y: 3),
				.line(x: 6, y: 2),
				.line(x: 7, y: 3),
				.line(x: 1, y: -1.5),
				// -1.5.5.5 is -1.5, .5 and .5: the line goes on to (.5, .5).
				.line(x: 0.5, y: 0.5),
				.close,
				// After the close, from where its subpath started.
				.move(x: 3, y: 4),
				.line(x: 5, y: 6)
			]
		)
	}

	func testCurves() {
		XCTAssertEqual(
			SVGSegment.path("M0 0c1 1 2 1 3 0s2-1 3 0Q9 3 12 0t6 0"),
			[
				.move(x: 0, y: 0),
				.curve(x1: 1, y1: 1, x2: 2, y2: 1, x: 3, y: 0),
				// Smooth: its first control point mirrors the last one's second.
				.curve(x1: 4, y1: -1, x2: 5, y2: -1, x: 6, y: 0),
				.curve(x1: 8, y1: 2, x2: 10, y2: 2, x: 12, y: 0),
				.curve(x1: 14, y1: -2, x2: 16, y2: -2, x: 18, y: 0)
			]
		)
	}

	func testArcs() {
		// A half circle of radius 1, with its flags run together, in two quarter turns.
		let segments = SVGSegment.path("M0 0a1 1 0 012 0")
		XCTAssertEqual(segments.count, 3)
		guard case .curve(_, _, _, _, let x, let y) = segments[1], case .curve(_, _, _, _, let endX, let endY) = segments[2] else {
			return XCTFail("curves")
		}
		XCTAssertEqual(x, 1, accuracy: 1e-9)
		XCTAssertEqual(abs(y), 1, accuracy: 1e-9)
		XCTAssertEqual(endX, 2, accuracy: 1e-9)
		XCTAssertEqual(endY, 0, accuracy: 1e-9)
	}

	func testIconsFromTheGateway() throws {
		let json = #"[["circle",{"cx":"12","cy":"12","r":"10"}],["path",{"d":"M12 2v2"}],["rect",{"width":"20","height":"14","x":"2","y":"3","rx":"2"}],["polyline",{"points":"22 12 18 12 15 21"}],["line",{"x1":"2","x2":"22","y1":"2","y2":"22"}]]"#
		let icon = try JSONDecoder().decode(IconNode.self, from: Data(json.utf8))
		XCTAssertEqual(icon.elements.map(\.tag), ["circle", "path", "rect", "polyline", "line"])
		let segments = icon.segments
		XCTAssertEqual(segments.first, .move(x: 2, y: 12))
		XCTAssertTrue(segments.contains(.move(x: 22, y: 12)))
		XCTAssertTrue(segments.contains(.line(x: 15, y: 21)))
		XCTAssertEqual(segments.last, .line(x: 22, y: 22))
		XCTAssertEqual(segments.filter { $0 == .close }.count, 2)
	}
}
