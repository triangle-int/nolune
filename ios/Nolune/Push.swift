import UIKit
import UserNotifications
import WebKit

/**
 * The bell's notifications on this iPhone. Once someone has signed in, the app asks to show them
 * and registers with Apple; the token Apple gives it goes to the family's nolune from the page, as
 * whoever is signed in there (`POST /api/push`, packages/web/src/routes/api/push). Their nolune
 * then sends each new notification through nolune's relay, which passes it on to Apple
 * (packages/core/src/push.ts, packages/relay/src/push.ts). Signing out stops them.
 */
@MainActor
enum Push {
	private static var started = false

	/// Asks to show notifications, the first time, and registers with Apple, once a launch. Not when
	/// launched with `-push NO`, as CI is for its screens, which the question would cover.
	static func start() {
		guard !started, UserDefaults.standard.object(forKey: "push") == nil || UserDefaults.standard.bool(forKey: "push") else { return }
		started = true
		Task {
			let center = UNUserNotificationCenter.current()
			let status = await center.notificationSettings().authorizationStatus
			var allowed = status == .authorized || status == .provisional || status == .ephemeral
			if status == .notDetermined {
				allowed = (try? await center.requestAuthorization(options: [.alert, .sound])) ?? false
			}
			if allowed { UIApplication.shared.registerForRemoteNotifications() }
		}
	}

	/// Registers the token with the nolune the page is on. True once it has it.
	static func register(_ token: String, on webView: WKWebView) async -> Bool {
		let status = await run(
			"""
			const res = await fetch('/api/push', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ token, sandbox })
			});
			return res.status;
			""",
			["token": token, "sandbox": sandbox],
			on: webView
		)
		return (200..<300).contains(status)
	}

	/// Tells the nolune the page is on to stop sending notifications here.
	static func unregister(on webView: WKWebView) async {
		_ = await run("const res = await fetch('/api/push', { method: 'DELETE' }); return res.status;", [:], on: webView)
	}

	/// Runs a function in the page, as it would itself, and gives back the number it returns.
	private static func run(_ body: String, _ arguments: [String: Any], on webView: WKWebView) async -> Int {
		await withCheckedContinuation { continuation in
			webView.callAsyncJavaScript(body, arguments: arguments, in: nil, in: .page) { result in
				switch result {
				case .success(let value): continuation.resume(returning: (value as? NSNumber)?.intValue ?? 0)
				case .failure: continuation.resume(returning: 0)
				}
			}
		}
	}

	/**
	 * Whether Apple's sandbox delivers this build's notifications: a development build's, as its
	 * provisioning profile says, or the simulator's. TestFlight and the App Store's are production.
	 */
	static let sandbox: Bool = {
		#if targetEnvironment(simulator)
		return true
		#else
		guard let url = Bundle.main.url(forResource: "embedded", withExtension: "mobileprovision"),
			let data = try? Data(contentsOf: url),
			let text = String(data: data, encoding: .isoLatin1)
		else { return false }
		return text.range(
			of: "<key>aps-environment</key>\\s*<string>development</string>",
			options: .regularExpression
		) != nil
		#endif
	}()
}

/// Notifications that arrive while the app is open, and taps on them.
final class NotificationHandler: NSObject, UNUserNotificationCenterDelegate {
	/// Shown as well while the app is open: the bell is easy to miss.
	func userNotificationCenter(
		_ center: UNUserNotificationCenter,
		willPresent notification: UNNotification,
		withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
	) {
		completionHandler([.banner, .list, .sound])
	}

	/// Tapped: the bell opens with it (`?notification=<id>`), on the nolune it came from.
	func userNotificationCenter(
		_ center: UNUserNotificationCenter,
		didReceive response: UNNotificationResponse,
		withCompletionHandler completionHandler: @escaping () -> Void
	) {
		let info = response.notification.request.content.userInfo
		let path = info["path"] as? String
		let origin = (info["origin"] as? String).flatMap(URL.init(string:))
		Task { @MainActor in
			AppModel.shared.open(path: path, from: origin)
			completionHandler()
		}
	}
}
