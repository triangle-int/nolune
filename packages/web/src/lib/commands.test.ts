import { describe, expect, it } from 'vitest';
import { firstLine, partialToolInput } from './commands';

describe('firstLine', () => {
	it('keeps the first line, cut to length', () => {
		expect(firstLine('  ls -la\ncd ~  ')).toBe('ls -la');
		expect(firstLine('x'.repeat(10), 4)).toBe('xxxx…');
	});
});

describe('partialToolInput', () => {
	it('reads finished input', () => {
		expect(
			partialToolInput('{"command":"ls ~","summary":" Looking around ","icon":"folder"}')
		).toEqual({ command: 'ls ~', summary: 'Looking around', icon: 'folder' });
	});

	it('reads fields out of input that is still streaming in', () => {
		expect(partialToolInput('{"summary":"Checking the wea')).toEqual({
			command: null,
			summary: 'Checking the wea',
			icon: null
		});
	});

	it('drops an escape cut off halfway', () => {
		expect(partialToolInput('{"command":"echo caf\\u00')).toMatchObject({ command: 'echo caf' });
	});

	it('waits for the icon name to finish', () => {
		expect(partialToolInput('{"icon":"cloud').icon).toBeNull();
		expect(partialToolInput('{"icon":"cloud-sun","command":"cu').icon).toBe('cloud-sun');
	});
});
