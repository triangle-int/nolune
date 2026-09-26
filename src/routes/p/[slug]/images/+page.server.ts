import { statSync } from 'node:fs';
import { fail, redirect } from '@sveltejs/kit';
import {
	IMAGE_SHAPES,
	MAX_UPLOAD_BYTES,
	MAX_UPLOADS,
	checkTemplateImages,
	createConversation,
	getDefaultPreset,
	imageGenerationStatus,
	profileImageTemplatesDir,
	resolveImageTemplate,
	saveUploads,
	scanImageTemplates,
	sendMessage,
	templateMessage,
	type ImageShape
} from '@btw/core';
import { requireProfile } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

function templatesFor(slug: string) {
	return scanImageTemplates(profileImageTemplatesDir(slug)).templates;
}

export const load: PageServerLoad = ({ locals, params }) => {
	const { profile } = requireProfile(locals, params.slug);
	const status = imageGenerationStatus();
	return {
		templates: templatesFor(profile.slug).map((t) => ({
			id: t.id,
			name: t.name,
			description: t.description,
			category: t.category,
			icon: t.icon,
			color: t.color,
			// Changes when the picture does, so the browser doesn't keep an old one.
			cover: t.cover ? Math.round(statSync(t.cover).mtimeMs) : null,
			image: t.image,
			imageLabel: t.imageLabel,
			maxImages: t.maxImages,
			size: t.size,
			settings: t.settings.map((s) =>
				s.type === 'select'
					? {
							type: s.type,
							id: s.id,
							label: s.label,
							default: s.default,
							options: s.options.map((o) => ({ value: o.value, label: o.label }))
						}
					: {
							type: s.type,
							id: s.id,
							label: s.label,
							default: s.default,
							placeholder: s.placeholder,
							required: s.required
						}
			)
		})),
		ready: status.ready && !!getDefaultPreset(),
		problem: getDefaultPreset() ? status.problem : 'No chat model is set up yet.',
		maxUploads: MAX_UPLOADS,
		maxUploadBytes: MAX_UPLOAD_BYTES
	};
};

function message(err: unknown): string {
	return err instanceof Error ? err.message : String(err);
}

export const actions: Actions = {
	/**
	 * Starts a new chat that asks btw for a picture: with the prompt a template builds from its
	 * settings, or from a description. Attached pictures are saved in the profile folder and
	 * attached to the message.
	 */
	default: async ({ locals, params, request }) => {
		const { user, profile } = requireProfile(locals, params.slug);
		const form = await request.formData();
		const templateId = form.get('template')?.toString() ?? '';
		const files = form
			.getAll('image')
			.filter((f): f is File => f instanceof File && f.size > 0 && f.name !== '');
		const preset = getDefaultPreset();
		if (!preset)
			return fail(400, { template: templateId, message: 'No chat model is set up yet.' });

		let text: string;
		let title: string;
		if (templateId) {
			const template = templatesFor(profile.slug).find((t) => t.id === templateId);
			if (!template) {
				return fail(400, { template: templateId, message: 'That template no longer exists.' });
			}
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
				checkTemplateImages(template, files.length);
				text = templateMessage(resolved, {
					shape,
					hasImages: files.length > 0,
					extra: form.get('extra')?.toString()
				});
			} catch (err) {
				return fail(400, { template: templateId, message: message(err) });
			}
			title = template.name;
		} else {
			const description = form.get('text')?.toString().trim() ?? '';
			if (!description) return fail(400, { template: '', message: 'Describe the picture first.' });
			text = `Make an image: ${description}`;
			title = description.slice(0, 80);
		}

		let attachments;
		try {
			attachments = await saveUploads(
				profile.slug,
				await Promise.all(
					files.map(async (f) => ({ name: f.name, data: new Uint8Array(await f.arrayBuffer()) }))
				)
			);
		} catch (err) {
			return fail(400, { template: templateId, message: message(err) });
		}

		// Low reasoning: the work is one command, and the picture itself takes long enough.
		const conversation = createConversation({
			profile,
			presetId: preset.id,
			userId: user.id,
			effort: 'low',
			title
		});
		sendMessage(conversation.id, { id: user.id, name: user.name }, text, attachments);
		redirect(303, `/p/${profile.slug}/c/${conversation.id}`);
	}
};
