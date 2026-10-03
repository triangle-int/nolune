import SwiftUI
import XCTest
@testable import Nolune

/**
 * The native screens' parts that work without a screen: where links lead (Destination.swift), the
 * avatars (Avatar.swift), the family's JSON (Models.swift, Client.swift), and the preferences the
 * pages share (Settings.swift).
 */
final class NativeTests: XCTestCase {
	private let smiths = URL(string: "https://smiths.nolune.family")!

	private func client(_ table: [String: Answers.Answer]) -> Client {
		Client(origin: smiths, session: Answers.session(table))
	}

	// MARK: Where links lead

	private func assertLeads(
		_ path: String,
		to destination: Destination,
		in slug: String?,
		file: StaticString = #filePath,
		line: UInt = #line
	) {
		let found = Destination.of(path: path)
		XCTAssertEqual(found?.destination, destination, path, file: file, line: line)
		XCTAssertEqual(found?.slug, slug, path, file: file, line: line)
	}

	func testLinksToWhatTheAppShowsLeadThere() {
		assertLeads("/p/smiths", to: .newChat, in: "smiths")
		assertLeads("/p/smiths/", to: .newChat, in: "smiths")
		assertLeads("/p/smiths/c/abc123", to: .chat("abc123"), in: "smiths")
		assertLeads("/p/smiths/f/trip", to: .folder("trip"), in: "smiths")
		assertLeads("/p/smiths/memory", to: .page(.memory), in: "smiths")
		// The profile's settings are the web sidebar's "People & profile".
		assertLeads("/p/smiths/settings", to: .page(.people), in: "smiths")
		assertLeads("/profiles", to: .page(.profiles), in: nil)
		assertLeads("/admin", to: .page(.modelsAndKeys), in: nil)
		assertLeads("/admin/people", to: .page(.adminPeople), in: nil)
	}

	func testOtherPagesStayOnTheWeb() {
		for path in ["/", "/login", "/invite/abc", "/p/smiths/welcome", "/p/smiths/c/abc/files", "/admin/logs"] {
			XCTAssertNil(Destination.of(path: path), path)
		}
	}

	func testEveryWebPageLeadsBackToItself() {
		for page in WebPage.allCases {
			let found = Destination.of(path: page.path(in: "smiths"))
			XCTAssertEqual(found?.destination, .page(page), page.rawValue)
			XCTAssertEqual(found?.slug, WebPage.profilePages.contains(page) ? "smiths" : nil, page.rawValue)
		}
	}

	// MARK: Avatars

	func testSVGPathData() {
		var elements: [Path.Element] = []
		Path(svg: "M1 2L3-4C5 6 7 8 9 10ZM0.5 0.25 1 1Z").forEach { elements.append($0) }
		XCTAssertEqual(
			elements,
			[
				.move(to: CGPoint(x: 1, y: 2)),
				.line(to: CGPoint(x: 3, y: -4)),
				.curve(to: CGPoint(x: 9, y: 10), control1: CGPoint(x: 5, y: 6), control2: CGPoint(x: 7, y: 8)),
				.closeSubpath,
				.move(to: CGPoint(x: 0.5, y: 0.25)),
				.line(to: CGPoint(x: 1, y: 1)),
				.closeSubpath
			]
		)
	}

	func testEveryAvatarIsDrawnOnTheGrid() {
		// packages/core/src/avatars.ts's AVATARS.
		let avatars = ["probe", "campfire", "lantern", "planet", "quantum", "comet", "moon", "satellite"]
		XCTAssertEqual(Set(AvatarGlyph.all.keys), Set(avatars))
		let grid = CGRect(x: -1, y: -1, width: 26, height: 26)
		for (name, glyph) in AvatarGlyph.all {
			XCTAssertFalse(glyph.shapes.isEmpty, name)
			for shape in glyph.shapes {
				XCTAssertFalse(shape.path.boundingRect.isEmpty, name)
				XCTAssertTrue(grid.contains(shape.path.boundingRect), name)
			}
		}
	}

	func testAnAvatarFromALaterNoluneIsTheProbe() {
		XCTAssertEqual(AvatarGlyph.named("someday").light, AvatarGlyph.named("probe").light)
	}

	// MARK: The family's JSON

	func testChatsComeAPageAtATime() async throws {
		let client = self.client([
			"https://smiths.nolune.family/api/p/smiths/chats?limit=50": .json(
				#"{"chats":[{"id":"a","title":"","presetName":"Fast","folderId":null,"updatedAt":1700000000000,"running":true,"later":1}],"next":"1700000000000.a"}"#
			),
			"https://smiths.nolune.family/api/p/smiths/chats?limit=50&after=1700000000000.a": .json(
				#"{"chats":[{"id":"b","title":"Trip","presetName":"Smart","folderId":"f","updatedAt":1690000000000,"running":false}],"next":null}"#
			)
		])
		let first = try await client.chats("smiths")
		XCTAssertEqual(first.chats.map(\.id), ["a"])
		XCTAssertEqual(first.chats[0].title, "")
		XCTAssertTrue(first.chats[0].running)
		XCTAssertEqual(first.chats[0].updated, Date(timeIntervalSince1970: 1_700_000_000))
		let next = try await client.chats("smiths", after: try XCTUnwrap(first.next))
		XCTAssertEqual(next.chats.map(\.folderId), ["f"])
		XCTAssertNil(next.next)
	}

	func testTakingAChatOutOfItsFolderSaysNull() async throws {
		let client = self.client([
			"https://smiths.nolune.family/api/c/abc/folder": .json(#"{"ok":true}"#)
		])
		try await client.moveChat("abc", to: nil)
		XCTAssertEqual(Answers.sent.last?.body, #"{"folderId":null}"#)
		try await client.moveChat("abc", to: "trip")
		XCTAssertEqual(Answers.sent.last?.body, #"{"folderId":"trip"}"#)
		XCTAssertEqual(Answers.sent.last?.method, "POST")
		XCTAssertEqual(Answers.sent.last?.headers["Content-Type"], "application/json")
	}

	func testRenamingAFolderAsksForIt() async throws {
		let client = self.client([
			"https://smiths.nolune.family/api/p/smiths/folders/trip": .json(#"{"id":"trip","name":"Japan"}"#)
		])
		let folder = try await client.renameFolder("smiths", "trip", to: "Japan")
		XCTAssertEqual(folder, Folder(id: "trip", name: "Japan"))
		XCTAssertEqual(Answers.sent.last?.method, "PATCH")
		XCTAssertEqual(Answers.sent.last?.body, #"{"name":"Japan"}"#)
	}

	func testAnErrorSaysWhatTheNoluneSaid() async {
		let client = self.client([
			"https://smiths.nolune.family/api/c/abc": .json(#"{"message":"That title is too long."}"#, status: 400),
			"https://smiths.nolune.family/api/c/gone": Answers.Answer(status: 500)
		])
		do {
			_ = try await client.renameChat("abc", to: String(repeating: "a", count: 500))
			XCTFail("a 400 is an error")
		} catch {
			XCTAssertEqual(error as? Refusal, Refusal(address: "smiths.nolune.family", status: 400, message: "That title is too long."))
			XCTAssertEqual(error.localizedDescription, "That title is too long.")
		}
		do {
			try await client.deleteChat("gone")
			XCTFail("a 500 is an error")
		} catch {
			XCTAssertEqual((error as? Refusal)?.message, nil)
			XCTAssertEqual((error as? Refusal)?.status, 500)
		}
	}

	func testTheBellCountsWhatsNew() async throws {
		let bell = try await client([
			"https://smiths.nolune.family/api/notifications": .json(
				#"""
				{"items":[
					{"id":"n2","title":"Rain tomorrow","body":"Take an **umbrella**.","level":"info","createdAt":2000,"profile":{"slug":"smiths","name":"Smiths","avatar":"moon"},"conversationId":null},
					{"id":"n1","title":"Backup failed","body":"","level":"error","createdAt":1000,"profile":{"slug":"work","name":"Work","avatar":"comet"},"conversationId":"c1"}
				],"seenAt":1500}
				"""#
			)
		]).bell()
		XCTAssertEqual(bell.items.map(\.id), ["n2", "n1"])
		XCTAssertEqual(bell.unseen, 1)
		XCTAssertEqual(bell.items[1].conversationId, "c1")
		XCTAssertEqual(bell.items[0].profile.avatar, "moon")
	}

	func testSomeoneSignedIn() async throws {
		let me = try await client([
			"https://smiths.nolune.family/api/me": .json(
				#"{"id":"u1","name":"Anna","email":"anna@example.com","isAdmin":true,"picture":null}"#
			)
		]).me()
		XCTAssertEqual(me, Me(id: "u1", name: "Anna", email: "anna@example.com", isAdmin: true, picture: nil))
	}

	func testANotificationContinuesInAChat() async throws {
		let place = try await client([
			"https://smiths.nolune.family/api/notifications/n1/continue": .json(#"{"slug":"smiths","conversationId":"c9"}"#)
		]).continueInChat("n1")
		XCTAssertEqual(place.slug, "smiths")
		XCTAssertEqual(place.conversationId, "c9")
		XCTAssertEqual(Answers.sent.last?.method, "POST")
	}

	// MARK: Preferences

	func testPreferencesReadWhatThePagesWrite() throws {
		let read = try JSONDecoder().decode(
			Preferences.self,
			from: Data(#"{"technical":true,"sounds":"loud","language":"de","later":1}"#.utf8)
		)
		XCTAssertTrue(read.technical)
		XCTAssertFalse(read.expandSteps)
		// Something it can't read is the default.
		XCTAssertTrue(read.sounds)
		XCTAssertEqual(read.language, "de")
		XCTAssertEqual(try JSONDecoder().decode(Preferences.self, from: Data("{}".utf8)), Preferences())
	}
}
