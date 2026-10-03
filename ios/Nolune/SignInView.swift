import SwiftUI

/**
 * Signing in, natively: the account on the family's nolune, as its sign-in page asks for it
 * (packages/web/src/routes/login), in the connect screen's clothes. `signIn` gives back what went
 * wrong, in words, or nil once someone is signed in.
 */
struct SignInView: View {
	let address: String
	let signIn: (_ email: String, _ password: String) async -> String?
	let connectElsewhere: () -> Void
	@Environment(\.accessibilityReduceMotion) private var reduceMotion
	@State private var email = ""
	@State private var password = ""
	@State private var working = false
	@State private var problem: String?
	@FocusState private var focus: Field?
	@State private var sky: Sky = {
		let sky = Sky()
		sky.stage = .aurora
		return sky
	}()

	private enum Field {
		case email, password
	}

	private var ready: Bool {
		!email.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !password.isEmpty && !working
	}

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
						Text("Welcome back")
							.font(Theme.font(22, weight: 500))
						Text("Sign in with the account you were given.")
							.font(Theme.font(15))
							.foregroundStyle(Theme.muted)
						Text(verbatim: address)
							.font(Theme.font(13))
							.foregroundStyle(Theme.muted)
					}
					.multilineTextAlignment(.center)
					fields
					Button(action: submit) {
						if working {
							ProgressView().tint(Theme.primaryForeground)
						} else {
							Text("Continue")
						}
					}
					.buttonStyle(PillButtonStyle(large: true))
					.disabled(!ready)
					Text("Forgot your password? Ask an admin to reset it under People, or to run \(Text(verbatim: "nolune user passwd").font(Theme.mono(12))).")
						.font(Theme.font(12))
						.foregroundStyle(Theme.muted)
						.multilineTextAlignment(.center)
					Button("Connect to another nolune", action: connectElsewhere)
						.buttonStyle(QuietLinkStyle())
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

	private var fields: some View {
		VStack(alignment: .leading, spacing: 10) {
			WelcomeField(placeholder: String(localized: "Email address"), text: $email)
				.keyboardType(.emailAddress)
				.textContentType(.username)
				.textInputAutocapitalization(.never)
				.autocorrectionDisabled()
				.submitLabel(.next)
				.focused($focus, equals: .email)
				.onSubmit { focus = .password }
			WelcomeField(placeholder: String(localized: "Password"), text: $password, secure: true)
				.textContentType(.password)
				.submitLabel(.go)
				.focused($focus, equals: .password)
				.onSubmit(submit)
			if let problem {
				Text(problem)
					.font(Theme.font(13))
					.foregroundStyle(Color(hex: 0xFD767B))
					.padding(.horizontal, 16)
					.fixedSize(horizontal: false, vertical: true)
			}
		}
		.onChange(of: email) { _ in problem = nil }
		.onChange(of: password) { _ in problem = nil }
	}

	private func submit() {
		guard ready else { return }
		working = true
		problem = nil
		focus = nil
		Task {
			problem = await signIn(email, password)
			working = false
		}
	}
}
