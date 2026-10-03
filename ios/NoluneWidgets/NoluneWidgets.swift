import ActivityKit
import SwiftUI
import WidgetKit

/**
 * nolune's widgets (#138): the bell's latest notifications, a button to ask nolune something, and
 * a reply being written, as a Live Activity. They read what the app shares (Shared.swift), and
 * open the app with `nolune://open?path=…`.
 */
@main
struct NoluneWidgets: WidgetBundle {
	var body: some Widget {
		BellWidget()
		AskWidget()
		ReplyActivityWidget()
	}
}

// MARK: - The bell

struct BellEntry: TimelineEntry {
	let date: Date
	let bell: Shared.Bell
}

struct BellTimeline: TimelineProvider {
	func placeholder(in context: Context) -> BellEntry {
		BellEntry(date: Date(), bell: Self.sample)
	}

	func getSnapshot(in context: Context, completion: @escaping (BellEntry) -> Void) {
		let bell = Shared.bell
		completion(BellEntry(date: Date(), bell: context.isPreview && bell.items.isEmpty ? Self.sample : bell))
	}

	/// The app reloads it when the bell changes; until then it stays as it is.
	func getTimeline(in context: Context, completion: @escaping (Timeline<BellEntry>) -> Void) {
		completion(Timeline(entries: [BellEntry(date: Date(), bell: Shared.bell)], policy: .never))
	}

	static let sample = Shared.Bell(
		items: [
			.init(
				id: "sample",
				title: String(localized: "Umbrellas tomorrow"),
				body: String(localized: "Rain from 3pm."),
				createdAt: Date(),
				profile: .init(slug: "family", name: "Family", avatar: "moon"),
				new: true
			)
		],
		unseen: 1
	)
}

struct BellWidget: Widget {
	var body: some WidgetConfiguration {
		StaticConfiguration(kind: "bell", provider: BellTimeline()) { entry in
			BellWidgetView(bell: entry.bell)
				.widgetBackground()
		}
		.configurationDisplayName("Notifications")
		.description("What nolune found for you lately.")
		.supportedFamilies([.systemSmall, .systemMedium, .systemLarge, .accessoryRectangular])
	}
}

struct BellWidgetView: View {
	let bell: Shared.Bell
	@Environment(\.widgetFamily) private var family

	private var shown: Int {
		switch family {
		case .systemLarge: return 5
		case .systemMedium: return 2
		default: return 1
		}
	}

	var body: some View {
		if family == .accessoryRectangular {
			accessory
		} else {
			VStack(alignment: .leading, spacing: 8) {
				HStack {
					Image(systemName: bell.unseen > 0 ? "bell.badge" : "bell")
					Text("Notifications")
						.font(.caption.weight(.semibold))
					Spacer()
					if bell.unseen > 0 {
						Text(verbatim: "\(bell.unseen)")
							.font(.caption2.weight(.bold))
							.foregroundStyle(.white)
							.padding(.horizontal, 6)
							.padding(.vertical, 2)
							.background(Capsule().fill(Palette.plain.destructive))
					}
				}
				.foregroundStyle(.secondary)
				if bell.items.isEmpty {
					Spacer()
					Text("Nothing new.")
						.font(.subheadline)
						.foregroundStyle(.secondary)
					Spacer()
				} else {
					ForEach(bell.items.prefix(shown)) { item in
						Link(destination: Shared.open(item.path)) {
							row(item)
						}
					}
					Spacer(minLength: 0)
				}
			}
			.widgetURL(Shared.open(bell.items.first?.path ?? "/"))
		}
	}

	private func row(_ item: Shared.Bell.Item) -> some View {
		HStack(alignment: .top, spacing: 8) {
			AvatarView(avatar: item.profile.avatar)
				.frame(width: 18, height: 18)
			VStack(alignment: .leading, spacing: 2) {
				Text(verbatim: item.title)
					.font(.subheadline.weight(.semibold))
					.lineLimit(1)
				Text(verbatim: item.body)
					.font(.caption)
					.foregroundStyle(.secondary)
					.lineLimit(family == .systemSmall ? 3 : 2)
			}
		}
	}

	private var accessory: some View {
		VStack(alignment: .leading, spacing: 1) {
			if let item = bell.items.first {
				Text(verbatim: item.title)
					.font(.headline)
					.lineLimit(1)
				Text(verbatim: item.body)
					.font(.caption)
					.lineLimit(2)
			} else {
				Label("Nothing new.", systemImage: "bell")
			}
		}
		.widgetURL(Shared.open(bell.items.first?.path ?? "/"))
	}
}

// MARK: - Asking

struct AskEntry: TimelineEntry {
	let date: Date
	let profile: Shared.Profile?
}

struct AskTimeline: TimelineProvider {
	func placeholder(in context: Context) -> AskEntry {
		AskEntry(date: Date(), profile: nil)
	}

	func getSnapshot(in context: Context, completion: @escaping (AskEntry) -> Void) {
		completion(AskEntry(date: Date(), profile: Shared.profiles.first))
	}

	func getTimeline(in context: Context, completion: @escaping (Timeline<AskEntry>) -> Void) {
		completion(Timeline(entries: [AskEntry(date: Date(), profile: Shared.profiles.first)], policy: .never))
	}
}

/// A new chat in the profile last open, from the Home Screen.
struct AskWidget: Widget {
	var body: some WidgetConfiguration {
		StaticConfiguration(kind: "ask", provider: AskTimeline()) { entry in
			AskWidgetView(profile: entry.profile)
				.widgetBackground()
		}
		.configurationDisplayName("Ask nolune")
		.description("Start a new chat.")
		.supportedFamilies([.systemSmall, .accessoryCircular])
	}
}

struct AskWidgetView: View {
	let profile: Shared.Profile?
	@Environment(\.widgetFamily) private var family

	var body: some View {
		Group {
			if family == .accessoryCircular {
				ZStack {
					AccessoryWidgetBackground()
					Image(systemName: "square.and.pencil")
						.font(.title3)
				}
			} else {
				VStack(alignment: .leading) {
					AvatarView(avatar: profile?.avatar ?? "probe")
						.frame(width: 40, height: 40)
					Spacer()
					Text("Ask nolune")
						.font(.headline)
					if let profile {
						Text(verbatim: profile.name)
							.font(.caption)
							.foregroundStyle(.secondary)
					}
				}
				.frame(maxWidth: .infinity, alignment: .leading)
			}
		}
		.widgetURL(Shared.open(profile.map { "/p/\($0.slug)" } ?? "/"))
	}
}

// MARK: - A reply being written

struct ReplyActivityWidget: Widget {
	var body: some WidgetConfiguration {
		ActivityConfiguration(for: ReplyActivity.self) { context in
			ReplyLockScreen(attributes: context.attributes, state: context.state)
				.padding()
				.activityBackgroundTint(nil)
				.widgetURL(Shared.open(context.attributes.path))
		} dynamicIsland: { context in
			DynamicIsland {
				DynamicIslandExpandedRegion(.leading) {
					AvatarView(avatar: context.attributes.avatar)
						.frame(width: 36, height: 36)
				}
				DynamicIslandExpandedRegion(.trailing) {
					if context.state.running {
						ProgressView()
					} else {
						Image(systemName: "checkmark.circle.fill")
							.foregroundStyle(.green)
					}
				}
				DynamicIslandExpandedRegion(.bottom) {
					VStack(alignment: .leading, spacing: 2) {
						Text(verbatim: ReplyLockScreen.title(context.state))
							.font(.headline)
							.lineLimit(1)
						Text(verbatim: ReplyLockScreen.step(context.state))
							.font(.subheadline)
							.foregroundStyle(.secondary)
							.lineLimit(2)
					}
					.frame(maxWidth: .infinity, alignment: .leading)
				}
			} compactLeading: {
				AvatarView(avatar: context.attributes.avatar)
					.frame(width: 18, height: 18)
			} compactTrailing: {
				if context.state.running {
					ProgressView()
						.progressViewStyle(.circular)
				} else {
					Image(systemName: "checkmark")
						.foregroundStyle(.green)
				}
			} minimal: {
				AvatarView(avatar: context.attributes.avatar)
					.frame(width: 18, height: 18)
			}
			.widgetURL(Shared.open(context.attributes.path))
		}
	}
}

struct ReplyLockScreen: View {
	let attributes: ReplyActivity
	let state: ReplyActivity.ContentState

	var body: some View {
		HStack(alignment: .top, spacing: 12) {
			AvatarView(avatar: attributes.avatar)
				.frame(width: 40, height: 40)
			VStack(alignment: .leading, spacing: 3) {
				Text(verbatim: Self.title(state))
					.font(.headline)
					.lineLimit(1)
				Text(verbatim: Self.step(state))
					.font(.subheadline)
					.foregroundStyle(.secondary)
					.lineLimit(2)
				Text(verbatim: attributes.profile)
					.font(.caption)
					.foregroundStyle(.tertiary)
			}
			Spacer(minLength: 0)
			if state.running {
				ProgressView()
			}
		}
	}

	static func title(_ state: ReplyActivity.ContentState) -> String {
		state.title.isEmpty ? String(localized: "New chat") : state.title
	}

	/// What nolune does, or that it's working, or that it's done.
	static func step(_ state: ReplyActivity.ContentState) -> String {
		if !state.step.isEmpty { return state.step }
		return state.running ? String(localized: "nolune is working") : String(localized: "Done")
	}
}

extension View {
	/// The widget's background, the web's page color: the container's on iOS 17, where it's required.
	@ViewBuilder func widgetBackground() -> some View {
		if #available(iOS 17.0, *) {
			containerBackground(Palette.plain.background, for: .widget)
		} else {
			padding().background(Palette.plain.background)
		}
	}
}
