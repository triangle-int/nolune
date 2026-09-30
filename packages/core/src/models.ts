import type Anthropic from '@anthropic-ai/sdk';
import * as anthropic from './anthropic.ts';
import * as claudePlan from './claude-plan.ts';
import * as chatgptPlan from './chatgpt-plan.ts';
import type { Usage } from './conversations.ts';
import type { ImageMediaType } from './images.ts';
import * as custom from './custom-providers.ts';
import { replyBlocks, toolCalls, type Message, type ToolCallBlock } from './format.ts';
import * as openai from './openai-chat.ts';
import * as openrouter from './openrouter.ts';
import * as xai from './xai.ts';
import {
	PlanError,
	isAgentPlan,
	isPlan,
	isPlanStopped,
	type AgentPlan,
	type PlanTurn
} from './plans.ts';

/*
 * A model call as the rest of nolune sees it, whichever provider runs it. Each provider's module
 * speaks its own API; this one picks the module for a conversation's provider and turns what it
 * returns into the same shape. Requests are built from nolune's own format (format.ts) by each
 * provider's module. The reply's `content` is still the provider's own, and is stored and sent
 * back exactly as it came.
 *
 * The Claude plan is the exception (plans.ts): Claude Code runs the agent loop, so the runner
 * hands it whole turns (runPlanTurn) rather than calling streamTurn. Chats on the ChatGPT plan
 * are OpenAI's, with the plan's sign-in instead of a key (chatgpt-plan.ts), and so are chats on
 * xAI's Grok models, whose API is OpenAI's Responses API at xAI (xai.ts).
 */

/**
 * `claude-plan` and `chatgpt-plan` run on someone's subscription instead of an API key (plans.ts);
 * `custom-openai` and `custom-anthropic` on custom providers, the family's own servers, in OpenAI's
 * or Anthropic's API (custom-providers.ts).
 */
export const PROVIDERS = [
	'anthropic',
	'openai',
	'openrouter',
	'xai',
	'custom-openai',
	'custom-anthropic',
	'claude-plan',
	'chatgpt-plan'
] as const;
export type Provider = (typeof PROVIDERS)[number];

export function isProvider(value: string): value is Provider {
	return (PROVIDERS as readonly string[]).includes(value);
}

/** How the admin page and the CLI name each provider. */
export const PROVIDER_LABELS: Record<Provider, string> = {
	anthropic: 'Anthropic',
	openai: 'OpenAI',
	openrouter: 'OpenRouter',
	xai: 'xAI',
	'custom-openai': custom.CUSTOM_LABELS['custom-openai'],
	'custom-anthropic': custom.CUSTOM_LABELS['custom-anthropic'],
	'claude-plan': 'Claude plan',
	'chatgpt-plan': 'ChatGPT plan'
};

export const EFFORTS = ['low', 'medium', 'high', 'xhigh', 'max'] as const;
export type Effort = (typeof EFFORTS)[number];

/**
 * How long a cached prompt lives: an hour for chats people come back to, 5 minutes for
 * subagents. Only Claude takes it (from Anthropic or through OpenRouter); OpenAI caches on its
 * own.
 */
export type CacheTtl = '5m' | '1h';

/** The live reply, block by block, for the chat to draw while it streams. */
export type StreamEvent =
	| {
			type: 'block_start';
			index: number;
			block: { type: 'text' | 'thinking' | 'tool'; id?: string };
	  }
	| { type: 'delta'; index: number; text: string };

export type ToolCall = ToolCallBlock;

export interface ModelReply {
	/** What the provider returned, to store and send back unchanged. */
	content: unknown[];
	/**
	 * In Anthropic's words, whichever provider answered: `end_turn`, `tool_use` (the calls are
	 * complete and can run), `max_tokens` (cut off) or `refusal`.
	 */
	stopReason: string | null;
	/** Null when it isn't known. */
	usage: Usage | null;
	calls: ToolCall[];
	/** The reply's text, without thinking or calls. */
	texts: string[];
}

function fromContent(content: unknown[], stopReason: string | null, usage: Usage): ModelReply {
	const texts = replyBlocks(content).flatMap((b) => (b.type === 'text' ? [b.text] : []));
	return { content, stopReason, usage, calls: toolCalls(content), texts };
}

/**
 * One model call. The request shape must stay identical across calls in a conversation (only
 * `messages` grows), otherwise the prompt cache is lost: `tools`, `system` and `cacheTtl` are the
 * conversation's own, fixed when it was created. `cacheKey` (the conversation's id) keeps a
 * conversation's calls together in OpenAI's cache, and on one provider behind OpenRouter.
 */
export async function streamTurn(opts: {
	provider: Provider;
	model: string;
	effort: Effort;
	system: string;
	tools: Anthropic.Tool[];
	cacheTtl: CacheTtl;
	cacheKey: string;
	/** The conversation in nolune's format; each provider's module turns it into its request. */
	messages: Message[];
	signal: AbortSignal;
	onEvent: (event: StreamEvent) => void;
}): Promise<ModelReply> {
	const { provider, cacheKey, ...request } = opts;
	if (isAgentPlan(provider)) {
		throw new Error('Chats on the Claude plan run whole turns through runPlanTurn');
	}
	if (provider === 'openrouter') {
		const reply = await openrouter.streamTurn({ ...request, cacheKey });
		return fromContent(reply.content, reply.stopReason, reply.usage);
	}
	if (
		provider === 'openai' ||
		provider === 'custom-openai' ||
		provider === 'chatgpt-plan' ||
		provider === 'xai'
	) {
		// A custom provider's chats are OpenAI's with its own client, the plan's with its sign-in,
		// and xAI's at xAI.
		const response =
			provider === 'openai'
				? await openai.streamResponse({ ...request, cacheKey })
				: provider === 'chatgpt-plan'
					? await chatgptPlan.streamResponse({ ...request, cacheKey })
					: provider === 'xai'
						? await xai.streamResponse({ ...request, cacheKey })
						: await custom.streamResponse({ ...request, cacheKey });
		return fromContent(
			response.output ?? [],
			openai.stopReason(response),
			openai.summarizeUsage(response.usage)
		);
	}
	// And Anthropic's.
	const message =
		provider === 'custom-anthropic'
			? await custom.streamMessage(request)
			: await anthropic.streamTurn(request);
	return fromContent(message.content, message.stop_reason, summarizeAnthropicUsage(message.usage));
}

export function summarizeAnthropicUsage(usage: Anthropic.Usage): Usage {
	return {
		input: usage.input_tokens,
		cacheRead: usage.cache_read_input_tokens ?? 0,
		cacheWrite: usage.cache_creation_input_tokens ?? 0,
		output: usage.output_tokens
	};
}

/**
 * One short exchange at low effort, not streamed, for chores like naming a chat. `text` is null
 * when the reply didn't finish (a refusal, or cut off).
 */
export async function quickReply(opts: {
	provider: Provider;
	model: string;
	system: string;
	input: string;
	maxTokens: number;
	timeoutMs: number;
}): Promise<{ text: string | null; usage: Usage }> {
	if (opts.provider === 'claude-plan') return claudePlan.quickReply(opts);
	if (opts.provider === 'openrouter') return openrouter.quickReply(opts);
	if (
		opts.provider === 'openai' ||
		opts.provider === 'custom-openai' ||
		opts.provider === 'chatgpt-plan' ||
		opts.provider === 'xai'
	) {
		const response =
			opts.provider === 'openai'
				? await openai.createResponse(opts)
				: opts.provider === 'chatgpt-plan'
					? await chatgptPlan.createResponse(opts)
					: opts.provider === 'xai'
						? await xai.createResponse(opts)
						: await custom.createResponse(opts);
		const usage = openai.summarizeUsage(response.usage);
		if (openai.stopReason(response) !== 'end_turn') return { text: null, usage };
		const text = textOf(response.output ?? []);
		return { text: opts.provider === 'custom-openai' ? custom.withoutThinking(text) : text, usage };
	}
	const reply =
		opts.provider === 'custom-anthropic'
			? await custom.createMessage(opts)
			: await anthropic.createMessage(opts);
	const usage = summarizeAnthropicUsage(reply.usage);
	if (reply.stop_reason !== 'end_turn') return { text: null, usage };
	const text = textOf(reply.content);
	return { text: opts.provider === 'anthropic' ? text : custom.withoutThinking(text), usage };
}

function textOf(content: unknown[]): string {
	return replyBlocks(content)
		.flatMap((b) => (b.type === 'text' ? [b.text] : []))
		.join('');
}

/** Whether the provider can say what an uploaded PDF costs; the others' are estimated. */
export function countsDocumentTokens(provider: Provider): boolean {
	return provider === 'anthropic' || provider === 'openai';
}

/**
 * What an uploaded PDF costs in every request of a conversation on this model. The provider
 * reads the whole document, so it also throws for PDFs it can't use (encrypted, too many pages).
 */
export function countDocumentTokens(
	provider: Provider,
	model: string,
	fileId: string
): Promise<number> {
	if (isPlan(provider)) {
		return Promise.reject(new Error('Chats on a plan have no Files API to count PDFs with'));
	}
	if (!countsDocumentTokens(provider)) {
		return Promise.reject(new Error(`${PROVIDER_LABELS[provider]} can't count a PDF's tokens`));
	}
	return provider === 'openai'
		? openai.countDocumentTokens(model, fileId)
		: anthropic.countDocumentTokens(model, fileId);
}

/**
 * What the model can be sent besides text. Every model of Anthropic's, OpenAI's and the plans
 * sees pictures and reads PDFs; OpenRouter says per model; xAI's models that see pictures get
 * them, and PDFs as their paths; custom providers' models get them all as their paths.
 */
export async function modelInputs(
	provider: Provider,
	model: string
): Promise<{ pictures: boolean; pdfs: boolean }> {
	if (provider === 'openrouter') return openrouter.modelInputs(model);
	if (provider === 'xai') return xai.modelInputs(model);
	if (custom.isCustomProvider(provider)) return custom.modelInputs();
	return { pictures: true, pdfs: true };
}

/**
 * The picture formats the provider takes, when that isn't all of nolune's (JPEG, PNG, GIF, WebP):
 * xAI takes JPEG and PNG. Pictures in the others are converted before they're sent.
 */
export function pictureTypes(provider: Provider): readonly ImageMediaType[] | undefined {
	return provider === 'xai' ? xai.PICTURE_TYPES : undefined;
}

/**
 * The messages as `model` can take them, before resolveFiles gives the provider its copies:
 * pictures and PDFs a model on OpenRouter can't read, and all of them on custom providers,
 * become notes. A chat that switched to that model may hold them. Other providers' models take
 * them all.
 */
export function readableMessages(
	provider: Provider,
	model: string,
	messages: Message[]
): Promise<Message[]> {
	if (provider === 'openrouter') return openrouter.readableMessages(messages, model);
	if (provider === 'xai') return xai.readableMessages(messages, model);
	if (custom.isCustomProvider(provider)) return custom.readableMessages(messages, model);
	return Promise.resolve(messages);
}

/**
 * Throws if the provider doesn't know the model. Null when its window isn't known. For a plan, it
 * checks that someone is signed in to one: to Claude Code, which can't check a model id, so
 * whether it takes the model shows at the chat's first reply; or with ChatGPT, whose plan lists
 * its models. On OpenRouter, the model must also be able to call tools. A custom provider is only
 * asked whether it lists the model.
 */
export async function fetchContextWindow(
	provider: Provider,
	model: string
): Promise<number | null> {
	if (provider === 'claude-plan') {
		await claudePlan.checkClaudePlan();
		return claudePlan.knownContextWindow(model);
	}
	if (provider === 'chatgpt-plan') return chatgptPlan.fetchContextWindow(model);
	if (provider === 'openrouter') return openrouter.fetchContextWindow(model);
	if (provider === 'xai') return xai.fetchContextWindow(model);
	if (custom.isCustomProvider(provider)) return custom.fetchContextWindow(provider, model);
	return provider === 'openai'
		? openai.fetchContextWindow(model)
		: anthropic.fetchContextWindow(model);
}

/** A model a preset can pick, as its provider lists it. */
export interface ModelChoice {
	/** What the preset stores. */
	id: string;
	/** The provider's name for it, when it gives one. */
	name: string | null;
	/** A line about it, when the provider gives one. */
	description: string | null;
	/** What a preset gets without an override: the same as fetchContextWindow says. */
	contextWindow: number | null;
}

/**
 * The models the provider offers, for the admin page to pick from: the newest first, or in the
 * plan's own order, or as the custom provider `customId` lists them (their ids
 * `<customId>/<model>`). Throws what describeApiError explains.
 */
export async function listModels(provider: Provider, customId = ''): Promise<ModelChoice[]> {
	if (provider === 'claude-plan') return claudePlan.listModels();
	if (provider === 'chatgpt-plan') return chatgptPlan.listModels();
	if (provider === 'openrouter') return openrouter.listModels();
	if (provider === 'xai') return xai.listModels();
	if (custom.isCustomProvider(provider)) return custom.listModels(customId);
	return provider === 'openai' ? openai.listModels() : anthropic.listModels();
}

/** Plans say what went wrong in their own words (plans.ts). */
export function describeApiError(err: unknown): string {
	if (err instanceof PlanError) return err.message;
	// A custom provider's errors are OpenAI's or Anthropic's SDK's classes, and OpenRouter's and
	// xAI's are OpenAI's, so they're asked first.
	if (custom.isCustomProviderError(err)) return custom.describeApiError(err);
	if (openrouter.isOpenRouterError(err)) return openrouter.describeApiError(err);
	if (xai.isXaiError(err)) return xai.describeApiError(err);
	return openai.isOpenAIError(err) ? openai.describeApiError(err) : anthropic.describeApiError(err);
}

/** The API's own message, without the status and JSON around it: for notes shown to the model. */
export function shortApiError(err: unknown): string {
	if (err instanceof PlanError) return err.message;
	if (custom.isCustomProviderError(err)) return custom.shortApiError(err);
	if (openrouter.isOpenRouterError(err)) return openrouter.shortApiError(err);
	if (xai.isXaiError(err)) return xai.shortApiError(err);
	return openai.isOpenAIError(err) ? openai.shortApiError(err) : anthropic.shortApiError(err);
}

export function isAbortError(err: unknown): boolean {
	return (
		anthropic.isAbortError(err) ||
		openai.isAbortError(err) ||
		openrouter.isAbortError(err) ||
		custom.isAbortError(err) ||
		isPlanStopped(err)
	);
}

/**
 * A turn of a chat on the Claude plan, which Claude Code runs. Throws a PlanError when it fails
 * and a PlanStopped when it was stopped.
 */
export function runPlanTurn(_plan: AgentPlan, turn: PlanTurn): Promise<void> {
	return claudePlan.runTurn(turn);
}

/** Whether a plan's turn failed on the chat's session (plans.ts), and how. */
export function planSessionProblem(_plan: AgentPlan, err: unknown) {
	return claudePlan.sessionProblem(err);
}
