import UIKit
import UserNotifications

/**
 * nolune for iPhone and iPad: the family's nolune in a window of its own, with the bell's
 * notifications. It asks once which nolune to open (ConnectView), then shows its web app
 * (BrowserController) and, once someone has signed in, registers for notifications (Push).
 */
@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
	private let notifications = NotificationHandler()

	func application(
		_ application: UIApplication,
		didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
	) -> Bool {
		Theme.registerFonts()
		// Before launching ends, so a tap that opened the app reaches it.
		UNUserNotificationCenter.current().delegate = notifications
		return true
	}

	func application(_ application: UIApplication, didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data) {
		AppModel.shared.deviceToken = deviceToken.map { String(format: "%02x", $0) }.joined()
	}

	func application(_ application: UIApplication, didFailToRegisterForRemoteNotificationsWithError error: Error) {
		print("nolune: no notifications: \(error.localizedDescription)")
	}
}

final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
	var window: UIWindow?

	func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options connectionOptions: UIScene.ConnectionOptions) {
		guard let scene = scene as? UIWindowScene else { return }
		let window = UIWindow(windowScene: scene)
		window.rootViewController = RootController()
		window.makeKeyAndVisible()
		self.window = window
		if let url = connectionOptions.urlContexts.first?.url { AppModel.shared.open(link: url) }
	}

	/// A widget or a Live Activity tapped while the app was running: `nolune://open?path=…`.
	func scene(_ scene: UIScene, openURLContexts URLContexts: Set<UIOpenURLContext>) {
		if let url = URLContexts.first?.url { AppModel.shared.open(link: url) }
	}

	func sceneDidBecomeActive(_ scene: UIScene) {
		AppModel.shared.becameActive()
	}
}
