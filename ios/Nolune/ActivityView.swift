import SwiftUI

/**
 * nolune's work between two pieces of its text, folded into one line as the web folds it: what it
 * does now ("Thinking", a command's summary), or how long it took ("Worked for 12s"). Opened, or
 * with the steps preference on, it lists each step: its thinking, the commands it ran with their
 * output, and summaries of the conversation.
 */
struct ActivityView: View {
	let activity: Part.Activity
	let context: ChatContext
	/// The work happening right now.
	let active: Bool
	/// The newest reply's, whose running command shows its output as it comes.
	let showsOutput: Bool
	/// Set once the reader opens or closes it; until then the preference decides.
	@State private var choice: Bool?

	private var open: Bool { choice ?? context.preferences.expandSteps }
	private var technical: Bool { context.preferences.technical }

	private var statuses: [CommandResult.Status] {
		activity.commands.compactMap { context.results[$0.id]?.status }
	}

	var body: some View {
		VStack(alignment: .leading, spacing: 12) {
			Button {
				withAnimation(.easeInOut(duration: 0.2)) { choice = !open }
			} label: {
				HStack(spacing: 4) {
					Text(verbatim: label)
						.font(active && technical ? .subheadline.monospaced() : .subheadline)
						.lineLimit(1)
					if !active {
						let failed = statuses.filter { $0 == .failed }.count
						let blocked = statuses.filter { $0 == .blocked }.count
						if failed > 0, technical {
							Text("· \(failed) failed")
								.foregroundStyle(Palette.plain.destructive)
						}
						if blocked > 0 {
							Text("· \(blocked) blocked")
								.foregroundStyle(Palette.plain.warning)
						}
					}
					if !open, !pictures.isEmpty {
						Text(verbatim: "·")
						ForEach(Array(pictures.enumerated()), id: \.offset) { _, url in
							RemotePicture(url: url)
								.frame(width: 36, height: 24)
						}
						.accessibilityHidden(true)
					}
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
				VStack(alignment: .leading, spacing: 14) {
					ForEach(Array(activity.steps.enumerated()), id: \.offset) { _, step in
						StepRow(step: step, context: context, showsOutput: showsOutput)
					}
				}
				.padding(.leading, 4)
			}
		}
	}

	/// As the web says it (`activeStepLabel` and Activity.svelte).
	private var label: String {
		if active {
			switch activity.steps.last {
			case .compaction(let summary, _)? where summary.isEmpty:
				return String(localized: "Summarizing the conversation so far")
			case .command(let command)? where context.results[command.id] == nil:
				if technical, let text = command.command { return String(localized: "Running \(Self.firstLine(text, 80))") }
				return command.summary ?? String(localized: "Running a command")
			default:
				return String(localized: "Thinking")
			}
		}
		if statuses.contains(.stopped) { return String(localized: "Stopped") }
		let duration = Self.duration(activity.endedAt - activity.startedAt)
		let commands = activity.commands.count
		if commands == 0 {
			return duration.map { String(localized: "Thought for \($0)") } ?? String(localized: "Thought for a moment")
		}
		if technical {
			let ran = String(localized: "Ran \(commands) commands")
			return duration.map { "\(ran) · \($0)" } ?? ran
		}
		return duration.map { String(localized: "Worked for \($0)") } ?? String(localized: "Worked for a moment")
	}

	/// The last pictures its commands showed (`nolune view`), seen while it's folded.
	private var pictures: [URL] {
		Array(activity.commands.flatMap { context.results[$0.id]?.pictures ?? [] }.compactMap(context.url).suffix(3))
	}

	/// 12s, 1m 5s, 1h 2m; nil under a second.
	static func duration(_ milliseconds: Double) -> String? {
		let seconds = Int((milliseconds / 1000).rounded())
		guard seconds >= 1 else { return nil }
		return Duration.seconds(seconds).formatted(.units(allowed: [.hours, .minutes, .seconds], width: .narrow, maximumUnitCount: 2))
	}

	static func firstLine(_ text: String, _ length: Int) -> String {
		let line = text.split(separator: "\n", omittingEmptySubsequences: true).first.map(String.init) ?? ""
		return line.count > length ? String(line.prefix(length)) + "…" : line
	}
}

/// One step of the work.
private struct StepRow: View {
	let step: Step
	let context: ChatContext
	let showsOutput: Bool

	var body: some View {
		switch step {
		case .thinking(let text):
			HStack(alignment: .firstTextBaseline, spacing: 10) {
				Circle()
					.fill(Color.secondary.opacity(0.6))
					.frame(width: 6, height: 6)
				MarkdownView(text: text.isEmpty ? String(localized: "Thinking…") : text)
					.font(.subheadline)
					.foregroundStyle(.secondary)
			}
		case .compaction(let summary, _):
			HStack(alignment: .firstTextBaseline, spacing: 10) {
				Image(systemName: "list.bullet.indent")
					.foregroundStyle(.secondary)
				VStack(alignment: .leading, spacing: 6) {
					Text(summary.isEmpty ? "Summarizing the conversation so far" : "Summarized the conversation so far")
						.font(.subheadline)
						.foregroundStyle(.secondary)
					if !summary.isEmpty {
						Text("The chat had grown too long for the model, so it goes on from this summary.")
							.font(.caption)
							.foregroundStyle(.secondary)
						MarkdownView(text: summary)
							.font(.subheadline)
					}
				}
			}
		case .command(let command):
			CommandStep(command: command, context: context, showsOutput: showsOutput)
		case .unknown:
			EmptyView()
		}
	}
}

/// A command nolune ran: what it does, how it ended, and opened, the command and its output.
private struct CommandStep: View {
	@Environment(\.palette) private var palette
	let command: Step.Command
	let context: ChatContext
	let showsOutput: Bool
	@State private var open = false

	private var result: CommandResult? { context.results[command.id] }
	private var technical: Bool { context.preferences.technical }
	/// Still running: no result yet, and nolune is at work.
	private var running: Bool { result == nil && context.running }

	var body: some View {
		VStack(alignment: .leading, spacing: 8) {
			Button {
				withAnimation(.easeInOut(duration: 0.2)) { open.toggle() }
			} label: {
				HStack(alignment: .firstTextBaseline, spacing: 8) {
					LucideIcon(name: command.icon)
						.foregroundStyle(.secondary)
					Text(verbatim: label)
						.font(technical && command.command != nil ? .subheadline.monospaced() : .subheadline)
						.lineLimit(open ? nil : 1)
						.frame(maxWidth: .infinity, alignment: .leading)
					status
						.font(.caption)
				}
				.contentShape(Rectangle())
			}
			.buttonStyle(.plain)
			if open { details }
		}
	}

	private var label: String {
		if technical {
			guard let text = command.command else { return String(localized: "Preparing a command…") }
			return "$ \(ActivityView.firstLine(text, 120))"
		}
		if let summary = command.summary { return summary }
		if command.command == nil { return String(localized: "Getting ready…") }
		return running ? String(localized: "Running a command") : String(localized: "Ran a command")
	}

	@ViewBuilder private var status: some View {
		if running {
			ProgressView()
				.controlSize(.small)
		} else if let result {
			switch result.status {
			case .failed:
				Text(technical ? "failed" : "didn't work").foregroundStyle(Palette.plain.destructive)
			case .blocked:
				Text("blocked").foregroundStyle(Palette.plain.warning)
			case .stopped:
				Text("stopped").foregroundStyle(.secondary)
			case .done:
				EmptyView()
			}
		} else {
			Text("not run").foregroundStyle(.secondary)
		}
	}

	private var details: some View {
		VStack(alignment: .leading, spacing: 8) {
			Group {
				if technical {
					Text(verbatim: command.summary ?? String(localized: "Command"))
				} else {
					Text("The command nolune ran")
				}
			}
			.font(.caption)
			.foregroundStyle(.secondary)
			if let text = command.command {
				ScrollView(.horizontal, showsIndicators: false) {
					Text(verbatim: text)
						.font(.caption.monospaced())
						.textSelection(.enabled)
						.fixedSize()
				}
			}
			if technical, let cwd = command.cwd {
				Text("in \(cwd)")
					.font(.caption.monospaced())
					.foregroundStyle(.secondary)
			}
			Divider()
			ScrollView([.horizontal, .vertical]) {
				Text(verbatim: output)
					.font(.caption.monospaced())
					.foregroundStyle(outputColor)
					.textSelection(.enabled)
					.fixedSize()
					.frame(maxWidth: .infinity, alignment: .leading)
			}
			.frame(maxHeight: 260)
			if let pictures = result?.pictures, !pictures.isEmpty {
				ScrollView(.horizontal, showsIndicators: false) {
					HStack(spacing: 8) {
						ForEach(Array(pictures.enumerated()), id: \.offset) { _, picture in
							if let url = context.url(picture) {
								Link(destination: url) {
									RemotePicture(url: url)
										.frame(height: 120)
								}
							}
						}
					}
				}
			}
		}
		.padding(10)
		.background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(palette.muted.opacity(0.5)))
	}

	/// What it printed: all of it once it's done, as it comes while it runs.
	private var output: String {
		if let result {
			let text = result.output.trimmingCharacters(in: .newlines)
			// The end of a long output, which is what matters most, without making the text huge.
			return text.isEmpty ? String(localized: "(no output)") : String(text.suffix(20_000))
		}
		if showsOutput, let live = context.toolOutput, live.id == command.id, !live.text.isEmpty {
			return String(live.text.suffix(20_000))
		}
		return running ? String(localized: "No output yet…") : String(localized: "(no output)")
	}

	private var outputColor: Color {
		switch result?.status {
		case .failed?: return Palette.plain.destructive
		case .blocked?: return Palette.plain.warning
		default: return .primary
		}
	}
}
