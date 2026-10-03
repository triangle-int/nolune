import Combine
import SwiftUI
import UIKit

/// The connect screen until there's a nolune to open, then its web app (or, with the native screens
/// on, NativeController); the status bar follows.
final class RootController: UIViewController {
	private var current: UIViewController?
	private var subscription: AnyCancellable?

	override func viewDidLoad() {
		super.viewDidLoad()
		view.backgroundColor = UIColor(Theme.space)
		// Called at once with the nolune there is, then whenever it changes.
		subscription = AppModel.shared.$origin
			.removeDuplicates()
			.sink { [weak self] origin in self?.show(origin) }
	}

	private func show(_ origin: URL?) {
		let next: UIViewController
		if let origin {
			next = AppModel.shared.native ? NativeController(origin: origin) : BrowserController(origin: origin)
		} else {
			next = UIHostingController(onboarding: ConnectView().environmentObject(AppModel.shared))
		}
		let previous = current
		current = next
		replace(previous, with: next)
	}

	override var childForStatusBarStyle: UIViewController? { current }
	override var childForHomeIndicatorAutoHidden: UIViewController? { current }
}

extension UIViewController {
	/// Shows `next` in place of `previous`, filling the screen, cross-dissolving when there was one.
	func replace(_ previous: UIViewController?, with next: UIViewController) {
		previous?.willMove(toParent: nil)
		addChild(next)
		next.view.frame = view.bounds
		next.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
		let swap = {
			previous?.view.removeFromSuperview()
			self.view.addSubview(next.view)
		}
		if previous == nil {
			swap()
		} else {
			UIView.transition(with: view, duration: 0.3, options: .transitionCrossDissolve, animations: swap)
		}
		previous?.removeFromParent()
		next.didMove(toParent: self)
		setNeedsStatusBarAppearanceUpdate()
	}
}

extension UIHostingController {
	/// A screen that wears the onboarding (ConnectView's): dark, on deep space.
	convenience init(onboarding screen: Content) {
		self.init(rootView: screen)
		overrideUserInterfaceStyle = .dark
		view.backgroundColor = UIColor(Theme.space)
	}
}
