import { statSync } from 'node:fs';
import { fail, redirect } from '@sveltejs/kit';
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
} from '@btw/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

function templatesFor(slug: string) {
	return scanImageTemplates(profileImageTemplatesDir(slug)).templates;
}

export const load: PageServerLoad = ({ locals, params }) => {
	const { profile } = requireProfile(locals, params.slug);
	const status = imageGenerationStatus();
	const preset = getDefaultPreset();
	return {
		templates: templatesFor(profile.slug).map((t) => ({
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
		problem: preset ? status.problem : translations(locals.locale).m.images.noModel,
		/** The provider whose key is missing, so the page can say who adds it and where. */
		missingKey: status.missingKey && { label: API_KEYS[status.missingKey].label }
	};
};

function message(err: unknown): string {
	return err instanceof Error ? err.message : String(err);
}

export const actions: Actions = {
	/**
	 * Starts a new chat that asks btw for a picture: with the prompt a template builds from its
	 * settings, or from a description. Pictures were uploaded already (like files in the chat
	 * composer) and are attached to the message by their upload ids.
	 */
	default: async ({ locals, params, request }) => {
		const { user, profile } = requireProfile(locals, params.slug);
		const { m } = translations(locals.locale);
		const form = await request.formData();
		const templateId = form.get('template')?.toString() ?? '';
		const uploads = form.getAll('upload').map(String);
		const problem = (text: string) => fail(400, { template: templateId, message: text });
		const preset = getDefaultPreset();
		if (!preset) return problem(m.images.noModel);
		try {
			// Checked before the conversation exists, so a stale file doesn't leave an empty chat.
			const notPicture = findUploads(profile.id, user.id, uploads).find(
				(u) => !u.mime.startsWith('image/')
			);
			if (templateId && notPicture) return problem(m.images.notPicture(notPicture.name));
		} catch (err) {
			if (err instanceof AttachmentError) return problem(err.message);
			throw err;
		}

		let text: string;
		if (templateId) {
			const template = templatesFor(profile.slug).find((t) => t.id === templateId);
			if (!template) return problem(m.images.templateGone);
			const values: Record<string, string> = {};
			for (const setting of template.settings) {
				const value = form.get(`setting:${setting.id}`)?.toString();
				if (value !== undefined) values[setting.id] = value;
			}
			const shapeInput = form.get('shape')?.toString() ?? '';
			const shape = (IMAGE_SHAPES as readonly string[]).includes(shapeInput)
				? (shapeInput as ImageShape)
				: template.size;
			try {
				const resolved = resolveImageTemplate(template, values);
				checkTemplateImages(template, uploads.length);
				text = templateMessage(resolved, {
					shape,
					images: uploads.length,
					extra: form.get('extra')?.toString()
				});
			} catch (err) {
				return problem(message(err));
			}
		} else {
			const description = form.get('text')?.toString().trim() ?? '';
			if (!description) return problem(m.images.describeFirst);
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
		await sendMessage(conversation.id, { id: user.id, name: user.name }, text, uploads);
		redirect(303, `/p/${profile.slug}/c/${conversation.id}`);
	}
};
