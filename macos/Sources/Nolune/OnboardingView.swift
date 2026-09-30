import AppKit
import SwiftUI

/// The onboarding window: the sky behind, one screen at a time in front.
struct OnboardingView: View {
	static let size = CGSize(width: 760, height: 520)

	@ObservedObject var onboarding: Onboarding
	private let layout = IntroLayout(size: OnboardingView.size)

	var body: some View {
		ZStack {
			SkyView(sky: onboarding.sky)
				// Quieter behind the questions, as on the web.
				.opacity(Onboarding.steps.contains(onboarding.phase) ? 0.6 : 1)
				.animation(.easeInOut(duration: 0.7), value: onboarding.phase)

			screen
				.id(onboarding.phase)
				.transition(.asymmetric(insertion: .opacity.combined(with: .offset(y: 16)), removal: .opacity))

			header

			// Esc skips the intro.
			Button("") { onboarding.skipIntro() }
				.keyboardShortcut(.cancelAction)
				.opacity(0)
				.allowsHitTesting(false)
		}
		.frame(width: Self.size.width, height: Self.size.height)
		.background(Theme.space)
		.preferredColorScheme(.dark)
		.task { await onboarding.startIntro(layout: layout) }
	}

	@ViewBuilder private var screen: some View {
		switch onboarding.phase {
		case .intro: IntroView(onboarding: onboarding, layout: layout)
		case .welcome: WelcomeScreen(onboarding: onboarding)
		case .account: AccountScreen(onboarding: onboarding)
		case .files: FilesScreen(onboarding: onboarding)
		case .service: ServiceScreen(onboarding: onboarding)
		}
	}

	private var header: some View {
		VStack {
			ZStack {
				if let index = Onboarding.steps.firstIndex(of: onboarding.phase) {
					StepBars(count: Onboarding.steps.count, lit: index + 1)
						.transition(.opacity)
				}
				HStack {
					Spacer()
					SpeakerButton(music: onboarding.music)
				}
			}
			.frame(height: 52)
			.padding(.horizontal, 14)
			Spacer()
		}
	}
}

// MARK: Screens

/// "Let's set up nolune.": where the intro lands.
private struct WelcomeScreen: View {
	@ObservedObject var onboarding: Onboarding

	var body: some View {
		VStack(spacing: 18) {
			Text("Let's set up nolune.")
				.font(Theme.font(36, weight: 600))
				.foregroundStyle(Theme.foreground)
				.flyIn()
			Text("A few steps, and your family can start chatting.")
				.font(Theme.font(16))
				.foregroundStyle(Theme.muted)
				.flyIn(0.08)
			VStack(spacing: 12) {
				Button {
					onboarding.begin()
				} label: {
					HStack(spacing: 8) {
						Text("Get started")
						Image(systemName: "arrow.right")
					}
				}
				.buttonStyle(PillButtonStyle(large: true))
				.keyboardShortcut(.defaultAction)
				Text("Takes about a minute")
					.font(Theme.font(12))
					.foregroundStyle(Theme.muted)
			}
			.padding(.top, 10)
			.flyIn(0.16)
		}
		.multilineTextAlignment(.center)
	}
}

/// "Who's setting this up?": the admin account, as `nolune setup` makes it.
private struct AccountScreen: View {
	@ObservedObject var onboarding: Onboarding

	var body: some View {
		VStack(spacing: 0) {
			StepTitle(
				title: "Who's setting this up?",
				subtitle: "You'll be the admin. Add your family after."
			)
			.padding(.bottom, 26)
			VStack(spacing: 10) {
				WelcomeField(placeholder: "Your name", text: $onboarding.name)
				WelcomeField(placeholder: "Email", text: $onboarding.email)
				HStack(spacing: 8) {
					WelcomeField(placeholder: "Password", text: $onboarding.password, monospaced: true)
					CopyButton(text: onboarding.password)
				}
				Text("You'll sign in with this. Your browser can save it.")
					.font(Theme.font(12))
					.foregroundStyle(Theme.muted)
					.frame(maxWidth: .infinity, alignment: .leading)
					.padding(.leading, 4)
			}
			.frame(width: 340)
			if let problem = onboarding.problem {
				Text(problem)
					.font(Theme.font(13))
					.foregroundStyle(Color(hex: 0xF87171))
					.multilineTextAlignment(.center)
					.frame(maxWidth: 420)
					.padding(.top, 12)
			}
			Button {
				Task { await onboarding.createAccount() }
			} label: {
				if onboarding.working {
					ProgressView().controlSize(.small).tint(Theme.primaryForeground)
				} else {
					Text("Continue")
				}
			}
			.buttonStyle(PillButtonStyle())
			.disabled(!onboarding.canCreateAccount)
			.keyboardShortcut(.defaultAction)
			.padding(.top, 22)
		}
		.padding(.top, 20)
		.task { await onboarding.checkAccount() }
	}
}

/// "Let nolune see your files.": Full Disk Access, and the switch that flips with the real one.
private struct FilesScreen: View {
	@ObservedObject var onboarding: Onboarding

	var body: some View {
		let granted = onboarding.granted
		VStack(spacing: 0) {
			StepTitle(
				title: granted ? "Got it." : "Let nolune see your files.",
				subtitle: granted
					? "nolune can reach Documents, Desktop, Photos and Mail now."
					: "Turn on nolune in the list. We'll notice right away."
			)
			.id(granted)
			.transition(.opacity)
			AccessSwitch(on: granted)
				.padding(.top, 34)
			HStack(spacing: 8) {
				PulsingDot(color: granted ? Color(hex: 0x4ADE80) : Theme.muted, pulsing: !granted)
				Text(granted ? "Access granted" : "Waiting for the switch…")
			}
			.font(Theme.font(13))
			.foregroundStyle(Theme.muted)
			.padding(.top, 16)
			VStack(spacing: 14) {
				Button("Open System Settings") { DiskAccess.openSettings() }
					.buttonStyle(PillButtonStyle())
				HStack(spacing: 10) {
					DraggableAppIcon()
					Text("Not in the list? Drag this into it.")
						.font(Theme.font(12))
						.foregroundStyle(Theme.muted)
				}
				Button("Skip for now") { onboarding.skipAccess() }
					.buttonStyle(QuietLinkStyle())
			}
			.padding(.top, 30)
			.opacity(granted ? 0 : 1)
			.animation(.easeOut(duration: 0.3), value: granted)
		}
		.padding(.top, 36)
		.task { await onboarding.watchAccess() }
	}
}

/// The gateway starting in the background, then "You're all set."
private struct ServiceScreen: View {
	@ObservedObject var onboarding: Onboarding

	var body: some View {
		VStack(spacing: 0) {
			switch onboarding.service {
			case .starting:
				StepTitle(
					title: "Starting nolune…",
					subtitle: "From now on it runs in the background, even with this window closed."
				)
				ProgressView()
					.controlSize(.small)
					.padding(.top, 26)
			case .needsApproval:
				StepTitle(
					title: "One more switch.",
					subtitle: "Allow nolune under Login Items, so it can run in the background."
				)
				Button("Open Login Items") { Service.openLoginItems() }
					.buttonStyle(PillButtonStyle())
					.padding(.top, 26)
			case let .failed(message):
				StepTitle(title: "nolune didn't start.", subtitle: message)
				HStack(spacing: 10) {
					Button("Try again") { Task { await onboarding.startService() } }
						.buttonStyle(PillButtonStyle())
					Button("View the log") { NSWorkspace.shared.open(Runtime.shared.logFile) }
						.buttonStyle(OutlineButtonStyle())
				}
				.padding(.top, 26)
			case .ready:
				StepTitle(title: "You're all set.", subtitle: "nolune starts with your Mac. It lives up here ↗")
				Button {
					onboarding.openNolune()
				} label: {
					HStack(spacing: 8) {
						Text("Open nolune")
						Image(systemName: "arrow.right")
					}
				}
				.buttonStyle(PillButtonStyle(large: true))
				.keyboardShortcut(.defaultAction)
				.padding(.top, 26)
			}
		}
		.transition(.opacity)
		.task { await onboarding.startService() }
	}
}

// MARK: Pieces

private struct StepTitle: View {
	let title: String
	let subtitle: String

	var body: some View {
		VStack(spacing: 12) {
			Text(title)
				.font(Theme.font(34, weight: 600))
				.foregroundStyle(Theme.foreground)
			Text(subtitle)
				.font(Theme.font(15))
				.foregroundStyle(Theme.muted)
				.frame(maxWidth: 440)
		}
		.multilineTextAlignment(.center)
	}
}

/// The big switch: off while it waits, lit with the eight colors once access is on.
private struct AccessSwitch: View {
	let on: Bool

	private static let lit = LinearGradient(
		colors: [1, 2, 4, 0, 5, 3, 7].map { Theme.avatars[$0] },
		startPoint: .leading,
		endPoint: .trailing
	)

	var body: some View {
		ZStack(alignment: on ? .trailing : .leading) {
			Capsule()
				.fill(on ? AnyShapeStyle(AccessSwitch.lit) : AnyShapeStyle(Color.white.opacity(0.08)))
				.overlay(Capsule().strokeBorder(Color.white.opacity(on ? 0.3 : 0.15)))
			Circle()
				.fill(Theme.foreground)
				.shadow(color: .black.opacity(0.35), radius: 4, y: 2)
				.padding(5)
		}
		.frame(width: 112, height: 60)
		.shadow(color: on ? Theme.avatars[3].opacity(0.55) : .clear, radius: 26)
	}
}

private struct PulsingDot: View {
	let color: Color
	let pulsing: Bool
	@State private var dim = false

	var body: some View {
		Circle()
			.fill(color)
			.frame(width: 7, height: 7)
			.opacity(pulsing && dim ? 0.35 : 1)
			.onAppear {
				withAnimation(.easeInOut(duration: 0.9).repeatForever()) { dim = true }
			}
	}
}

/// The app's icon, to drag into the Full Disk Access list when it isn't there by itself.
private struct DraggableAppIcon: View {
	var body: some View {
		Image(nsImage: NSApp.applicationIconImage)
			.resizable()
			.frame(width: 40, height: 40)
			.onDrag { NSItemProvider(object: Bundle.main.bundleURL as NSURL) }
			.help("Drag into the Full Disk Access list")
	}
}

private struct CopyButton: View {
	let text: String
	@State private var copied = false

	var body: some View {
		Button {
			NSPasteboard.general.clearContents()
			NSPasteboard.general.setString(text, forType: .string)
			copied = true
			DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { copied = false }
		} label: {
			Image(systemName: copied ? "checkmark" : "doc.on.doc")
				.font(.system(size: 13))
				.foregroundStyle(Theme.muted)
				.frame(width: 44, height: 44)
				.background(RoundedRectangle(cornerRadius: 12).fill(Theme.card.opacity(0.7)))
				.overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Theme.input))
				.contentShape(Rectangle())
		}
		.buttonStyle(.plain)
		.help("Copy")
	}
}

/// Sound on or off, as the web welcome's speaker button.
private struct SpeakerButton: View {
	let music: Music
	@State private var muted = false

	var body: some View {
		Button {
			muted.toggle()
			music.muted = muted
		} label: {
			Image(systemName: muted ? "speaker.slash" : "speaker.wave.2")
				.font(.system(size: 13))
				.foregroundStyle(Theme.muted)
				.frame(width: 28, height: 28)
				.contentShape(Rectangle())
		}
		.buttonStyle(.plain)
		.help(muted ? "Turn sound on" : "Turn sound off")
		.onAppear { muted = music.muted }
	}
}

/// Comes up from a little below, as the web welcome's lines do.
private struct FlyIn: ViewModifier {
	let delay: Double
	@State private var shown = false

	func body(content: Content) -> some View {
		let visible = shown || Snapshot.active
		content
			.opacity(visible ? 1 : 0)
			.offset(y: visible ? 0 : 14)
			.onAppear {
				withAnimation(.easeOut(duration: 0.6).delay(delay)) { shown = true }
			}
	}
}

extension View {
	fileprivate func flyIn(_ delay: Double = 0) -> some View {
		modifier(FlyIn(delay: delay))
	}
}
