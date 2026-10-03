import Combine
import QuickLook
import SwiftUI
import UIKit
import WebKit

/**
 * The family's nolune: its web app in a web view, as Safari shows it, and what an app adds. Files
 * it hands over open in Quick Look, links elsewhere open in Safari, the page's color goes around
 * it (with a status bar that reads on it), and it bridges to the app: notifications (Push.swift)
 * and the `nolune` messages the page sends (packages/web/src/lib/ios.ts). With the native screens
 * on (NativeController.swift), it may have a note above it, and leaves signing in to them.
 *
 * It also shows a single page inside a native screen (`page:`), under the app's navigation: its
 * web views say so in the user agent, and the page leaves out its own header and sidebar. A link
 * to what the app shows natively (a chat, another of its screens) goes to the app instead.
 */
final class BrowserController: UIViewController {
	/// A page inside a native screen, and what the app opens itself, true when it does.
	struct Embedded {
		let path: String
		let open: (URL) -> Bool
	}

	/// The user agent's mark for web views inside native screens (packages/web/src/lib/ios.ts).
	static let embeddedAgent = "nolune-embedded"

	private let origin: URL
	private let embedded: Embedded?
	/// A line above the page, which its × closes (NoteBar).
	private let note: String?
	/// Signed in natively: called instead of showing the sign-in page, once someone signs out.
	private var signedOut: (() -> Void)?
	private var model: AppModel { AppModel.shared }
	private var webView: WKWebView!
	private let spinner = UIActivityIndicatorView(style: .medium)
	private var unreachable: UIViewController?
	private var observations: [NSKeyValueObservation] = []
	private var subscriptions: Set<AnyCancellable> = []
	/// The token this page's session has, so it's sent once.
	private var registered: String?
	/// The last page loaded was an error, like the relay's while the family's computer is off.
	private var errorPage = false
	private var downloads: [WKDownload: URL] = [:]
	/// Quick Look's, which it doesn't keep itself.
	private var preview: Preview?

	init(origin: URL, note: String? = nil, signedOut: (() -> Void)? = nil) {
		self.origin = origin
		self.embedded = nil
		self.note = note
		self.signedOut = signedOut
		super.init(nibName: nil, bundle: nil)
	}

	/// One page, inside a native screen.
	init(origin: URL, page: Embedded, signedOut: (() -> Void)? = nil) {
		self.origin = origin
		self.embedded = page
		self.note = nil
		self.signedOut = signedOut
		super.init(nibName: nil, bundle: nil)
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("not from a storyboard")
	}

	override func viewDidLoad() {
		super.viewDidLoad()
		view.backgroundColor = .page

		let configuration = WKWebViewConfiguration()
		// Safari's, and the app's: the page can tell, though it asks for `nolune` messages instead.
		configuration.applicationNameForUserAgent = "Mobile/15E148 nolune/\(Bundle.main.version)"
			+ (embedded == nil ? "" : " \(BrowserController.embeddedAgent)")
		configuration.allowsInlineMediaPlayback = true
		configuration.userContentController.add(ScriptHandler(self), name: "nolune")
		configuration.userContentController.addUserScript(
			WKUserScript(source: BrowserController.pageColor, injectionTime: .atDocumentEnd, forMainFrameOnly: true)
		)

		let webView = WKWebView(frame: .zero, configuration: configuration)
		webView.navigationDelegate = self
		webView.uiDelegate = self
		webView.allowsBackForwardNavigationGestures = true
		webView.allowsLinkPreview = false
		// The page's color shows while it loads, not white.
		webView.isOpaque = false
		webView.backgroundColor = .clear
		webView.scrollView.backgroundColor = .clear
		#if DEBUG
		if #available(iOS 16.4, *) { webView.isInspectable = true }
		#endif
		// The note above the page, if any: closed, the page takes its place.
		let stack = UIStackView(arrangedSubviews: [webView])
		stack.axis = .vertical
		if let note {
			let bar = NoteBar(note)
			bar.close = { [weak bar, weak stack] in
				UIView.animate(withDuration: 0.25) {
					bar?.isHidden = true
					stack?.layoutIfNeeded()
				}
			}
			stack.insertArrangedSubview(bar, at: 0)
		}
		stack.translatesAutoresizingMaskIntoConstraints = false
		view.addSubview(stack)
		// Inside the safe area, and above the keyboard, so the composer stays in sight.
		NSLayoutConstraint.activate([
			stack.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
			stack.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor),
			stack.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor),
			stack.bottomAnchor.constraint(equalTo: view.keyboardLayoutGuide.topAnchor)
		])
		self.webView = webView

		spinner.color = .secondaryLabel
		spinner.translatesAutoresizingMaskIntoConstraints = false
		view.addSubview(spinner)
		NSLayoutConstraint.activate([
			spinner.centerXAnchor.constraint(equalTo: view.centerXAnchor),
			spinner.centerYAnchor.constraint(equalTo: view.centerYAnchor)
		])

		observations = [
			webView.observe(\.url, options: [.new]) { [weak self] _, _ in
				Task { @MainActor in self?.pageChanged() }
			}
		]

		if let embedded {
			// The native screens open what's pending, and register for notifications themselves.
			load(embedded.path)
			model.$activations
				.dropFirst()
				.receive(on: DispatchQueue.main)
				.sink { [weak self] _ in self?.becameActive() }
				.store(in: &subscriptions)
			return
		}
		load(model.takePending() ?? "/")

		// After each change has landed (@Published tells before), on the main queue.
		model.$pending
			.compactMap { $0 }
			.receive(on: DispatchQueue.main)
			.sink { [weak self] _ in
				guard let self, let path = self.model.takePending() else { return }
				self.load(path)
			}
			.store(in: &subscriptions)
		model.$deviceToken
			.receive(on: DispatchQueue.main)
			.sink { [weak self] _ in self?.registerDevice() }
			.store(in: &subscriptions)
		model.$activations
			.dropFirst()
			.receive(on: DispatchQueue.main)
			.sink { [weak self] _ in self?.becameActive() }
			.store(in: &subscriptions)
	}

	override var preferredStatusBarStyle: UIStatusBarStyle {
		(view.backgroundColor ?? .page).isDark(in: traitCollection) ? .lightContent : .darkContent
	}

	// MARK: Pages

	private func load(_ path: String) {
		guard let url = URL(string: path, relativeTo: origin)?.absoluteURL else { return }
		hideUnreachable()
		spinner.startAnimating()
		webView.load(URLRequest(url: url))
	}

	private func isFamily(_ url: URL) -> Bool {
		Address.sameOrigin(url, origin)
	}

	/// Where nobody is signed in: the sign-in page, and the invite links' pages.
	private func isSignIn(_ url: URL) -> Bool {
		url.path == "/login" || url.path.hasPrefix("/invite/")
	}

	/**
	 * The page changed, also within the web app: someone may have signed in, or out. Only once a
	 * page has loaded: its address changes as soon as it starts to, before a redirect to the
	 * sign-in page, or an error, says whether anyone is signed in.
	 */
	private func pageChanged() {
		guard let url = webView.url, isFamily(url), !webView.isLoading else { return }
		if isSignIn(url) {
			registered = nil
			if url.path == "/login" { leftForSignIn() }
			return
		}
		guard !errorPage else { return }
		if let embedded {
			// The page moved on by itself (the router, not a link) to something the app shows.
			if embedded.open(url), webView.canGoBack { webView.goBack() }
			return
		}
		// Not in the middle of a profile's welcome, which asks enough already.
		if !url.path.hasSuffix("/welcome") { Push.start() }
		registerDevice()
	}

	/// Signed out, with the native screens: they sign in again, not the page.
	private func leftForSignIn() {
		guard let signedOut else { return }
		self.signedOut = nil
		signedOut()
	}

	/// Sends the token Apple gave the app to the nolune the page is signed in to, once.
	private func registerDevice() {
		guard embedded == nil, let token = model.deviceToken, registered != token, !errorPage,
			let webView, !webView.isLoading, let url = webView.url, isFamily(url), !isSignIn(url)
		else { return }
		registered = token
		Task {
			let taken = await Push.register(token, on: webView)
			if !taken, registered == token { registered = nil }
		}
	}

	/**
	 * Says the page's background (`color` messages), now and whenever it changes: the web app's
	 * theme, picked in its settings or following the system's, is a class and a color scheme on
	 * `<html>`. Its background is on `<body>`, which WebKit's own `underPageBackgroundColor`
	 * doesn't see (it reads clear there).
	 */
	private static let pageColor = """
		(() => {
			const send = () => {
				for (const element of [document.body, document.documentElement]) {
					const color = element && getComputedStyle(element).backgroundColor;
					if (color && color !== 'transparent' && color !== 'rgba(0, 0, 0, 0)') {
						window.webkit.messageHandlers.nolune.postMessage({ type: 'color', value: color });
						return;
					}
				}
			};
			send();
			addEventListener('load', send);
			new MutationObserver(send).observe(document.documentElement, {
				attributes: true,
				attributeFilter: ['class', 'style']
			});
			matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => setTimeout(send, 50));
		})();
		"""

	/// The page's background around it, with a status bar that reads on it.
	private func colorChanged(_ css: String) {
		guard let color = UIColor(css: css) else { return }
		view.backgroundColor = color
		setNeedsStatusBarAppearanceUpdate()
	}

	private func becameActive() {
		if unreachable != nil { webView.reload() }
		registerDevice()
	}

	/// A message from the page: its web app's (packages/web/src/lib/ios.ts), or `pageColor`'s.
	fileprivate func received(_ type: String, _ value: String?) {
		switch type {
		case "connect":
			// This nolune stops sending notifications here first.
			Task {
				await Push.unregister(on: webView)
				model.disconnect()
			}
		case "color":
			if let value { colorChanged(value) }
		default:
			break
		}
	}

	// MARK: When it can't be reached

	private func showUnreachable(_ detail: String) {
		hideUnreachable()
		let screen = UIHostingController(
			onboarding: UnreachableView(
				address: Address.display(origin),
				detail: detail,
				retry: { [weak self] in
					guard let self else { return }
					self.hideUnreachable()
					self.spinner.startAnimating()
					if self.webView.url == nil { self.load("/") } else { self.webView.reload() }
				},
				connectElsewhere: { [weak self] in self?.model.disconnect() }
			)
		)
		addChild(screen)
		screen.view.frame = view.bounds
		screen.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
		view.addSubview(screen.view)
		screen.didMove(toParent: self)
		unreachable = screen
		setNeedsStatusBarAppearanceUpdate()
	}

	private func hideUnreachable() {
		guard let screen = unreachable else { return }
		screen.willMove(toParent: nil)
		screen.view.removeFromSuperview()
		screen.removeFromParent()
		unreachable = nil
		setNeedsStatusBarAppearanceUpdate()
	}

	override var childForStatusBarStyle: UIViewController? { unreachable }

	// MARK: Files

	private func show(_ file: URL) {
		let preview = Preview(file)
		self.preview = preview
		let controller = QLPreviewController()
		controller.dataSource = preview
		(presentedViewController ?? self).present(controller, animated: true)
	}
}

// MARK: - Navigation

extension BrowserController: WKNavigationDelegate {
	func webView(
		_ webView: WKWebView,
		decidePolicyFor navigationAction: WKNavigationAction,
		decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
	) {
		guard let url = navigationAction.request.url else { return decisionHandler(.cancel) }
		if navigationAction.shouldPerformDownload { return decisionHandler(.download) }
		switch url.scheme?.lowercased() ?? "" {
		case "http", "https":
			let frame = navigationAction.targetFrame
			if frame == nil {
				// A new window (`target="_blank"`): here for the family's pages, Safari for the rest.
				if isFamily(url) { webView.load(navigationAction.request) } else { UIApplication.shared.open(url) }
				return decisionHandler(.cancel)
			}
			// Elsewhere on the web goes to Safari, but in a frame on the page.
			if frame?.isMainFrame == true, !isFamily(url) {
				UIApplication.shared.open(url)
				return decisionHandler(.cancel)
			}
			// Signed out (`/logout` goes on to it), with the native screens to sign in again.
			if frame?.isMainFrame == true, url.path == "/login", signedOut != nil {
				leftForSignIn()
				return decisionHandler(.cancel)
			}
			// Inside a native screen, a link to what the app shows natively goes to the app.
			if frame?.isMainFrame == true, let embedded, embedded.open(url) {
				return decisionHandler(.cancel)
			}
			decisionHandler(.allow)
		case "blob", "data", "about":
			decisionHandler(.allow)
		default:
			// mailto:, tel:, maps: and the like, for the apps that open them.
			UIApplication.shared.open(url)
			decisionHandler(.cancel)
		}
	}

	func webView(
		_ webView: WKWebView,
		decidePolicyFor navigationResponse: WKNavigationResponse,
		decisionHandler: @escaping (WKNavigationResponsePolicy) -> Void
	) {
		if navigationResponse.isForMainFrame, let response = navigationResponse.response as? HTTPURLResponse {
			let disposition = response.value(forHTTPHeaderField: "Content-Disposition") ?? ""
			if disposition.lowercased().hasPrefix("attachment") { return decisionHandler(.download) }
			errorPage = response.statusCode >= 400
		}
		decisionHandler(navigationResponse.canShowMIMEType ? .allow : .download)
	}

	func webView(_ webView: WKWebView, navigationAction: WKNavigationAction, didBecome download: WKDownload) {
		download.delegate = self
	}

	func webView(_ webView: WKWebView, navigationResponse: WKNavigationResponse, didBecome download: WKDownload) {
		download.delegate = self
	}

	func webView(_ webView: WKWebView, didCommit navigation: WKNavigation!) {
		spinner.stopAnimating()
		hideUnreachable()
	}

	func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
		spinner.stopAnimating()
		pageChanged()
	}

	func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
		spinner.stopAnimating()
		let error = error as NSError
		// Stopped on purpose: another page started, or it became a download.
		if error.domain == NSURLErrorDomain, error.code == NSURLErrorCancelled { return }
		if error.domain == "WebKitErrorDomain", error.code == 102 { return }
		showUnreachable(error.localizedDescription)
	}

	func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
		spinner.stopAnimating()
	}

	func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
		webView.reload()
	}
}

// MARK: - Windows and dialogs

extension BrowserController: WKUIDelegate {
	/// `window.open()`: the family's pages here, the rest in Safari.
	func webView(
		_ webView: WKWebView,
		createWebViewWith configuration: WKWebViewConfiguration,
		for navigationAction: WKNavigationAction,
		windowFeatures: WKWindowFeatures
	) -> WKWebView? {
		if let url = navigationAction.request.url {
			if isFamily(url) { webView.load(navigationAction.request) } else { UIApplication.shared.open(url) }
		}
		return nil
	}

	func webView(
		_ webView: WKWebView,
		runJavaScriptAlertPanelWithMessage message: String,
		initiatedByFrame frame: WKFrameInfo,
		completionHandler: @escaping () -> Void
	) {
		let alert = UIAlertController(title: nil, message: message, preferredStyle: .alert)
		alert.addAction(UIAlertAction(title: String(localized: "OK"), style: .default) { _ in completionHandler() })
		present(alert, otherwise: completionHandler)
	}

	func webView(
		_ webView: WKWebView,
		runJavaScriptConfirmPanelWithMessage message: String,
		initiatedByFrame frame: WKFrameInfo,
		completionHandler: @escaping (Bool) -> Void
	) {
		let alert = UIAlertController(title: nil, message: message, preferredStyle: .alert)
		alert.addAction(UIAlertAction(title: String(localized: "Cancel"), style: .cancel) { _ in completionHandler(false) })
		alert.addAction(UIAlertAction(title: String(localized: "OK"), style: .default) { _ in completionHandler(true) })
		present(alert) { completionHandler(false) }
	}

	func webView(
		_ webView: WKWebView,
		runJavaScriptTextInputPanelWithPrompt prompt: String,
		defaultText: String?,
		initiatedByFrame frame: WKFrameInfo,
		completionHandler: @escaping (String?) -> Void
	) {
		let alert = UIAlertController(title: nil, message: prompt, preferredStyle: .alert)
		alert.addTextField { $0.text = defaultText }
		alert.addAction(UIAlertAction(title: String(localized: "Cancel"), style: .cancel) { _ in completionHandler(nil) })
		alert.addAction(UIAlertAction(title: String(localized: "OK"), style: .default) { [weak alert] _ in
			completionHandler(alert?.textFields?.first?.text ?? "")
		})
		present(alert) { completionHandler(nil) }
	}

	/// The page waits for its answer, so with something else on screen it gets one at once.
	private func present(_ alert: UIAlertController, otherwise: @escaping () -> Void) {
		guard presentedViewController == nil, view.window != nil else { return otherwise() }
		present(alert, animated: true)
	}
}

// MARK: - Files nolune hands over

extension BrowserController: WKDownloadDelegate {
	func download(
		_ download: WKDownload,
		decideDestinationUsing response: URLResponse,
		suggestedFilename: String,
		completionHandler: @escaping (URL?) -> Void
	) {
		let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
		do {
			try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
		} catch {
			return completionHandler(nil)
		}
		let name = suggestedFilename.isEmpty ? "file" : suggestedFilename
		let file = folder.appendingPathComponent(name, isDirectory: false)
		downloads[download] = file
		completionHandler(file)
	}

	func downloadDidFinish(_ download: WKDownload) {
		guard let file = downloads.removeValue(forKey: download) else { return }
		show(file)
	}

	func download(_ download: WKDownload, didFailWithError error: Error, resumeData: Data?) {
		downloads.removeValue(forKey: download)
	}
}

/// The file Quick Look shows, with its button to share or save it.
private final class Preview: NSObject, QLPreviewControllerDataSource {
	let file: URL

	init(_ file: URL) {
		self.file = file
	}

	func numberOfPreviewItems(in controller: QLPreviewController) -> Int { 1 }

	func previewController(_ controller: QLPreviewController, previewItemAt index: Int) -> QLPreviewItem {
		file as NSURL
	}
}

/**
 * The page's `nolune` messages, passed on without the web view keeping the controller (it keeps
 * what it sends messages to). Only from the family's own page, not a frame from elsewhere.
 */
private final class ScriptHandler: NSObject, WKScriptMessageHandler {
	private weak var controller: BrowserController?

	init(_ controller: BrowserController) {
		self.controller = controller
	}

	func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
		guard message.frameInfo.isMainFrame,
			let body = message.body as? [String: Any],
			let type = body["type"] as? String
		else { return }
		let value = body["value"] as? String
		let controller = controller
		Task { @MainActor in controller?.received(type, value) }
	}
}

// MARK: - A note above the page

/// One line, like "Update nolune for the full app", with a × that closes it.
private final class NoteBar: UIView {
	var close: (() -> Void)?

	init(_ text: String) {
		super.init(frame: .zero)
		backgroundColor = .secondarySystemBackground
		let label = UILabel()
		label.text = text
		label.font = .preferredFont(forTextStyle: .footnote)
		label.adjustsFontForContentSizeCategory = true
		label.textColor = .secondaryLabel
		label.numberOfLines = 0
		let button = UIButton(type: .close)
		button.addAction(UIAction { [weak self] _ in self?.close?() }, for: .primaryActionTriggered)
		button.setContentHuggingPriority(.required, for: .horizontal)
		let row = UIStackView(arrangedSubviews: [label, button])
		row.alignment = .center
		row.spacing = 12
		row.isLayoutMarginsRelativeArrangement = true
		row.directionalLayoutMargins = NSDirectionalEdgeInsets(top: 6, leading: 16, bottom: 6, trailing: 10)
		row.translatesAutoresizingMaskIntoConstraints = false
		addSubview(row)
		NSLayoutConstraint.activate([
			row.topAnchor.constraint(equalTo: topAnchor),
			row.leadingAnchor.constraint(equalTo: leadingAnchor),
			row.trailingAnchor.constraint(equalTo: trailingAnchor),
			row.bottomAnchor.constraint(equalTo: bottomAnchor)
		])
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("not from a storyboard")
	}
}

// MARK: - The page's color

extension UIColor {
	/// The web app's background (packages/web/src/routes/layout.css), until the page says its own.
	static let page = UIColor { traits in
		traits.userInterfaceStyle == .dark ? UIColor(red: 0x21 / 255, green: 0x21 / 255, blue: 0x21 / 255, alpha: 1) : .white
	}

	/// A color as `getComputedStyle` gives it, `rgb(33, 33, 33)`, when it's opaque; nil otherwise.
	convenience init?(css: String) {
		guard css.hasPrefix("rgb") else { return nil }
		let numbers = css.split(whereSeparator: { !"0123456789.".contains($0) }).compactMap { Double($0) }
		guard numbers.count >= 3, numbers.count < 4 || numbers[3] > 0.99 else { return nil }
		self.init(red: numbers[0] / 255, green: numbers[1] / 255, blue: numbers[2] / 255, alpha: 1)
	}

	func isDark(in traits: UITraitCollection) -> Bool {
		var red: CGFloat = 0, green: CGFloat = 0, blue: CGFloat = 0, alpha: CGFloat = 0
		guard resolvedColor(with: traits).getRed(&red, green: &green, blue: &blue, alpha: &alpha) else { return false }
		return 0.299 * red + 0.587 * green + 0.114 * blue < 0.5
	}
}

extension Bundle {
	var version: String {
		infoDictionary?["CFBundleShortVersionString"] as? String ?? "1.0"
	}
}
