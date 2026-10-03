import Foundation

/// Where the native screens go: what the sidebar lists, and where links lead.
enum Destination: Hashable {
	case newChat
	case chat(String)
	case folder(String)
	case page(WebPage)
	case settings
	/// Any other page of the family's nolune, like a profile's welcome after an invite.
	case web(String)

	/**
	 * Where an address on the family's nolune leads natively, with the profile it's in (nil for
	 * pages outside one). Nil for what the app doesn't show itself, which stays on the web.
	 */
	static func of(path: String) -> (slug: String?, destination: Destination)? {
		let parts = path.split(separator: "/").map(String.init)
		switch parts.count {
		case 1:
			switch parts[0] {
			case "profiles": return (nil, .page(.profiles))
			case "card": return (nil, .page(.card))
			case "admin": return (nil, .page(.modelsAndKeys))
			default: return nil
			}
		case 2 where parts[0] == "admin":
			switch parts[1] {
			case "services": return (nil, .page(.services))
			case "people": return (nil, .page(.adminPeople))
			default: return nil
			}
		case 2 where parts[0] == "p":
			return (parts[1], .newChat)
		case 3 where parts[0] == "p":
			let page: WebPage?
			switch parts[2] {
			case "images": page = .images
			case "automations": page = .automations
			case "memory": page = .memory
			case "skills": page = .skills
			case "settings": page = .people
			default: page = nil
			}
			return page.map { (parts[1], .page($0)) }
		case 4 where parts[0] == "p" && parts[2] == "c":
			return (parts[1], .chat(parts[3]))
		case 4 where parts[0] == "p" && parts[2] == "f":
			return (parts[1], .folder(parts[3]))
		default:
			return nil
		}
	}
}

/**
 * The web app's pages the native screens show inside them (BrowserController's `page:`) until
 * they're native themselves.
 */
enum WebPage: String, Hashable, CaseIterable {
	case images, automations, memory, skills, people
	case profiles, card, modelsAndKeys, services, adminPeople

	/// The profile's own pages, which the sidebar lists.
	static let profilePages: [WebPage] = [.images, .automations, .memory, .skills, .people]

	func path(in slug: String) -> String {
		switch self {
		case .images, .automations, .memory, .skills: return "/p/\(slug)/\(rawValue)"
		case .people: return "/p/\(slug)/settings"
		case .profiles: return "/profiles"
		case .card: return "/card"
		case .modelsAndKeys: return "/admin"
		case .services: return "/admin/services"
		case .adminPeople: return "/admin/people"
		}
	}

	var title: String {
		switch self {
		case .images: return String(localized: "Images")
		case .automations: return String(localized: "Automations")
		case .memory: return String(localized: "Memory")
		case .skills: return String(localized: "Skills")
		case .people: return String(localized: "People & profile")
		case .profiles: return String(localized: "All profiles")
		case .card: return String(localized: "Your card")
		case .modelsAndKeys: return String(localized: "Models & keys")
		case .services: return String(localized: "Connected services")
		case .adminPeople: return String(localized: "People")
		}
	}

	/// SF Symbols for the web sidebar's Lucide icons.
	var symbol: String {
		switch self {
		case .images: return "photo.on.rectangle"
		case .automations: return "clock"
		case .memory: return "brain"
		case .skills: return "puzzlepiece.extension"
		case .people: return "person.2"
		case .profiles: return "person.3"
		case .card: return "person.text.rectangle"
		case .modelsAndKeys: return "shippingbox"
		case .services: return "powerplug"
		case .adminPeople: return "person.crop.rectangle.stack"
		}
	}
}
