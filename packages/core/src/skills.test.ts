import { mkdirSync, symlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { paths } from './paths.ts';
import { scanSkills } from './skills.ts';

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
