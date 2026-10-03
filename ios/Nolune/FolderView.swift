import PhotosUI
import SwiftUI

/**
 * A folder, natively (#137): a new chat in it, its instructions for every chat inside, its files
 * (added from Photos or Files, opened in Quick Look), and its chats. Renaming and deleting it are
 * in its menu, as in the sidebar's.
 */
struct FolderView: View {
	@Environment(\.palette) private var palette
	@ObservedObject var family: Family
	let slug: String
	let id: String
	/// Opens a chat.
	let open: (Destination) -> Void
	/// Opens a new chat in the folder.
	let newChat: () -> Void
	/// The folder was deleted.
	let deleted: () -> Void
	@StateObject private var uploads: Attachments
	@State private var folder: FolderDetail?
	@State private var instructions = ""
	@State private var preview: MediaPreview.Item?
	@State private var renaming = false
	@State private var newName = ""
	@State private var deleting = false
	@State private var photos: [PhotosPickerItem] = []
	@State private var photosShown = false
	@State private var filesShown = false
	@State private var problem: String?

	init(
		family: Family,
		slug: String,
		id: String,
		open: @escaping (Destination) -> Void,
		newChat: @escaping () -> Void,
		deleted: @escaping () -> Void
	) {
		_family = ObservedObject(wrappedValue: family)
		self.slug = slug
		self.id = id
		self.open = open
		self.newChat = newChat
		self.deleted = deleted
		_uploads = StateObject(wrappedValue: Attachments(client: family.client, slug: slug))
	}

	private var client: Client { family.client }
	private var chats: [ChatSummary] { family.chats.filter { $0.folderId == id } }

	var body: some View {
		List {
			rows
				.listRowBackground(palette.muted)
		}
		.pageBackground(palette)
		.navigationTitle(folder?.name ?? family.folders.first { $0.id == id }?.name ?? "")
		.toolbar {
			ToolbarItem(placement: .primaryAction) {
				Menu {
					Button {
						newName = folder?.name ?? ""
						renaming = true
					} label: {
						Label("Rename", systemImage: "pencil")
					}
					Button(role: .destructive) {
						deleting = true
					} label: {
						Label("Delete", systemImage: "trash")
					}
				} label: {
					Image(systemName: "ellipsis.circle")
				}
				.accessibilityLabel(Text("Folder options"))
			}
		}
		.refreshable { await load() }
		.task { await load() }
		.sheet(item: $preview) { MediaPreview(item: $0) }
		.photosPicker(isPresented: $photosShown, selection: $photos, maxSelectionCount: 10, matching: .any(of: [.images, .videos]))
		.onChange(of: photos) { picked in
			guard !picked.isEmpty else { return }
			photos = []
			Task {
				for (index, item) in picked.enumerated() {
					guard let data = try? await item.loadTransferable(type: Data.self) else { continue }
					let type = item.supportedContentTypes.first
					uploads.add(data, named: "Photo \(index + 1).\(type?.preferredFilenameExtension ?? "jpg")")
				}
			}
		}
		.fileImporter(isPresented: $filesShown, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
			guard case .success(let urls) = result else { return }
			for url in urls {
				let scoped = url.startAccessingSecurityScopedResource()
				defer { if scoped { url.stopAccessingSecurityScopedResource() } }
				if let data = try? Data(contentsOf: url) { uploads.add(data, named: url.lastPathComponent) }
			}
		}
		.onChange(of: uploads.uploading) { uploading in
			// Every file is up: they join the folder together.
			if !uploading, !uploads.ready.isEmpty { addUploads() }
		}
		.alert("Rename folder", isPresented: $renaming) {
			TextField("Folder name", text: $newName)
			Button("Save") {
				guard let found = family.folders.first(where: { $0.id == id }) else { return }
				Task {
					await family.rename(found, to: newName)
					await load()
				}
			}
			Button("Cancel", role: .cancel) {}
		}
		.confirmationDialog("Delete folder?", isPresented: $deleting, titleVisibility: .visible) {
			Button("Delete", role: .destructive) {
				guard let found = family.folders.first(where: { $0.id == id }) else { return }
				Task {
					if await family.delete(found) { deleted() }
				}
			}
		} message: {
			Text("\(folder?.name ?? "") is deleted for everyone in the profile. Its chats move back to your chat list, without its instructions and files. The files are moved to ~/.nolune/trash.")
		}
		.alert(problem ?? "", isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } })) {
			Button("OK", role: .cancel) {}
		}
	}

	/// The rows, apart from `body`, which the compiler then type-checks in time.
	@ViewBuilder private var rows: some View {
		if let folder {
			Section {
				Button(action: newChat) {
					Label("New chat in \(folder.name)", systemImage: "square.and.pencil")
				}
			}
			Section {
				TextField("We're planning two weeks in Japan in April with the kids (7 and 10). Keep plans relaxed and the budget under ¥600,000.", text: $instructions, axis: .vertical)
					.lineLimit(3...12)
				if instructions != folder.instructions {
					HStack {
						Text(verbatim: "\(instructions.count) / \(folder.maxInstructions)")
							.font(.caption.monospacedDigit())
							.foregroundStyle(instructions.count > folder.maxInstructions ? Palette.plain.destructive : Color.secondary)
						Spacer()
						Button("Cancel") { instructions = folder.instructions }
							.buttonStyle(.borderless)
						Button("Save") { saveInstructions() }
							.buttonStyle(.borderless)
							.disabled(instructions.count > folder.maxInstructions)
					}
				}
			} header: {
				Text("Instructions")
			} footer: {
				Text("What nolune should know or do in every chat here.")
			}
			Section {
				if folder.files.isEmpty, uploads.items.isEmpty {
					Text("No files yet")
						.foregroundStyle(.secondary)
				}
				ForEach(folder.files) { file in
					fileRow(file)
				}
				ForEach(uploads.items) { item in
					HStack {
						ProgressView()
						Text(verbatim: item.name)
							.foregroundStyle(.secondary)
						if let problem = item.problem {
							Text(verbatim: problem)
								.font(.caption)
								.foregroundStyle(Palette.plain.destructive)
						}
					}
				}
				Menu {
					Button {
						photosShown = true
					} label: {
						Label("Photos", systemImage: "photo.on.rectangle")
					}
					Button {
						filesShown = true
					} label: {
						Label("Files", systemImage: "folder")
					}
				} label: {
					Label("Add files", systemImage: "plus")
				}
				.disabled(folder.files.count >= folder.maxFiles)
			} header: {
				Text("Files")
			} footer: {
				Text("Pictures, documents, anything. nolune gets where they're saved and opens them when they matter.")
			}
			Section("Chats") {
				if chats.isEmpty {
					Text("Chats you start here show up here. You can also drag chats onto the folder in the sidebar.")
						.foregroundStyle(.secondary)
				}
				ForEach(chats) { chat in
					Button {
						open(.chat(chat.id))
					} label: {
						HStack {
							ChatRow(title: chat.title.isEmpty ? String(localized: "New chat") : chat.title, running: family.running.contains(chat.id))
								.foregroundStyle(Palette.plain.foreground)
							Text(chat.updated, format: .relative(presentation: .named))
								.font(.caption)
								.foregroundStyle(.secondary)
						}
					}
				}
			}
		} else if problem == nil {
			ProgressView()
				.frame(maxWidth: .infinity)
		}
	}

	private func fileRow(_ file: FolderDetail.File) -> some View {
		Button {
			preview = MediaPreview.Item(url: client.folderFile(slug, id, file.id))
		} label: {
			HStack(spacing: 10) {
				if file.viewable {
					RemotePicture(url: client.folderFile(slug, id, file.id))
						.frame(width: 40, height: 40)
				} else {
					Image(systemName: "doc")
						.font(.title3)
						.frame(width: 40, height: 40)
						.foregroundStyle(.secondary)
				}
				VStack(alignment: .leading, spacing: 2) {
					Text(verbatim: file.name)
						.foregroundStyle(Palette.plain.foreground)
						.lineLimit(1)
						.truncationMode(.middle)
					Text(verbatim: ByteCountFormatter.string(fromByteCount: Int64(file.bytes), countStyle: .file))
						.font(.caption)
						.foregroundStyle(.secondary)
				}
			}
		}
		.swipeActions {
			Button(role: .destructive) {
				remove(file)
			} label: {
				Label("Remove", systemImage: "trash")
			}
		}
	}

	// MARK: Doing

	private func load() async {
		do {
			let loaded = try await client.folder(slug, id)
			folder = loaded
			instructions = loaded.instructions
		} catch {
			// The screen went away while it loaded: it loads again when it's back.
			if !error.isCancellation { problem = error.localizedDescription }
		}
	}

	private func saveInstructions() {
		Task {
			do {
				try await client.setInstructions(slug, id, instructions)
				Haptics.success()
				await load()
			} catch {
				problem = error.localizedDescription
			}
		}
	}

	private func addUploads() {
		let ready = uploads.ready
		Task {
			do {
				let files = try await client.addFolderFiles(slug, id, uploads: ready)
				uploads.sent()
				if let current = folder {
					folder = FolderDetail(
						id: current.id,
						name: current.name,
						instructions: current.instructions,
						files: files,
						maxFiles: current.maxFiles,
						maxInstructions: current.maxInstructions
					)
				}
				Haptics.success()
			} catch {
				problem = error.localizedDescription
			}
		}
	}

	private func remove(_ file: FolderDetail.File) {
		Task {
			do {
				try await client.removeFolderFile(slug, id, file.id)
				await load()
			} catch {
				problem = error.localizedDescription
			}
		}
	}
}
