import SwiftUI

/**
 * Automations, natively (#137): the month as a calendar of what ran and what will, a day's runs
 * under it, and every automation with its schedule. One opens to run it now, pause or resume it,
 * change what it says and does, see its last runs, or delete it. New ones are asked for in a chat.
 */
struct AutomationsView: View {
	@Environment(\.palette) private var palette
	@ObservedObject var family: Family
	let slug: String
	/// Opens a chat a run happened in.
	let openChat: (String) -> Void
	@State private var overview: AutomationsOverview?
	@State private var month: String?
	@State private var day: String?
	@State private var problem: String?

	private var client: Client { family.client }

	var body: some View {
		List {
			Group {
				if let overview {
					if overview.triggers.isEmpty {
						Section {
							Text("No automations yet. Ask nolune in a chat, for example:")
							Text("\"Every weekday at 7:30, check the weather and tell us if we need umbrellas.\"")
								.foregroundStyle(.secondary)
							Text("\"Remind Anna tomorrow at 17:00 to pick up the parcel.\"")
								.foregroundStyle(.secondary)
						}
					} else {
						Section {
							MonthView(calendar: overview.calendar, day: $day) { month = $0 }
						} footer: {
							Text("Times are in the computer's time zone (\(overview.timeZone)).")
						}
						if let cell = overview.calendar.cells.first(where: { $0.key == (day ?? overview.calendar.selected) }) {
							Section(cell.relative.map { "\($0) · \(cell.title)" } ?? cell.title) {
								if cell.entries.isEmpty {
									Text(cell.isPast ? "Nothing ran on this day." : "Nothing runs on this day.")
										.foregroundStyle(.secondary)
								}
								ForEach(Array(cell.entries.enumerated()), id: \.offset) { _, entry in
									entryRow(entry)
								}
							}
						}
						Section("All automations") {
							ForEach(overview.triggers) { automation in
								NavigationLink {
									AutomationView(
										automation: automation,
										slug: slug,
										client: client,
										openChat: openChat,
										ran: { chat in family.startLiveActivity(chat, title: automation.name) }
									) {
										await load()
									}
								} label: {
									AutomationRow(automation: automation)
								}
							}
						}
					}
				} else if problem == nil {
					ProgressView()
						.frame(maxWidth: .infinity)
				}
			}
			.listRowBackground(palette.muted)
		}
		.pageBackground(palette)
		.navigationTitle("Automations")
		.environment(\.noluneClient, client)
		.refreshable { await load() }
		.task(id: month) { await load() }
		.alert(problem ?? "", isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } })) {
			Button("OK", role: .cancel) {}
		}
	}

	private func entryRow(_ entry: AutomationsOverview.Calendar.Day.Entry) -> some View {
		Button {
			if let chat = entry.conversationId { openChat(chat) }
		} label: {
			HStack(spacing: 10) {
				Text(verbatim: entry.time)
					.font(.subheadline.monospacedDigit())
					.foregroundStyle(.secondary)
				LucideIcon(name: entry.icon, fallback: entry.kind == "webhook" ? "link" : "clock")
				Text(verbatim: entry.name)
					.foregroundStyle(Palette.plain.foreground)
				Spacer()
				if let status = entry.status {
					Text(verbatim: AutomationView.describe(status))
						.font(.caption)
						.foregroundStyle(status == "failed" ? Palette.plain.destructive : .secondary)
				}
			}
		}
		.disabled(entry.conversationId == nil)
	}

	private func load() async {
		do {
			overview = try await client.automations(slug, month: month)
		} catch {
			// The screen went away while it loaded: it loads again when it's back.
			if !error.isCancellation { problem = error.localizedDescription }
		}
	}
}

/// A month, Monday first: each day with the icons of what runs on it.
private struct MonthView: View {
	let calendar: AutomationsOverview.Calendar
	@Binding var day: String?
	let showMonth: (String) -> Void

	private let columns = Array(repeating: GridItem(.flexible(), spacing: 2), count: 7)

	var body: some View {
		VStack(spacing: 8) {
			HStack {
				Text(verbatim: calendar.title)
					.font(.headline)
				Spacer()
				if let current = calendar.current {
					Button("Today") {
						day = nil
						showMonth(current)
					}
					.font(.subheadline)
				}
				Button {
					if let prev = calendar.prev { day = nil; showMonth(prev) }
				} label: {
					Image(systemName: "chevron.left")
				}
				.disabled(calendar.prev == nil)
				.accessibilityLabel(Text("Previous month"))
				Button {
					day = nil
					showMonth(calendar.next)
				} label: {
					Image(systemName: "chevron.right")
				}
				.accessibilityLabel(Text("Next month"))
			}
			.buttonStyle(.borderless)
			LazyVGrid(columns: columns, spacing: 2) {
				ForEach(calendar.weekdays, id: \.self) { weekday in
					Text(verbatim: weekday)
						.font(.caption2)
						.foregroundStyle(.secondary)
				}
				ForEach(calendar.cells) { cell in
					Button {
						day = cell.key
					} label: {
						VStack(spacing: 2) {
							Text(verbatim: "\(cell.day)")
								.font(.callout.weight(cell.isToday ? .bold : .regular))
								.foregroundStyle(cell.isToday ? Palette.plain.primary : cell.inMonth ? Palette.plain.foreground : Color.secondary)
							HStack(spacing: 1) {
								ForEach(cell.icons, id: \.key) { icon in
									LucideIcon(name: icon.icon, fallback: icon.kind == "webhook" ? "link" : "clock")
										.scaleEffect(0.7)
										.frame(width: 11, height: 11)
								}
							}
							.frame(height: 12)
						}
						.frame(maxWidth: .infinity, minHeight: 40)
						.background(
							RoundedRectangle(cornerRadius: 8, style: .continuous)
								.fill(cell.key == (day ?? calendar.selected) ? Palette.plain.primary.opacity(0.15) : Color.clear)
						)
						.opacity(cell.inMonth ? 1 : 0.4)
					}
					.buttonStyle(.plain)
					.accessibilityLabel(Text(verbatim: cell.title))
				}
			}
			if !calendar.frequent.isEmpty {
				ForEach(calendar.frequent) { automation in
					HStack {
						LucideIcon(name: automation.icon, fallback: "clock")
						Text(verbatim: automation.name)
						Spacer()
						Text(verbatim: automation.schedule)
							.font(.caption)
							.foregroundStyle(.secondary)
					}
				}
			}
		}
		.padding(.vertical, 4)
	}
}

private struct AutomationRow: View {
	let automation: AutomationsOverview.Automation

	var body: some View {
		HStack(alignment: .top, spacing: 10) {
			LucideIcon(name: automation.icon, fallback: automation.kind == "webhook" ? "link" : "clock")
				.foregroundStyle(.secondary)
			VStack(alignment: .leading, spacing: 3) {
				HStack {
					Text(verbatim: automation.name)
						.font(.body.weight(.medium))
					if automation.state != "on" {
						Text(automation.state == "done" ? "done" : "paused")
							.font(.caption2.weight(.semibold))
							.padding(.horizontal, 6)
							.padding(.vertical, 2)
							.background(Capsule().fill(Color.secondary.opacity(0.2)))
					}
				}
				Text(verbatim: [automation.schedule, automation.next.map { String(localized: "next \($0)") }].compactMap { $0 }.joined(separator: " · "))
					.font(.caption)
					.foregroundStyle(.secondary)
				if let summary = automation.summary {
					Text(verbatim: summary)
						.font(.subheadline)
						.foregroundStyle(.secondary)
						.lineLimit(2)
				}
			}
		}
		.opacity(automation.state == "on" ? 1 : 0.6)
	}
}

/// An automation: to run now, pause or resume, change, see its runs, or delete.
private struct AutomationView: View {
	@Environment(\.palette) private var palette
	let automation: AutomationsOverview.Automation
	let slug: String
	let client: Client
	let openChat: (String) -> Void
	/// Its run started, in this chat.
	let ran: (String) -> Void
	let changed: () async -> Void
	@Environment(\.dismiss) private var dismiss
	@State private var summary = ""
	@State private var text = ""
	@State private var deleting = false
	@State private var message: String?
	@State private var working = false

	private var edited: Bool { summary != (automation.summary ?? "") || text != automation.text }

	var body: some View {
		Form {
			Group {
				Section {
					Text(verbatim: automation.schedule)
					if let next = automation.next {
						Text("next \(next)")
							.foregroundStyle(.secondary)
					}
					Button("Run now") {
						act {
							if let chat = try await client.runAutomation(slug, automation.id) { ran(chat) }
						}
					}
					if automation.state != "done" {
						Button(automation.state == "on" ? "Pause" : "Resume") {
							act { try await client.setAutomation(slug, automation.id, enabled: automation.state != "on") }
						}
					}
				} footer: {
					if let message {
						Text(verbatim: message)
					}
				}
				Section("Description") {
					TextField("What it does, in one sentence", text: $summary, axis: .vertical)
				}
				Section {
					TextField("", text: $text, axis: .vertical)
						.font(automation.action == "script" ? .callout.monospaced() : .body)
						.lineLimit(3...12)
					if edited {
						Button("Save") {
							act { try await client.editAutomation(slug, automation.id, summary: summary, text: text) }
						}
						.disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
					}
				} header: {
					Text("Instructions for nolune")
				} footer: {
					if automation.action == "script" {
						Text("Script: runs without the model and calls \("nolune wake") when nolune is needed")
					}
				}
				if let webhook = automation.webhookUrl {
					Section {
						Text(verbatim: webhook)
							.font(.caption.monospaced())
							.textSelection(.enabled)
					} footer: {
						Text("Webhook URL. Keep it secret: anyone with it can start a run. POST JSON to it.")
					}
				}
				Section("Recent runs (\(automation.runs.count))") {
					if automation.runs.isEmpty {
						Text("No runs yet")
							.foregroundStyle(.secondary)
					}
					ForEach(automation.runs) { run in
						RunRow(run: run, script: automation.action == "script") { openChat($0) }
					}
				}
				Section {
					Button("Delete", role: .destructive) { deleting = true }
				}
			}
			.listRowBackground(palette.muted)
		}
		.pageBackground(palette)
		.navigationTitle(automation.name)
		.navigationBarTitleDisplayMode(.inline)
		.disabled(working)
		.onAppear {
			summary = automation.summary ?? ""
			text = automation.text
		}
		.confirmationDialog("Delete \"\(automation.name)\"?", isPresented: $deleting, titleVisibility: .visible) {
			Button("Delete", role: .destructive) {
				act(leave: true) { try await client.deleteAutomation(slug, automation.id) }
			}
		}
	}

	private func act(leave: Bool = false, _ work: @escaping () async throws -> Void) {
		Task {
			working = true
			defer { working = false }
			do {
				try await work()
				Haptics.success()
				await changed()
				if leave { dismiss() }
			} catch {
				message = error.localizedDescription
				Haptics.failure()
			}
		}
	}

	/// A run's status, in words.
	static func describe(_ status: String) -> String {
		switch status {
		case "pending": return String(localized: "waiting")
		case "running": return String(localized: "running")
		case "ok": return String(localized: "done")
		case "notified": return String(localized: "notified")
		case "silent": return String(localized: "nothing to report")
		case "stopped": return String(localized: "stopped")
		case "failed": return String(localized: "failed")
		default: return status
		}
	}

	static func source(_ source: String) -> String {
		switch source {
		case "cron", "once": return String(localized: "scheduled")
		case "webhook": return String(localized: "webhook")
		case "wake": return String(localized: "woken by script")
		case "manual": return String(localized: "run by hand")
		default: return source
		}
	}
}

private struct RunRow: View {
	let run: AutomationsOverview.Automation.Run
	let script: Bool
	let openChat: (String) -> Void
	@State private var open = false

	var body: some View {
		VStack(alignment: .leading, spacing: 6) {
			HStack {
				Text(verbatim: run.at)
				Spacer()
				Text(verbatim: script ? String(localized: "script \(AutomationView.describe(run.status))") : AutomationView.describe(run.status))
					.foregroundStyle(run.status == "failed" ? Palette.plain.destructive : .secondary)
			}
			.font(.subheadline)
			HStack(spacing: 12) {
				Text(verbatim: AutomationView.source(run.source))
					.foregroundStyle(.secondary)
				if let chat = run.conversationId {
					Button("view") { openChat(chat) }
						.buttonStyle(.borderless)
				}
				if run.output != nil {
					Button("output") { open.toggle() }
						.buttonStyle(.borderless)
				}
			}
			.font(.caption)
			if open, let output = run.output {
				ScrollView(.horizontal, showsIndicators: false) {
					Text(verbatim: output)
						.font(.caption.monospaced())
						.textSelection(.enabled)
						.fixedSize()
				}
			}
		}
	}
}
