import XCTest
@testable import Nolune

/// Replies' Markdown in blocks (Markdown.swift), as the web's `marked` reads them.
final class MarkdownTests: XCTestCase {
	func testParagraphsAndHeadings() {
		XCTAssertEqual(
			Markdown.blocks("# Title\n\nOne line\nand the next.\n\n### Smaller ###\nSetext\n==="),
			[
				.heading(level: 1, text: "Title"),
				.paragraph("One line\nand the next."),
				.heading(level: 3, text: "Smaller"),
				.heading(level: 1, text: "Setext")
			]
		)
		// Not headings: no space after the #s, or seven of them.
		XCTAssertEqual(Markdown.blocks("#hashtag"), [.paragraph("#hashtag")])
		XCTAssertEqual(Markdown.blocks("####### seven"), [.paragraph("####### seven")])
	}

	func testRulesAndQuotes() {
		XCTAssertEqual(
			Markdown.blocks("Before\n\n---\n\n> Quoted\nlazily\n> > nested\n\n* * *"),
			[
				.paragraph("Before"),
				.rule,
				.quote([.paragraph("Quoted\nlazily"), .quote([.paragraph("nested")])]),
				.rule
			]
		)
		// Three dashes under text make it a heading, as in CommonMark.
		XCTAssertEqual(Markdown.blocks("Section\n---"), [.heading(level: 2, text: "Section")])
	}

	func testCodeBlocks() {
		XCTAssertEqual(
			Markdown.blocks("Run:\n\n```swift\nlet a = 1\n\n  print(a)\n```\nAfter"),
			[.paragraph("Run:"), .code(language: "swift", text: "let a = 1\n\n  print(a)"), .paragraph("After")]
		)
		// A fence interrupts a paragraph, and a longer one closes it.
		XCTAssertEqual(
			Markdown.blocks("Text\n~~~~\n~~~\n~~~~~"),
			[.paragraph("Text"), .code(language: nil, text: "~~~")]
		)
		// Still streaming: the code runs to the end.
		XCTAssertEqual(Markdown.blocks("```sh\nls -la\n# Not a heading"), [.code(language: "sh", text: "ls -la\n# Not a heading")])
	}

	func testLists() {
		XCTAssertEqual(
			Markdown.blocks("- **one**\n- two\n  - nested\n    more\n- three\n\nAfter"),
			[
				.list(
					MarkdownList(
						start: nil,
						items: [
							.init(checked: nil, blocks: [.paragraph("**one**")]),
							.init(
								checked: nil,
								blocks: [
									.paragraph("two"),
									.list(MarkdownList(start: nil, items: [.init(checked: nil, blocks: [.paragraph("nested\nmore")])]))
								]
							),
							.init(checked: nil, blocks: [.paragraph("three")])
						]
					)
				),
				.paragraph("After")
			]
		)
	}

	func testNumberedListsAndTasks() {
		XCTAssertEqual(
			Markdown.blocks("Steps:\n3. Open it\n\n4. Then:\n  - [x] done\n  - [ ] not yet"),
			[
				// A numbered list from 3 doesn't interrupt a paragraph.
				.paragraph("Steps:\n3. Open it"),
				.list(
					MarkdownList(
						start: 4,
						items: [
							.init(
								checked: nil,
								blocks: [
									.paragraph("Then:"),
									.list(
										MarkdownList(
											start: nil,
											items: [
												.init(checked: true, blocks: [.paragraph("done")]),
												.init(checked: false, blocks: [.paragraph("not yet")])
											]
										)
									)
								]
							)
						]
					)
				)
			]
		)
		XCTAssertEqual(
			Markdown.blocks("1. one\n2. two\n- other"),
			[
				.list(
					MarkdownList(
						start: 1,
						items: [.init(checked: nil, blocks: [.paragraph("one")]), .init(checked: nil, blocks: [.paragraph("two")])]
					)
				),
				.list(MarkdownList(start: nil, items: [.init(checked: nil, blocks: [.paragraph("other")])]))
			]
		)
		// Code inside an item.
		XCTAssertEqual(
			Markdown.blocks("1. Run\n   ```\n   make\n   ```"),
			[
				.list(
					MarkdownList(
						start: 1,
						items: [.init(checked: nil, blocks: [.paragraph("Run"), .code(language: nil, text: "make")])]
					)
				)
			]
		)
	}

	func testTables() {
		XCTAssertEqual(
			Markdown.blocks("Prices:\n| Plan | Price | Note |\n|:-----|------:|:----:|\n| Free | 0 | a \\| b |\n| Pro | 20\n\nAfter"),
			[
				.paragraph("Prices:"),
				.table(
					MarkdownTable(
						header: ["Plan", "Price", "Note"],
						alignments: [.leading, .trailing, .center],
						rows: [["Free", "0", "a | b"], ["Pro", "20", ""]]
					)
				),
				.paragraph("After")
			]
		)
		// Without the line under the header, it's text.
		XCTAssertEqual(Markdown.blocks("a | b\nc | d"), [.paragraph("a | b\nc | d")])
	}

	func testPictures() {
		XCTAssertEqual(
			Markdown.blocks("![Chart](/home/anna/chart.png)\n![Map](<my map.png> \"The map\")\n\nSee ![this](a.png) here."),
			[
				.image(alt: "Chart", target: "/home/anna/chart.png"),
				.image(alt: "Map", target: "my map.png"),
				.paragraph("See ![this](a.png) here.")
			]
		)
	}

	func testLinesRunOnUntilABreak() {
		XCTAssertEqual(Markdown.lines("One\ntwo  \nthree\\\nfour  "), ["One two", "three", "four"])
		XCTAssertEqual(Markdown.lines("Alone"), ["Alone"])
	}

	func testLinksToThisComputer() {
		for target in ["/home/anna/a.png", "~/report.pdf", "chart.png", "./out/x.csv", "file:///tmp/a"] {
			XCTAssertTrue(Markdown.isLocal(target), target)
		}
		for target in ["https://nolune.dev", "mailto:a@b.c", "#top", "?q=1", "//cdn.example.com/a.png", " "] {
			XCTAssertFalse(Markdown.isLocal(target), target)
		}
	}

	func testTheReleaseNotesExample() {
		let notes = """
			nolune now installs like any Mac app.

			## What's new

			### 🌌 nolune for Mac, no Terminal needed

			- **your account**, with a strong password made for you;
			- **an address for the family**, like `https://smiths.nolune.family`;

			After that nolune lives in the menu bar.

			## Also in this release

			- The new site at [nolune.dev](https://nolune.dev).
			"""
		let blocks = Markdown.blocks(notes)
		XCTAssertEqual(blocks.count, 7)
		XCTAssertEqual(blocks[1], .heading(level: 2, text: "What's new"))
		guard case .list(let list) = blocks[3] else { return XCTFail("a list") }
		XCTAssertEqual(list.items.count, 2)
		XCTAssertEqual(blocks[6], .list(MarkdownList(start: nil, items: [.init(checked: nil, blocks: [.paragraph("The new site at [nolune.dev](https://nolune.dev).")])])))
	}
}
