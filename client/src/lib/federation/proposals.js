// @ts-check
/**
 * Pure helpers for proposals from paired companions (#111): what a peer
 * proposed (a meeting, a reminder, a task handed over), as
 * `GET /api/federation/proposals` lists it, viewed for the Activity page.
 * The server's record is the only source. The details are the peer's own
 * words and are returned as such, for the card to show as plain text
 * marked as theirs; nothing here renders markup, reads a store, or talks
 * to the API. Where a proposal stands is read against the clock the caller
 * passes, so an open one past its deadline reads as expired before the
 * server says so.
 */

import { relativeTime } from "../activity/receipts.js";
import { shortId } from "./companions.js";

/**
 * @typedef {import("../api/types.js").FederationPeerProposal} FederationPeerProposal
 * @typedef {import("../api/types.js").FederationProposalStatus} FederationProposalStatus
 * @typedef {import("../api/types.js").FederationTaskHandoff} FederationTaskHandoff
 * @typedef {{ title: string; items: string[]; code?: boolean }} HandoffSection
 */

/** @param {number} unixSeconds */
function momentLabel(unixSeconds) {
	return new Date(unixSeconds * 1000).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

/** @param {number} unixSeconds */
function timeLabel(unixSeconds) {
	return new Date(unixSeconds * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/**
 * "in 3h", "in 2d", or "now" for a moment ahead of the clock.
 * @param {number} unixSeconds
 * @param {number} nowSeconds
 */
function untilLabel(unixSeconds, nowSeconds) {
	const diff = unixSeconds - nowSeconds;
	if (diff <= 0) return "now";
	if (diff < 60) return `in ${diff}s`;
	if (diff < 3600) return `in ${Math.floor(diff / 60)}m`;
	if (diff < 86400) return `in ${Math.floor(diff / 3600)}h`;
	return `in ${Math.floor(diff / 86400)}d`;
}

/**
 * Where the proposal stands at `nowSeconds`: the server's status, except
 * that an open proposal past its deadline reads as expired.
 * @param {FederationPeerProposal} proposal
 * @param {number} nowSeconds
 * @returns {FederationProposalStatus}
 */
export function proposalStatus(proposal, nowSeconds) {
	if (proposal.status === "open" && proposal.expires_at <= nowSeconds) return "expired";
	return proposal.status;
}

/**
 * What was proposed by whom: the kind and the companion, never the words.
 * @param {FederationPeerProposal} proposal
 */
export function proposalLabel(proposal) {
	const peer = shortId(proposal.sender);
	switch (proposal.details.kind) {
		case "meeting":
			return `Meeting proposed by companion ${peer}`;
		case "reminder":
			return `Reminder proposed by companion ${peer}`;
		case "handoff":
			return `Task handed over by companion ${peer}`;
		default:
			return `Proposal from companion ${peer}`;
	}
}

/**
 * When the proposal is for: the meeting's window or the reminder's time;
 * nothing for a task handed over.
 * @param {FederationPeerProposal} proposal
 */
export function proposalWhen(proposal) {
	const details = proposal.details;
	switch (details.kind) {
		case "meeting": {
			const sameDay = new Date(details.window.from * 1000).toDateString() === new Date(details.window.to * 1000).toDateString();
			return `Between ${momentLabel(details.window.from)} and ${sameDay ? timeLabel(details.window.to) : momentLabel(details.window.to)}`;
		}
		case "reminder":
			return `At ${momentLabel(details.at)}`;
		default:
			return "";
	}
}

/**
 * The peer's words for a meeting or a reminder, as sent; a task handed
 * over is laid out by `handoffSections` instead.
 * @param {FederationPeerProposal} proposal
 */
export function proposalText(proposal) {
	const details = proposal.details;
	switch (details.kind) {
		case "meeting":
			return String(details.description ?? "");
		case "reminder":
			return String(details.text ?? "");
		default:
			return "";
	}
}

/**
 * One phrase on where the proposal stands.
 * @param {FederationPeerProposal} proposal
 * @param {number} nowSeconds
 */
export function proposalStatusLabel(proposal, nowSeconds) {
	switch (proposalStatus(proposal, nowSeconds)) {
		case "open":
			return "Needs your decision";
		case "accepted":
			return "Accepted";
		case "dismissed":
			return "Dismissed";
		case "expired":
			return "Expired";
		default:
			return "Unknown";
	}
}

/**
 * Which decisions the owner can still take: both on an open proposal,
 * only dismissing on a lapsed one (to clear it), none once decided.
 * @param {FederationPeerProposal} proposal
 * @param {number} nowSeconds
 */
export function proposalActions(proposal, nowSeconds) {
	const status = proposalStatus(proposal, nowSeconds);
	return { accept: status === "open", dismiss: status === "open" || status === "expired" };
}

/**
 * What accepting wrote, in the owner's terms.
 * @param {FederationPeerProposal} proposal
 */
function outcomeLabel(proposal) {
	switch (proposal.outcome?.kind) {
		case "commitment":
			return proposal.details.kind === "reminder" ? "a reminder was set as a commitment on this server" : "the meeting was added as a commitment on this server";
		case "continuity":
			return "the task was added to your unfinished tasks on this server";
		default:
			return "a record was written on this server";
	}
}

/**
 * What happens next, or what happened, in one sentence, from the record
 * alone: nothing is written until the owner accepts, and accepting writes
 * exactly one record here.
 * @param {FederationPeerProposal} proposal
 * @param {number} nowSeconds
 */
export function proposalNote(proposal, nowSeconds) {
	switch (proposalStatus(proposal, nowSeconds)) {
		case "open":
			return `Nothing is written until you accept · lapses ${untilLabel(proposal.expires_at, nowSeconds)}.`;
		case "accepted":
			return `Accepted ${relativeTime(proposal.decided_at ?? proposal.received_at, nowSeconds)}: ${outcomeLabel(proposal)}.`;
		case "dismissed":
			return `Dismissed ${relativeTime(proposal.decided_at ?? proposal.received_at, nowSeconds)}; nothing was written.`;
		case "expired":
			return "Lapsed before you decided; it can no longer be accepted and nothing was written.";
		default:
			return "";
	}
}

/**
 * A handed-over task laid out for review: the goal, what was done, the
 * next step, the blockers, the resource references (labels on the other
 * owner's server, shown as code and never followed), and where each entry
 * came from. Only the sections with something in them. Every item is the
 * peer's own words.
 * @param {FederationTaskHandoff} task
 * @returns {HandoffSection[]}
 */
export function handoffSections(task) {
	/** @type {HandoffSection[]} */
	const sections = [];
	if (task.goal) sections.push({ title: "Goal", items: [task.goal] });
	if (task.completed_steps.length > 0) sections.push({ title: "Done so far", items: [...task.completed_steps] });
	if (task.next_step) sections.push({ title: "Next step", items: [task.next_step] });
	if (task.blockers.length > 0) sections.push({ title: "Blockers", items: [...task.blockers] });
	if (task.resources.length > 0) {
		sections.push({ title: "Resources on their side", items: task.resources.map((r) => `${resourceKindLabel(r.kind)} · ${r.label}`), code: true });
	}
	if (task.provenance.length > 0) {
		sections.push({ title: "Where it came from", items: task.provenance.map((p) => `${sourceLabel(p.source)} · ${momentLabel(p.at)} · ${p.note}`) });
	}
	return sections;
}

/** @param {string} kind */
function resourceKindLabel(kind) {
	switch (kind) {
		case "upload":
			return "upload";
		case "memory":
			return "memory";
		case "machine_path":
			return "path on a computer";
		default:
			return kind;
	}
}

/** @param {string} source */
function sourceLabel(source) {
	switch (source) {
		case "user":
			return "their owner";
		case "chat":
			return "their chat";
		case "tool":
			return "their companion";
		case "server":
			return "their server";
		default:
			return source;
	}
}

/**
 * Insert or replace a proposal by its id, newest first.
 * @param {FederationPeerProposal[]} proposals
 * @param {FederationPeerProposal} proposal
 */
export function upsertProposal(proposals, proposal) {
	const next = proposals.filter((p) => p.id !== proposal.id);
	next.push(proposal);
	next.sort((a, b) => b.received_at - a.received_at || (b.id < a.id ? -1 : 1));
	return next;
}
