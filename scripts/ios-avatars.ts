// Writes ios/Shared/Avatars.swift, the avatars as nolune for iOS draws them, from
// packages/core/src/avatars.ts and the avatar colors in the web's layout.css. Run it after
// changing either: `node scripts/ios-avatars.ts`. A test in core checks the file is current.

import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { iosAvatarSource, parseAvatarColors } from '../packages/core/src/avatars-ios.ts';

const root = fileURLToPath(new URL('..', import.meta.url));
const css = readFileSync(`${root}packages/web/src/routes/layout.css`, 'utf8');
writeFileSync(`${root}ios/Shared/Avatars.swift`, iosAvatarSource(parseAvatarColors(css)));
console.log('Wrote ios/Shared/Avatars.swift');
