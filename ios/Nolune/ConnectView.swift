import SwiftUI

/**
 * The first screen: which nolune to open. The family's name on nolune's relay will do (`smiths`),
 * or its address, or any link from it, like an invite. It wears the macOS app's onboarding: its
 * sky (Sky.swift), type and controls (Theme.swift).
 */
struct ConnectView: View {
	@EnvironmentObject private var model: AppModel
	@Environment(\.accessibilityReduceMotion) private var reduceMotion
	@State private var text = ""
	@State private var checking = false
	@State private var problem: String?
	@State private var sky: Sky = {
		let sky = Sky()
		sky.stage = .aurora
		return sky
	}()

	private var address: Address? { Address(text) }

	var body: some View {
		ZStack {
			Theme.space.ignoresSafeArea()
			SkyView(sky: sky)
				.opacity(0.7)
				.ignoresSafeArea()
			ScrollView {
				VStack(spacing: 32) {
					Text(verbatim: "nolune")
						.font(Theme.font(44, weight: 600))
						.padding(.top, 72)
					VStack(spacing: 10) {
						Text("Connect to your family's nolune")
							.font(Theme.font(22, weight: 500))
						Text("Type its name or address, or paste a link someone sent you from it.")
							.font(Theme.font(15))
							.foregroundStyle(Theme.muted)
					}
					.multilineTextAlignment(.center)
					field
					Button(action: connect) {
						if checking {
							ProgressView().tint(Theme.primaryForeground)
						} else {
							Text("Connect")
						}
					}
					.buttonStyle(PillButtonStyle(large: true))
					.disabled(address == nil || checking)
					if let previous = model.previous {
						Button("Back to \(Address.display(previous))") {
							model.connect(to: Address(origin: previous))
						}
						.buttonStyle(QuietLinkStyle())
					}
					HStack(spacing: 20) {
						Link("What's nolune?", destination: URL(string: "https://nolune.dev")!)
						Link("Privacy", destination: URL(string: "https://nolune.dev/privacy")!)
					}
					.font(Theme.font(13))
					.foregroundStyle(Theme.muted)
					.padding(.top, 24)
				}
				.foregroundStyle(Theme.foreground)
				.padding(.horizontal, 24)
				.padding(.bottom, 32)
				.frame(maxWidth: 440)
				.frame(maxWidth: .infinity)
			}
			.scrollDismissesKeyboard(.interactively)
		}
		.onAppear { sky.still = reduceMotion }
	}

	private var field: some View {
		VStack(alignment: .leading, spacing: 10) {
			WelcomeField(placeholder: "smiths.nolune.family", text: $text)
				.keyboardType(.URL)
				.textContentType(.URL)
				.textInputAutocapitalization(.never)
				.autocorrectionDisabled()
				.submitLabel(.go)
				.onSubmit(connect)
				.onChange(of: text) { _ in problem = nil }
			Group {
				if let problem {
					Text(problem).foregroundStyle(Color(hex: 0xFD767B))
				} else if let address, address.isOnRelay, !text.contains(".") {
					// A name: where it leads.
					Text("Opens \(address.display)").foregroundStyle(Theme.muted)
				}
			}
			.font(Theme.font(13))
			.padding(.horizontal, 16)
			.fixedSize(horizontal: false, vertical: true)
		}
	}

	private func connect() {
		guard let address, !checking else { return }
		checking = true
		problem = nil
		Task {
			let result = await address.check()
			checking = false
			switch result {
			case .success(let answered): model.connect(to: answered)
			case .failure(let found): problem = address.describe(found)
			}
		}
	}
}
