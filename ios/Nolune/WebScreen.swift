import SwiftUI

/**
 * A page of the family's nolune inside a native screen, under the app's navigation bar, with its
 * back gesture (#131). It's BrowserController's, so files, alerts, links elsewhere and the page's
 * color work as in the web app; the page leaves out its header and sidebar. Links to what the app
 * shows natively go to `open`, which says whether it took them.
 */
struct WebScreen: UIViewControllerRepresentable {
	let origin: URL
	let path: String
	let open: (URL) -> Bool
	let signedOut: () -> Void

	func makeUIViewController(context: Context) -> BrowserController {
		BrowserController(
			origin: origin,
			page: BrowserController.Embedded(path: path, open: open),
			signedOut: signedOut
		)
	}

	func updateUIViewController(_ controller: BrowserController, context: Context) {}
}
