import AppIntents

/**
 * "Ask nolune" (#138): from Siri, Shortcuts or Spotlight, a new chat with the question, in the
 * profile last open. The app opens on it as nolune answers (MainView), with its Live Activity.
 */
struct AskNolune: AppIntent {
	static var title: LocalizedStringResource { "Ask nolune" }
	static var description: IntentDescription? { IntentDescription("Starts a chat with nolune with your question.") }
	static var openAppWhenRun: Bool { true }

	@Parameter(title: "Question", requestValueDialog: IntentDialog("What should nolune do?"))
	var question: String

	@MainActor
	func perform() async throws -> some IntentResult {
		AppModel.shared.ask(question)
		return .result()
	}
}

/// What Siri and Spotlight offer without setting anything up.
struct NoluneShortcuts: AppShortcutsProvider {
	static var appShortcuts: [AppShortcut] {
		AppShortcut(
			intent: AskNolune(),
			phrases: [
				"Ask \(.applicationName)",
				"Start a chat in \(.applicationName)",
			]
		)
	}
}
