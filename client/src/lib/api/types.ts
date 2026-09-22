export type ChatRole = "user" | "assistant";

export type MessageKind = "message" | "tool_call" | "tool_output" | "mcp_app" | "compaction";

export interface ChatMessage {
	id: string;
	role: ChatRole;
	content: string;
	created_at: string;
	kind?: MessageKind;
	tool_name?: string;
	mcp_app_html?: string;
	mcp_app_input?: string;
	model?: string;
}

export interface ChatRequest {
	instance_slug: string;
	content: string;
	chat_id?: string;
	voice_mode?: boolean;
	/** The computer chosen for this conversation (#80): a machine id or `server-home`. */
	machine_id?: string | null;
}

export interface ChatResponse {
	instance_slug: string;
	chat_id: string;
	messages: ChatMessage[];
	agent_running: boolean;
}

export interface ChatSummary {
	id: string;
	title: string;
	message_count: number;
	last_message_at: string | null;
	created_at: string;
}

/** The one companion this server owns; `exists` is false until onboarding created it. */
export interface CompanionContext {
	slug: string;
	exists: boolean;
	companion_name: string;
	soul_exists: boolean;
}

export interface LlmSummary {
	model: string | null;
	configured: boolean;
}

export interface ServerMeta {
	app: string;
	version: string;
	commit: string;
	port: number;
	workspace_dir: string;
	/** Stable slug of the one companion this server owns. */
	companion_slug: string;
	instances_count: number;
	skills_count: number;
	llm: LlmSummary;
}

export interface UpdateLlmRequest {
	api_key: string;
}

export interface Soul {
	content: string;
	exists: boolean;
}

export interface SoulTemplate {
	id: string;
	name: string;
	description: string;
	content: string;
}

export type DropKind =
	| "thought"
	| "idea"
	| "poem"
	| "observation"
	| "reflection"
	| "recommendation"
	| "story"
	| "question"
	| "note";

export interface Drop {
	id: string;
	kind: DropKind;
	title: string;
	content: string;
	mood: string;
	created_at: string;
	image_url?: string;
}

/** One proactive run (#92/#94): receipts only, never model text. */
export type Tagged = { kind: string; [key: string]: unknown };
export interface ActionReceipt { tool: string; summary: string }
export interface RunOutcome { actions: ActionReceipt[]; messages_sent: number; tokens: number }
export interface Approval { side_effect: string; allowed: boolean; reason?: string; at: number }
export interface ProactiveRun {
	version: number;
	id: string;
	trigger: Tagged;
	reason: string;
	target: Tagged;
	dedupe_key: string;
	status: Tagged;
	attempt: number;
	retry_of?: string;
	started_at: number;
	finished_at?: number;
	approvals: Approval[];
	outcome?: RunOutcome | null;
}
export interface QuietHours { start_hour: number; end_hour: number }

/** A resumable task record (#81): links and provenance, never file contents. */
export type ContinuityState = "active" | "waiting" | "ready_to_resume" | "completed" | "dismissed" | "failed";
/** The user's stated priority on a task (#83); absent means normal. */
export type ContinuityPriority = "low" | "normal" | "high";
export type ProvenanceSource = "user" | "chat" | "tool" | "server";
export interface Provenance { source: ProvenanceSource; at: number; note: string }
export type ResourceRef =
	| { kind: "upload"; id: string }
	| { kind: "memory"; path: string }
	| { kind: "machine_path"; machine_id: string; path: string };
export interface ResourceLink { resource: ResourceRef; provenance: Provenance }
export interface ContinuityStep { summary: string; provenance: Provenance }
export type BlockerKind =
	| { kind: "machine_unavailable"; machine_id: string }
	| { kind: "resource_missing"; resource: ResourceRef }
	| { kind: "other" };
export interface ContinuityBlocker { kind: BlockerKind; detail: string; provenance: Provenance }
export interface ContinuityRecord {
	version: number;
	id: string;
	goal: string;
	state: ContinuityState;
	origin: { chat_id: string; message_id?: string };
	machine_ids: string[];
	resources: ResourceLink[];
	completed_steps: ContinuityStep[];
	blockers: ContinuityBlocker[];
	next_step?: string;
	/** The user's stated priority (#83); absent means normal. */
	priority?: ContinuityPriority | null;
	/** When the user wants it done, unix seconds (#83); absent means no deadline. */
	due_at?: number | null;
	/** The user's handoff decision (#82), absent until one is made. */
	handoff?: HandoffDecision | null;
	created_at: number;
	updated_at: number;
	provenance: Provenance[];
}
/** Files under continuity/ that could not be read; surfaced, never deleted. */
export interface ContinuityRecordError { file: string; reason: string }
export interface ContinuityListing { records: ContinuityRecord[]; errors: ContinuityRecordError[] }

/**
 * A reviewable handoff (#82): the card is derived by the server from a
 * continuity record and the known machines, never from model text, and
 * stays useful while the origin computer is offline.
 */
export interface ComputerSummary {
	machine_id: string;
	/** The user's name, the hostname, or the id when the machine is unknown. */
	display_name: string;
	known: boolean;
	online: boolean;
	health: MachineHealth;
	platform: MachinePlatform | null;
	last_seen: number | null;
}
export type HandoffOutcomeStatus = "completed" | "failed" | "cancelled";
/** The receipt of a finished continuation: the run's status and a short summary. */
export interface HandoffOutcome { status: HandoffOutcomeStatus; finished_at: number; summary: string }
export type HandoffDecision =
	| { kind: "accepted"; machine_id: string; run_id: string; at: number; outcome?: HandoffOutcome | null }
	| { kind: "kept"; machine_id?: string | null; at: number }
	| { kind: "dismissed"; at: number };
export interface HandoffResource { resource: ResourceRef; label: string; available: boolean }
export type HandoffPermission = "screen_capture" | "accessibility";
export interface HandoffRequirements { capabilities: string[]; permissions: HandoffPermission[] }
export interface HandoffCard {
	record_id: string;
	goal: string;
	state: ContinuityState;
	origin_chat_id: string;
	origin: ComputerSummary | null;
	completed_steps: string[];
	resources: HandoffResource[];
	blockers: string[];
	next_step: string | null;
	required: HandoffRequirements;
	decision: HandoffDecision | null;
	bound_to: ComputerSummary | null;
	/** Resumable and not kept or dismissed since the last explicit update. */
	offered: boolean;
	created_at: number;
	updated_at: number;
}
export interface HandoffListing { handoffs: HandoffCard[]; errors: ContinuityRecordError[] }
/** `blocking` refuses the continuation; `approval` means the desktop will ask; `note` is information. */
export type CheckSeverity = "blocking" | "approval" | "note";
export interface ContinuationCheck { kind: Tagged; severity: CheckSeverity; detail: string }
/** The pre-continuation preview: the destination and every check, nothing started. */
export interface ContinuationPreview { card: HandoffCard; destination: ComputerSummary; checks: ContinuationCheck[]; ready: boolean }
/** An accepted handoff; `already_running` when a continuation was already going and nothing new started. */
export interface HandoffAccepted { card: HandoffCard; run: ProactiveRun; already_running: boolean }
/** One explicit change; lists are added to, never replaced. `note` is required provenance. */
export interface ContinuityUpdate {
	goal?: string;
	state?: ContinuityState;
	completed_step?: string;
	blocker?: string;
	clear_blockers?: boolean;
	next_step?: string;
	priority?: ContinuityPriority;
	due_at?: number;
	clear_due?: boolean;
	machine_ids?: string[];
	resources?: ResourceRef[];
	note: string;
}
/** A tracked promise (#85): a bounded record the server keeps, never model text. */
export type CommitmentStatus = "active" | "waiting" | "blocked" | "due" | "completed" | "dismissed" | "failed";
export type CommitmentOwner = "companion" | "user";
export type CommitmentDeadline = { kind: "at"; at: number } | { kind: "window"; start: number; end: number };
export type CommitmentWait = { kind: "until"; until: number } | { kind: "event"; event: string } | { kind: "user_reply" };
export type CommitmentProvenance =
	| { kind: "manual" }
	| { kind: "chat"; chat_id: string; message_id?: string }
	| { kind: "run"; run_id: string };
/** Why it counts as done; the server refuses a completion with none of these. */
export interface CompletionEvidence { confirmed_by_user?: boolean; summary?: string; run_id?: string; at?: number }
export type CommitmentCheckOutcome =
	| { kind: "unchanged" }
	| { kind: "triggered" }
	| { kind: "failed"; error: string; retryable: boolean }
	| { kind: "observed"; event: string };
export interface CommitmentCheck { at: number; outcome: CommitmentCheckOutcome; run_id?: string; pending_event?: string }
export interface Commitment {
	version: number;
	id: string;
	promise: string;
	owner: CommitmentOwner;
	status: CommitmentStatus;
	deadline?: CommitmentDeadline | null;
	dependencies: string[];
	waiting_on?: CommitmentWait | null;
	next_check?: number | null;
	continuity_ids: string[];
	provenance: CommitmentProvenance;
	completion?: CompletionEvidence | null;
	snoozed_until?: number | null;
	snooze_count: number;
	last_check?: CommitmentCheck | null;
	created_at: number;
	updated_at: number;
	status_changed_at: number;
}
/** Absent fields are kept; the `clear_*` flags remove optional ones. */
export interface CommitmentPatch {
	promise?: string;
	owner?: CommitmentOwner;
	deadline?: CommitmentDeadline;
	clear_deadline?: boolean;
	dependencies?: string[];
	waiting_on?: CommitmentWait;
	clear_waiting_on?: boolean;
	next_check?: number;
	clear_next_check?: boolean;
	continuity_ids?: string[];
}
export type CommitmentListFilter = "open" | "closed" | "all";

/**
 * The Resume my work ritual (#83): opt-in, backed only by continuity
 * records; at most one suggestion per trigger, delivered as a handoff card.
 */
export interface ResumeRitualPolicy {
	enabled: boolean;
	/** Away for at least this long counts as a break. */
	break_minutes: number;
	/** Least seconds between spontaneous suggestions, and after a refusal. */
	cooldown_secs: number;
	snooze_until: number | null;
	dismissed_record_ids: string[];
}
/** The editable part of the policy; snooze and dismissals have routes of their own. */
export type ResumePolicyEdit = Pick<ResumeRitualPolicy, "enabled" | "break_minutes" | "cooldown_secs">;
export type RitualTrigger =
	| { kind: "manual" }
	| { kind: "opened_after_break"; away_secs: number }
	| { kind: "machine_connected"; machine_id: string };
/** One bounded suggestion: the record, why it was picked, why now, and the card it leads to. */
export interface ResumeOffer {
	id: string;
	record_id: string;
	goal: string;
	trigger: RitualTrigger;
	why_now: string;
	why_this: string;
	destination_id: string;
	suggested_at: number;
	card: HandoffCard;
}
export type ResumeHeld =
	| { kind: "disabled" }
	| { kind: "quiet_hours" }
	| { kind: "cooldown"; until: number }
	| { kind: "snoozed"; until: number }
	| { kind: "no_break" }
	| { kind: "nothing_to_resume" }
	| { kind: "storage"; message: string };
export interface ResumeStatus {
	policy: ResumeRitualPolicy;
	suggestion: ResumeOffer | null;
	quiet_hours_active: boolean;
}
/** What a trigger produced: one suggestion, or why there is none. */
export interface ResumeOutcome { suggestion: ResumeOffer | null; held?: ResumeHeld }
export interface ProactivePolicy {
	enabled: boolean;
	quiet_hours: QuietHours | null;
	cooldown_secs: number;
	daily_reach_out_budget: number;
	retention_max: number;
	retention_days: number;
	check_in_interval_hours: number;
	reflection_enabled: boolean;
	reflection_interval_hours: number;
}


export interface SkillSource {
	repo: string;
	version: string;
}

export interface Skill {
	id: string;
	name: string;
	description: string;
	icon: string;
	builtin: boolean;
	enabled: boolean;
	kind?: "local" | "anthropic";
	anthropic_skill_id?: string;
	instructions: string;
	source?: SkillSource;
	resources?: string[];
}

export interface RegistryEntry {
	id: string;
	name: string;
	description: string;
	icon: string;
	repo: string;
	git_ref: string;
	author: string;
	path: string;
	installed: boolean;
}

export interface UploadMeta {
	id: string;
	original_name: string;
	stored_name: string;
	mime_type: string;
	size: number;
	uploaded_at: string;
}

export interface ContextSection {
	name: string;
	chars: number;
	tokens: number;
}

export interface ContextStats {
	system_prompt: ContextSection[];
	system_prompt_total_tokens: number;
	tools: string[];
	tools_count: number;
	tools_tokens_estimate: number;
	history_messages: number;
	history_tokens_estimate: number;
	total_input_tokens_estimate: number;
}

/** User-set flags on a text memory (#84); a media memory carries none. */
export interface MemoryFlags {
	/** Always auto-recalled into the user's chat. */
	pinned: boolean;
	/** Hidden from the companion's own routines (check-in, reflection). */
	exclude_from_proactive: boolean;
}

export interface MemoryEntry extends MemoryFlags {
	path: string;
	summary: string;
	size: number;
}

export interface MemoryGraph {
	edges: [string, string][];
}

/** How auto-recall surfaced a memory (#84). */
export type RecallReason = "semantic" | "keyword" | "linked_to" | "matched" | "pinned";
/** Coarse confidence bucket; raw scores never leave the server. */
export type RecallConfidence = "high" | "medium" | "low";

/** One recalled memory as carried by the `memory_recall` event and persisted receipts. */
export interface RecalledMemory {
	/** Memory path as the library shows it (for media, the media file). */
	path: string;
	/** Canonical file whose text was recalled; media memories cite their bound text. */
	source: string;
	excerpt: string;
	reason: RecallReason;
	/** The recalled memory this one was linked from (`linked_to` only). */
	linked_from?: string;
	confidence: RecallConfidence;
	/** RFC 3339 UTC timestamp of the retrieval. */
	retrieved_at: string;
	/** Resolved on every receipt read; a deleted source is reported, not dropped. */
	source_status: "present" | "missing";
}

/** Persisted provenance for one assistant message (`chats/{chat_id}/receipts/{message_id}.json`). */
export interface MemoryReceipt {
	message_id: string;
	chat_id: string;
	memories: RecalledMemory[];
}

/** One side of a correction conflict, in full. */
export interface CorrectionStatement {
	id: string;
	statement: string;
	corrected_at: string;
}

/** Two user statements about one memory; the user picks which one stays. */
export interface CorrectionConflict {
	conflict_id: string;
	path: string;
	current: CorrectionStatement;
	proposed: CorrectionStatement;
}

export type CorrectionStatus = "applied" | "superseded" | "needs_resolution" | "withdrawn";

/** One entry of the companion's correction ledger. */
export interface CorrectionEntry {
	id: string;
	path: string;
	statement: string;
	previous: string;
	status: CorrectionStatus;
	corrected_at: string;
	resolved_at?: string;
	conflicts_with?: string;
}

export interface CorrectionLedger {
	version: number;
	entries: CorrectionEntry[];
}

/** `PUT …/memory/{path}`: applied, a no-op, or a conflict for the user to settle (409). */
export type CorrectionResponse =
	| { status: "applied"; path: string; correction: CorrectionEntry }
	| { status: "unchanged"; path: string }
	| ({ status: "needs_resolution" } & CorrectionConflict);

export type MachinePlatform = "macos" | "windows" | "linux";
export type MachineLocation = "server_local" | "desktop";
/** `healthy`, `degraded` (open socket, stale heartbeat), or `unavailable` (offline). */
export type MachineHealth = "healthy" | "degraded" | "unavailable";
export type MachinePermission = "granted" | "denied" | "prompt_required" | "unavailable";
export interface MachinePermissions {
	accessibility: MachinePermission;
	screen_capture: MachinePermission;
}

/**
 * A computer that has ever connected to the companion through the Nolune
 * desktop app (#80). Disconnected computers stay listed as offline; a
 * `machine_updated` server event carries the same shape on every change
 * (including a heartbeat that goes stale), and `machine_forgotten` names a
 * row to drop.
 */
export interface MachineInfo {
	/** Stable id the desktop persists; survives reconnects and hostname changes. */
	machine_id: string;
	/** The user's name when set, otherwise the hostname. */
	display_name: string;
	/** The user's name, `null` while the hostname is shown. */
	custom_name: string | null;
	hostname: string;
	os: string;
	platform: MachinePlatform | null;
	location: MachineLocation;
	/**
	 * The Cua driver's grants (Accessibility and Screen Recording as the
	 * driver holds them), live while a driver is registered on this machine;
	 * `null` for a desktop without one. The desktop app's own grants are never
	 * reported: nothing inside the app uses them (#19).
	 */
	permissions: MachinePermissions | null;
	/** Toolcall names the desktop app executes (shell and file work). */
	capabilities: string[];
	/** Unix seconds of the first registration. */
	first_seen: number;
	/** Unix seconds of the last heartbeat or disconnect. */
	last_seen: number;
	instance_slug: string | null;
	online: boolean;
	health: MachineHealth;
	/** Reserved for the Cua driver (#18); `null` means not reported. */
	driver_version: string | null;
	/** Reserved for the Cua driver's own health (#18); `null` means not reported. */
	cua_health: MachineHealth | null;
}

export type ServerEvent =
	| {
			type: "chat_message_created";
			instance_slug: string;
			chat_id: string;
			message: ChatMessage;
	  }
	| {
			type: "mood_updated";
			instance_slug: string;
			mood: string;
	  }
	| {
			type: "agent_running";
			instance_slug: string;
			chat_id: string;
	  }
	| {
			type: "agent_stopped";
			instance_slug: string;
			chat_id: string;
	  }
	| {
			type: "tool_activity";
			instance_slug: string;
			chat_id: string;
			tool_name: string;
			summary: string;
	  }
	| {
			type: "drop_created";
			instance_slug: string;
			drop: Drop;
	  }
	| {
			type: "activity_updated";
			instance_slug: string;
			run: ProactiveRun;
	  }
	| {
			/** A commitment was created or changed (#85): the whole record, no model text. */
			type: "commitment_updated";
			instance_slug: string;
			commitment: Commitment;
	  }
	| {
			/** A request sent to a paired companion was queued or changed (#110): the outbox entry. */
			type: "outbox_updated";
			instance_slug: string;
			entry: FederationOutboxEntry;
	  }
	| {
			type: "machine_updated";
			instance_slug: string;
			machine: MachineInfo;
	  }
	| {
			/** An offline computer was forgotten, migrated into its stable id, or evicted: drop its row. */
			type: "machine_forgotten";
			instance_slug: string;
			machine_id: string;
	  }
	| {
			/** A handoff card changed (#82): a decision was recorded or a continuation finished. */
			type: "handoff_updated";
			instance_slug: string;
			card: HandoffCard;
	  }
	| {
			/** The resume ritual's offer changed (#83): a new suggestion, or none. */
			type: "resume_updated";
			instance_slug: string;
			suggestion: ResumeOffer | null;
	  }
	| {
			type: "context_compacting";
			instance_slug: string;
			chat_id: string;
			messages_compacted: number;
	  }
	| {
			type: "chat_stream_delta";
			instance_slug: string;
			chat_id: string;
			message_id: string;
			delta: string;
	  }
	| {
			type: "secret_request";
			instance_slug: string;
			id: string;
			prompt: string;
			target: string;
	  }
	| {
			type: "tool_output_chunk";
			instance_slug: string;
			chat_id: string;
			chunk: string;
	  }
	| {
			type: "mcp_app_result";
			instance_slug: string;
			chat_id: string;
			message_id: string;
			tool_output: string;
	  }
	| {
			type: "mcp_app_start";
			instance_slug: string;
			chat_id: string;
			tool_name: string;
			html: string;
	  }
	| {
			type: "chat_audio_ready";
			instance_slug: string;
			chat_id: string;
			audio_base64: string;
			message_ids: string[];
	  }
	| {
			type: "mcp_app_input_delta";
			instance_slug: string;
			chat_id: string;
			delta: string;
	  }
	| {
			type: "memory_recall";
			instance_slug: string;
			chat_id: string;
			memories: RecalledMemory[];
	  }
;

// ---------------------------------------------------------------------------
// Companion federation (#108)
// ---------------------------------------------------------------------------

/** A companion's public, self-signed identity document (`federation/identity.json`). */
export interface FederationIdentity {
	version: number;
	/** Derived from `public_key`; the only thing a peer binds trust to. */
	companion_id: string;
	public_key: string;
	created_at: number;
	signature: string;
}

/** `invited` lives in memory as an outstanding invite; the rest are peer records. */
export type FederationPeerState = "invited" | "pending" | "paired" | "revoked";
/** Which side minted the invite: `issuer` (this server) confirms; `accepter` waits. */
export type FederationPairingRole = "issuer" | "accepter";

/** A key rotation: the new identity endorsed by the previous key. */
export interface FederationKeyRotation {
	version: number;
	previous: FederationIdentity;
	identity: FederationIdentity;
	rotated_at: number;
	endorsement: string;
	signature: string;
}

/** A rotation this server accepted from a peer, kept on its record. */
export interface FederationKeyTransition {
	rotation: FederationKeyRotation;
	accepted_at: number;
}

/**
 * A peer companion as `GET /api/federation/peers` lists it: its key and
 * id, the handshake state, the origins this owner approved, and when
 * something it signed last verified here. No name, host, or profile.
 */
export interface FederationPeer {
	companion_id: string;
	public_key: string;
	state: FederationPeerState;
	role: FederationPairingRole;
	pairing_id: string;
	approved_origins: string[];
	/** The origin the peer reported while pairing, until the owner approves it. */
	pending_origin?: string;
	created_at: number;
	updated_at: number;
	/** Unix seconds; absent until something the peer signed verified here. */
	last_seen_at?: number;
	rotation_history: FederationKeyTransition[];
}

/** An outstanding invite as the list shows it: a handle and a clock, no secret. */
export interface FederationInvite {
	id: string;
	state: FederationPeerState;
	created_at: number;
	expires_at: number;
}

/** `GET /api/federation/peers`. */
export interface FederationOverview {
	companion_id: string;
	identity: FederationIdentity;
	rotations: FederationKeyRotation[];
	invites: FederationInvite[];
	peers: FederationPeer[];
}

/**
 * `POST /api/federation/invites`: the one response that carries the secret,
 * as its fields and packed into `invite`, the one line to hand over.
 */
export interface IssuedFederationInvite {
	id: string;
	secret: string;
	/** `nolune-invite-v1.…`: origin, secret, and issuer document in one line. */
	invite: string;
	created_at: number;
	expires_at: number;
	expires_in_secs: number;
	origin: string;
	issuer: FederationIdentity;
}

/** `POST /api/federation/rotate`. */
export interface FederationRotationReport {
	identity: FederationIdentity;
	rotation: FederationKeyRotation;
	notified: string[];
	unreachable: string[];
}

// ---------------------------------------------------------------------------
// Federation policy and approvals (#109)
// ---------------------------------------------------------------------------

/** What a rule, or the default for a pair, says. */
export type FederationAccess = "allow" | "ask" | "deny";

/** One owner rule: what a peer may do with one intent at one class, until `expires_at` if set. */
export interface FederationPolicyRule {
	intent: string;
	disclosure: string;
	access: FederationAccess;
	granted_at: number;
	expires_at?: number;
}

export interface FederationRateLimit {
	max_requests: number;
	window_secs: number;
}

/** The owner's rules for one peer, keyed by companion id in the document. */
export interface FederationPeerPolicy {
	rules: FederationPolicyRule[];
	rate_limit?: FederationRateLimit;
}

/** `federation/policy.json` as the owner routes list it. */
export interface FederationPolicyDocument {
	version: number;
	quiet_hours?: { start_hour: number; end_hour: number; timezone?: string };
	rate_limit: FederationRateLimit;
	peers: Record<string, FederationPeerPolicy>;
}

/** One row of the defaults table: what applies to a pair when no rule says otherwise. */
export interface FederationDefaultAccess {
	intent: string;
	disclosure: string;
	access: FederationAccess;
}

/** `GET /api/federation/policy`. */
export interface FederationPolicyView {
	document: FederationPolicyDocument;
	defaults: FederationDefaultAccess[];
}

export type FederationApprovalStatus = "pending" | "approved" | "denied";

/**
 * A request that asked the owner, as `GET /api/federation/approvals` lists
 * it: who asked for which intent at which class, when, until when, and what
 * the owner said so far. Never what the peer sent.
 */
export interface FederationApproval {
	version: number;
	id: string;
	pairing_id: string;
	requester: string;
	intent: string;
	disclosure: string;
	status: FederationApprovalStatus;
	requested_at: number;
	decided_at?: number;
	expires_at: number;
	summary: string;
}

/** How far an approval or denial reaches: one use, a deadline, or the class. */
export type FederationApprovalScope = { scope: "once" } | { scope: "until"; expires_at: number } | { scope: "class" };

/** What `POST /api/federation/approvals/{id}/approve` (or `/deny`) left behind. */
export interface FederationApprovalOutcome {
	approval?: FederationApproval;
	rule?: FederationPolicyRule;
}

/** The body of `POST /api/federation/peers/{id}/rules`. */
export interface FederationRuleRequest {
	intent: string;
	disclosure: string;
	access: FederationAccess;
	expires_at?: number;
}

// Inbound federation intents (#110)

export type FederationInboundStatus = "pending" | "accepted" | "denied";

/** The typed answer a peer was given, as the server sealed it. */
export type FederationIntentResponse =
	| { outcome: "accepted"; version: number; correlation_id: string; responder: string; disclosure: string; answer: { kind: string; at?: number; windows?: { from: number; to: number; state: string }[] } }
	| { outcome: "denied"; version: number; correlation_id: string; responder: string; reason: string; retry_after_secs?: number }
	| { outcome: "needs_owner"; version: number; correlation_id: string; responder: string; reason: string };

/**
 * One request a paired companion delivered, as `GET /api/federation/inbox`
 * lists it: who asked for what at which class, on whose behalf and to what
 * end (the peer's own words), what it was answered, and what it led to.
 * Never what the peer sent.
 */
export interface FederationInboundIntent {
	version: number;
	sender: string;
	correlation_id: string;
	pairing_id: string;
	intent: string;
	disclosure: string;
	represented_owner: string;
	purpose: string;
	status: FederationInboundStatus;
	/** Why it stands where it does: the engine's reason, or `owner_approved` / `owner_denied` when the owner's word decided it. */
	reason: string;
	/** The answer the peer was given; for a request the owner denied once, still `needs_owner`. */
	response: FederationIntentResponse;
	approval_id?: string;
	receipt_id?: string;
	message_id?: string;
	requested_at: number;
	updated_at: number;
	expires_at: number;
}

/** Why an intent was answered the way it was. */
export type FederationReceiptBasis = { kind: "policy"; reason: string; rule_id?: string } | { kind: "owner_approval"; approval_id: string };

/** What this side kept of one intent: who asked whom for what, what was granted, and why. */
export interface FederationIntentReceipt {
	version: number;
	id: string;
	side: "requesting" | "answering" | "owner";
	pairing_id: string;
	correlation_id: string;
	requester: string;
	represented_owner: string;
	responder: string;
	intent: string;
	purpose: string;
	requested: string;
	granted: string;
	outcome: "accepted" | "denied" | "needs_owner";
	basis: FederationReceiptBasis;
	at: number;
	summary: string;
}

/** `GET /api/federation/inbox`: live records and kept receipts, newest first. */
export interface FederationInbox {
	intents: FederationInboundIntent[];
	receipts: FederationIntentReceipt[];
}

// Outbound federation intents (#110)

export type FederationOutboxStatus = "queued" | "waiting_owner" | "delivered" | "denied" | "failed" | "expired";

/** How one delivery attempt ended. */
export type FederationOutboxAttemptOutcome =
	| { kind: "in_flight" }
	| { kind: "interrupted" }
	| { kind: "unreachable" }
	| { kind: "refused"; status?: number; code: string }
	| { kind: "malformed" }
	| { kind: "answered"; outcome: "accepted" | "denied" | "needs_owner"; reason?: string };

export interface FederationOutboxAttempt {
	at: number;
	outcome: FederationOutboxAttemptOutcome;
}

/** The intent this companion sent, as the wire carries it: this owner's own words. */
export interface FederationOutboundIntent {
	version: number;
	correlation_id: string;
	sender: string;
	represented_owner: string;
	purpose: string;
	disclosure: string;
	issued_at: number;
	expires_at: number;
	intent: { type: string; body?: string; text?: string; at?: number; description?: string; window?: { from: number; to: number } };
}

/**
 * One request this companion queued for a paired companion, as
 * `GET /api/federation/outbox` lists it and `outbox_updated` carries it:
 * the intent, where it stands, every attempt, and the peer's typed
 * response. Nothing else the peer said.
 */
export interface FederationOutboxEntry {
	version: number;
	recipient: string;
	pairing_id: string;
	intent: FederationOutboundIntent;
	status: FederationOutboxStatus;
	attempts: FederationOutboxAttempt[];
	next_attempt_at?: number;
	response?: FederationIntentResponse;
	receipt_id?: string;
	chat_id: string;
	created_at: number;
	updated_at: number;
}

/** `GET /api/federation/outbox`: entries and kept receipts, newest first. */
export interface FederationOutbox {
	entries: FederationOutboxEntry[];
	receipts: FederationIntentReceipt[];
}
