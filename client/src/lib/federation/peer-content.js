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

/**
 * @typedef {{
 *   sender: string;
 *   preface: string;
 *   text: string;
 * }} PeerContent
 */

/**
 * The peer content a chat message carries, or `null` when the message is
 * not one the server framed.
 *
 * @param {string} content
 * @returns {PeerContent | null}
 */
export function peerContent(content) {
	void content;
	return null;
}
