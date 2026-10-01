import XCTest
@testable import Nolune

/// What people type to connect, and where it leads (Address.swift).
final class AddressTests: XCTestCase {
	private func address(_ text: String) -> (origin: String, path: String?)? {
		Address(text).map { ($0.origin.absoluteString, $0.path) }
	}

	func testANameIsOnTheRelay() {
		XCTAssertEqual(address("smiths")?.origin, "https://smiths.nolune.family")
		XCTAssertEqual(address("  Smiths \n")?.origin, "https://smiths.nolune.family")
		XCTAssertEqual(address("cozy-otter-42")?.origin, "https://cozy-otter-42.nolune.family")
		XCTAssertTrue(Address("smiths")!.isOnRelay)
		for name in ["ab", "-smiths", "smiths-", "smi--ths", "smiths_", "two words", "", String(repeating: "x", count: 33)] {
			XCTAssertNil(Address(name), name)
		}
	}

	func testAnAddressIsHTTPS() {
		XCTAssertEqual(address("smiths.nolune.family")?.origin, "https://smiths.nolune.family")
		XCTAssertEqual(address("Nolune.Example.com")?.origin, "https://nolune.example.com")
		XCTAssertEqual(address("http://nolune.example.com")?.origin, "https://nolune.example.com")
		XCTAssertEqual(address("https://nolune.example.com:8443/")?.origin, "https://nolune.example.com:8443")
		XCTAssertNil(address("https://nolune.example.com/")?.path)
		XCTAssertFalse(Address("nolune.example.com")!.isOnRelay)
		XCTAssertNil(Address("ftp://nolune.example.com"))
		XCTAssertNil(Address("https://nolune"))
	}

	func testHTTPOnlyOnThisNetwork() {
		XCTAssertEqual(address("http://localhost:5173")?.origin, "http://localhost:5173")
		XCTAssertEqual(address("http://mac-mini.local:5780")?.origin, "http://mac-mini.local:5780")
		XCTAssertEqual(address("http://192.168.1.20:5780")?.origin, "http://192.168.1.20:5780")
		XCTAssertEqual(address("http://172.20.0.2")?.origin, "http://172.20.0.2")
		XCTAssertEqual(address("http://172.32.0.2")?.origin, "https://172.32.0.2")
		XCTAssertEqual(address("http://203.0.113.7")?.origin, "https://203.0.113.7")
		XCTAssertEqual(Address("http://localhost:5173")?.display, "http://localhost:5173")
		XCTAssertEqual(Address("smiths")?.display, "smiths.nolune.family")
	}

	func testALinkOpensItsPage() {
		let invite = address("https://smiths.nolune.family/invite/abc123")
		XCTAssertEqual(invite?.origin, "https://smiths.nolune.family")
		XCTAssertEqual(invite?.path, "/invite/abc123")
		XCTAssertEqual(address("smiths.nolune.family/p/family?folder=3")?.path, "/p/family?folder=3")
	}

	func testPathsStayOnTheAddress() {
		XCTAssertTrue(Address.isPath("/p/family?notification=1"))
		XCTAssertFalse(Address.isPath("//elsewhere.com"))
		XCTAssertFalse(Address.isPath("/\\elsewhere.com"))
		XCTAssertFalse(Address.isPath("https://elsewhere.com"))
		let family = URL(string: "https://smiths.nolune.family")!
		XCTAssertTrue(Address.sameOrigin(family, URL(string: "https://Smiths.nolune.family:443/p/family")!))
		XCTAssertFalse(Address.sameOrigin(family, URL(string: "http://smiths.nolune.family")!))
		XCTAssertFalse(Address.sameOrigin(family, URL(string: "https://jones.nolune.family")!))
	}

	@MainActor
	func testATappedNotificationOpensOnlyOnItsNolune() {
		let model = AppModel.shared
		model.connect(to: Address("smiths")!)
		defer { model.disconnect() }
		_ = model.takePending()

		model.open(path: "/p/family?notification=1", from: URL(string: "https://jones.nolune.family"))
		XCTAssertNil(model.pending)
		model.open(path: "//elsewhere.com", from: URL(string: "https://smiths.nolune.family"))
		XCTAssertNil(model.pending)
		model.open(path: "/p/family?notification=1", from: URL(string: "https://smiths.nolune.family"))
		XCTAssertEqual(model.takePending(), "/p/family?notification=1")
	}
}
