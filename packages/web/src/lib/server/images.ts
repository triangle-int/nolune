import { statSync } from 'node:fs';
import {
	API_KEYS,
	AttachmentError,
	IMAGE_SHAPES,
	checkTemplateImages,
	createConversation,
	findUploads,
	getDefaultPreset,
	imageGenerationStatus,
	profileImageTemplatesDir,
	resolveImageTemplate,
	scanImageTemplates,
	sendMessage,
	templateMessage,
	type ImageShape
} from '@nolune/core';
import type { Messages } from '$lib/i18n';
import type { requireProfile } from './access';

type Person = ReturnType<typeof requireProfile>['user'];
type Profile = ReturnType<typeof requireProfile>['profile'];

function templatesFor(slug: string) {
	return scanImageTemplates(profileImageTemplatesDir(slug)).templates;
}

/**
 * The Images page as it loads, and as apps read it (`/api/p/<slug>/images`): the profile's
 * picture templates, with their settings, and whether nolune can make pictures (and if not, why).
 */
export function imagesOverview(slug: string, m: Messages) {
	const status = imageGenerationStatus();
	const preset = getDefaultPreset();
	return {
		templates: templatesFor(slug).map((t) => ({
			id: t.id,
			name: t.name,
			title: t.title,
			description: t.description,
			sentence: t.sentence,
			category: t.category,
			icon: t.icon,
			color: t.color,
			// Changes when the picture does, so the browser doesn't keep an old one.
			cover: t.cover ? Math.round(statSync(t.cover).mtimeMs) : null,
			image: t.image,
			imageLabel: t.imageLabel,
			imageSource: t.imageSource,
			maxImages: t.maxImages,
			size: t.size,
			settings: t.settings.map((s) => {
				if (s.type === 'select') {
					const options = s.options.map((o) => ({ value: o.value, label: o.label }));
					const custom = s.custom !== null;
					return { type: s.type, id: s.id, label: s.label, default: s.default, options, custom };
				}
				if (s.type === 'emoji') {
					return { type: s.type, id: s.id, label: s.label, default: s.default, max: s.max };
				}
				return {
					type: s.type,
					id: s.id,
					label: s.label,
					default: s.default,
					placeholder: s.placeholder,
					required: s.required
				};
			})
		})),
		ready: status.ready && !!preset,
		problem: preset ? status.problem : m.images.noModel,
		/** The provider whose key is missing, so the page can say who adds it and where. */
		missingKey: status.missingKey && { label: API_KEYS[status.missingKey].label }
	};
}

export interface PictureRequest {
	/** A template's id; empty for a description. */
	template: string;
	/** Pictures uploaded already, like files in the chat composer. */
	uploads: string[];
	/** The template's settings, by their ids. */
	settings: Record<string, string>;
	shape?: string;
	extra?: string;
	/** The description, without a template. */
	text?: string;
}

/**
 * Starts a new chat that asks nolune for a picture: with the prompt a template builds from its
 * settings, or from a description. The chat's id, or what's wrong with what was asked.
 */
export async function startPictureChat(
	user: Person,
	profile: Profile,
	asked: PictureRequest,
	m: Messages
): Promise<{ conversationId: string } | { problem: string }> {
	const preset = getDefaultPreset();
	if (!preset) return { problem: m.images.noModel };
	try {
		// Checked before the conversation exists, so a stale file doesn't leave an empty chat.
		const notPicture = findUploads(profile.id, user.id, asked.uploads).find(
			(u) => !u.mime.startsWith('image/')
		);
		if (asked.template && notPicture) return { problem: m.images.notPicture(notPicture.name) };
	} catch (err) {
		if (err instanceof AttachmentError) return { problem: err.message };
		throw err;
	}

	let text: string;
	if (asked.template) {
		const template = templatesFor(profile.slug).find((t) => t.id === asked.template);
		if (!template) return { problem: m.images.templateGone };
		const values: Record<string, string> = {};
		for (const setting of template.settings) {
			const value = asked.settings[setting.id];
			if (value !== undefined) values[setting.id] = value;
		}
		const shape = (IMAGE_SHAPES as readonly string[]).includes(asked.shape ?? '')
			? (asked.shape as ImageShape)
			: template.size;
		try {
			const resolved = resolveImageTemplate(template, values);
			checkTemplateImages(template, asked.uploads.length);
			text = templateMessage(resolved, { shape, images: asked.uploads.length, extra: asked.extra });
		} catch (err) {
			return { problem: err instanceof Error ? err.message : String(err) };
		}
	} else {
		const description = asked.text?.trim() ?? '';
		if (!description) return { problem: m.images.describeFirst };
		// For the model, like a template's prompt: the same in every interface language.
		text = `Make an image: ${description}`;
	}

	// Low reasoning: the work is one command, and the picture itself takes long enough.
	const conversation = createConversation({
		profile,
		presetId: preset.id,
		userId: user.id,
		effort: 'low'
	});
	await sendMessage(conversation.id, { id: user.id, name: user.name }, text, asked.uploads);
	return { conversationId: conversation.id };
}
