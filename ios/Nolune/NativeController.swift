import Combine
import SwiftUI
import UIKit

/**
 * The family's nolune with the native screens, which stay behind a flag until they're
 * whole (`AppModel.native`). It asks the nolune which it is (`/api/version`). One that serves the
 * API for apps gets native sign-in (SignInView), then the native screens (MainView); one from
 * before gets today's web app, with a note to update it. The session is shared with the web app's
 * pages both ways (Cookies.swift), so either can sign in.
 */
final class NativeController: UIViewController {
	enum Screen: Equatable {
		/// Asking the nolune which it is.
		case checking
		case signIn
		/// The native screens, signed in.
		case signedIn
		/// Today's web app: for a nolune before the API for apps, with a note to update it, or
		/// for one that didn't answer (the relay's page for a computer that's off, or no network).
		case web(note: Bool)
	}

	/// Where what a nolune answered at `/api/version` leads.
	nonisolated static func screen(for answer: Result<Server?, Error>) -> Screen {
		switch answer {
		case .success(let server?) where server.isNative:
			return server.capabilities == nil ? .signIn : .signedIn
		case .success:
			return .web(note: true)
		case .failure:
			return .web(note: false)
		}
	}

	private let origin: URL
	private var client: Client { Client(origin: origin) }
	private var model: AppModel { AppModel.shared }
	private var screen = Screen.checking
	/// What the nolune serves apps, signed in: which screens are native.
	private var capabilities: [String] = []
	private var current: UIViewController?
	private let spinner = UIActivityIndicatorView(style: .medium)
	private var activations: AnyCancellable?

	init(origin: URL) {
		self.origin = origin
		super.init(nibName: nil, bundle: nil)
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("not from a storyboard")
	}

	override func viewDidLoad() {
		super.viewDidLoad()
		view.backgroundColor = UIColor(Theme.space)
		spinner.color = UIColor(Theme.muted)
		spinner.translatesAutoresizingMaskIntoConstraints = false
		view.addSubview(spinner)
		NSLayoutConstraint.activate([
			spinner.centerXAnchor.constraint(equalTo: view.centerXAnchor),
			spinner.centerYAnchor.constraint(equalTo: view.centerYAnchor)
		])
		spinner.startAnimating()
		Task { await check() }
		activations = model.$activations
			.dropFirst()
			.receive(on: DispatchQueue.main)
			.sink { [weak self] _ in self?.becameActive() }
	}

	override var preferredStatusBarStyle: UIStatusBarStyle { .lightContent }
	override var childForStatusBarStyle: UIViewController? { current }

	/**
	 * Asks the nolune which it is, as whoever signed in on the app. When nobody has, it shows the
	 * sign-in, then looks at the web views' cookies, which take longer to read (WebKit's own
	 * process): someone signed in on a page, like the web app before the native screens, is
	 * signed in natively too.
	 */
	private func check() async {
		show(Self.screen(for: await answer()))
		guard screen == .signIn, await Cookies.fromWeb(origin, unlessSignedIn: true) else { return }
		if Self.screen(for: await answer()) == .signedIn { show(.signedIn) }
	}

	/// What the nolune says it is, keeping what it serves apps.
	private func answer() async -> Result<Server?, Error> {
		do {
			let server = try await client.version()
			capabilities = server?.capabilities ?? []
			return .success(server)
		} catch {
			return .failure(error)
		}
	}

	/// Back to the front: a nolune that didn't answer may now.
	private func becameActive() {
		if screen == .web(note: false) { Task { await check() } }
	}

	private func show(_ next: Screen) {
		guard next != screen else { return }
		screen = next
		let controller: UIViewController
		switch next {
		case .checking:
			return
		case .signIn:
			controller = UIHostingController(
				onboarding: SignInView(
					address: Address.display(origin),
					signIn: { [weak self] email, password in await self?.signIn(email, password) },
					connectElsewhere: { AppModel.shared.disconnect() }
				)
			)
		case .signedIn:
			controller = UIHostingController(
				rootView: MainView(
					family: Family(client: client, capabilities: capabilities),
					signOut: { [weak self] in self?.signedOut() },
					connectElsewhere: { [weak self] in self?.connectElsewhere() }
				)
			)
		case .web(let note):
			let update = String(localized: "Update nolune for the full app")
			controller = BrowserController(origin: origin, note: note ? update : nil)
		}
		spinner.stopAnimating()
		// An invite's sheet, say, once someone turned out to be signed in.
		if next != .signIn, presentedViewController != nil { dismiss(animated: true) }
		let previous = current
		current = controller
		replace(previous, with: controller)
		// An invite link makes the account on its page, over the sign-in.
		if next == .signIn, let path = model.pending, path.hasPrefix("/invite/") {
			_ = model.takePending()
			invite(path)
		}
	}

	/// Signs in, and opens the native screens. Says what went wrong otherwise.
	private func signIn(_ email: String, _ password: String) async -> String? {
		do {
			try await client.signIn(email: email, password: password)
		} catch let problem as Client.Problem {
			switch problem {
			case .wrongPassword: return String(localized: "Wrong email or password.")
			case .tooManyTries: return String(localized: "Too many attempts. Wait a minute and try again.")
			case .refused(let status): return Address(origin: origin).describe(.refused(status))
			}
		} catch {
			return Address(origin: origin).describe(.unreachable(error.localizedDescription))
		}
		await Cookies.toWeb(origin)
		// Signed in, it says what it serves apps.
		_ = await answer()
		show(.signedIn)
		return nil
	}

	/**
	 * Signed out: from the account menu, on a page, or a session that ended. This session too, which
	 * may be another (`POST /logout` is a no-op for one that's gone), then the sign-in.
	 */
	private func signedOut() {
		Task {
			await client.signOut()
			await Cookies.signOut(origin)
			show(.signIn)
		}
	}

	/// Back to the first screen, after telling this nolune to stop sending notifications here.
	private func connectElsewhere() {
		Task {
			await client.forgetPush()
			model.disconnect()
		}
	}

	// MARK: Invites

	private func invite(_ path: String) {
		let page = InviteController(origin: origin, path: path) { [weak self] landed in
			self?.dismiss(animated: true)
			if let landed { self?.joined(landed) }
		}
		let sheet = UINavigationController(rootViewController: page)
		sheet.isModalInPresentation = true
		present(sheet, animated: true)
	}

	/**
	 * The invite made the account, and its page signed in: the app takes the session, and opens
	 * the page it went on to, like the profile's welcome.
	 */
	private func joined(_ path: String) {
		Task {
			await Cookies.fromWeb(origin)
			model.pending = path
			await check()
		}
	}
}
