import AppKit
import SwiftUI

/// Whether the gateway answers, who has an account, and where it's opened: for the menu bar extra.
@MainActor
final class GatewayStatus: ObservableObject {
	@Published private(set) var running: Bool?
	@Published private(set) var people: [Runtime.Person] = []
	/// Where people open nolune: the relay's address while there is one, else this Mac's.
	@Published private(set) var origin = Runtime.shared.origin
	/// Whether that's the relay's, which opens from anywhere.
	@Published private(set) var fromAnywhere = false
	/// A newer nolune to download, once the gateway has heard of one.
	@Published private(set) var update: Runtime.Update?
	private var timer: Timer?

	/// Checks now and every few seconds while the menu is open.
	func watch() {
		guard !Snapshot.active else { return }
		Task { await refresh(people: true) }
		timer?.invalidate()
		timer = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in
			Task { @MainActor in await self?.refresh(people: false) }
		}
	}

	/// For `--snapshot`.
	func pose(running: Bool, people: [Runtime.Person], relay: URL? = nil, update: Runtime.Update? = nil) {
		self.running = running
		self.people = people
		origin = relay ?? Runtime.shared.localURL
		fromAnywhere = relay != nil
		self.update = update
	}

	func stopWatching() {
		timer?.invalidate()
		timer = nil
	}

	func refresh(people alsoPeople: Bool) async {
		// First, and from config.json: the address shows as the menu opens.
		origin = Runtime.shared.origin
		fromAnywhere = Runtime.shared.config.relay != nil
		update = Runtime.shared.update
		running = await Service.isUp()
		if alsoPeople, let people = await Runtime.shared.people() { self.people = people }
	}
}

/// The menu bar extra: is it running, who's in, where it lives, and a few things to do.
struct StatusMenu: View {
	@ObservedObject var status: GatewayStatus
	/// Opens the address step (AppDelegate.showAddress), for a Mac without the relay's address.
	var openFromAnywhere: @MainActor () -> Void = {}
	@State private var copied = false

	private var address: String {
		let origin = status.origin
		return origin.host.map { host in origin.port.map { "\(host):\($0)" } ?? host } ?? origin.absoluteString
	}

	var body: some View {
		VStack(alignment: .leading, spacing: 0) {
			HStack(spacing: 8) {
				Circle()
					.fill(dotColor)
					.frame(width: 8, height: 8)
				Text(title)
					.font(Theme.font(14, weight: 600))
			}
			.padding(.horizontal, 14)
			.padding(.top, 14)

			if !status.people.isEmpty {
				HStack(spacing: 6) {
					ForEach(status.people.prefix(7)) { person in
						PersonAvatar(person: person)
							.help(person.name)
					}
				}
				.padding(.horizontal, 14)
				.padding(.top, 12)
			}

			Divider().padding(.vertical, 10)

			HStack {
				Text(address)
					.font(Theme.mono(12))
					.foregroundStyle(.secondary)
					.lineLimit(1)
					.truncationMode(.middle)
				Spacer()
				Button {
					NSPasteboard.general.clearContents()
					NSPasteboard.general.setString(status.origin.absoluteString, forType: .string)
					copied = true
					DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { copied = false }
				} label: {
					Image(systemName: copied ? "checkmark" : "doc.on.doc")
						.foregroundStyle(.secondary)
				}
				.buttonStyle(.plain)
				.help("Copy the address")
			}
			.padding(.horizontal, 14)

			Divider().padding(.vertical, 10)

			if let update = status.update {
				MenuRow(title: "Download nolune \(update.version)", icon: "arrow.down.circle", tint: Color(hex: 0x38BDF8)) {
					NSWorkspace.shared.open(update.download ?? update.page)
				}
				Text(updateNote(update))
					.font(Theme.font(11))
					.foregroundStyle(.secondary)
					.padding(.horizontal, 14)
					.padding(.top, 2)
				Divider().padding(.vertical, 10)
			}

			MenuRow(title: "Open nolune", icon: "arrow.up.right.square") {
				NSWorkspace.shared.open(status.origin)
			}
			if !status.fromAnywhere {
				MenuRow(title: "Open it from anywhere…", icon: "globe") { openFromAnywhere() }
			}
			if status.running == false {
				MenuRow(title: "Start", icon: "play") {
					Service.start()
					Task { await status.refresh(people: false) }
				}
			} else {
				MenuRow(title: "Restart", icon: "arrow.clockwise") {
					Service.restart()
					Task { await status.refresh(people: false) }
				}
			}
			MenuRow(title: "View the log", icon: "doc.text") {
				NSWorkspace.shared.open(Runtime.shared.logFile)
			}

			Divider().padding(.vertical, 6)

			MenuRow(title: "Quit", icon: "power") { NSApp.terminate(nil) }
			Text("Quitting stops nolune until you open it again.")
				.font(Theme.font(11))
				.foregroundStyle(.secondary)
				.padding(.horizontal, 14)
				.padding(.top, 2)
				.padding(.bottom, 12)
		}
		.frame(width: 280)
		.onAppear { status.watch() }
		.onDisappear { status.stopWatching() }
	}

	/// What to do with the download, and the release's notes.
	private func updateNote(_ update: Runtime.Update) -> AttributedString {
		var note = AttributedString("Then quit nolune, drag the new one to Applications and open it. ")
		var notes = AttributedString("What’s new")
		notes.link = update.page
		note.append(notes)
		return note
	}

	private var title: String {
		switch status.running {
		case .some(true): return "nolune is running"
		case .some(false): return "nolune isn't running"
		case .none: return "nolune"
		}
	}

	private var dotColor: Color {
		switch status.running {
		case .some(true): return Color(hex: 0x4ADE80)
		case .some(false): return Color(hex: 0xFBBF24)
		case .none: return .secondary
		}
	}
}

/// Someone as the web app draws them (UserAvatar.svelte): their picture, or their initial on their color.
private struct PersonAvatar: View {
	let person: Runtime.Person

	var body: some View {
		Group {
			if let picture = person.picture {
				Image(nsImage: picture)
					.resizable()
					.scaledToFill()
					.background(Color.primary.opacity(0.1))
			} else {
				Text(person.name.prefix(1).uppercased())
					.font(Theme.font(12, weight: 600))
					.foregroundStyle(.white)
					.frame(maxWidth: .infinity, maxHeight: .infinity)
					.background(color)
			}
		}
		.frame(width: 26, height: 26)
		.clipShape(Circle())
	}

	/// `hsl(hue 55% 45%)`, the hue from their name as the web works it out, so each person has the
	/// same color everywhere.
	private var color: Color {
		// Each character's first UTF-16 unit, as JavaScript's `charCodeAt(0)`.
		let hue = person.name.unicodeScalars.reduce(7) { sum, scalar in
			(sum * 31 + Int(String(scalar).utf16.first ?? 0)) % 360
		}
		// HSL's lightness and saturation as HSB's brightness and saturation.
		let (saturation, lightness) = (0.55, 0.45)
		let brightness = lightness + saturation * min(lightness, 1 - lightness)
		return Color(
			hue: Double(hue) / 360,
			saturation: 2 * (1 - lightness / brightness),
			brightness: brightness
		)
	}
}

private struct MenuRow: View {
	let title: String
	let icon: String
	/// The icon's color, for a row that stands out; secondary otherwise.
	var tint: Color?
	let action: () -> Void
	@State private var hovered = false

	var body: some View {
		Button(action: action) {
			HStack(spacing: 10) {
				Image(systemName: icon)
					.frame(width: 16)
					.foregroundStyle(tint.map { AnyShapeStyle($0) } ?? AnyShapeStyle(.secondary))
				Text(title)
					.font(Theme.font(13))
				Spacer()
			}
			.padding(.horizontal, 10)
			.frame(height: 28)
			.background(RoundedRectangle(cornerRadius: 6).fill(hovered ? Color.white.opacity(0.08) : .clear))
			.contentShape(Rectangle())
		}
		.buttonStyle(.plain)
		.padding(.horizontal, 4)
		.onHover { hovered = $0 }
	}
}

/// The menu bar icon: a star with one planet on a tilted orbit, drawn as a template image.
enum MenuIcon {
	static let image: NSImage = {
		let image = NSImage(size: NSSize(width: 18, height: 18), flipped: false) { _ in
			let center = NSPoint(x: 9, y: 9)
			var tilt = AffineTransform()
			tilt.translate(x: center.x, y: center.y)
			tilt.rotate(byDegrees: 20)
			tilt.translate(x: -center.x, y: -center.y)

			NSColor.black.set()
			NSBezierPath(ovalIn: NSRect(x: 6.4, y: 6.4, width: 5.2, height: 5.2)).fill()

			let orbit = NSBezierPath(ovalIn: NSRect(x: 1.25, y: 5.5, width: 15.5, height: 7))
			orbit.transform(using: tilt)
			orbit.lineWidth = 1.2
			orbit.stroke()

			// The planet, on the orbit's near side.
			let angle = -0.9
			let planet = tilt.transform(NSPoint(x: 9 + 7.75 * cos(angle), y: 9 + 3.5 * sin(angle)))
			NSBezierPath(ovalIn: NSRect(x: planet.x - 1.9, y: planet.y - 1.9, width: 3.8, height: 3.8)).fill()
			return true
		}
		image.isTemplate = true
		return image
	}()
}
