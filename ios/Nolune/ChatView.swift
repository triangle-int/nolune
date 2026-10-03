import SwiftUI

/**
 * A chat, natively (#134): its transcript as it streams (Conversation), each reply's Markdown
 * (MarkdownView) and work (ActivityView), and the composer under it (ChatComposer). It follows the
 * newest message while a reply streams, unless the person scrolled up, then offers to jump back.
 * Pictures and files open in Quick Look; links to the family's nolune open in the app.
 */
struct ChatView: View {
	@ObservedObject var family: Family
	@StateObject private var chat: Conversation
	/// A link to a page of the family's nolune: true when the app opened it.
	let open: (URL) -> Bool
	/// The chat was deleted from its menu.
	let deleted: () -> Void
	@State private var preferences = Preferences()
	@State private var following = true
	@State private var away = false
	@State private var preview: MediaPreview.Item?
	@State private var renaming = false
	@State private var newTitle = ""
	@State private var deleting = false

	private static let bottom = "bottom"

	init(id: String, family: Family, open: @escaping (URL) -> Bool, deleted: @escaping () -> Void) {
		_family = ObservedObject(wrappedValue: family)
		_chat = StateObject(wrappedValue: Conversation(id: id, slug: family.slug ?? "", client: family.client))
		self.open = open
		self.deleted = deleted
	}

	private var context: ChatContext {
		ChatContext(
			chat: chat.id,
			client: family.client,
			me: family.me?.id,
			members: Dictionary((family.profile?.members ?? []).map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a }),
			avatar: family.profile?.avatar ?? "probe",
			preferences: preferences,
			results: chat.state.results,
			toolOutput: chat.state.toolOutput,
			running: chat.state.running
		)
	}

	var body: some View {
		ScrollViewReader { proxy in
			ScrollView {
				LazyVStack(alignment: .leading, spacing: 22) {
					if !chat.state.loaded {
						ProgressView()
							.frame(maxWidth: .infinity)
							.padding(.top, 40)
					} else if chat.state.entries.isEmpty, !chat.state.running {
						Text("Ask for something to get started.")
							.foregroundStyle(.secondary)
							.frame(maxWidth: .infinity)
							.padding(.top, 40)
					}
					let last = chat.state.entries.last?.id
					ForEach(chat.state.entries) { entry in
						EntryView(entry: entry, context: context, last: entry.id == last) { change in
							Task { await chat.undo(change) }
						}
						.equatable()
					}
					ChatTail(chat: chat, context: context)
					Color.clear
						.frame(height: 1)
						.id(Self.bottom)
						.onAppear {
							following = true
							away = false
						}
						.onDisappear { away = true }
				}
				.padding(.horizontal, 16)
				.padding(.vertical, 12)
				.frame(maxWidth: 780)
				.frame(maxWidth: .infinity)
			}
			.scrollDismissesKeyboard(.interactively)
			// Scrolling to read stops following the reply; letting go at the newest message follows again.
			.modifier(Scrolling(began: { following = false }, ended: { if !away { following = true } }))
			.onChange(of: chat.revision) { _ in
				if following { proxy.scrollTo(Self.bottom, anchor: .bottom) }
			}
			.overlay(alignment: .bottom) {
				if away, !following {
					Button {
						following = true
						withAnimation(.easeInOut(duration: 0.25)) { proxy.scrollTo(Self.bottom, anchor: .bottom) }
					} label: {
						Image(systemName: "arrow.down")
							.font(.body.weight(.semibold))
							.padding(10)
							.background(.regularMaterial, in: Circle())
							.shadow(color: .black.opacity(0.15), radius: 4, y: 1)
					}
					.buttonStyle(.plain)
					.accessibilityLabel(Text("Scroll to the newest message"))
					.padding(.bottom, 8)
					.transition(.opacity)
				}
			}
			.safeAreaInset(edge: .bottom, spacing: 0) {
				ChatComposer(chat: chat, family: family) {
					following = true
					proxy.scrollTo(Self.bottom, anchor: .bottom)
				}
			}
		}
		.environment(\.noluneClient, family.client)
		.environment(\.openURL, OpenURLAction { url in route(url) })
		.navigationTitle(title)
		.toolbar {
			ToolbarItem(placement: .primaryAction) { menu }
		}
		.sheet(item: $preview) { item in
			MediaPreview(item: item)
		}
		.alert("Rename chat", isPresented: $renaming) {
			TextField("Chat name", text: $newTitle)
			Button("Save") {
				let title = newTitle.trimmingCharacters(in: .whitespacesAndNewlines)
				guard !title.isEmpty, let summary = family.chat(chat.id) else { return }
				Task { await family.rename(summary, to: title) }
			}
			Button("Cancel", role: .cancel) {}
		}
		.confirmationDialog("Delete chat?", isPresented: $deleting, titleVisibility: .visible) {
			Button("Delete", role: .destructive) {
				guard let summary = family.chat(chat.id) else { return }
				Task {
					if await family.delete(summary) { deleted() }
				}
			}
		} message: {
			Text("This deletes \(title) for everyone in the profile.")
		}
		.task {
			chat.start()
			preferences = await Preferences.read(family.client.origin)
		}
		.onDisappear { chat.stop() }
	}

	private var title: String {
		let title = chat.state.title.isEmpty ? (family.chat(chat.id)?.title ?? "") : chat.state.title
		return title.isEmpty ? String(localized: "New chat") : title
	}

	private var menu: some View {
		Menu {
			Button {
				newTitle = chat.state.title
				renaming = true
			} label: {
				Label("Rename", systemImage: "pencil")
			}
			if let summary = family.chat(chat.id) {
				Menu {
					if summary.folderId != nil {
						Button("No folder") { Task { await family.move(summary, to: nil) } }
					}
					ForEach(family.folders.filter { $0.id != summary.folderId }) { folder in
						Button(folder.name) { Task { await family.move(summary, to: folder.id) } }
					}
				} label: {
					Label("Move to folder", systemImage: "folder")
				}
			}
			if preferences.technical, let model = chat.state.model, let usage = chat.state.lastUsage, let window = model.contextWindow {
				Section {
					Text("Context \(Tokens.format(usage.prompt + usage.output)) / \(Tokens.format(window))")
				}
			}
			Divider()
			Button(role: .destructive) {
				deleting = true
			} label: {
				Label("Delete", systemImage: "trash")
			}
		} label: {
			Image(systemName: "ellipsis.circle")
		}
		.accessibilityLabel(Text("Chat options"))
	}

	/// A link tapped in the chat: a picture or file of it to Quick Look, a page of the family's
	/// nolune to the app, anything else to Safari.
	private func route(_ url: URL) -> OpenURLAction.Result {
		let origin = family.client.origin
		guard Address.sameOrigin(url, origin) else { return .systemAction }
		if url.path.hasPrefix("/api/c/"), url.path.contains("/media/") {
			preview = MediaPreview.Item(url: url)
			return .handled
		}
		return open(url) ? .handled : .systemAction
	}
}

/// When the person scrolls the chat themselves, as opposed to it following a reply.
private struct Scrolling: ViewModifier {
	let began: () -> Void
	let ended: () -> Void

	func body(content: Content) -> some View {
		if #available(iOS 18.0, *) {
			content.onScrollPhaseChange { _, phase in
				if phase == .interacting { began() } else if phase == .idle { ended() }
			}
		} else {
			content.simultaneousGesture(
				DragGesture(minimumDistance: 10)
					.onChanged { drag in
						if drag.translation.height > 0 { began() }
					}
					.onEnded { _ in ended() }
			)
		}
	}
}

/// What every part of a chat needs to show itself.
struct ChatContext {
	let chat: String
	let client: Client
	/// The person reading, whose messages need no name.
	let me: String?
	/// The profile's members, for others' names and pictures.
	let members: [String: Profile.Member]
	/// The profile's assistant.
	let avatar: String
	let preferences: Preferences
	let results: [String: CommandResult]
	let toolOutput: ToolOutput?
	let running: Bool

	/// Where a reply's links and pictures lead: the copies nolune kept of them.
	func links(_ media: [String: ChatMedia]) -> MarkdownLinks {
		let (client, chat) = (client, chat)
		return MarkdownLinks { target in
			guard let found = media[target], found.isCopied, let id = found.id else { return nil }
			return MarkdownLinks.Media(url: client.media(chat, id), name: found.name, viewable: found.viewable == true)
		}
	}

	func url(_ media: ChatMedia) -> URL? {
		media.id.map { client.media(chat, $0) }
	}

	/// A member's picture, from the family's nolune.
	func picture(_ person: String?) -> URL? {
		guard let person, let path = members[person]?.picture else { return nil }
		return URL(string: path, relativeTo: client.origin)?.absoluteURL
	}
}

// MARK: - Entries

private struct EntryView: View, Equatable {
	let entry: Entry
	let context: ChatContext
	/// The newest entry, which shows the command running now.
	let last: Bool
	let undo: (Int) -> Void

	static func == (a: EntryView, b: EntryView) -> Bool {
		guard a.entry == b.entry, a.last == b.last, a.context.preferences == b.context.preferences, a.context.me == b.context.me,
			a.context.avatar == b.context.avatar
		else { return false }
		guard case .reply(let reply) = a.entry else { return true }
		let commands = reply.parts.flatMap { part -> [String] in
			if case .activity(let activity) = part { return activity.commands.map(\.id) } else { return [] }
		}
		guard commands.map({ a.context.results[$0] }) == commands.map({ b.context.results[$0] }) else { return false }
		return !a.last || (a.context.running == b.context.running && a.context.toolOutput == b.context.toolOutput)
	}

	var body: some View {
		switch entry {
		case .message(_, let message):
			switch message.kind {
			case "human":
				HumanMessage(message: message, context: context)
			case "trigger":
				Card(symbol: "clock", title: String(localized: "Automation · \(message.title ?? "")")) {
					Text(verbatim: message.text ?? "")
						.textSelection(.enabled)
				}
			case "agent_message":
				Card(symbol: "cpu", title: String(localized: "From nolune, to \(message.title ?? "")")) {
					MarkdownView(text: message.text ?? "")
				}
			case "task_result":
				TaskResultCard(message: message, technical: context.preferences.technical)
			default:
				EmptyView()
			}
		case .memory(_, let look):
			MemoryCard(look: look, undo: undo)
		case .compaction(_, let summary, let asked, let live):
			SummaryCard(summary: summary, asked: asked, live: live)
		case .reply(let reply):
			ReplyView(reply: reply, context: context, last: last)
		case .unknown:
			EmptyView()
		}
	}
}

/// Someone's message: theirs on the right, with whoever sent it when it isn't the reader.
private struct HumanMessage: View {
	let message: ChatMessage
	let context: ChatContext
	var queued = false
	@Environment(\.openURL) private var openURL

	var body: some View {
		VStack(alignment: .trailing, spacing: 6) {
			if message.senderId != context.me {
				HStack(spacing: 6) {
					PersonPicture(url: context.picture(message.senderId), name: message.senderName ?? "")
						.frame(width: 20, height: 20)
					Text(verbatim: message.senderName ?? "")
						.font(.caption)
						.foregroundStyle(.secondary)
				}
			}
			ForEach(Array((message.attachments ?? []).enumerated()), id: \.offset) { _, media in
				attachment(media)
			}
			if let text = message.text, !text.isEmpty {
				Text(verbatim: text)
					.padding(.horizontal, 14)
					.padding(.vertical, 10)
					.background(
						RoundedRectangle(cornerRadius: 18, style: .continuous)
							.fill(Color(.secondarySystemBackground))
					)
					.overlay {
						if queued {
							RoundedRectangle(cornerRadius: 18, style: .continuous)
								.strokeBorder(Color.secondary.opacity(0.5), style: StrokeStyle(lineWidth: 1, dash: [4, 3]))
						}
					}
					.opacity(queued ? 0.7 : 1)
					.contextMenu {
						Button {
							UIPasteboard.general.string = text
						} label: {
							Label("Copy", systemImage: "doc.on.doc")
						}
						ShareLink(item: text)
					}
			}
		}
		.frame(maxWidth: .infinity, alignment: .trailing)
		.padding(.leading, 40)
	}

	@ViewBuilder private func attachment(_ media: ChatMedia) -> some View {
		if media.isCopied, let url = context.url(media) {
			if media.viewable == true {
				Button {
					openURL(url)
				} label: {
					RemotePicture(url: url)
						.frame(maxWidth: 260, maxHeight: 192)
				}
				.buttonStyle(.plain)
				.accessibilityLabel(Text(verbatim: media.name))
			} else {
				FileCard(name: media.name, detail: media.bytes.map(Self.size)) { openURL(url) }
			}
		} else {
			Label(media.name, systemImage: "exclamationmark.triangle")
				.font(.caption)
				.foregroundStyle(.secondary)
		}
		if let note = media.note {
			Text("nolune got only where it's saved: \(note)")
				.font(.caption)
				.foregroundStyle(.secondary)
		}
	}

	static func size(_ bytes: Int) -> String {
		ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)
	}
}

/// A person's picture, or their initial.
struct PersonPicture: View {
	let url: URL?
	let name: String

	var body: some View {
		if let url {
			AsyncImage(url: url) { image in
				image.resizable().scaledToFill()
			} placeholder: {
				initial
			}
			.clipShape(Circle())
		} else {
			initial
		}
	}

	private var initial: some View {
		Circle()
			.fill(Color.secondary.opacity(0.25))
			.overlay(
				Text(verbatim: String(name.prefix(1)).uppercased())
					.font(.caption2.weight(.semibold))
					.foregroundStyle(.secondary)
			)
	}
}

/// nolune's reply: its text and its work, then copying it, and its usage for the technical.
private struct ReplyView: View {
	let reply: Reply
	let context: ChatContext
	let last: Bool

	var body: some View {
		VStack(alignment: .leading, spacing: 12) {
			AvatarView(avatar: context.avatar)
				.frame(width: 24, height: 24)
			ForEach(Array(reply.parts.enumerated()), id: \.element.id) { index, part in
				switch part {
				case .text(_, let text, let media, _):
					MarkdownView(text: text, links: context.links(media))
				case .activity(let activity):
					ActivityView(
						activity: activity,
						context: context,
						active: reply.live && index == reply.parts.count - 1,
						showsOutput: last
					)
				}
			}
			if reply.live, waiting {
				WorkingDots()
					.accessibilityLabel(Text("nolune is working"))
			}
			if reply.stopReasons.contains("max_tokens") {
				Notice(text: String(localized: "The reply was cut off because it got too long."))
			}
			if reply.stopReasons.contains("refusal") {
				Notice(text: String(localized: "nolune declined to continue this request."))
			}
			if !reply.live, !reply.text.isEmpty {
				HStack(spacing: 16) {
					Button {
						UIPasteboard.general.string = reply.text
						Haptics.tap()
					} label: {
						Image(systemName: "doc.on.doc")
					}
					.accessibilityLabel(Text("Copy"))
					ShareLink(item: reply.text) {
						Image(systemName: "square.and.arrow.up")
					}
					if context.preferences.technical, let usage = reply.usage {
						Text(Self.usage(usage, models: reply.models))
							.font(.caption)
							.foregroundStyle(.secondary)
							.lineLimit(2)
					}
				}
				.font(.subheadline)
				.foregroundStyle(.secondary)
				.buttonStyle(.plain)
			}
		}
	}

	/// Nothing streams yet: no part, or text that's done while nolune goes on.
	private var waiting: Bool {
		guard let last = reply.parts.last else { return true }
		if case .text(_, _, _, let pending) = last { return !pending }
		return false
	}

	private static func usage(_ usage: Usage, models: [String]) -> String {
		let tokens = String(localized: "\(Tokens.format(usage.prompt)) tokens in, \(Tokens.format(usage.output)) out")
		let rate = usage.prompt > 0 ? Double(usage.cacheRead) / Double(usage.prompt) : 0
		let cached = String(localized: "\(rate.formatted(.percent.precision(.fractionLength(0)))) cached overall")
		return ([models.joined(separator: ", ")] + [tokens, cached]).filter { !$0.isEmpty }.joined(separator: " · ")
	}
}

/// Token counts as the web shows them: 950, 12K, 1.5M.
enum Tokens {
	static func format(_ count: Int) -> String {
		count.formatted(.number.notation(.compactName).precision(.significantDigits(1...2)))
	}
}

/// Three dots that pulse while nolune is about to write.
private struct WorkingDots: View {
	@State private var on = false

	var body: some View {
		HStack(spacing: 4) {
			ForEach(0..<3, id: \.self) { index in
				Circle()
					.fill(Color.secondary)
					.frame(width: 6, height: 6)
					.opacity(on ? 1 : 0.3)
					.animation(.easeInOut(duration: 0.6).repeatForever().delay(Double(index) * 0.2), value: on)
			}
		}
		.onAppear { on = true }
	}
}

private struct Notice: View {
	let text: String

	var body: some View {
		Label(text, systemImage: "exclamationmark.circle")
			.font(.footnote)
			.foregroundStyle(.secondary)
	}
}

/// A card for what isn't a message or a reply: an automation, a subagent, a result, a note.
private struct Card<Content: View>: View {
	let symbol: String
	let title: String
	var tint = Color.secondary
	@ViewBuilder let content: Content

	var body: some View {
		VStack(alignment: .leading, spacing: 8) {
			Label(title, systemImage: symbol)
				.font(.subheadline.weight(.medium))
				.foregroundStyle(tint)
			content
				.font(.subheadline)
		}
		.padding(12)
		.frame(maxWidth: .infinity, alignment: .leading)
		.background(
			RoundedRectangle(cornerRadius: 12, style: .continuous)
				.strokeBorder(Color(.separator), lineWidth: 0.5)
		)
	}
}

/// A card that opens to what's in it.
private struct Folding<Content: View>: View {
	let symbol: String
	let title: String
	var detail: String?
	var tint = Color.secondary
	var shimmer = false
	@ViewBuilder let content: Content
	@State private var open = false

	var body: some View {
		VStack(alignment: .leading, spacing: 10) {
			Button {
				withAnimation(.easeInOut(duration: 0.2)) { open.toggle() }
			} label: {
				HStack(spacing: 6) {
					Image(systemName: symbol)
					Text(verbatim: title)
						.lineLimit(1)
					if let detail {
						Text(verbatim: "· \(detail)")
							.foregroundStyle(tint)
					}
					Spacer(minLength: 4)
					Image(systemName: "chevron.right")
						.font(.caption.weight(.semibold))
						.rotationEffect(.degrees(open ? 90 : 0))
				}
				.font(.subheadline)
				.foregroundStyle(.secondary)
				.contentShape(Rectangle())
			}
			.buttonStyle(.plain)
			.accessibilityValue(open ? Text("Show less") : Text("Show all"))
			if open {
				content
					.font(.subheadline)
			}
		}
		.padding(12)
		.background(
			RoundedRectangle(cornerRadius: 12, style: .continuous)
				.strokeBorder(Color(.separator), lineWidth: 0.5)
		)
	}
}

private struct TaskResultCard: View {
	let message: ChatMessage
	let technical: Bool

	var body: some View {
		Folding(
			symbol: "terminal",
			title: String(localized: "Finished in the background · \(message.title ?? "")"),
			detail: message.isError == true ? (technical ? String(localized: "failed") : String(localized: "didn't work")) : nil,
			tint: .red
		) {
			ScrollView(.horizontal, showsIndicators: false) {
				Text(verbatim: message.output ?? "")
					.font(.caption.monospaced())
					.textSelection(.enabled)
					.fixedSize()
			}
		}
	}
}

private struct MemoryCard: View {
	let look: MemoryLook
	let undo: (Int) -> Void

	var body: some View {
		let undone = look.changes.allSatisfy { $0.undone != nil }
		Folding(symbol: "brain", title: String(localized: "Saved \(look.changes.count) memories")) {
			VStack(alignment: .leading, spacing: 10) {
				ForEach(look.changes) { change in
					HStack(alignment: .firstTextBaseline) {
						VStack(alignment: .leading, spacing: 2) {
							Text(verbatim: change.fact)
								.strikethrough(change.undone != nil)
							Text(verbatim: change.note)
								.font(.caption)
								.foregroundStyle(.secondary)
						}
						Spacer(minLength: 8)
						if change.undone == nil {
							Button("Undo") { undo(change.id) }
								.font(.caption)
								.buttonStyle(.bordered)
						}
					}
				}
			}
		}
		.opacity(undone ? 0.6 : 1)
	}
}

private struct SummaryCard: View {
	let summary: String
	let asked: Bool
	let live: Bool

	var body: some View {
		Folding(
			symbol: "list.bullet.indent",
			title: live ? String(localized: "Summarizing the conversation so far") : String(localized: "Summarized the conversation so far")
		) {
			VStack(alignment: .leading, spacing: 8) {
				Text(asked ? "Someone in the chat asked for this summary: the model goes on from it." : "The chat went quiet, so it was summarized: the model goes on from this.")
					.font(.caption)
					.foregroundStyle(.secondary)
				MarkdownView(text: summary)
			}
		}
	}
}

// MARK: - After the entries

/// What comes after the transcript: work in the background, messages waiting, who's typing, and
/// a reply that went wrong or didn't come.
private struct ChatTail: View {
	@ObservedObject var chat: Conversation
	let context: ChatContext

	var body: some View {
		let state = chat.state
		if !state.background.isEmpty {
			Card(symbol: "gearshape.2", title: String(localized: "Working in the background")) {
				VStack(alignment: .leading, spacing: 8) {
					ForEach(state.background) { item in
						HStack(spacing: 8) {
							ProgressView()
								.controlSize(.small)
							Text(verbatim: Self.describe(item, technical: context.preferences.technical))
								.lineLimit(1)
						}
					}
					Button("Stop", role: .destructive) {
						Task { await chat.stopWork() }
					}
					.buttonStyle(.bordered)
				}
			}
		}
		ForEach(state.queued.filter { $0.kind == "human" }, id: \.id) { message in
			VStack(alignment: .trailing, spacing: 4) {
				HumanMessage(message: message, context: context, queued: true)
				Label {
					if message.senderId == context.me {
						Text("nolune reads this after its current step")
					} else {
						Text(verbatim: "\(message.senderName ?? "") · ") + Text("nolune reads this after its current step")
					}
				} icon: {
					Image(systemName: "clock")
				}
				.font(.caption)
				.foregroundStyle(.secondary)
			}
			.frame(maxWidth: .infinity, alignment: .trailing)
		}
		let typing = state.typing.filter { $0.id != context.me }
		if !typing.isEmpty {
			HStack(spacing: 6) {
				Spacer()
				ForEach(typing) { person in
					PersonPicture(url: context.picture(person.id), name: person.name)
						.frame(width: 18, height: 18)
				}
				Text(Self.typing(typing.map(\.name)))
					.font(.caption)
					.foregroundStyle(.secondary)
				WorkingDots()
			}
		}
		if let error = state.error, !state.running {
			VStack(alignment: .leading, spacing: 8) {
				Label("Something went wrong while nolune was answering.", systemImage: "exclamationmark.triangle")
					.font(.subheadline.weight(.medium))
					.foregroundStyle(.red)
				Text(verbatim: error)
					.font(.caption)
					.foregroundStyle(.secondary)
					.textSelection(.enabled)
				Button("Try again") { Task { await chat.resume() } }
					.buttonStyle(.bordered)
			}
			.padding(12)
			.frame(maxWidth: .infinity, alignment: .leading)
			.background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Color.red.opacity(0.08)))
		} else if state.unanswered {
			HStack {
				Text("nolune hasn't answered this yet.")
					.font(.subheadline)
					.foregroundStyle(.secondary)
				Spacer()
				Button("Continue") { Task { await chat.resume() } }
					.buttonStyle(.bordered)
			}
		}
	}

	static func describe(_ item: BackgroundItem, technical: Bool) -> String {
		if item.kind == "subagent" {
			let name = String(localized: "Subagent \(item.name ?? "")")
			return item.status == "stopping" ? "\(name) · \(String(localized: "stopping"))" : name
		}
		if technical, let command = item.command { return "$ \(command)" }
		return item.summary ?? item.command ?? String(localized: "A command")
	}

	static func typing(_ names: [String]) -> String {
		let list = ListFormatter.localizedString(byJoining: names)
		return names.count == 1 ? String(localized: "\(list) is typing") : String(localized: "\(list) are typing")
	}
}
