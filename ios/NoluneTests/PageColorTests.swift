import UIKit
import XCTest
@testable import Nolune

/// The page's background, as the page's script says it (BrowserController.swift).
final class PageColorTests: XCTestCase {
	private func components(_ css: String) -> [CGFloat]? {
		guard let color = UIColor(css: css) else { return nil }
		var red: CGFloat = 0, green: CGFloat = 0, blue: CGFloat = 0, alpha: CGFloat = 0
		color.getRed(&red, green: &green, blue: &blue, alpha: &alpha)
		return [red, green, blue, alpha].map { ($0 * 255).rounded() }
	}

	func testOpaqueColorsAsTheBrowserGivesThem() {
		XCTAssertEqual(components("rgb(33, 33, 33)"), [33, 33, 33, 255])
		XCTAssertEqual(components("rgba(8, 21, 44, 1)"), [8, 21, 44, 255])
		XCTAssertEqual(components("rgb(255 255 255)"), [255, 255, 255, 255])
	}

	func testNothingForSeeThroughOrOtherKindsOfColor() {
		for css in ["rgba(0, 0, 0, 0)", "rgb(8 21 44 / 0.5)", "transparent", "oklch(0.2 0 0)", "", "rgb(1, 2)"] {
			XCTAssertNil(UIColor(css: css), css)
		}
	}

	func testTheStatusBarReadsOnIt() {
		let traits = UITraitCollection(userInterfaceStyle: .light)
		XCTAssertTrue(UIColor(css: "rgb(33, 33, 33)")!.isDark(in: traits))
		XCTAssertTrue(UIColor(css: "rgb(8, 21, 44)")!.isDark(in: traits))
		XCTAssertFalse(UIColor(css: "rgb(255, 255, 255)")!.isDark(in: traits))
		XCTAssertTrue(UIColor.page.isDark(in: UITraitCollection(userInterfaceStyle: .dark)))
		XCTAssertFalse(UIColor.page.isDark(in: traits))
	}
}
