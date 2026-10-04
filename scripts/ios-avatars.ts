// Writes ios/Shared/Avatars.swift, the avatars as nolune for iOS draws them, from
// packages/core/src/avatars.ts and the avatar colors in the web's layout.css; and
// ios/Shared/Palettes.swift, the web's colors and each avatar's tint of them
// (packages/core/src/tint.ts). Run it after changing any of them: `node scripts/ios-avatars.ts`.
// Tests in core check both files are current.

import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { iosAvatarSource, parseAvatarColors } from '../packages/core/src/avatars-ios.ts';
import { iosPaletteSource } from '../packages/core/src/palette-ios.ts';

const root = fileURLToPath(new URL('..', import.meta.url));
const css = readFileSync(`${root}packages/web/src/routes/layout.css`, 'utf8');
writeFileSync(`${root}ios/Shared/Avatars.swift`, iosAvatarSource(parseAvatarColors(css)));
console.log('Wrote ios/Shared/Avatars.swift');
writeFileSync(`${root}ios/Shared/Palettes.swift`, iosPaletteSource(css));
console.log('Wrote ios/Shared/Palettes.swift');
