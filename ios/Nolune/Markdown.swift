import Foundation

/**
 * A reply's Markdown, in blocks (#135), read as the web reads it (`marked`, with GitHub's tables
 * and task lists): paragraphs, headings, lists (nested too), quotes, rules, fenced code, tables,
 * and pictures on lines of their own. The text inside a block (emphasis, links, inline code) is
 * Foundation's to read (MarkdownView.swift), which does inline Markdown well but not blocks.
 *
 * The blocks are values, so a view of a reply that's still streaming draws again only the blocks
 * that changed. A code fence that isn't closed yet runs to the end, as it will once it is.
 */
indirect enum MarkdownBlock: Hashable {
	case paragraph(String)
	case heading(level: Int, text: String)
	case code(language: String?, text: String)
	case quote([MarkdownBlock])
	case list(MarkdownList)
	case table(MarkdownTable)
	/// `![alt](target)` alone on its line.
	case image(alt: String, target: String)
	case rule
}

struct MarkdownList: Hashable {
	struct Item: Hashable {
		/// A task's box, ticked or not (`- [x] …`); nil for an item that isn't one.
		var checked: Bool?
		var blocks: [MarkdownBlock]
	}

	/// The first item's number in a numbered list; nil for bullets.
	var start: Int?
	var items: [Item]
}

struct MarkdownTable: Hashable {
	enum Alignment: Hashable {
		case leading, center, trailing
	}

	var header: [String]
	var alignments: [Alignment]
	/// As many cells as the header, each.
	var rows: [[String]]
}

enum Markdown {
	static func blocks(_ text: String) -> [MarkdownBlock] {
		let lines = text
			.replacingOccurrences(of: "\r\n", with: "\n")
			.split(separator: "\n", omittingEmptySubsequences: false)
			.map(expandingTabs)
		return blocks(lines)
	}

	/**
	 * A block's text as the lines it shows: its own lines run on as one, joined by a space, and
	 * break only where Markdown says, after two spaces or a backslash at the end.
	 */
	static func lines(_ text: String) -> [String] {
		let written = text.split(separator: "\n", omittingEmptySubsequences: false)
		var lines: [String] = []
		var line = ""
		for (index, part) in written.enumerated() {
			let breaks = index < written.count - 1 && (part.hasSuffix("  ") || part.hasSuffix("\\"))
			var words = part.hasSuffix("\\") && breaks ? String(part.dropLast()) : String(part)
			while words.hasSuffix(" ") { words.removeLast() }
			line += line.isEmpty || index == 0 ? words : " " + words
			if breaks {
				lines.append(line)
				line = ""
			}
		}
		lines.append(line)
		return lines
	}

	/// A link to a path on the computer (absolute, `~/…`, relative, or `file:`), as the web tells
	/// them apart (`isLocalHref`, packages/core/src/media-refs.ts).
	static func isLocal(_ target: String) -> Bool {
		if target.lowercased().hasPrefix("file:") { return true }
		if target.trimmingCharacters(in: .whitespaces).isEmpty { return false }
		if target.range(of: "^[a-zA-Z][a-zA-Z0-9+.-]*:", options: .regularExpression) != nil { return false }
		return !target.hasPrefix("#") && !target.hasPrefix("?") && !target.hasPrefix("//")
	}

	fileprivate static func blocks(_ lines: [String]) -> [MarkdownBlock] {
		var parser = Parser(lines: lines)
		return parser.blocks()
	}

	/// Tabs that indent a line, as four spaces each.
	private static func expandingTabs(_ line: Substring) -> String {
		guard line.first == "\t" || line.first == " " else { return String(line) }
		var indent = ""
		var rest = line
		while let first = rest.first, first == " " || first == "\t" {
			indent += first == "\t" ? "    " : " "
			rest = rest.dropFirst()
		}
		return indent + rest
	}
}

// MARK: - Reading blocks

private struct Parser {
	let lines: [String]
	var i = 0

	init(lines: [String]) {
		self.lines = lines
	}

	mutating func blocks() -> [MarkdownBlock] {
		var blocks: [MarkdownBlock] = []
		while i < lines.count {
			let line = lines[i]
			if line.isBlank {
				i += 1
			} else if let fence = Fence(line) {
				blocks.append(code(fence))
			} else if let heading = Self.heading(line) {
				blocks.append(heading)
				i += 1
			} else if Self.isRule(line) {
				blocks.append(.rule)
				i += 1
			} else if Self.quoted(line) != nil {
				blocks.append(quote())
			} else if let marker = ListMarker(line) {
				blocks.append(list(marker))
			} else if let table = table() {
				blocks.append(table)
			} else {
				blocks.append(contentsOf: paragraph())
			}
		}
		return blocks
	}

	/// Whether a line starts a block that ends a paragraph before it, as CommonMark has it.
	static func interrupts(_ line: String) -> Bool {
		if Fence(line) != nil || heading(line) != nil || isRule(line) || quoted(line) != nil { return true }
		// A numbered list only from 1, so a line like "2024. was a year" stays text.
		guard let marker = ListMarker(line), !marker.content.isBlank else { return false }
		return marker.number == nil || marker.number == 1
	}

	// MARK: Code

	private mutating func code(_ fence: Fence) -> MarkdownBlock {
		i += 1
		var body: [String] = []
		while i < lines.count, !fence.isClosed(by: lines[i]) {
			// Lines lose as much indentation as the fence had.
			body.append(String(lines[i].dropFirst(min(fence.indent, lines[i].leadingSpaces))))
			i += 1
		}
		i += 1
		return .code(language: fence.language, text: body.joined(separator: "\n"))
	}

	// MARK: Headings and rules

	static func heading(_ line: String) -> MarkdownBlock? {
		let indent = line.leadingSpaces
		guard indent <= 3 else { return nil }
		let rest = line.dropFirst(indent)
		let level = rest.prefix { $0 == "#" }.count
		guard (1...6).contains(level) else { return nil }
		let after = rest.dropFirst(level)
		guard after.isEmpty || after.first == " " else { return nil }
		var text = after.trimmingCharacters(in: .whitespaces)
		// A closing run of #s isn't part of it.
		if let closing = text.range(of: #"(^|\s)#+$"#, options: .regularExpression) {
			text = text[..<closing.lowerBound].trimmingCharacters(in: .whitespaces)
		}
		return .heading(level: level, text: text)
	}

	static func isRule(_ line: String) -> Bool {
		guard line.leadingSpaces <= 3 else { return false }
		let marks = line.filter { $0 != " " }
		guard marks.count >= 3, let first = marks.first, "-*_".contains(first) else { return false }
		return marks.allSatisfy { $0 == first }
	}

	/// The underline of a heading on the line above: `===` for a first-level one, `---` second.
	private static func underline(_ line: String) -> Int? {
		guard line.leadingSpaces <= 3 else { return nil }
		let marks = line.trimmingCharacters(in: .whitespaces)
		if !marks.isEmpty, marks.allSatisfy({ $0 == "=" }) { return 1 }
		// At least three, or a list's first "-" while a reply streams would flash as a heading.
		if marks.count >= 3, marks.allSatisfy({ $0 == "-" }) { return 2 }
		return nil
	}

	// MARK: Quotes

	/// A quoted line without its `>`.
	static func quoted(_ line: String) -> String? {
		let indent = line.leadingSpaces
		guard indent <= 3 else { return nil }
		var rest = line.dropFirst(indent)
		guard rest.first == ">" else { return nil }
		rest = rest.dropFirst()
		if rest.first == " " { rest = rest.dropFirst() }
		return String(rest)
	}

	private mutating func quote() -> MarkdownBlock {
		var inside: [String] = []
		while i < lines.count {
			let line = lines[i]
			if let content = Self.quoted(line) {
				inside.append(content)
			} else if !line.isBlank, let last = inside.last, !last.isBlank, !Self.interrupts(line) {
				// A lazy line, going on with the quote's paragraph without a `>`.
				inside.append(line)
			} else {
				break
			}
			i += 1
		}
		return .quote(Markdown.blocks(inside))
	}

	// MARK: Lists

	private mutating func list(_ first: ListMarker) -> MarkdownBlock {
		var items: [MarkdownList.Item] = []
		var marker = first
		while true {
			var body = [marker.content]
			i += 1
			// Lines indented under the item are its own; models nest under "1." with two spaces too.
			let needed = min(marker.contentIndent, marker.indent + 2)
			while i < lines.count {
				let line = lines[i]
				if line.isBlank {
					body.append("")
				} else if line.leadingSpaces >= needed {
					body.append(String(line.dropFirst(min(line.leadingSpaces, marker.contentIndent))))
				} else if let last = body.last, !last.isBlank, ListMarker(line) == nil, !Self.interrupts(line) {
					// A lazy line, going on with the item's paragraph.
					body.append(line)
				} else {
					break
				}
				i += 1
			}
			items.append(Self.item(body))
			guard i < lines.count, let next = ListMarker(lines[i]), next.isSibling(of: first) else { break }
			marker = next
		}
		return .list(MarkdownList(start: first.number, items: items))
	}

	private static func item(_ lines: [String]) -> MarkdownList.Item {
		var lines = lines
		var checked: Bool?
		if let first = lines.first, first.count >= 3, first.hasPrefix("["), Array(first)[2] == "]",
			first.count == 3 || Array(first)[3] == " "
		{
			switch Array(first)[1] {
			case " ": checked = false
			case "x", "X": checked = true
			default: break
			}
			if checked != nil { lines[0] = String(first.dropFirst(3)).trimmingCharacters(in: .whitespaces) }
		}
		return MarkdownList.Item(checked: checked, blocks: Markdown.blocks(lines))
	}

	// MARK: Tables

	private mutating func table() -> MarkdownBlock? {
		guard i + 1 < lines.count, let table = Self.tableStart(lines[i], lines[i + 1]) else { return nil }
		var rows: [[String]] = []
		i += 2
		while i < lines.count, !lines[i].isBlank, !Self.interrupts(lines[i]) {
			let cells = Self.cells(lines[i])
			let count = table.header.count
			rows.append(Array(cells.prefix(count)) + Array(repeating: "", count: max(0, count - cells.count)))
			i += 1
		}
		return .table(MarkdownTable(header: table.header, alignments: table.alignments, rows: rows))
	}

	/// A table's header and its alignments, when the two lines start one.
	static func tableStart(_ line: String, _ next: String) -> MarkdownTable? {
		guard line.contains("|"), let alignments = delimiters(next) else { return nil }
		let header = cells(line)
		guard header.count == alignments.count else { return nil }
		return MarkdownTable(header: header, alignments: alignments, rows: [])
	}

	/// The line under a table's header: `| --- | :---: | ---: |`.
	private static func delimiters(_ line: String) -> [MarkdownTable.Alignment]? {
		guard line.leadingSpaces <= 3, line.contains("-") else { return nil }
		var alignments: [MarkdownTable.Alignment] = []
		for cell in cells(line) {
			let left = cell.hasPrefix(":")
			let right = cell.count > 1 && cell.hasSuffix(":")
			let dashes = cell.dropFirst(left ? 1 : 0).dropLast(right ? 1 : 0)
			guard !dashes.isEmpty, dashes.allSatisfy({ $0 == "-" }) else { return nil }
			alignments.append(left && right ? .center : right ? .trailing : .leading)
		}
		// One column needs a pipe: `---` alone is a rule.
		guard alignments.count > 1 || line.contains("|") else { return nil }
		return alignments
	}

	/// A table row's cells, split at its pipes but not at escaped ones (`\|`).
	static func cells(_ line: String) -> [String] {
		var text = Substring(line.trimmingCharacters(in: .whitespaces))
		if text.hasPrefix("|") { text = text.dropFirst() }
		if text.hasSuffix("|"), !text.hasSuffix("\\|") { text = text.dropLast() }
		var cells: [String] = []
		var cell = ""
		var escaped = false
		for character in text {
			if escaped {
				if character != "|" { cell.append("\\") }
				cell.append(character)
				escaped = false
			} else if character == "\\" {
				escaped = true
			} else if character == "|" {
				cells.append(cell.trimmingCharacters(in: .whitespaces))
				cell = ""
			} else {
				cell.append(character)
			}
		}
		if escaped { cell.append("\\") }
		cells.append(cell.trimmingCharacters(in: .whitespaces))
		return cells
	}

	// MARK: Paragraphs

	private mutating func paragraph() -> [MarkdownBlock] {
		var text = [lines[i].trimmingLeadingSpaces]
		i += 1
		while i < lines.count {
			let line = lines[i]
			if line.isBlank { break }
			if let level = Self.underline(line) {
				i += 1
				return [.heading(level: level, text: text.joined(separator: " ").trimmingCharacters(in: .whitespaces))]
			}
			if Self.interrupts(line) { break }
			if i + 1 < lines.count, Self.tableStart(line, lines[i + 1]) != nil { break }
			text.append(line.trimmingLeadingSpaces)
			i += 1
		}
		// Pictures each on a line of their own are pictures; one among words stays in the text.
		let images = text.map(Self.image)
		if images.allSatisfy({ $0 != nil }) { return images.compactMap { $0 } }
		return [.paragraph(text.joined(separator: "\n"))]
	}

	private static func image(_ line: String) -> MarkdownBlock? {
		let text = line.trimmingCharacters(in: .whitespaces)
		guard text.hasPrefix("!["), text.hasSuffix(")"),
			let close = text.range(of: "]("),
			!text[text.index(text.startIndex, offsetBy: 2)..<close.lowerBound].contains("]")
		else { return nil }
		let alt = String(text[text.index(text.startIndex, offsetBy: 2)..<close.lowerBound])
		var target = text[close.upperBound..<text.index(before: text.endIndex)].trimmingCharacters(in: .whitespaces)
		// Without a title (`"…"` after the address), and without the angle brackets around one with spaces.
		if let title = target.range(of: #"\s+"[^"]*"$"#, options: .regularExpression) {
			target = String(target[..<title.lowerBound])
		}
		if target.hasPrefix("<"), target.hasSuffix(">") { target = String(target.dropFirst().dropLast()) }
		guard !target.isEmpty, !target.contains(")") else { return nil }
		return .image(alt: alt, target: target)
	}
}

/// A code fence: three or more backticks or tildes, and the code's language after them.
private struct Fence {
	let indent: Int
	let mark: Character
	let length: Int
	let language: String?

	init?(_ line: String) {
		indent = line.leadingSpaces
		guard indent <= 3 else { return nil }
		let rest = line.dropFirst(indent)
		guard let first = rest.first, first == "`" || first == "~" else { return nil }
		mark = first
		length = rest.prefix { $0 == first }.count
		guard length >= 3 else { return nil }
		let info = rest.dropFirst(length).trimmingCharacters(in: .whitespaces)
		// Backticks after backticks are inline code, not a fence.
		if first == "`", info.contains("`") { return nil }
		language = info.split(separator: " ").first.map(String.init)
	}

	func isClosed(by line: String) -> Bool {
		guard line.leadingSpaces <= 3 else { return false }
		let rest = line.drop { $0 == " " }
		let run = rest.prefix { $0 == mark }.count
		return run >= length && rest.dropFirst(run).allSatisfy { $0 == " " }
	}
}

/// A list item's marker: `-`, `*` or `+`, or a number and `.` or `)`.
private struct ListMarker {
	let indent: Int
	/// `-`, `*` or `+`; or `.` or `)` after a number.
	let mark: Character
	let number: Int?
	/// Where the item's text starts, for the lines under it.
	let contentIndent: Int
	let content: String

	init?(_ line: String) {
		indent = line.leadingSpaces
		guard indent <= 3 else { return nil }
		let rest = Array(line.dropFirst(indent))
		var width: Int
		if let first = rest.first, "-*+".contains(first) {
			mark = first
			number = nil
			width = 1
		} else {
			let digits = rest.prefix { $0.isASCII && $0.isNumber }
			guard (1...9).contains(digits.count), digits.count < rest.count, ".)".contains(rest[digits.count]) else {
				return nil
			}
			mark = rest[digits.count]
			number = Int(String(digits))
			width = digits.count + 1
		}
		let after = rest[width...]
		guard after.isEmpty || after.first == " " else { return nil }
		let spaces = after.prefix { $0 == " " }.count
		// After five spaces or more, the text starts after one: the rest indents it.
		let gap = after.isEmpty ? 1 : spaces > 4 ? 1 : spaces
		contentIndent = indent + width + gap
		content = String(after.dropFirst(min(gap, spaces)))
	}

	func isSibling(of first: ListMarker) -> Bool {
		mark == first.mark && (number == nil) == (first.number == nil)
	}
}

private extension StringProtocol {
	var leadingSpaces: Int { prefix { $0 == " " }.count }
	var isBlank: Bool { allSatisfy { $0 == " " } }
	var trimmingLeadingSpaces: String { String(drop { $0 == " " }) }
}
