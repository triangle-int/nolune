import XCTest
@testable import Nolune

/// The family's nolune's JSON (Client.swift), and which screens it leads to (NativeController.swift).
final class ClientTests: XCTestCase {
	private let smiths = URL(string: "https://smiths.nolune.family")!

	private func client(_ table: [String: Answers.Answer]) -> Client {
		Client(origin: smiths, session: Answers.session(table))
	}

	// MARK: Which nolune is it?

	func testANoluneSaysWhichItIs() async throws {
		let signedOut = try await client([
			"https://smiths.nolune.family/api/version": .json(#"{"version":"0.6.0","api":1}"#)
		]).version()
		XCTAssertEqual(signedOut, Server(version: "0.6.0", api: 1, capabilities: nil))
		let signedIn = try await client([
			"https://smiths.nolune.family/api/version": .json(
				#"{"version":"0.6.0","api":1,"capabilities":["chats","notifications","transcript"],"later":true}"#
			)
		]).version()
		XCTAssertEqual(signedIn?.capabilities, ["chats", "notifications", "transcript"])
	}

	func testANoluneFromBeforeTheAPISaysNothing() async throws {
		for answer in [
			Answers.Answer.json(#"{"message":"Not signed in"}"#, status: 401),
			.json(#"{"message":"Not found"}"#, status: 404),
			.json("<!doctype html>")
		] {
			let server = try await client(["https://smiths.nolune.family/api/version": answer]).version()
			XCTAssertNil(server)
		}
	}

	func testANoluneThatDoesntAnswerIsNoAnswer() async {
		// The relay's page for a computer that's off.
		do {
			_ = try await client(["https://smiths.nolune.family/api/version": Answers.Answer(status: 503)]).version()
			XCTFail("a 503 is no answer")
		} catch {
			XCTAssertEqual(error as? Client.Problem, .refused(503))
		}
	}

	func testWhichScreensANoluneGets() {
		let current = Server(version: "0.6.0", api: 1, capabilities: nil)
		let signedIn = Server(version: "0.6.0", api: 1, capabilities: ["chats"])
		let older = Server(version: "0.5.9", api: 0, capabilities: nil)
		XCTAssertEqual(NativeController.screen(for: .success(current)), .signIn)
		XCTAssertEqual(NativeController.screen(for: .success(signedIn)), .signedIn)
		// Today's web app, with the note, for a nolune the native screens can't read.
		XCTAssertEqual(NativeController.screen(for: .success(older)), .web(note: true))
		XCTAssertEqual(NativeController.screen(for: .success(nil)), .web(note: true))
		// And without it, for one that didn't answer.
		XCTAssertEqual(NativeController.screen(for: .failure(URLError(.notConnectedToInternet))), .web(note: false))
	}

	// MARK: Signing in and out

	func testSigningInKeepsTheSessionsCookie() async throws {
		let client = self.client([
			"https://smiths.nolune.family/api/auth/sign-in/email": .json(
				#"{"redirect":false,"token":"abc"}"#,
				headers: ["Set-Cookie": "__Secure-better-auth.session_token=abc.sig; Max-Age=604800; Path=/; HttpOnly; Secure; SameSite=Lax"]
			)
		])
		try await client.signIn(email: "  Anna@Example.com ", password: "Launcher-Check-42")

		let sent = try XCTUnwrap(Answers.sent.last)
		XCTAssertEqual(sent.method, "POST")
		XCTAssertEqual(
			try JSONDecoder().decode([String: String].self, from: Data(sent.body.utf8)),
			["email": "anna@example.com", "password": "Launcher-Check-42"]
		)
		// No `Origin`, so better-auth doesn't hold it to the address the nolune was set up with.
		XCTAssertNil(sent.headers["Origin"])

		let cookies = client.session.configuration.httpCookieStorage?.cookies(for: smiths) ?? []
		XCTAssertEqual(cookies.map(\.name), ["__Secure-better-auth.session_token"])
		XCTAssertEqual(cookies.first?.value, "abc.sig")
		XCTAssertTrue(cookies.allSatisfy(Cookies.isSession))
	}

	func testSigningInSaysWhatWentWrong() async {
		let answers: [(Answers.Answer, Client.Problem)] = [
			(.json(#"{"code":"INVALID_EMAIL_OR_PASSWORD"}"#, status: 401), .wrongPassword),
			(.json(#"{"code":"VALIDATION_ERROR"}"#, status: 400), .wrongPassword),
			(.json(#"{"message":"Too many requests."}"#, status: 429, headers: ["X-Retry-After": "60"]), .tooManyTries),
			(Answers.Answer(status: 500), .refused(500))
		]
		for (answer, problem) in answers {
			do {
				try await client(["https://smiths.nolune.family/api/auth/sign-in/email": answer])
					.signIn(email: "anna@example.com", password: "wrong")
				XCTFail("signed in with \(answer.status)")
			} catch {
				XCTAssertEqual(error as? Client.Problem, problem)
			}
		}
	}

	func testSigningOutStopsNotificationsFirst() async {
		await client([
			"https://smiths.nolune.family/api/push": Answers.Answer(status: 204),
			"https://smiths.nolune.family/logout": .redirect(to: "https://smiths.nolune.family/login", status: 303),
			"https://smiths.nolune.family/login": Answers.Answer()
		]).signOut()
		XCTAssertEqual(
			Answers.sent.prefix(2).map { "\($0.method) \($0.url)" },
			["DELETE https://smiths.nolune.family/api/push", "POST https://smiths.nolune.family/logout"]
		)
	}

	func testTheCookiesThatGoToANolune() {
		func cookie(_ name: String, domain: String, secure: Bool = false) -> HTTPCookie {
			var properties: [HTTPCookiePropertyKey: Any] = [.name: name, .value: "x", .domain: domain, .path: "/"]
			if secure { properties[.secure] = "TRUE" }
			return HTTPCookie(properties: properties)!
		}
		XCTAssertTrue(Cookies.goes(cookie("a", domain: "smiths.nolune.family"), to: smiths))
		XCTAssertTrue(Cookies.goes(cookie("a", domain: ".nolune.family"), to: smiths))
		XCTAssertFalse(Cookies.goes(cookie("a", domain: "jones.nolune.family"), to: smiths))
		XCTAssertFalse(Cookies.goes(cookie("a", domain: "localhost", secure: true), to: URL(string: "http://localhost:5173")!))
		XCTAssertTrue(Cookies.isSession(cookie("better-auth.session_token", domain: "localhost")))
		XCTAssertFalse(Cookies.isSession(cookie("nolune-preferences", domain: "localhost")))
	}
}
