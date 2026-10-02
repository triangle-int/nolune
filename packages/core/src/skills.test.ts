import { mkdirSync, symlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import {
	appendRow,
	commitQueuedRows,
	createConversation,
	getConversation,
	insertQueued
} from './conversations.ts';
import { paths, profileSkillsDir } from './paths.ts';
import { setSkillsEnabled } from './profiles.ts';
import { chatToolChanges, reloadTools } from './runner.ts';
import {
	catalogSkills,
	createSkill,
	renderSkillsCatalog,
	scanSkills,
	skillsInPrompt
} from './skills.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

function writeSkill(dir: string, name: string): void {
	mkdirSync(join(dir, name), { recursive: true });
	writeFileSync(
		join(dir, name, 'SKILL.md'),
		`---\nname: ${name}\ndescription: Does ${name} things.\n---\n\n# ${name}\n`
	);
}

describe('scanSkills', () => {
	it('finds a skill folder that is a symlink', () => {
		const target = join(paths.home, 'elsewhere');
		writeSkill(target, 'linked');
		mkdirSync(paths.globalSkills, { recursive: true });
		symlinkSync(join(target, 'linked'), join(paths.globalSkills, 'linked'));
		symlinkSync(join(target, 'missing'), join(paths.globalSkills, 'broken'));

		const { skills, warnings } = scanSkills(join(paths.home, 'no-profile-skills'));
		const linked = skills.find((s) => s.name === 'linked');
		expect(linked).toMatchObject({ scope: 'global', description: 'Does linked things.' });
		expect(skills.some((s) => s.name === 'broken')).toBe(false);
		expect(warnings).toEqual([]);
	});
});

describe('a chat already going', () => {
	function chatWithAReply() {
		const family = makeFamily();
		const chat = createConversation({
			profile: family.profile,
			presetId: makePreset().id,
			userId: family.user.id
		});
		insertQueued({
			conversationId: chat.id,
			senderId: family.user.id,
			senderName: family.user.name,
			text: 'Hi'
		});
		commitQueuedRows(chat.id);
		appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify([{ type: 'text', text: 'Hello!' }])
		});
		return { ...family, chat };
	}

	it('gets skills added, changed or turned off when its tools are reloaded', () => {
		const { chat, profile } = chatWithAReply();
		const dir = profileSkillsDir(profile.slug);
		createSkill(dir, 'tax-forms', 'Fills in tax forms.');

		expect(chatToolChanges(chat.id)).toEqual({
			skills: { added: ['tax-forms'], changed: [], removed: [] },
			services: null
		});
		// Its prompt stays as it was until someone reloads.
		expect(getConversation(chat.id)?.systemPrompt).toBe(chat.systemPrompt);
		expect(reloadTools(chat.id)?.skills?.added).toEqual(['tax-forms']);
		expect(getConversation(chat.id)?.systemPrompt).toContain(
			'<description>Fills in tax forms.</description>'
		);
		expect(chatToolChanges(chat.id)).toBeNull();

		writeFileSync(
			join(dir, 'tax-forms', 'SKILL.md'),
			'---\nname: tax-forms\ndescription: Fills in tax forms, and checks them.\n---\n'
		);
		expect(chatToolChanges(chat.id)?.skills).toEqual({
			added: [],
			changed: ['tax-forms'],
			removed: []
		});
		reloadTools(chat.id);
		expect(getConversation(chat.id)?.systemPrompt).toContain(
			'<description>Fills in tax forms, and checks them.</description>'
		);

		setSkillsEnabled(profile.id, ['tax-forms'], false);
		expect(chatToolChanges(chat.id)?.skills?.removed).toEqual(['tax-forms']);
		reloadTools(chat.id);
		expect(getConversation(chat.id)?.systemPrompt).not.toContain('tax-forms');
	});

	it("knows them from its prompt's catalog", () => {
		const { chat, profile } = chatWithAReply();
		expect(skillsInPrompt(chat.systemPrompt)).toEqual(catalogSkills(profile));
		const odd = {
			name: 'odd',
			description: 'Fills in <tax> forms & more.',
			location: '/a&b/SKILL.md'
		};
		expect(skillsInPrompt(`Before.\n${renderSkillsCatalog([odd])}\nAfter.`)).toEqual([odd]);
		expect(skillsInPrompt('Skills are folders… There are no skills yet.')).toEqual([]);
		// A prompt nolune can't read them back from: they count as unchanged.
		expect(skillsInPrompt('An old prompt.')).toBeNull();
	});
});
