import QuickLook
import SwiftUI

/**
 * A chat's picture or file in Quick Look: the original, downloaded with the session's cookie
 * (`?download`), under the name the nolune gives it, to look at, mark up or share.
 */
struct MediaPreview: View {
	struct Item: Identifiable {
		let id = UUID()
		let url: URL
	}

	let item: Item
	@Environment(\.dismiss) private var dismiss
	@State private var file: URL?
	@State private var failed: String?

	var body: some View {
		NavigationStack {
			Group {
				if let file {
					QuickLookPreview(url: file)
						.ignoresSafeArea(edges: .bottom)
				} else if let failed {
					VStack(spacing: 8) {
						Image(systemName: "exclamationmark.triangle")
							.font(.title)
						Text(verbatim: failed)
							.multilineTextAlignment(.center)
					}
					.foregroundStyle(.secondary)
					.padding()
				} else {
					ProgressView()
				}
			}
			.navigationTitle(file?.lastPathComponent ?? "")
			.navigationBarTitleDisplayMode(.inline)
			.toolbar {
				ToolbarItem(placement: .confirmationAction) {
					Button("Done") { dismiss() }
				}
				ToolbarItem(placement: .primaryAction) {
					if let file { ShareLink(item: file) }
				}
			}
		}
		.task { await download() }
	}

	private func download() async {
		var components = URLComponents(url: item.url, resolvingAgainstBaseURL: false)
		let query = components?.queryItems ?? []
		if !query.contains(where: { $0.name == "download" }) {
			components?.queryItems = query + [URLQueryItem(name: "download", value: nil)]
		}
		do {
			let (downloaded, response) = try await URLSession.shared.download(from: components?.url ?? item.url)
			let status = (response as? HTTPURLResponse)?.statusCode ?? 0
			guard (200..<300).contains(status) else {
				failed = Refusal(address: Address.display(item.url), status: status, message: nil).localizedDescription
				return
			}
			let name = response.suggestedFilename ?? item.url.lastPathComponent
			let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
			try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
			let destination = folder.appendingPathComponent(name)
			try FileManager.default.moveItem(at: downloaded, to: destination)
			file = destination
		} catch {
			failed = error.localizedDescription
		}
	}
}

/// QLPreviewController, for one file.
private struct QuickLookPreview: UIViewControllerRepresentable {
	let url: URL

	func makeUIViewController(context: Context) -> QLPreviewController {
		let controller = QLPreviewController()
		controller.dataSource = context.coordinator
		return controller
	}

	func updateUIViewController(_ controller: QLPreviewController, context: Context) {
		context.coordinator.url = url
		controller.reloadData()
	}

	func makeCoordinator() -> Coordinator {
		Coordinator(url: url)
	}

	final class Coordinator: NSObject, QLPreviewControllerDataSource {
		var url: URL

		init(url: URL) {
			self.url = url
		}

		func numberOfPreviewItems(in controller: QLPreviewController) -> Int { 1 }

		func previewController(_ controller: QLPreviewController, previewItemAt index: Int) -> QLPreviewItem {
			url as NSURL
		}
	}
}
