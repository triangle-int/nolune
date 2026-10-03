import XCTest
@testable import Nolune

/// What the chat's requests carry (ChatClient.swift).
final class ChatClientTests: XCTestCase {
	/// A file's name in `x-file-name`, as the gateway decodes it: ASCII only, as `encodeURIComponent` has it.
	func testAFileNameIsEncodedAsTheWebEncodesIt() {
		XCTAssertEqual(Client.uriComponent("Фото café (1).jpg"), "%D0%A4%D0%BE%D1%82%D0%BE%20caf%C3%A9%20(1).jpg")
		XCTAssertEqual(Client.uriComponent("a-b_c.d!~*'"), "a-b_c.d!~*'")
	}

	func testARequestNobodyWaitsForIsntAProblem() {
		XCTAssertTrue(URLError(.cancelled).isCancellation)
		XCTAssertTrue(CancellationError().isCancellation)
		XCTAssertFalse(URLError(.timedOut).isCancellation)
	}
}
