import PhotosUI
import SwiftUI
import UniformTypeIdentifiers

/**
 * The message box (#136), for a chat (ChatComposer) and a new chat (NewChatView): text that grows
 * as it's typed, files from Photos, the camera and Files uploaded as they're added, and Send, which
 * is Stop while nolune works and nothing's typed. With a hardware keyboard Return sends and
 * Shift-Return starts a new line; the on-screen keyboard's return starts one.
 */
struct ComposerBar<Accessories: View>: View {
	@Binding var text: String
	@ObservedObject var attachments: Attachments
	let placeholder: String
	/// nolune is working: Send is Stop until something's typed.
	var running = false
	var sending = false
	let send: () -> Void
	var stop: () -> Void = {}
	var focused: () -> Void = {}
	/// The model, command and folder menus, beside the attach button.
	@ViewBuilder let accessories: Accessories

	@State private var photos: [PhotosPickerItem] = []
	@State private var photosShown = false
	@State private var filesShown = false
	@State private var cameraShown = false

	private var empty: Bool { text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
	private var stops: Bool { running && empty && attachments.ready.isEmpty }
	private var canSend: Bool { (!empty || !attachments.ready.isEmpty) && !attachments.uploading && !sending }

	var body: some View {
		VStack(alignment: .leading, spacing: 8) {
			if !attachments.items.isEmpty {
				AttachmentStrip(attachments: attachments)
			}
			ComposerField(text: $text, placeholder: placeholder, submit: submit, focused: focused)
			HStack(spacing: 10) {
				attachMenu
				accessories
				Spacer(minLength: 0)
				if stops {
					Button(action: stop) {
						Image(systemName: "stop.fill")
							.font(.body.weight(.semibold))
							.frame(width: 34, height: 34)
							.background(Circle().fill(Color.primary))
							.foregroundStyle(Color(.systemBackground))
					}
					.accessibilityLabel(Text("Stop"))
				} else {
					Button(action: submit) {
						Group {
							if sending {
								ProgressView()
							} else {
								Image(systemName: "arrow.up")
									.font(.body.weight(.semibold))
							}
						}
						.frame(width: 34, height: 34)
						.background(Circle().fill(canSend ? Color.accentColor : Color.secondary.opacity(0.3)))
						.foregroundStyle(.white)
					}
					.disabled(!canSend)
					.accessibilityLabel(Text("Send"))
				}
			}
			.buttonStyle(.plain)
		}
		.padding(10)
		.background(
			RoundedRectangle(cornerRadius: 24, style: .continuous)
				.fill(Color(.secondarySystemBackground))
		)
		.overlay(
			RoundedRectangle(cornerRadius: 24, style: .continuous)
				.strokeBorder(Color(.separator), lineWidth: 0.5)
		)
		.photosPicker(
			isPresented: $photosShown,
			selection: $photos,
			maxSelectionCount: max(1, Attachments.most - attachments.items.count),
			matching: .any(of: [.images, .videos])
		)
		.onChange(of: photos) { picked in
			guard !picked.isEmpty else { return }
			photos = []
			Task { await add(picked) }
		}
		.fileImporter(isPresented: $filesShown, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
			guard case .success(let urls) = result else { return }
			for url in urls {
				let scoped = url.startAccessingSecurityScopedResource()
				defer { if scoped { url.stopAccessingSecurityScopedResource() } }
				if let data = try? Data(contentsOf: url) { attachments.add(data, named: url.lastPathComponent) }
			}
		}
		.fullScreenCover(isPresented: $cameraShown) {
			CameraPicker(taken: { data, name in attachments.add(data, named: name) }, close: { cameraShown = false })
				.ignoresSafeArea()
		}
	}

	private var attachMenu: some View {
		Menu {
			Button {
				photosShown = true
			} label: {
				Label("Photos", systemImage: "photo.on.rectangle")
			}
			if UIImagePickerController.isSourceTypeAvailable(.camera) {
				Button {
					cameraShown = true
				} label: {
					Label("Camera", systemImage: "camera")
				}
			}
			Button {
				filesShown = true
			} label: {
				Label("Files", systemImage: "folder")
			}
		} label: {
			Image(systemName: "plus")
				.font(.body.weight(.medium))
				.frame(width: 34, height: 34)
				.background(Circle().strokeBorder(Color(.separator)))
		}
		.disabled(attachments.isFull)
		.accessibilityLabel(Text("Attach files"))
	}

	private func submit() {
		guard canSend else { return }
		send()
	}

	private func add(_ items: [PhotosPickerItem]) async {
		for (index, item) in items.enumerated() {
			guard let data = try? await item.loadTransferable(type: Data.self) else { continue }
			let type = item.supportedContentTypes.first
			let kind = type?.conforms(to: .movie) == true ? "Video" : "Photo"
			let number = items.count > 1 ? " \(index + 1)" : ""
			attachments.add(data, named: "\(kind)\(number).\(type?.preferredFilenameExtension ?? "jpg")")
		}
	}
}

// MARK: - Files

/// The files for the next message, uploaded as they're added (`/api/p/<slug>/uploads`).
@MainActor
final class Attachments: ObservableObject {
	struct Item: Identifiable {
		let id = UUID()
		let name: String
		let preview: UIImage?
		var upload: Upload?
		var problem: String?
	}

	/// As the gateway allows: ten files a message, 100 MB each.
	static let most = 10
	static let largest = 100

	@Published private(set) var items: [Item] = []
	let client: Client
	var slug: String

	init(client: Client, slug: String) {
		self.client = client
		self.slug = slug
	}

	var isFull: Bool { items.count >= Self.most }
	/// Still uploading one.
	var uploading: Bool { items.contains { $0.upload == nil && $0.problem == nil } }
	/// The uploads to send with the message, in order.
	var ready: [String] { items.compactMap { $0.upload?.id } }

	func add(_ data: Data, named name: String) {
		guard !isFull else { return }
		var item = Item(name: name, preview: UIImage(data: data)?.preparingThumbnail(of: CGSize(width: 160, height: 160)))
		if data.count > Self.largest * 1024 * 1024 { item.problem = String(localized: "Larger than \(Self.largest) MB") }
		items.append(item)
		guard item.problem == nil else { return }
		let (id, slug, client) = (item.id, slug, client)
		Task {
			do {
				let upload = try await client.upload(slug, data, named: name)
				if let index = items.firstIndex(where: { $0.id == id }) {
					items[index].upload = upload
				} else {
					// Taken off while it uploaded.
					await client.deleteUpload(slug, upload.id)
				}
			} catch {
				if let index = items.firstIndex(where: { $0.id == id }) { items[index].problem = error.localizedDescription }
			}
		}
	}

	func remove(_ id: UUID) {
		guard let index = items.firstIndex(where: { $0.id == id }) else { return }
		let item = items.remove(at: index)
		if let upload = item.upload {
			let (slug, client) = (slug, client)
			Task { await client.deleteUpload(slug, upload.id) }
		}
	}

	/// The message went with them.
	func sent() {
		items = []
	}
}

private struct AttachmentStrip: View {
	@ObservedObject var attachments: Attachments

	var body: some View {
		ScrollView(.horizontal, showsIndicators: false) {
			HStack(spacing: 8) {
				ForEach(attachments.items) { item in
					ZStack(alignment: .topTrailing) {
						tile(item)
						Button {
							attachments.remove(item.id)
						} label: {
							Image(systemName: "xmark.circle.fill")
								.symbolRenderingMode(.palette)
								.foregroundStyle(.white, Color.black.opacity(0.6))
						}
						.buttonStyle(.plain)
						.padding(3)
						.accessibilityLabel(Text("Remove"))
					}
				}
			}
		}
	}

	@ViewBuilder private func tile(_ item: Attachments.Item) -> some View {
		Group {
			if let preview = item.preview {
				Image(uiImage: preview)
					.resizable()
					.scaledToFill()
					.frame(width: 64, height: 64)
			} else {
				VStack(spacing: 4) {
					Image(systemName: "doc")
					Text(verbatim: item.name)
						.font(.caption2)
						.lineLimit(2)
						.truncationMode(.middle)
				}
				.padding(6)
				.frame(width: 96, height: 64)
				.background(Color(.tertiarySystemBackground))
			}
		}
		.clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
		.overlay {
			if item.problem != nil {
				RoundedRectangle(cornerRadius: 10, style: .continuous).fill(Color.red.opacity(0.35))
				Image(systemName: "exclamationmark.triangle.fill")
					.foregroundStyle(.white)
			} else if item.upload == nil {
				RoundedRectangle(cornerRadius: 10, style: .continuous).fill(Color.black.opacity(0.3))
				ProgressView()
					.tint(.white)
			}
		}
		.accessibilityElement(children: .ignore)
		.accessibilityLabel(Text(verbatim: item.problem.map { "\(item.name): \($0)" } ?? item.name))
	}
}

/// The camera, for a photo or a video to attach.
private struct CameraPicker: UIViewControllerRepresentable {
	let taken: (Data, String) -> Void
	let close: () -> Void

	func makeUIViewController(context: Context) -> UIImagePickerController {
		let picker = UIImagePickerController()
		picker.sourceType = .camera
		picker.mediaTypes = [UTType.image.identifier, UTType.movie.identifier]
		picker.delegate = context.coordinator
		return picker
	}

	func updateUIViewController(_ picker: UIImagePickerController, context: Context) {}

	func makeCoordinator() -> Coordinator {
		Coordinator(self)
	}

	final class Coordinator: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
		let parent: CameraPicker

		init(_ parent: CameraPicker) {
			self.parent = parent
		}

		func imagePickerController(_ picker: UIImagePickerController, didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]) {
			if let movie = info[.mediaURL] as? URL, let data = try? Data(contentsOf: movie) {
				parent.taken(data, "Video.\(movie.pathExtension.isEmpty ? "mov" : movie.pathExtension)")
			} else if let image = info[.originalImage] as? UIImage, let data = image.jpegData(compressionQuality: 0.9) {
				parent.taken(data, "Photo.jpg")
			}
			parent.close()
		}

		func imagePickerControllerDidCancel(_ picker: UIImagePickerController) {
			parent.close()
		}
	}
}

// MARK: - The text

/// A text view that grows with its text, up to six lines, then scrolls.
struct ComposerField: UIViewRepresentable {
	@Binding var text: String
	let placeholder: String
	let submit: () -> Void
	var focused: () -> Void = {}

	func makeUIView(context: Context) -> ComposerTextView {
		let view = ComposerTextView(placeholder: placeholder)
		view.delegate = context.coordinator
		view.text = text
		view.textChanged()
		view.submit = submit
		return view
	}

	func updateUIView(_ view: ComposerTextView, context: Context) {
		context.coordinator.parent = self
		if view.text != text {
			view.text = text
			view.textChanged()
		}
		view.placeholder = placeholder
		view.submit = submit
	}

	func sizeThatFits(_ proposal: ProposedViewSize, uiView view: ComposerTextView, context: Context) -> CGSize? {
		let width = proposal.width ?? 320
		let fitting = view.sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude)).height
		let most = (view.font?.lineHeight ?? 20) * 6 + view.textContainerInset.top + view.textContainerInset.bottom
		view.isScrollEnabled = fitting > most
		return CGSize(width: width, height: min(fitting, most))
	}

	func makeCoordinator() -> Coordinator {
		Coordinator(self)
	}

	final class Coordinator: NSObject, UITextViewDelegate {
		var parent: ComposerField

		init(_ parent: ComposerField) {
			self.parent = parent
		}

		func textViewDidChange(_ textView: UITextView) {
			parent.text = textView.text
			(textView as? ComposerTextView)?.textChanged()
		}

		func textViewDidBeginEditing(_ textView: UITextView) {
			parent.focused()
		}
	}
}

final class ComposerTextView: UITextView {
	var submit: () -> Void = {}
	var placeholder = "" {
		didSet { placeholderLabel.text = placeholder }
	}

	private let placeholderLabel = UILabel()

	init(placeholder: String) {
		super.init(frame: .zero, textContainer: nil)
		font = .preferredFont(forTextStyle: .body)
		adjustsFontForContentSizeCategory = true
		backgroundColor = .clear
		isScrollEnabled = false
		textContainerInset = UIEdgeInsets(top: 6, left: 2, bottom: 6, right: 2)
		setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
		placeholderLabel.font = font
		placeholderLabel.adjustsFontForContentSizeCategory = true
		placeholderLabel.textColor = .placeholderText
		placeholderLabel.translatesAutoresizingMaskIntoConstraints = false
		addSubview(placeholderLabel)
		// Its own initializer doesn't run `placeholder`'s didSet.
		self.placeholder = placeholder
		placeholderLabel.text = placeholder
		NSLayoutConstraint.activate([
			placeholderLabel.leadingAnchor.constraint(equalTo: leadingAnchor, constant: textContainerInset.left + textContainer.lineFragmentPadding),
			placeholderLabel.topAnchor.constraint(equalTo: topAnchor, constant: textContainerInset.top)
		])
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("not from a storyboard")
	}

	func textChanged() {
		placeholderLabel.isHidden = !text.isEmpty
	}

	/// Return sends from a hardware keyboard; Shift-Return starts a new line. Not while a keyboard
	/// for Japanese, say, composes a word, which Return confirms.
	override var keyCommands: [UIKeyCommand]? {
		guard markedTextRange == nil else { return super.keyCommands }
		let send = UIKeyCommand(input: "\r", modifierFlags: [], action: #selector(sendMessage))
		send.wantsPriorityOverSystemBehavior = true
		let newLine = UIKeyCommand(input: "\r", modifierFlags: .shift, action: #selector(insertNewLine))
		newLine.wantsPriorityOverSystemBehavior = true
		return [send, newLine] + (super.keyCommands ?? [])
	}

	@objc private func sendMessage() {
		submit()
	}

	@objc private func insertNewLine() {
		insertText("\n")
	}
}
