import XCTest
@testable import Nolune

/// A chat as `/api/c/<id>/transcript` streams it (Transcript.swift), from core's own stream.
final class TranscriptTests: XCTestCase {
	/**
	 * What `streamTranscript` in core sent for a chat with a reply that ran a command, a note saved,
	 * an automation, a background command's result and a reply streaming, then what changed: more
	 * of the reply, a command's output, the reply ending in an error, a new title and command mode,
	 * and an event from a later nolune.
	 */
	private let stream = #"""
		{"type":"snapshot","snapshot":{"title":"Rain","model":{"presetId":"p1","presetName":"Sonnet","provider":"anthropic","effort":"medium","contextWindow":200000},"commands":{"mode":null,"fallback":"auto"},"toolChanges":null,"running":true,"error":null,"queued":[{"id":9,"kind":"human","senderId":"max","senderName":"Max","text":"And tomorrow?","attachments":[],"queued":true,"createdAt":30000}],"toolOutput":null,"background":[{"kind":"command","id":"b1","summary":"Watching","command":"tail -f log","startedAt":1}],"typing":[{"id":"max","name":"Max"}],"entries":[{"type":"human","key":"m1","message":{"id":1,"kind":"human","senderId":"anna","senderName":"Anna","text":"Will it rain?","attachments":[{"status":"ok","id":"md1","name":"sky.jpg","mime":"image/jpeg","bytes":2048,"width":640,"height":480,"viewable":true,"sentAs":"image"}],"queued":false,"createdAt":1000}},{"type":"reply","key":"reply-1","parts":[{"type":"activity","key":"reply-1-0","steps":[{"type":"thinking","text":"Check the forecast."},{"type":"command","id":"t1","command":"curl wttr.in","cwd":"/home/anna","summary":"Checking the forecast","icon":"cloud-rain"}],"startedAt":1000,"endedAt":14000},{"type":"text","key":"reply-1-1","text":"Yes, **at 3pm**. ![Map](/home/anna/map.png)","media":{"/home/anna/map.png":{"status":"ok","id":"md2","name":"map.png","mime":"image/png","bytes":100,"width":10,"height":10,"viewable":true}}}],"messageIds":[2,4],"usage":{"input":22,"cacheRead":220,"cacheWrite":5,"output":50},"stopReasons":["tool_use","end_turn"],"models":["claude-sonnet-5"],"live":false},{"type":"memory","key":"memory-4","look":{"after":4,"createdAt":16000,"changes":[{"id":1,"op":"add","note":"people/anna.md","fact":"Anna likes rain","before":null,"createdAt":16000,"undone":null,"card":null}]}},{"type":"trigger","key":"m5","message":{"id":5,"kind":"trigger","title":"Morning check","text":"Look at the weather","createdAt":20000}},{"type":"task_result","key":"m6","message":{"id":6,"kind":"task_result","title":"Backup","output":"done","isError":false,"createdAt":21000}},{"type":"reply","key":"reply-6","parts":[{"type":"text","key":"reply-6-0","text":"Looking","pending":true}],"messageIds":[],"usage":null,"stopReasons":[],"models":[],"live":true}],"results":{"t1":{"id":"t1","output":"Rain at 3pm","isError":false,"pictures":[]}}}}
		{"type":"transcript","order":["m1","reply-1","memory-4","m5","m6","reply-6"],"entries":[{"type":"reply","key":"reply-6","parts":[{"type":"text","key":"reply-6-0","text":"Looking at the sky.","pending":true}],"messageIds":[],"usage":null,"stopReasons":[],"models":[],"live":true}],"results":{}}
		{"type":"tool_output","id":"t9","chunk":"abc"}
		{"type":"transcript","order":["m1","reply-1","memory-4","m5","m6","reply-6"],"entries":[{"type":"reply","key":"reply-6","parts":[{"type":"text","key":"reply-6-0","text":"Looking at the sky.","pending":true}],"messageIds":[],"usage":null,"stopReasons":[],"models":[],"live":false}],"results":{}}
		{"type":"status","running":false,"error":"Overloaded"}
		{"type":"title","title":"Umbrellas"}
		{"type":"commands","commands":{"mode":"unrestricted","fallback":"auto"}}
		{"type":"something_new","x":1}
		"""#

	private func events() throws -> [ChatEvent] {
		try stream.split(separator: "\n").map { line in
			try JSONDecoder().decode(ChatEvent.self, from: Data(line.trimmingCharacters(in: .whitespaces).utf8))
		}
	}

	func testTheSnapshotHasTheTranscript() throws {
		var chat = ChatState()
		chat.apply(try XCTUnwrap(events().first))
		XCTAssertTrue(chat.loaded)
		XCTAssertEqual(chat.title, "Rain")
		XCTAssertEqual(chat.model?.presetName, "Sonnet")
		XCTAssertEqual(chat.commands.current, "auto")
		XCTAssertTrue(chat.running)
		XCTAssertEqual(chat.entries.map(\.id), ["m1", "reply-1", "memory-4", "m5", "m6", "reply-6"])
		XCTAssertEqual(chat.queued.map(\.text), ["And tomorrow?"])
		XCTAssertEqual(chat.typing.map(\.name), ["Max"])
		XCTAssertEqual(chat.background.first?.summary, "Watching")
		XCTAssertEqual(chat.results["t1"]?.status, .done)

		guard case .message(_, let anna) = chat.entries[0] else { return XCTFail("Anna's message") }
		XCTAssertEqual(anna.kind, "human")
		XCTAssertEqual(anna.attachments?.first?.isCopied, true)
		guard case .reply(let reply) = chat.entries[1] else { return XCTFail("a reply") }
		XCTAssertEqual(reply.usage?.prompt, 247)
		XCTAssertFalse(reply.live)
		guard case .activity(let work) = reply.parts[0], case .text(_, let text, let media, _) = reply.parts[1] else {
			return XCTFail("work, then text")
		}
		XCTAssertEqual(work.steps.first, .thinking("Check the forecast."))
		XCTAssertEqual(work.commands.map(\.summary), ["Checking the forecast"])
		XCTAssertEqual(work.commands.first?.icon, "cloud-rain")
		XCTAssertEqual(work.endedAt - work.startedAt, 13000)
		XCTAssertTrue(text.hasPrefix("Yes, **at 3pm**."))
		XCTAssertEqual(media["/home/anna/map.png"]?.id, "md2")
		XCTAssertEqual(reply.text, "Yes, **at 3pm**. ![Map](/home/anna/map.png)")
		guard case .memory(_, let look) = chat.entries[2] else { return XCTFail("a note") }
		XCTAssertEqual(look.changes.first?.fact, "Anna likes rain")
		XCTAssertEqual(chat.liveReply?.parts.count, 1)
	}

	func testWhatChangesAfterIt() throws {
		var chat = ChatState()
		for event in try events() { chat.apply(event) }
		guard case .reply(let streamed)? = chat.entries.last, case .text(_, let text, _, let pending) = streamed.parts[0] else {
			return XCTFail("the streamed reply")
		}
		XCTAssertEqual(text, "Looking at the sky.")
		XCTAssertTrue(pending)
		XCTAssertFalse(streamed.live)
		XCTAssertNil(chat.liveReply)
		XCTAssertEqual(chat.toolOutput, ToolOutput(id: "t9", text: "abc"))
		XCTAssertFalse(chat.running)
		XCTAssertEqual(chat.error, "Overloaded")
		XCTAssertEqual(chat.title, "Umbrellas")
		XCTAssertEqual(chat.commands.current, "unrestricted")
	}

	func testAnEntryLeavesWhenItsKeyDoes() throws {
		var chat = ChatState()
		chat.apply(try XCTUnwrap(events().first))
		chat.apply(.transcript(order: ["m1", "reply-1"], entries: [], results: [:]))
		XCTAssertEqual(chat.entries.map(\.id), ["m1", "reply-1"])
		// Its results stay: they're sent once.
		XCTAssertNotNil(chat.results["t1"])
	}

	func testAChatThatWasntAnswered() throws {
		var chat = ChatState()
		XCTAssertFalse(chat.unanswered)
		chat.apply(try XCTUnwrap(events().first))
		// Running, and someone's message waits: an answer is coming.
		XCTAssertFalse(chat.unanswered)
		chat.apply(.status(running: false, error: nil))
		chat.apply(.queued([]))
		XCTAssertFalse(chat.unanswered)
		// Without the streamed reply, the background command's result is the last word.
		chat.apply(.transcript(order: ["m1", "reply-1", "memory-4", "m5", "m6"], entries: [], results: [:]))
		XCTAssertTrue(chat.unanswered)
		// A note saved after a reply doesn't count.
		chat.apply(.transcript(order: ["m1", "reply-1", "memory-4"], entries: [], results: [:]))
		XCTAssertFalse(chat.unanswered)
	}

	/// What a chat's Live Activity shows (LiveActivities.swift), as the gateway's.
	func testWhatALiveActivityShows() throws {
		var chat = ChatState()
		chat.apply(try XCTUnwrap(events().first))
		XCTAssertEqual(chat.lastReply?.text, "Looking")
		XCTAssertNil(chat.runningCommand, "writing")
		let working = #"{"type":"transcript","order":["m1","reply-1","memory-4","m5","m6","reply-7"],"entries":[{"type":"reply","key":"reply-7","parts":[{"type":"activity","key":"reply-7-0","steps":[{"type":"command","id":"t7","command":"ls","summary":"Looking at the files"}],"startedAt":1,"endedAt":2}],"messageIds":[],"usage":null,"stopReasons":[],"models":[],"live":true}],"results":{}}"#
		chat.apply(try JSONDecoder().decode(ChatEvent.self, from: Data(working.utf8)))
		XCTAssertEqual(chat.runningCommand, "Looking at the files")
		chat.apply(.transcript(order: chat.entries.map(\.id), entries: [], results: ["t7": CommandResult(output: "a b", isError: false, pictures: [])]))
		XCTAssertNil(chat.runningCommand, "it ran")
		// A note saved after the reply doesn't count; a message with none yet does.
		chat.apply(.transcript(order: ["m1", "reply-1", "memory-4"], entries: [], results: [:]))
		XCTAssertEqual(chat.lastReply?.parts.count, 2)
		chat.apply(try XCTUnwrap(events().first))
		chat.apply(.transcript(order: ["m1", "reply-1", "memory-4", "m5"], entries: [], results: [:]))
		XCTAssertNil(chat.lastReply)
	}

	func testHowACommandEnded() {
		XCTAssertEqual(CommandResult(output: "ok", isError: false, pictures: []).status, .done)
		XCTAssertEqual(CommandResult(output: "Blocked by auto mode: it deletes files", isError: true, pictures: []).status, .blocked)
		XCTAssertEqual(CommandResult(output: "partial\nStopped by Anna", isError: true, pictures: []).status, .stopped)
		XCTAssertEqual(CommandResult(output: "exit 1", isError: true, pictures: []).status, .failed)
	}
}
