import PencilKit
import PhotosUI
import SwiftUI

/**
 * Images, natively (#137): the profile's picture templates in a grid, by group, and a box to
 * describe a picture instead. A template opens to add its photo (or draw one), fill in its
 * sentence and shape, and generate: that starts a chat which makes the picture, and opens it.
 */
struct ImagesView: View {
	@Environment(\.palette) private var palette
	@ObservedObject var family: Family
	let slug: String
	/// Opens the chat a picture is being made in.
	let started: (String) -> Void
	@StateObject private var attachments: Attachments
	@State private var overview: ImagesOverview?
	@State private var group: String?
	@State private var picked: PictureTemplate?
	@State private var text = ""
	@State private var sending = false
	@State private var problem: String?

	init(family: Family, slug: String, started: @escaping (String) -> Void) {
		_family = ObservedObject(wrappedValue: family)
		self.slug = slug
		self.started = started
		_attachments = StateObject(wrappedValue: Attachments(client: family.client, slug: slug))
	}

	private var client: Client { family.client }

	private var groups: [String] {
		var seen: [String] = []
		for template in overview?.templates ?? [] where !seen.contains(template.category) {
			seen.append(template.category)
		}
		return seen
	}

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: 16) {
				if let overview {
					if !overview.ready {
						Label {
							VStack(alignment: .leading, spacing: 4) {
								Text("nolune can't make pictures yet.")
									.font(.subheadline.weight(.semibold))
								if let reason = overview.problem {
									Text(verbatim: reason)
										.font(.footnote)
										.foregroundStyle(.secondary)
								}
							}
						} icon: {
							Image(systemName: "exclamationmark.triangle")
								.foregroundStyle(Palette.plain.warning)
						}
						.padding(12)
						.frame(maxWidth: .infinity, alignment: .leading)
						.background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Palette.plain.warning.opacity(0.1)))
					}
					if groups.count > 1 {
						Picker("Template groups", selection: Binding(get: { group ?? groups[0] }, set: { group = $0 })) {
							ForEach(groups, id: \.self) { Text(verbatim: $0).tag($0) }
						}
						.pickerStyle(.segmented)
					}
					if overview.templates.isEmpty {
						Text("No templates yet.")
							.foregroundStyle(.secondary)
					}
					LazyVGrid(columns: [GridItem(.adaptive(minimum: 150), spacing: 12)], spacing: 12) {
						ForEach(overview.templates.filter { groups.count < 2 || $0.category == (group ?? groups[0]) }) { template in
							Button {
								picked = template
							} label: {
								TemplateTile(template: template, cover: client.cover(slug, template))
							}
							.buttonStyle(.plain)
						}
					}
				} else if problem == nil {
					ProgressView()
						.frame(maxWidth: .infinity)
						.padding(.top, 40)
				}
			}
			.padding(16)
			.frame(maxWidth: 900)
			.frame(maxWidth: .infinity)
		}
		.background(palette.background)
		.navigationTitle("Images")
		.environment(\.noluneClient, client)
		.refreshable { await load() }
		.task { await load() }
		.safeAreaInset(edge: .bottom, spacing: 0) {
			ComposerBar(
				text: $text,
				attachments: attachments,
				placeholder: String(localized: "Describe an image"),
				sending: sending,
				send: describe
			) {}
			.padding(.horizontal, 12)
			.padding(.vertical, 6)
			.frame(maxWidth: 780)
			.frame(maxWidth: .infinity)
			.background(palette.background)
		}
		.sheet(item: $picked) { template in
			TemplateSheet(template: template, slug: slug, client: client) { chat in
				picked = nil
				started(chat)
			}
		}
		.alert(problem ?? "", isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } })) {
			Button("OK", role: .cancel) {}
		}
	}

	private func load() async {
		do {
			overview = try await client.images(slug)
		} catch {
			// The screen went away while it loaded: it loads again when it's back.
			if !error.isCancellation { problem = error.localizedDescription }
		}
	}

	/// A picture from a description, with the photos attached.
	private func describe() {
		sending = true
		Task {
			defer { sending = false }
			do {
				let chat = try await client.makePicture(slug, template: nil, uploads: attachments.ready, text: text)
				text = ""
				attachments.sent()
				Haptics.success()
				started(chat.id)
			} catch {
				problem = error.localizedDescription
				Haptics.failure()
			}
		}
	}
}

/// A template: its cover, or its icon on its color, and its name.
private struct TemplateTile: View {
	@Environment(\.palette) private var palette
	let template: PictureTemplate
	let cover: URL?

	var body: some View {
		VStack(alignment: .leading, spacing: 6) {
			ZStack {
				RoundedRectangle(cornerRadius: 14, style: .continuous)
					.fill(Color(hex: template.color) ?? palette.muted)
				if let cover {
					AsyncImage(url: cover) { image in
						image.resizable().scaledToFill()
					} placeholder: {
						Color.clear
					}
				} else {
					LucideIcon(name: template.icon, fallback: "photo")
						.scaleEffect(2.2)
						.foregroundStyle(.black.opacity(0.6))
				}
			}
			.aspectRatio(1, contentMode: .fit)
			.clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
			Text(verbatim: template.name)
				.font(.subheadline.weight(.medium))
				.lineLimit(2)
		}
	}
}

/**
 * A template opened: its picture (a photo, or one drawn here), its sentence with its settings,
 * anything else to add, and the shape. Generate starts the chat.
 */
private struct TemplateSheet: View {
	@Environment(\.palette) private var palette
	let template: PictureTemplate
	let slug: String
	let client: Client
	let started: (String) -> Void
	@Environment(\.dismiss) private var dismiss
	@StateObject private var pictures: Attachments
	@State private var values: [String: String] = [:]
	/// Select settings the person writes their own words for.
	@State private var custom: Set<String> = []
	@State private var shape: String
	@State private var extra = ""
	@State private var photos: [PhotosPickerItem] = []
	@State private var photosShown = false
	@State private var cameraShown = false
	@State private var drawing = false
	@State private var sending = false
	@State private var problem: String?

	private static let customTag = "\u{0}custom"

	init(template: PictureTemplate, slug: String, client: Client, started: @escaping (String) -> Void) {
		self.template = template
		self.slug = slug
		self.client = client
		self.started = started
		_pictures = StateObject(wrappedValue: Attachments(client: client, slug: slug))
		_shape = State(initialValue: template.size)
		_values = State(initialValue: Dictionary(template.settings.map { ($0.id, $0.initial) }, uniquingKeysWith: { a, _ in a }))
	}

	private var drawn: Bool { template.imageSource == "drawing" }
	private var missingPicture: Bool { template.image == "required" && pictures.ready.isEmpty }

	var body: some View {
		NavigationStack {
			Form {
				Group {
					Section {
						Text(verbatim: template.description)
							.foregroundStyle(.secondary)
						Text(verbatim: template.sentence(with: values, picture: pictures.items.isEmpty ? "" : (template.imageLabel ?? String(localized: "Picture")).lowercased()))
							.font(.title3.weight(.semibold))
					}
					if template.image != "none" {
						Section(template.imageLabel ?? String(localized: "Picture")) {
							ForEach(pictures.items) { item in
								HStack {
									if let preview = item.preview {
										Image(uiImage: preview)
											.resizable()
											.scaledToFill()
											.frame(width: 56, height: 56)
											.clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
									}
									Text(verbatim: item.problem ?? item.name)
										.foregroundStyle(item.problem == nil ? Palette.plain.foreground : Palette.plain.destructive)
									Spacer()
									if item.upload == nil, item.problem == nil { ProgressView() }
									Button {
										pictures.remove(item.id)
									} label: {
										Image(systemName: "xmark.circle.fill")
											.foregroundStyle(.secondary)
									}
									.buttonStyle(.borderless)
								}
							}
							if pictures.items.count < template.maxImages {
								if drawn {
									Button("Start drawing") { drawing = true }
									Button("Use a photo of a drawing") { photosShown = true }
								} else {
									if UIImagePickerController.isSourceTypeAvailable(.camera) {
										Button("Take a photo") { cameraShown = true }
									}
									Button("Choose a photo") { photosShown = true }
								}
							}
						}
					}
					if !template.settings.isEmpty {
						Section {
							ForEach(template.settings, id: \.id) { setting in
								row(setting)
							}
						}
					}
					Section {
						TextField("Add anything else…", text: $extra, axis: .vertical)
						Picker("Shape", selection: $shape) {
							Text("Square").tag("square")
							Text("Portrait").tag("portrait")
							Text("Landscape").tag("landscape")
							Text("Auto").tag("auto")
						}
					} footer: {
						if let problem {
							Text(verbatim: problem)
								.foregroundStyle(Palette.plain.destructive)
						} else if missingPicture {
							Text(drawn ? "Add a drawing first." : "Add a photo first.")
						}
					}
				}
				.listRowBackground(palette.muted)
			}
			.pageBackground(palette)
			.navigationTitle(template.title)
			.navigationBarTitleDisplayMode(.inline)
			.toolbar {
				ToolbarItem(placement: .cancellationAction) {
					Button("Cancel") { dismiss() }
				}
				ToolbarItem(placement: .confirmationAction) {
					Button("Generate", action: generate)
						.disabled(sending || pictures.uploading || missingPicture)
				}
			}
			.photosPicker(isPresented: $photosShown, selection: $photos, maxSelectionCount: max(1, template.maxImages - pictures.items.count), matching: .images)
			.onChange(of: photos) { picked in
				guard !picked.isEmpty else { return }
				photos = []
				Task {
					for item in picked {
						guard let data = try? await item.loadTransferable(type: Data.self) else { continue }
						pictures.add(data, named: "Photo.\(item.supportedContentTypes.first?.preferredFilenameExtension ?? "jpg")")
					}
				}
			}
			.fullScreenCover(isPresented: $cameraShown) {
				CameraPicker(taken: { data, name in pictures.add(data, named: name) }, close: { cameraShown = false }, photosOnly: true)
					.ignoresSafeArea()
			}
			.fullScreenCover(isPresented: $drawing) {
				DrawingView { png in pictures.add(png, named: "drawing.png") }
			}
		}
	}

	@ViewBuilder private func row(_ setting: PictureTemplate.Setting) -> some View {
		switch setting {
		case .select(let id, let label, _, let options, let canWriteOwn):
			Picker(label, selection: Binding(
				get: { custom.contains(id) ? Self.customTag : values[id] ?? "" },
				set: { value in
					if value == Self.customTag {
						custom.insert(id)
						values[id] = ""
					} else {
						custom.remove(id)
						values[id] = value
					}
				}
			)) {
				ForEach(options, id: \.value) { option in
					Text(verbatim: option.label).tag(option.value)
				}
				if canWriteOwn {
					Text("Custom…").tag(Self.customTag)
				}
			}
			if custom.contains(id) {
				TextField(label, text: binding(id))
			}
		case .emoji(let id, let label, _, let most):
			TextField(label, text: Binding(
				get: { values[id] ?? "" },
				set: { values[id] = String($0.filter { $0.isEmoji }.prefix(most)) }
			))
		case .text(let id, let label, _, let placeholder, _):
			TextField(label, text: binding(id), prompt: placeholder.map { Text(verbatim: $0) })
		}
	}

	private func binding(_ id: String) -> Binding<String> {
		Binding(get: { values[id] ?? "" }, set: { values[id] = $0 })
	}

	private func generate() {
		sending = true
		problem = nil
		Task {
			defer { sending = false }
			do {
				let chat = try await client.makePicture(
					slug,
					template: template.id,
					settings: values,
					uploads: pictures.ready,
					shape: shape,
					extra: extra.isEmpty ? nil : extra
				)
				Haptics.success()
				started(chat.id)
			} catch {
				problem = error.localizedDescription
				Haptics.failure()
			}
		}
	}
}

private extension Character {
	/// An emoji, not a digit or a letter that could be one.
	var isEmoji: Bool {
		unicodeScalars.contains { $0.properties.isEmojiPresentation || ($0.properties.isEmoji && $0.value > 0xFF) }
	}
}

private extension Color {
	/// `#f6b7c1`.
	init?(hex: String?) {
		guard let hex, hex.hasPrefix("#"), hex.count == 7, let value = UInt32(hex.dropFirst(), radix: 16) else { return nil }
		self.init(
			red: Double((value >> 16) & 0xFF) / 255,
			green: Double((value >> 8) & 0xFF) / 255,
			blue: Double(value & 0xFF) / 255
		)
	}
}

// MARK: - Drawing

/// A drawing to make a picture from: PencilKit's canvas, square and white, as a 1024-point PNG.
struct DrawingView: View {
	@Environment(\.palette) private var palette
	let done: (Data) -> Void
	@Environment(\.dismiss) private var dismiss
	@State private var canvas = PKCanvasView()

	var body: some View {
		NavigationStack {
			GeometryReader { geometry in
				let side = min(geometry.size.width, geometry.size.height)
				Board(view: canvas)
					.frame(width: side, height: side)
					.frame(maxWidth: .infinity, maxHeight: .infinity)
			}
			.background(palette.background)
			.navigationTitle("Draw")
			.navigationBarTitleDisplayMode(.inline)
			.toolbar {
				ToolbarItem(placement: .cancellationAction) {
					Button("Cancel") { dismiss() }
				}
				ToolbarItem(placement: .primaryAction) {
					Button {
						canvas.undoManager?.undo()
					} label: {
						Image(systemName: "arrow.uturn.backward")
					}
					.accessibilityLabel(Text("Undo"))
				}
				ToolbarItem(placement: .confirmationAction) {
					Button("Done") {
						if let png = picture() { done(png) }
						dismiss()
					}
				}
			}
		}
	}

	/// The drawing on white, 1024 points a side.
	private func picture() -> Data? {
		let bounds = canvas.bounds
		guard bounds.width > 0, !canvas.drawing.strokes.isEmpty else { return nil }
		let side: CGFloat = 1024
		let format = UIGraphicsImageRendererFormat()
		format.scale = 1
		let image = UIGraphicsImageRenderer(size: CGSize(width: side, height: side), format: format).image { context in
			UIColor.white.setFill()
			context.fill(CGRect(x: 0, y: 0, width: side, height: side))
			canvas.drawing.image(from: bounds, scale: side / bounds.width).draw(in: CGRect(x: 0, y: 0, width: side, height: side))
		}
		return image.pngData()
	}

	private struct Board: UIViewRepresentable {
		let view: PKCanvasView

		func makeUIView(context: Context) -> PKCanvasView {
			view.drawingPolicy = .anyInput
			view.backgroundColor = .white
			view.isOpaque = true
			// Ink as it looks on paper, whatever the phone's appearance.
			view.overrideUserInterfaceStyle = .light
			view.tool = PKInkingTool(.marker, color: .black, width: 12)
			context.coordinator.picker.setVisible(true, forFirstResponder: view)
			context.coordinator.picker.addObserver(view)
			DispatchQueue.main.async { view.becomeFirstResponder() }
			return view
		}

		func updateUIView(_ view: PKCanvasView, context: Context) {}

		func makeCoordinator() -> Coordinator {
			Coordinator()
		}

		final class Coordinator {
			let picker = PKToolPicker()
		}
	}
}
