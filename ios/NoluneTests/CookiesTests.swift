import XCTest
@testable import Nolune

/// The session in both of the app's cookie stores (Cookies.swift), through the simulator's WebKit.
final class CookiesTests: XCTestCase {
	private let origin = URL(string: "https://cookies-test.nolune.family")!

	private func session(_ value: String) -> HTTPCookie {
		HTTPCookie(properties: [
			.name: "better-auth.session_token",
			.value: value,
			.domain: "cookies-test.nolune.family",
			.path: "/",
			.expires: Date(timeIntervalSinceNow: 3600)
		])!
	}

	private func sessions() -> [String] {
		(HTTPCookieStorage.shared.cookies(for: origin) ?? []).filter(Cookies.isSession).map(\.value)
	}

	/// Run as the app does, on the main actor; WebKit's store answers there too, before a web view.
	func testTheSessionGoesBothWaysAndOutOfBoth() {
		let storage = HTTPCookieStorage.shared
		let done = expectation(description: "WebKit's cookie store answered")
		Task { @MainActor in
			storage.setCookie(session("native"))
			await Cookies.toWeb(origin)
			storage.deleteCookie(session("native"))
			let took = await Cookies.fromWeb(origin)
			XCTAssertTrue(took)
			XCTAssertEqual(sessions(), ["native"])

			// One the app signed in to meanwhile stays.
			storage.setCookie(session("newer"))
			let replaced = await Cookies.fromWeb(origin, unlessSignedIn: true)
			XCTAssertFalse(replaced)
			XCTAssertEqual(sessions(), ["newer"])

			await Cookies.signOut(origin)
			XCTAssertEqual(sessions(), [])
			let left = await Cookies.fromWeb(origin)
			XCTAssertFalse(left)
			done.fulfill()
		}
		wait(for: [done], timeout: 30)
	}
}
