import Foundation

/**
 * Answers the app's requests from a table of URLs instead of the network: a status, headers and
 * a body, or a redirect, which the session follows with another request here. Anything else is a
 * 404. It keeps the requests it was sent, with their bodies.
 */
final class Answers: URLProtocol {
	struct Answer {
		var status = 200
		var headers: [String: String] = [:]
		var body = Data()
		var location: String?

		static func json(_ text: String, status: Int = 200, headers: [String: String] = [:]) -> Answer {
			Answer(status: status, headers: headers, body: Data(text.utf8))
		}

		static func redirect(to location: String, status: Int = 308) -> Answer {
			Answer(status: status, location: location)
		}
	}

	struct Sent {
		let method: String
		let url: String
		let headers: [String: String]
		let body: String
	}

	static var table: [String: Answer] = [:]
	static var sent: [Sent] = []

	/// A session that asks the table, with a cookie storage of its own.
	static func session(_ table: [String: Answer]) -> URLSession {
		self.table = table
		sent = []
		let configuration = URLSessionConfiguration.ephemeral
		configuration.protocolClasses = [Answers.self]
		return URLSession(configuration: configuration)
	}

	override class func canInit(with request: URLRequest) -> Bool { true }
	override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

	override func startLoading() {
		guard let url = request.url else { return }
		Answers.sent.append(
			Sent(
				method: request.httpMethod ?? "GET",
				url: url.absoluteString,
				headers: request.allHTTPHeaderFields ?? [:],
				body: Answers.body(of: request)
			)
		)
		let answer = Answers.table[url.absoluteString] ?? Answer(status: 404)
		var headers = answer.headers
		if let location = answer.location { headers["Location"] = location }
		let response = HTTPURLResponse(url: url, statusCode: answer.status, httpVersion: "HTTP/1.1", headerFields: headers)!
		if let location = answer.location, let target = URL(string: location) {
			client?.urlProtocol(self, wasRedirectedTo: URLRequest(url: target), redirectResponse: response)
			return
		}
		client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
		client?.urlProtocol(self, didLoad: answer.body)
		client?.urlProtocolDidFinishLoading(self)
	}

	override func stopLoading() {}

	/// A request's body: a stream by the time it gets here.
	private static func body(of request: URLRequest) -> String {
		if let data = request.httpBody { return String(decoding: data, as: UTF8.self) }
		guard let stream = request.httpBodyStream else { return "" }
		stream.open()
		defer { stream.close() }
		var data = Data()
		var buffer = [UInt8](repeating: 0, count: 1024)
		while stream.hasBytesAvailable {
			let count = stream.read(&buffer, maxLength: buffer.count)
			if count <= 0 { break }
			data.append(buffer, count: count)
		}
		return String(decoding: data, as: UTF8.self)
	}
}
