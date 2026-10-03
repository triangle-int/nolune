import { describe, expect, it } from 'vitest';
import { firstLine } from './commands';

describe('firstLine', () => {
	it('keeps the first line, cut to length', () => {
		expect(firstLine('  ls -la\ncd ~  ')).toBe('ls -la');
		expect(firstLine('x'.repeat(10), 4)).toBe('xxxx…');
	});
});
