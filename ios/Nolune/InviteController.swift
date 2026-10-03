import UIKit
import WebKit

/**
 * An invite link's page, in a sheet over the native sign-in: it makes the account and signs in,
 * as in a browser (packages/web/src/routes/invite). Once it goes on to another of the family's
 * pages, the account is made, and the sheet hands that page over; to the sign-in page, the person
 * has one already, and it closes. Its cookies are the web views', which NativeController takes.
 */
final class InviteController: UIViewController {
	private let origin: URL
	private let path: String
	/// The page it went on to, signed in; nil when it closed without.
	private let done: (String?) -> Void
	private var webView: WKWebView!
	private var observation: NSKeyValueObservation?
	private var finished = false

	init(origin: URL, path: String, done: @escaping (String?) -> Void) {
		self.origin = origin
		self.path = path
		self.done = done
		super.init(nibName: nil, bundle: nil)
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("not from a storyboard")
	}

	override func viewDidLoad() {
		super.viewDidLoad()
		view.backgroundColor = .page
		navigationItem.title = Address.display(origin)
		navigationItem.leftBarButtonItem = UIBarButtonItem(
			systemItem: .cancel,
			primaryAction: UIAction { [weak self] _ in self?.finish(nil) }
		)

		let configuration = WKWebViewConfiguration()
		configuration.applicationNameForUserAgent = "Mobile/15E148 nolune/\(Bundle.main.version)"
		let webView = WKWebView(frame: .zero, configuration: configuration)
		webView.navigationDelegate = self
		webView.allowsLinkPreview = false
		webView.isOpaque = false
		webView.backgroundColor = .clear
		webView.translatesAutoresizingMaskIntoConstraints = false
		view.addSubview(webView)
		NSLayoutConstraint.activate([
			webView.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
			webView.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor),
			webView.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor),
			webView.bottomAnchor.constraint(equalTo: view.keyboardLayoutGuide.topAnchor)
		])
		self.webView = webView
		// The web app moves on in place once the account is made, without loading a page.
		observation = webView.observe(\.url, options: [.new]) { [weak self] _, _ in
			Task { @MainActor in self?.pageChanged() }
		}
		if let url = URL(string: path, relativeTo: origin)?.absoluteURL {
			webView.load(URLRequest(url: url))
		}
	}

	/// Only once the page has loaded, as BrowserController's `pageChanged`: a redirect may follow.
	private func pageChanged() {
		guard let url = webView.url, Address.sameOrigin(url, origin), !webView.isLoading else { return }
		if url.path.hasPrefix("/invite/") { return }
		if url.path == "/login" { return finish(nil) }
		var landed = url.path
		if let query = url.query, !query.isEmpty { landed += "?\(query)" }
		finish(landed)
	}

	private func finish(_ landed: String?) {
		guard !finished else { return }
		finished = true
		done(landed)
	}
}

extension InviteController: WKNavigationDelegate {
	/// The family's pages here; links elsewhere, like the privacy policy, in Safari.
	func webView(
		_ webView: WKWebView,
		decidePolicyFor navigationAction: WKNavigationAction,
		decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
	) {
		guard let url = navigationAction.request.url else { return decisionHandler(.cancel) }
		if navigationAction.targetFrame?.isMainFrame != false, !Address.sameOrigin(url, origin) {
			UIApplication.shared.open(url)
			return decisionHandler(.cancel)
		}
		decisionHandler(.allow)
	}

	func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
		pageChanged()
	}
}
