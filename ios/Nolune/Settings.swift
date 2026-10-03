import PhotosUI
import SwiftUI

/**
 * The web app's settings for this device (packages/web/src/lib/preferences.svelte.ts): a cookie
 * the server reads too, so native screens and the pages inside them agree.
 */
struct Preferences: Codable, Equatable {
	static let cookie = "nolune-prefs"

	/// The commands nolune runs, token usage and prompt-cache details.
	var technical = false
	/// The steps under each reply open without a tap.
	var expandSteps = false
	/// Soft sounds where a page moves by itself.
	var sounds = true
	/// The pages' language; the app's own is the system's.
	var language = "auto"

	init() {}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		technical = (try? container.decode(Bool.self, forKey: .technical)) ?? false
		expandSteps = (try? container.decode(Bool.self, forKey: .expandSteps)) ?? false
		sounds = (try? container.decode(Bool.self, forKey: .sounds)) ?? true
		language = (try? container.decode(String.self, forKey: .language)) ?? "auto"
	}

	/// As the pages left them, or the defaults.
	@MainActor
	static func read(_ origin: URL) async -> Preferences {
		guard let found = await Cookies.webCookie(Preferences.cookie, for: origin),
			let json = found.value.removingPercentEncoding,
			let preferences = try? JSONDecoder().decode(Preferences.self, from: Data(json.utf8))
		else { return Preferences() }
		return preferences
	}

	/// Kept for a year, as the pages keep it, encoded as their `encodeURIComponent` does.
	@MainActor
	func write(_ origin: URL) async {
		guard let json = try? JSONEncoder().encode(self), let text = String(data: json, encoding: .utf8) else { return }
		let unreserved = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_.!~*'()"))
		guard let value = text.addingPercentEncoding(withAllowedCharacters: unreserved),
			let cookie = HTTPCookie(properties: [
				.originURL: origin,
				.path: "/",
				.name: Preferences.cookie,
				.value: value,
				.expires: Date(timeIntervalSinceNow: 365 * 24 * 3600)
			])
		else { return }
		await Cookies.set(cookie)
	}
}

/// Settings: the person's name, how nolune looks on this device, and signing out.
struct SettingsView: View {
	@ObservedObject var family: Family
	let signOut: () -> Void
	@State private var name = ""
	@State private var preferences = Preferences()
	@State private var loaded = false
	@State private var photo: PhotosPickerItem?
	@State private var picturing = false
	@State private var problem: String?

	var body: some View {
		Form {
			Section {
				HStack(spacing: 14) {
					PersonPicture(url: picture, name: family.me?.name ?? "")
						.frame(width: 56, height: 56)
						.overlay { if picturing { ProgressView() } }
					VStack(alignment: .leading, spacing: 8) {
						PhotosPicker(selection: $photo, matching: .images) {
							Text(picture == nil ? "Add a picture" : "Change picture")
						}
						if picture != nil {
							Button("Remove picture", role: .destructive, action: removePicture)
						}
					}
					.buttonStyle(.borderless)
				}
				TextField("Your name", text: $name)
					.textContentType(.name)
					.submitLabel(.done)
					.onSubmit(saveName)
			} header: {
				Text("Your name")
			} footer: {
				if let problem {
					Text(verbatim: problem)
						.foregroundStyle(.red)
				} else {
					Text("Everyone in your profiles sees your name and picture, and nolune reads your name with each message you send.")
				}
			}
			Section {
				Toggle(isOn: $preferences.technical) {
					Text("Show technical details")
					Text("Show the exact commands nolune runs, token usage and prompt caching.")
				}
				Toggle(isOn: $preferences.expandSteps) {
					Text("Always show steps")
					Text("Open the list of what nolune did under each reply, instead of keeping it folded.")
				}
				Toggle(isOn: $preferences.sounds) {
					Text("Sounds")
					Text("Soft sounds where nolune moves on its own, like the welcome of a new profile.")
				}
			} footer: {
				Text("These settings are saved on this device only.")
			}
			Section {
				Button("Log out", role: .destructive, action: signOut)
			}
		}
		.navigationTitle("Settings")
		.onChange(of: photo) { item in
			guard let item else { return }
			photo = nil
			setPicture(item)
		}
		.task {
			name = family.me?.name ?? ""
			preferences = await Preferences.read(family.client.origin)
			loaded = true
		}
		.onChange(of: preferences) { changed in
			guard loaded else { return }
			Task { await changed.write(family.client.origin) }
		}
	}

	/// The person's picture, from the family's nolune.
	private var picture: URL? {
		guard let path = family.me?.picture else { return nil }
		return URL(string: path, relativeTo: family.client.origin)?.absoluteURL
	}

	/// A photo, as a small square from its middle, as the web's cropper makes it by default.
	private func setPicture(_ item: PhotosPickerItem) {
		picturing = true
		Task {
			defer { picturing = false }
			guard let data = try? await item.loadTransferable(type: Data.self), let image = UIImage(data: data) else {
				problem = String(localized: "Couldn’t open that picture. Try a JPEG or PNG.")
				return
			}
			let side = min(image.size.width, image.size.height)
			let target: CGFloat = 512
			let format = UIGraphicsImageRendererFormat()
			format.scale = 1
			let square = UIGraphicsImageRenderer(size: CGSize(width: target, height: target), format: format).image { _ in
				let scale = target / side
				let size = CGSize(width: image.size.width * scale, height: image.size.height * scale)
				image.draw(in: CGRect(x: (target - size.width) / 2, y: (target - size.height) / 2, width: size.width, height: size.height))
			}
			guard let jpeg = square.jpegData(compressionQuality: 0.85) else { return }
			do {
				try await family.client.setPicture(jpeg)
				problem = nil
				await family.reload()
				Haptics.success()
			} catch {
				problem = error.localizedDescription
				Haptics.failure()
			}
		}
	}

	private func removePicture() {
		Task {
			do {
				try await family.client.call("DELETE", "api/me/picture")
				await family.reload()
			} catch {
				problem = error.localizedDescription
			}
		}
	}

	private func saveName() {
		let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
		guard !trimmed.isEmpty, trimmed != family.me?.name else { return }
		Task {
			if let kept = await family.rename(me: trimmed) { name = kept }
		}
	}
}
