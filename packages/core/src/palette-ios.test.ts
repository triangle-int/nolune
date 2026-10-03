import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { iosPaletteSource, swiftShade } from './palette-ios.ts';

const root = fileURLToPath(new URL('../../../', import.meta.url));

describe("the web's colors for iOS", () => {
	it('are as layout.css and the tint make them: run `node scripts/ios-avatars.ts` if not', () => {
		const css = readFileSync(`${root}packages/web/src/routes/layout.css`, 'utf8');
		const swift = readFileSync(`${root}ios/Shared/Palettes.swift`, 'utf8');
		expect(swift).toBe(iosPaletteSource(css));
	});

	it('take hex colors and see-through ones', () => {
		expect(swiftShade('#0d0d0d')).toBe('Shade(0x0D0D0D)');
		expect(swiftShade('rgb(255 255 255 / 0.08)')).toBe('Shade(0xFFFFFF, 0.08)');
		expect(() => swiftShade('oklch(0.87 0 0)')).toThrow();
	});
});
