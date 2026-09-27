import { icons, type IconNode } from 'lucide';

/** One Lucide icon's drawing, by its kebab-case name (`cloud-sun`); null if Lucide has none. */
export function lucideIcon(name: string): IconNode | null {
	if (!/^[a-z0-9-]{1,64}$/.test(name)) return null;
	const key = name
		.split('-')
		.map((part) => part.charAt(0).toUpperCase() + part.slice(1))
		.join('') as keyof typeof icons;
	return Object.hasOwn(icons, key) ? icons[key] : null;
}
