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
		// A model of its own, so the app the tests run in doesn't open anything.
		let defaults = UserDefaults(suiteName: "AddressTests")!
		defer { defaults.removePersistentDomain(forName: "AddressTests") }
		let model = AppModel(defaults: defaults)
		model.connect(to: Address("smiths")!)
		_ = model.takePending()

		model.open(path: "/p/family?notification=1", from: URL(string: "https://jones.nolune.family"))
		XCTAssertNil(model.pending)
		model.open(path: "//elsewhere.com", from: URL(string: "https://smiths.nolune.family"))
		XCTAssertNil(model.pending)
		model.open(path: "/p/family?notification=1", from: URL(string: "https://smiths.nolune.family"))
		XCTAssertEqual(model.takePending(), "/p/family?notification=1")

		model.disconnect()
		XCTAssertNil(model.origin)
		XCTAssertEqual(model.previous?.absoluteString, "https://smiths.nolune.family")
		XCTAssertNil(AppModel(defaults: defaults).origin)
	}

	// MARK: Is there a nolune there?

	/// A session whose answers come from `Answers`, not the network.
	private func session(_ answers: [String: (status: Int, location: String?)]) -> URLSession {
		Answers.table = answers
		let configuration = URLSessionConfiguration.ephemeral
		configuration.protocolClasses = [Answers.self]
		return URLSession(configuration: configuration)
	}

	func testANoluneAnswersWithItsSignInPage() async {
		let checked = await Address("smiths")!.check(
			session: session(["https://smiths.nolune.family/login": (200, nil)])
		)
		XCTAssertEqual(try? checked.get().origin.absoluteString, "https://smiths.nolune.family")
	}

	func testTheAddressIsWhereItAnsweredAfterARedirect() async {
		let checked = await Address("example.com/invite/abc")!.check(
			session: session([
				"https://example.com/login": (308, "https://www.example.com/login"),
				"https://www.example.com/login": (200, nil)
			])
		)
		XCTAssertEqual(try? checked.get().origin.absoluteString, "https://www.example.com")
		XCTAssertEqual(try? checked.get().path, "/invite/abc")
	}

	func testTheRelaysOfflinePageCounts() async {
		let checked = await Address("smiths")!.check(
			session: session(["https://smiths.nolune.family/login": (503, nil)])
		)
		XCTAssertEqual(try? checked.get().origin.absoluteString, "https://smiths.nolune.family")
	}

	func testNothingThereOrAnError() async {
		let missing = await Address("jones")!.check(
			session: session(["https://jones.nolune.family/login": (404, nil)])
		)
		XCTAssertEqual(missing.failure, .nothingThere)
		let broken = await Address("smiths")!.check(
			session: session(["https://smiths.nolune.family/login": (500, nil)])
		)
		XCTAssertEqual(broken.failure, .refused(500))
	}

	func testAnAddressIsSchemeHostAndPort() {
		XCTAssertEqual(
			Address.origin(of: URL(string: "https://WWW.example.com:8443/login?x=1")!)?.absoluteString,
			"https://www.example.com:8443"
		)
		XCTAssertEqual(Address.origin(of: URL(string: "http://localhost:5173/login")!)?.absoluteString, "http://localhost:5173")
		XCTAssertNil(Address.origin(of: URL(string: "http://example.com/login")!))
	}
}

private extension Result {
	var failure: Failure? {
		if case .failure(let failure) = self { return failure }
		return nil
	}
}

/// Answers requests from a table of URLs: a status, and where a redirect goes.
private final class Answers: URLProtocol {
	static var table: [String: (status: Int, location: String?)] = [:]

	override class func canInit(with request: URLRequest) -> Bool { true }
	override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

	override func startLoading() {
		guard let url = request.url else { return }
		let (status, location) = Answers.table[url.absoluteString] ?? (404, nil)
		let response = HTTPURLResponse(
			url: url,
			statusCode: status,
			httpVersion: "HTTP/1.1",
			headerFields: location.map { ["Location": $0] } ?? [:]
		)!
		if let location, let target = URL(string: location) {
			// The session follows it, with another request here.
			client?.urlProtocol(self, wasRedirectedTo: URLRequest(url: target), redirectResponse: response)
			return
		}
		client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
		client?.urlProtocol(self, didLoad: Data())
		client?.urlProtocolDidFinishLoading(self)
	}

	override func stopLoading() {}
}
