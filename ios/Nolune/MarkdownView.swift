import SwiftUI

/**
 * A reply's Markdown, natively (#135): its blocks (Markdown.swift) as views, and the text in them
 * as Foundation reads inline Markdown, with its emphasis, inline code and links. Text selects
 * block by block; links go to the `openURL` around it, which the chat routes (a reply's file to
 * Quick Look, anything else to Safari).
 *
 * Pictures and files the reply points to show from the copies nolune kept of them (`links`), as
 * on the web: a path on the computer without a copy is words without a link, and a picture without
 * one doesn't load from anywhere else. While a reply streams, each block draws again only when it
 * changed.
 */
struct MarkdownView: View {
	let text: String
	var links = MarkdownLinks.plain

	var body: some View {
		MarkdownBlocks(blocks: Markdown.blocks(text), links: links)
	}
}

/// Where a reply's links and pictures lead.
struct MarkdownLinks {
	/// The copy of a picture or a file the reply pointed to.
	struct Media {
		let url: URL
		let name: String
		/// A picture the app shows; anything else opens as a file.
		let viewable: Bool
	}

	/// The copy kept of what the reply pointed to, by the target as written; nil without one.
	var media: (String) -> Media?

	/// Text that isn't a reply's: links as written, and no copies.
	static let plain = MarkdownLinks(media: { _ in nil })

	/// A link's address: its copy, none for a path on the computer without one, or as written.
	func link(_ target: String) -> URL? {
		if let media = media(target) { return media.url }
		if Markdown.isLocal(target) { return nil }
		return URL(string: target)
	}
}

private struct MarkdownBlocks: View {
	let blocks: [MarkdownBlock]
	let links: MarkdownLinks
	var depth = 0

	var body: some View {
		VStack(alignment: .leading, spacing: 10) {
			ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
				MarkdownBlockView(block: block, links: links, depth: depth)
					.equatable()
			}
		}
	}
}

private struct MarkdownBlockView: View, Equatable {
	let block: MarkdownBlock
	let links: MarkdownLinks
	let depth: Int

	static func == (a: MarkdownBlockView, b: MarkdownBlockView) -> Bool {
		a.block == b.block && a.depth == b.depth
	}

	var body: some View {
		switch block {
		case .paragraph(let text):
			inline(text)
		case .heading(let level, let text):
			inline(text)
				.font(Self.font(level))
				.padding(.top, level <= 2 ? 6 : 2)
				.accessibilityAddTraits(.isHeader)
		case .code(let language, let text):
			CodeBlock(language: language, code: text)
		case .quote(let blocks):
			HStack(alignment: .top, spacing: 10) {
				RoundedRectangle(cornerRadius: 1.5)
					.fill(Color.secondary.opacity(0.35))
					.frame(width: 3)
				MarkdownBlocks(blocks: blocks, links: links, depth: depth)
					.foregroundStyle(.secondary)
			}
			.fixedSize(horizontal: false, vertical: true)
		case .list(let list):
			ListBlock(list: list, links: links, depth: depth)
		case .table(let table):
			TableBlock(table: table, links: links)
		case .image(let alt, let target):
			MediaBlock(alt: alt, media: links.media(target))
		case .rule:
			Divider()
				.padding(.vertical, 4)
		}
	}

	private func inline(_ text: String) -> some View {
		Text(MarkdownInline.text(text, links: links))
			.fixedSize(horizontal: false, vertical: true)
			.textSelection(.enabled)
	}

	private static func font(_ level: Int) -> Font {
		switch level {
		case 1: return .title2.weight(.semibold)
		case 2: return .title3.weight(.semibold)
		case 3: return .headline
		default: return .subheadline.weight(.semibold)
		}
	}
}

// MARK: - Inline

extension Markdown {
	/**
	 * Markdown as text without its markup, for a widget or a Live Activity: a line for each
	 * paragraph, heading, list item or table row, and a picture's description.
	 */
	static func plain(_ markdown: String) -> String {
		blocks(markdown).flatMap(plainLines).joined(separator: "\n")
	}

	private static func plainLines(_ block: MarkdownBlock) -> [String] {
		func inline(_ text: String) -> String {
			String(MarkdownInline.text(text, links: .plain).characters)
		}
		switch block {
		case .paragraph(let text), .heading(_, let text):
			return [inline(text)]
		case .code(_, let text):
			return [text]
		case .quote(let blocks):
			return blocks.flatMap(plainLines)
		case .list(let list):
			return list.items.map { item in
				"• " + item.blocks.flatMap(plainLines).joined(separator: " ")
			}
		case .table(let table):
			return ([table.header] + table.rows).map { $0.map(inline).joined(separator: " · ") }
		case .image(let alt, _):
			return alt.isEmpty ? [] : [alt]
		case .rule:
			return []
		}
	}
}

enum MarkdownInline {
	/**
	 * A block's text: Markdown's emphasis, inline code and links. Its lines run on as one, and break
	 * only where Markdown says (two spaces or a backslash at the end), as the web's `marked` has
	 * them. Links and pictures go where `links` says; a picture among words is a link to it.
	 */
	static func text(_ markdown: String, links: MarkdownLinks) -> AttributedString {
		var text = AttributedString()
		for (index, line) in Markdown.lines(markdown).enumerated() {
			if index > 0 { text.append(AttributedString("\n")) }
			text.append(inline(line, links: links))
		}
		return text
	}

	private static func inline(_ markdown: String, links: MarkdownLinks) -> AttributedString {
		let options = AttributedString.MarkdownParsingOptions(
			allowsExtendedAttributes: false,
			interpretedSyntax: .inlineOnlyPreservingWhitespace,
			failurePolicy: .returnPartiallyParsedIfPossible
		)
		var text = (try? AttributedString(markdown: markdown, options: options)) ?? AttributedString(markdown)
		for run in text.runs {
			let range = run.range
			if let link = run.link {
				text[range].link = links.link(Self.target(link))
			} else if let image = run.imageURL {
				text[range].link = links.media(Self.target(image))?.url
				text[range].imageURL = nil
			}
			// As the web has them: links in the text's color (the tint), underlined in a light grey;
			// code on the `secondary` grey.
			if text[range].link != nil {
				text[range].underlineStyle = Text.LineStyle(pattern: .solid, color: Palette.plain.color(\.ring))
			}
			if run.inlinePresentationIntent?.contains(.code) == true {
				text[range].backgroundColor = Palette.plain.color(\.secondary)
			}
		}
		return text
	}

	/// Where a link pointed, as the reply wrote it.
	private static func target(_ url: URL) -> String {
		let written = url.scheme == nil ? url.relativeString : url.absoluteString
		return written.removingPercentEncoding ?? written
	}
}

// MARK: - Blocks

private struct CodeBlock: View {
	@Environment(\.palette) private var palette
	let language: String?
	let code: String
	@State private var copied = false

	var body: some View {
		VStack(alignment: .leading, spacing: 0) {
			HStack {
				Text(verbatim: language ?? "text")
					.font(.caption.monospaced())
					.foregroundStyle(.secondary)
				Spacer()
				Button(action: copy) {
					Label(copied ? "Copied" : "Copy", systemImage: copied ? "checkmark" : "doc.on.doc")
						.font(.caption)
				}
				.buttonStyle(.borderless)
			}
			.padding(.horizontal, 12)
			.padding(.vertical, 6)
			Divider()
			ScrollView(.horizontal, showsIndicators: false) {
				Text(verbatim: code)
					.font(.system(.callout, design: .monospaced))
					.textSelection(.enabled)
					.fixedSize()
					.padding(12)
			}
		}
		.background(RoundedRectangle(cornerRadius: 16, style: .continuous).fill(palette.muted.opacity(0.5)))
		.overlay(RoundedRectangle(cornerRadius: 16, style: .continuous).strokeBorder(Palette.plain.border))
	}

	private func copy() {
		UIPasteboard.general.string = code
		Haptics.tap()
		copied = true
		Task {
			try? await Task.sleep(nanoseconds: 1_500_000_000)
			copied = false
		}
	}
}

private struct ListBlock: View {
	let list: MarkdownList
	let links: MarkdownLinks
	let depth: Int

	var body: some View {
		VStack(alignment: .leading, spacing: 6) {
			ForEach(Array(list.items.enumerated()), id: \.offset) { index, item in
				HStack(alignment: .firstTextBaseline, spacing: 6) {
					marker(index, item)
						.frame(minWidth: 18, alignment: .trailing)
					MarkdownBlocks(blocks: item.blocks, links: links, depth: depth + 1)
				}
			}
		}
	}

	@ViewBuilder private func marker(_ index: Int, _ item: MarkdownList.Item) -> some View {
		if let checked = item.checked {
			Image(systemName: checked ? "checkmark.square.fill" : "square")
				.foregroundStyle(checked ? Palette.plain.primary : Color.secondary)
				.accessibilityLabel(checked ? Text("Done") : Text("Not done"))
		} else if let start = list.start {
			Text(verbatim: "\(start + index).")
				.monospacedDigit()
		} else {
			Text(verbatim: depth % 2 == 0 ? "•" : "◦")
		}
	}
}

private struct TableBlock: View {
	let table: MarkdownTable
	let links: MarkdownLinks

	var body: some View {
		ScrollView(.horizontal, showsIndicators: false) {
			Grid(alignment: .leading, horizontalSpacing: 0, verticalSpacing: 0) {
				GridRow {
					ForEach(Array(table.header.enumerated()), id: \.offset) { column, text in
						cell(text, column)
							.font(.subheadline.weight(.semibold))
					}
				}
				ForEach(Array(table.rows.enumerated()), id: \.offset) { _, row in
					Divider()
					GridRow {
						ForEach(Array(row.enumerated()), id: \.offset) { column, text in
							cell(text, column)
								.font(.subheadline)
						}
					}
				}
			}
			.overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(Palette.plain.border))
			.clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
		}
	}

	private func cell(_ text: String, _ column: Int) -> some View {
		let alignment = column < table.alignments.count ? table.alignments[column] : .leading
		return Text(MarkdownInline.text(text, links: links))
			.multilineTextAlignment(alignment == .center ? .center : alignment == .trailing ? .trailing : .leading)
			.fixedSize(horizontal: false, vertical: true)
			// A long cell wraps instead of making the table as wide as its text.
			.frame(width: text.count > 36 ? 240 : nil)
			.padding(.horizontal, 10)
			.padding(.vertical, 6)
			.gridColumnAlignment(alignment == .center ? .center : alignment == .trailing ? .trailing : .leading)
			.textSelection(.enabled)
	}
}

/// A picture the reply shows, or a file it attached, from the copy nolune kept; a tap opens it.
private struct MediaBlock: View {
	let alt: String
	let media: MarkdownLinks.Media?
	@Environment(\.openURL) private var openURL

	var body: some View {
		if let media, media.viewable {
			Button {
				openURL(media.url)
			} label: {
				RemotePicture(url: media.url)
					.frame(maxWidth: 420, maxHeight: 360, alignment: .leading)
			}
			.buttonStyle(.plain)
			.accessibilityLabel(alt.isEmpty ? Text("Picture") : Text(alt))
		} else if let media {
			FileCard(name: media.name) { openURL(media.url) }
		} else {
			Label(alt.isEmpty ? String(localized: "Picture") : alt, systemImage: "photo")
				.font(.subheadline)
				.foregroundStyle(.secondary)
		}
	}
}

/// A picture from the family's nolune (the session's cookie goes with it), as it loads.
struct RemotePicture: View {
	@Environment(\.palette) private var palette
	let url: URL

	var body: some View {
		AsyncImage(url: url) { phase in
			switch phase {
			case .success(let image):
				image
					.resizable()
					.scaledToFit()
			case .failure:
				Image(systemName: "photo")
					.font(.title2)
					.foregroundStyle(.secondary)
					.frame(width: 120, height: 90)
			default:
				RoundedRectangle(cornerRadius: 10)
					.fill(palette.muted)
					.frame(width: 160, height: 120)
					.overlay(ProgressView())
			}
		}
		.clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
	}
}

/// A file to open: its name and a sign of what it is.
struct FileCard: View {
	@Environment(\.palette) private var palette
	let name: String
	var detail: String?
	let open: () -> Void

	var body: some View {
		Button(action: open) {
			HStack(spacing: 10) {
				Image(systemName: "doc")
					.font(.title3)
					.foregroundStyle(.secondary)
				VStack(alignment: .leading, spacing: 2) {
					Text(verbatim: name)
						.font(.subheadline)
						.lineLimit(1)
						.truncationMode(.middle)
					if let detail {
						Text(verbatim: detail)
							.font(.caption)
							.foregroundStyle(.secondary)
					}
				}
				Spacer(minLength: 0)
			}
			.padding(10)
			.frame(maxWidth: 320, alignment: .leading)
			.background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(palette.muted))
		}
		.buttonStyle(.plain)
	}
}
