import SwiftUI

/**
 * Skills, natively (#137): the profile's own, those shared by all profiles, and nolune's built-in
 * ones, each switched on or off for this profile, and a group's all at once.
 */
struct SkillsView: View {
	@ObservedObject var family: Family
	let slug: String
	@State private var skills: [Skill]?
	@State private var technical = false
	@State private var problem: String?

	private var client: Client { family.client }

	var body: some View {
		List {
			if let skills {
				if skills.isEmpty {
					Text("No skills yet. When nolune works out how to do something, it can save that as a skill for next time.")
						.foregroundStyle(.secondary)
				}
				if technical, !skills.isEmpty {
					let on = skills.filter(\.enabled)
					Text("\(on.count) of \(skills.count) on · about \(on.map(\.tokens).reduce(0, +)) tokens at the start of every new chat")
						.font(.footnote)
						.foregroundStyle(.secondary)
				}
				ForEach(["profile", "global", "builtin"], id: \.self) { scope in
					let group = skills.filter { $0.scope == scope }
					if !group.isEmpty {
						Section {
							ForEach(group) { skill in row(skill) }
						} header: {
							HStack {
								Text(verbatim: title(scope))
								Spacer()
								if group.count > 1 {
									let allOn = group.allSatisfy(\.enabled)
									Button(allOn ? "Turn all off" : "Turn all on") {
										set(group.map(\.name), enabled: !allOn)
									}
									.font(.caption)
									.textCase(nil)
								}
							}
						}
					}
				}
			} else if problem == nil {
				ProgressView()
					.frame(maxWidth: .infinity)
			}
		}
		.navigationTitle("Skills")
		.refreshable { await load() }
		.task {
			technical = await Preferences.read(client.origin).technical
			await load()
		}
		.alert(problem ?? "", isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } })) {
			Button("OK", role: .cancel) {}
		}
	}

	private func row(_ skill: Skill) -> some View {
		Toggle(isOn: Binding(get: { skill.enabled }, set: { set([skill.name], enabled: $0) })) {
			VStack(alignment: .leading, spacing: 3) {
				HStack {
					Text(verbatim: skill.name)
						.font(.body.weight(.medium))
					if technical {
						Text("~\(skill.tokens) tokens")
							.font(.caption.monospacedDigit())
							.foregroundStyle(.secondary)
					}
				}
				Text(verbatim: skill.description)
					.font(.caption)
					.foregroundStyle(.secondary)
					.lineLimit(3)
			}
		}
	}

	private func title(_ scope: String) -> String {
		switch scope {
		case "profile": return String(localized: "Made for \(family.profile?.name ?? "")")
		case "global": return String(localized: "Shared by all profiles")
		default: return String(localized: "Built into nolune")
		}
	}

	private func load() async {
		do {
			skills = try await client.skills(slug)
		} catch {
			// The screen went away while it loaded: it loads again when it's back.
			if !error.isCancellation { problem = error.localizedDescription }
		}
	}

	private func set(_ names: [String], enabled: Bool) {
		// As it will be, while the nolune is asked.
		skills = skills?.map { names.contains($0.name) ? Skill(name: $0.name, description: $0.description, scope: $0.scope, enabled: enabled, tokens: $0.tokens) : $0 }
		Task {
			do {
				skills = try await client.setSkills(slug, names, enabled: enabled)
			} catch {
				problem = error.localizedDescription
				await load()
			}
		}
	}
}
