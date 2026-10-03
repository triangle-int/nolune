import ImageIO
import SwiftUI
import UIKit
import UniformTypeIdentifiers

/**
 * "nolune" in the share sheet (#138): photos, videos, files, a link or text from any app, sent into
 * a new chat or one of the latest, in any of the person's profiles, as the person signed in to the
 * app (Shared.swift). Files go up as the composer's do (`/api/p/<slug>/uploads`), then the message.
 */
final class ShareViewController: UIViewController {
	override func viewDidLoad() {
		super.viewDidLoad()
		let model = ShareModel(context: extensionContext)
		let host = UIHostingController(rootView: ShareView(model: model))
		addChild(host)
		host.view.frame = view.bounds
		host.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
		view.addSubview(host.view)
		host.didMove(toParent: self)
	}
}

@MainActor
final class ShareModel: ObservableObject {
	/// A file shared, copied where the extension may read it until it's done.
	struct File: Identifiable {
		let id = UUID()
		let url: URL
		let name: String
		let thumbnail: UIImage?
		let bytes: Int
	}

	/// As many as a message takes, each as large (Attachments in the app).
	static let most = 10
	static let largest = 100

	let session: Shared.Session?
	let profiles = Shared.profiles
	@Published private(set) var files: [File] = []
	@Published var text = ""
	/// The profile to send to: the one last open in the app at first.
	@Published var slug: String? {
		didSet { if slug != oldValue { Task { await loadChats() } } }
	}
	@Published private(set) var chats: [ShareClient.Chat] = []
	/// The chat to send to; nil for a new one.
	@Published var chat: String?
	@Published private(set) var reading = true
	/// What it's doing while it sends: uploading which file, or sending.
	@Published private(set) var sending: String?
	@Published private(set) var sent = false
	@Published var problem: String?

	private let context: NSExtensionContext?
	private let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)

	init(context: NSExtensionContext?) {
		self.context = context
		session = Shared.session
		slug = profiles.first?.slug
		Task {
			await read()
			await loadChats()
		}
	}

	var canSend: Bool {
		session != nil && slug != nil && !reading && sending == nil && !sent
			&& (!files.isEmpty || !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
	}

	// MARK: What was shared

	/// Files to send, and text and links for the message.
	private func read() async {
		defer { reading = false }
		var words: [String] = []
		let providers = (context?.inputItems as? [NSExtensionItem] ?? []).flatMap { $0.attachments ?? [] }
		for provider in providers {
			if let link = await Self.link(provider) {
				words.append(link)
			} else if let text = await Self.text(provider) {
				words.append(text)
			} else if files.count < Self.most, let file = await file(provider) {
				if file.bytes > Self.largest * 1024 * 1024 {
					problem = String(localized: "\(file.name) is larger than \(Self.largest) MB.")
				} else {
					files.append(file)
				}
			}
		}
		text = words.joined(separator: "\n\n")
	}

	/// A web page's address: Safari's, say.
	private static func link(_ provider: NSItemProvider) async -> String? {
		let types: [UTType] = [.fileURL, .image, .movie]
		guard provider.hasItemConformingToTypeIdentifier(UTType.url.identifier),
			!types.contains(where: { provider.hasItemConformingToTypeIdentifier($0.identifier) }),
			let item = try? await provider.loadItem(forTypeIdentifier: UTType.url.identifier)
		else { return nil }
		if let url = item as? URL { return url.isFileURL ? nil : url.absoluteString }
		if let data = item as? Data, let url = URL(dataRepresentation: data, relativeTo: nil) { return url.absoluteString }
		return nil
	}

	/// Text, not a text file.
	private static func text(_ provider: NSItemProvider) async -> String? {
		guard provider.hasItemConformingToTypeIdentifier(UTType.plainText.identifier),
			!provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier),
			let item = try? await provider.loadItem(forTypeIdentifier: UTType.plainText.identifier)
		else { return nil }
		if let text = item as? String { return text }
		if let data = item as? Data { return String(data: data, encoding: .utf8) }
		return nil
	}

	/// Anything else, as a file: copied here, as the one given goes away.
	private func file(_ provider: NSItemProvider) async -> File? {
		let type = provider.registeredTypeIdentifiers.first { UTType($0)?.conforms(to: .data) == true }
			?? UTType.data.identifier
		return await Self.copy(provider, type: type, into: folder)
	}

	/// Off the main thread, where the provider hands the file over.
	nonisolated private static func copy(_ provider: NSItemProvider, type: String, into folder: URL) async -> File? {
		let suggested = provider.suggestedName
		return await withCheckedContinuation { continuation in
			_ = provider.loadFileRepresentation(forTypeIdentifier: type) { url, _ in
				guard let url else { return continuation.resume(returning: nil) }
				var name = suggested ?? url.lastPathComponent
				if (name as NSString).pathExtension.isEmpty {
					let ext = UTType(type)?.preferredFilenameExtension ?? url.pathExtension
					if !ext.isEmpty { name += ".\(ext)" }
				}
				let copy = folder.appendingPathComponent(UUID().uuidString).appendingPathComponent(name)
				do {
					try FileManager.default.createDirectory(at: copy.deletingLastPathComponent(), withIntermediateDirectories: true)
					try FileManager.default.copyItem(at: url, to: copy)
				} catch {
					return continuation.resume(returning: nil)
				}
				let bytes = (try? copy.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
				let image = UTType(type)?.conforms(to: .image) == true ? Self.thumbnail(copy) : nil
				continuation.resume(returning: File(url: copy, name: name, thumbnail: image, bytes: bytes))
			}
		}
	}

	/// A small picture of a photo, without reading all of it: the extension has little memory.
	nonisolated private static func thumbnail(_ url: URL) -> UIImage? {
		guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
		let options: [CFString: Any] = [
			kCGImageSourceCreateThumbnailFromImageAlways: true,
			kCGImageSourceCreateThumbnailWithTransform: true,
			kCGImageSourceThumbnailMaxPixelSize: 180
		]
		guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
		return UIImage(cgImage: image)
	}

	// MARK: Sending

	private func loadChats() async {
		guard let session, let slug else { return }
		chat = nil
		let latest = (try? await ShareClient(session: session).chats(slug)) ?? []
		if slug == self.slug { chats = latest }
	}

	func send() {
		guard canSend, let session, let slug else { return }
		let client = ShareClient(session: session)
		let message = text.trimmingCharacters(in: .whitespacesAndNewlines)
		let chat = self.chat
		problem = nil
		Task {
			do {
				var uploads: [String] = []
				for (index, file) in files.enumerated() {
					sending = files.count == 1
						? String(localized: "Uploading…")
						: String(localized: "Uploading \(index + 1) of \(files.count)…")
					uploads.append(try await client.upload(slug, file: file.url, named: file.name))
				}
				sending = String(localized: "Sending…")
				if let chat {
					try await client.send(chat, text: message, uploads: uploads)
				} else {
					_ = try await client.startChat(slug, text: message, uploads: uploads)
				}
				sending = nil
				sent = true
				UINotificationFeedbackGenerator().notificationOccurred(.success)
				try? await Task.sleep(nanoseconds: 700_000_000)
				close(sent: true)
			} catch {
				sending = nil
				problem = error.localizedDescription
				UINotificationFeedbackGenerator().notificationOccurred(.error)
			}
		}
	}

	func close(sent: Bool = false) {
		try? FileManager.default.removeItem(at: folder)
		if sent {
			context?.completeRequest(returningItems: nil)
		} else {
			context?.cancelRequest(withError: NSError(domain: NSCocoaErrorDomain, code: NSUserCancelledError))
		}
	}
}

struct ShareView: View {
	@ObservedObject var model: ShareModel

	var body: some View {
		NavigationStack {
			Group {
				if model.session == nil || model.profiles.isEmpty {
					VStack(spacing: 12) {
						Image(systemName: "person.crop.circle.badge.exclamationmark")
							.font(.largeTitle)
							.foregroundStyle(.secondary)
						Text("Open nolune and sign in, then share to it.")
							.multilineTextAlignment(.center)
							.foregroundStyle(.secondary)
					}
					.padding()
					.frame(maxWidth: .infinity, maxHeight: .infinity)
				} else {
					form
				}
			}
			.navigationTitle("nolune")
			.navigationBarTitleDisplayMode(.inline)
			.toolbar {
				ToolbarItem(placement: .cancellationAction) {
					Button("Cancel") { model.close() }
						.disabled(model.sending != nil)
				}
				ToolbarItem(placement: .confirmationAction) {
					if model.sending != nil {
						ProgressView()
					} else if model.sent {
						Image(systemName: "checkmark.circle.fill")
							.foregroundStyle(.green)
					} else if model.session != nil {
						Button("Send") { model.send() }
							.fontWeight(.semibold)
							.disabled(!model.canSend)
					}
				}
			}
		}
	}

	private var form: some View {
		Form {
			if model.reading {
				ProgressView()
					.frame(maxWidth: .infinity)
			}
			if !model.files.isEmpty {
				Section {
					ForEach(model.files) { file in
						HStack(spacing: 12) {
							if let thumbnail = file.thumbnail {
								Image(uiImage: thumbnail)
									.resizable()
									.scaledToFill()
									.frame(width: 44, height: 44)
									.clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
							} else {
								Image(systemName: "doc")
									.font(.title2)
									.frame(width: 44, height: 44)
									.foregroundStyle(.secondary)
							}
							VStack(alignment: .leading, spacing: 2) {
								Text(verbatim: file.name)
									.lineLimit(1)
								Text(verbatim: ByteCountFormatter.string(fromByteCount: Int64(file.bytes), countStyle: .file))
									.font(.caption)
									.foregroundStyle(.secondary)
							}
						}
					}
				}
			}
			Section {
				TextField("Add a message", text: $model.text, axis: .vertical)
					.lineLimit(3...10)
			} footer: {
				if let sending = model.sending {
					Text(verbatim: sending)
				} else if let problem = model.problem {
					Text(verbatim: problem)
						.foregroundStyle(.red)
				}
			}
			if model.profiles.count > 1 {
				Section("Profile") {
					Picker("Profile", selection: $model.slug) {
						ForEach(model.profiles) { profile in
							Label {
								Text(verbatim: profile.name)
							} icon: {
								AvatarView(avatar: profile.avatar)
									.frame(width: 24, height: 24)
							}
							.tag(Optional(profile.slug))
						}
					}
					.pickerStyle(.inline)
					.labelsHidden()
				}
			}
			Section("Send to") {
				row(String(localized: "New chat"), chat: nil)
				ForEach(model.chats) { chat in
					row(chat.title.isEmpty ? String(localized: "New chat") : chat.title, chat: chat.id)
				}
			}
		}
		.disabled(model.sending != nil || model.sent)
	}

	private func row(_ title: String, chat: String?) -> some View {
		Button {
			model.chat = chat
		} label: {
			HStack {
				Text(verbatim: title)
					.foregroundStyle(.primary)
					.lineLimit(1)
				Spacer()
				if model.chat == chat {
					Image(systemName: "checkmark")
						.foregroundStyle(.tint)
				}
			}
		}
	}
}
