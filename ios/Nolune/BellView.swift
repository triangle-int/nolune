import SwiftUI

/**
 * The bell, natively (#133): the notifications of the person's profiles, the new ones first,
 * each expanding to its whole text, to dismiss or continue as a chat. Opening it marks them seen;
 * they stay marked new while it's open. A tapped notification on the lock screen opens it with
 * that one expanded (`expand`).
 */
struct BellView: View {
	@Environment(\.palette) private var palette
	@ObservedObject var family: Family
	let expand: String?
	/// Opens the chat a notification continues in.
	let open: (ChatPlace) -> Void
	@Environment(\.dismiss) private var dismiss
	@State private var expanded: String?
	/// When the person had last opened the bell, as of this time: newer ones show as new.
	@State private var newAfter: Double?
	@State private var busy: String?
	@State private var clearing = false

	private var items: [Bell.Item] {
		let after = newAfter ?? family.bell.seenAt
		return family.bell.items.sorted { a, b in
			let (aNew, bNew) = (a.createdAt > after, b.createdAt > after)
			return aNew == bNew ? a.createdAt > b.createdAt : aNew
		}
	}

	var body: some View {
		NavigationStack {
			List {
				Group {
					if family.bell.items.isEmpty {
						Text("Nothing yet. Ask nolune for a reminder or a daily check, and what it finds shows up here.")
							.foregroundStyle(.secondary)
					}
					ForEach(items) { item in
						row(item)
							.swipeActions(edge: .trailing) {
								Button(role: .destructive) {
									Task { await family.dismiss(item) }
								} label: {
									Label("Dismiss", systemImage: "xmark")
								}
							}
					}
				}
				.listRowBackground(palette.muted)
			}
			.pageBackground(palette)
			.listStyle(.insetGrouped)
			.navigationTitle("Notifications")
			.navigationBarTitleDisplayMode(.inline)
			.refreshable { await family.reloadBell() }
			.toolbar {
				ToolbarItem(placement: .confirmationAction) {
					Button("Done") { dismiss() }
				}
				ToolbarItem(placement: .cancellationAction) {
					if !family.bell.items.isEmpty {
						Button("Clear all") { clearing = true }
					}
				}
			}
			.confirmationDialog("Clear all", isPresented: $clearing) {
				Button("Clear all", role: .destructive) {
					Task { await family.clearBell() }
				}
			}
		}
		.task {
			if newAfter == nil { newAfter = family.bell.seenAt }
			expanded = expand
			await family.markBellSeen()
		}
	}

	private func row(_ item: Bell.Item) -> some View {
		let open = expanded == item.id
		let new = item.createdAt > (newAfter ?? family.bell.seenAt)
		return VStack(alignment: .leading, spacing: 8) {
			HStack(alignment: .firstTextBaseline, spacing: 8) {
				AvatarView(avatar: item.profile.avatar)
					.frame(width: 18, height: 18)
					.alignmentGuide(.firstTextBaseline) { $0[.bottom] - 3 }
				Text(item.title)
					.font(.headline)
					.foregroundStyle(item.level == "error" ? Palette.plain.destructive : Palette.plain.foreground)
					.lineLimit(open ? nil : 1)
				Spacer(minLength: 4)
				if new {
					Text("New")
						.font(.caption2.weight(.semibold))
						.foregroundStyle(.white)
						.padding(.horizontal, 6)
						.padding(.vertical, 2)
						.background(Capsule().fill(Palette.plain.destructive))
				}
				Text(item.created, format: .relative(presentation: .named))
					.font(.caption)
					.foregroundStyle(.secondary)
			}
			if !item.body.isEmpty {
				Text(Self.text(item.body))
					.font(.subheadline)
					.foregroundStyle(.secondary)
					.lineLimit(open ? nil : 2)
					.textSelection(.enabled)
			}
			Text(item.profile.name)
				.font(.caption)
				.foregroundStyle(.tertiary)
			if open {
				HStack {
					Button {
						Task {
							busy = item.id
							if let place = await family.continueInChat(item) {
								dismiss()
								self.open(place)
							}
							busy = nil
						}
					} label: {
						if busy == item.id {
							ProgressView()
						} else {
							Text(item.conversationId == nil ? "Continue in chat" : "Open chat")
						}
					}
					.buttonStyle(.borderedProminent)
					Button("Dismiss", role: .destructive) {
						Task { await family.dismiss(item) }
					}
					.buttonStyle(.bordered)
				}
				.padding(.top, 4)
			}
		}
		.padding(.vertical, 4)
		.contentShape(Rectangle())
		.onTapGesture {
			withAnimation(.easeInOut(duration: 0.2)) { expanded = open ? nil : item.id }
		}
		.accessibilityElement(children: .contain)
		.accessibilityAddTraits(.isButton)
		.accessibilityHint(open ? Text("Show less") : Text("Show all"))
	}

	/// A notification's text, its links and emphasis as Markdown has them, the rest as it is.
	static func text(_ body: String) -> AttributedString {
		let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
		return (try? AttributedString(markdown: body, options: options)) ?? AttributedString(body)
	}
}
