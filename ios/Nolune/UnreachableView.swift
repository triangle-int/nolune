import SwiftUI

/// When the family's nolune can't be reached at all: no network, or no such address any more.
struct UnreachableView: View {
	let address: String
	let detail: String
	let retry: () -> Void
	let connectElsewhere: () -> Void
	@State private var sky: Sky = {
		let sky = Sky()
		sky.stage = .aurora
		sky.still = true
		return sky
	}()

	var body: some View {
		ZStack {
			Theme.space.ignoresSafeArea()
			SkyView(sky: sky)
				.opacity(0.5)
				.ignoresSafeArea()
			VStack(spacing: 20) {
				VStack(spacing: 10) {
					Text("Can't reach \(address)")
						.font(Theme.font(22, weight: 500))
					Text(detail)
						.font(Theme.font(15))
						.foregroundStyle(Theme.muted)
				}
				.multilineTextAlignment(.center)
				Button("Try again", action: retry)
					.buttonStyle(PillButtonStyle(large: true))
				Button("Connect to another nolune", action: connectElsewhere)
					.buttonStyle(QuietLinkStyle())
			}
			.foregroundStyle(Theme.foreground)
			.padding(.horizontal, 24)
			.frame(maxWidth: 440)
		}
	}
}
