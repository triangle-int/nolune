import Combine
import SwiftUI
import UIKit

/// The connect screen until there's a nolune to open, then its web app; the status bar follows.
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
			next = BrowserController(origin: origin)
		} else {
			let connect = UIHostingController(rootView: ConnectView().environmentObject(AppModel.shared))
			connect.overrideUserInterfaceStyle = .dark
			connect.view.backgroundColor = UIColor(Theme.space)
			next = connect
		}
		let previous = current
		previous?.willMove(toParent: nil)
		addChild(next)
		next.view.frame = view.bounds
		next.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
		current = next
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

	override var childForStatusBarStyle: UIViewController? { current }
	override var childForHomeIndicatorAutoHidden: UIViewController? { current }
}
