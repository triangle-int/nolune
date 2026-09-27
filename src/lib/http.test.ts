import { describe, expect, it } from 'vitest';
import { errorMessage } from './http';

describe('errorMessage', () => {
	it("takes the message of SvelteKit's error()", () => {
		expect(errorMessage('{"message":"Too large"}', 'application/json')).toBe('Too large');
	});

	it('keeps plain text', () => {
		const csrf = 'Cross-site POST form submissions are forbidden';
		expect(errorMessage(csrf, 'text/plain;charset=UTF-8')).toBe(csrf);
	});

	it("leaves out a tunnel's error page", () => {
		const page = '<!DOCTYPE html> <html><body>ngrok gateway error</body></html>';
		expect(errorMessage(page, 'text/html; charset=utf-8')).toBeNull();
	});

	it('has nothing for an empty body', () => {
		expect(errorMessage('  ', null)).toBeNull();
	});
});
