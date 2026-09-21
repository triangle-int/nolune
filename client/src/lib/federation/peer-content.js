// @ts-check
/**
 * Peer content in the conversation (#110): a message a paired companion
 * delivered enters the chat as one user-role message whose text sits
 * inside the server's untrusted block. This helper finds that block so the
 * bubble can show the text as data from a named companion, never as the
 * owner's words, never as markdown or HTML. Pure: no DOM, no storage.
 */

/** Opening line of the server's untrusted block, before the sender. */
export const PEER_CONTENT_OPEN = "<<<UNTRUSTED PEER CONTENT";
/** Closing line of the block, before the boundary. */
export const PEER_CONTENT_CLOSE = "<<<END UNTRUSTED PEER CONTENT";

/** The opening line: the sender's id and the 32-hex boundary the server drew. */
const OPEN_LINE = /^<<<UNTRUSTED PEER CONTENT from companion (\S+); [^\n]*boundary ([0-9a-f]{32})>>>$/;

/**
 * @typedef {{
 *   sender: string;
 *   preface: string;
 *   text: string;
 * }} PeerContent
 */

/**
 * The peer content a chat message carries, or `null` when the message is
 * not one the server framed: an opening line naming the sender and a
 * boundary, the text, and the one closing line carrying that same
 * boundary as the very last line. Anything else (no closing line, another
 * boundary, text after the close) is not a block and renders as the plain
 * message it is. The text between the lines is returned as sent: the
 * peer's own forged closing line, if any, is part of it.
 *
 * @param {string} content
 * @returns {PeerContent | null}
 */
export function peerContent(content) {
	if (!content) return null;
	const lines = content.replace(/\n$/, "").split("\n");
	const open = lines.findIndex((line) => OPEN_LINE.test(line));
	if (open === -1) return null;
	const match = OPEN_LINE.exec(lines[open]);
	if (!match) return null;
	const [, sender, boundary] = match;
	const close = lines.length - 1;
	if (close <= open || lines[close] !== `${PEER_CONTENT_CLOSE} boundary ${boundary}>>>`) return null;
	return {
		sender,
		preface: lines.slice(0, open).join("\n").trim(),
		text: lines.slice(open + 1, close).join("\n"),
	};
}
