<script lang="ts">
	import TerminalIcon from '@lucide/svelte/icons/terminal';
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import SearchIcon from '@lucide/svelte/icons/search';
	import GlobeIcon from '@lucide/svelte/icons/globe';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import BookOpenIcon from '@lucide/svelte/icons/book-open';
	import CodeIcon from '@lucide/svelte/icons/code';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import AppWindowIcon from '@lucide/svelte/icons/app-window';
	import PackageIcon from '@lucide/svelte/icons/package';
	import ImageIcon from '@lucide/svelte/icons/image';
	import LaptopIcon from '@lucide/svelte/icons/laptop';
	import DatabaseIcon from '@lucide/svelte/icons/database';
	import type { StepIcon } from '$lib/commands';

	let { icon, class: className = 'size-4' }: { icon: StepIcon; class?: string } = $props();

	const ICONS = {
		terminal: TerminalIcon,
		file: FileTextIcon,
		folder: FolderIcon,
		search: SearchIcon,
		globe: GlobeIcon,
		clock: ClockIcon,
		book: BookOpenIcon,
		code: CodeIcon,
		pencil: PencilIcon,
		trash: Trash2Icon,
		app: AppWindowIcon,
		package: PackageIcon,
		image: ImageIcon,
		laptop: LaptopIcon,
		database: DatabaseIcon
	};

	const Icon = $derived(ICONS[icon]);
</script>

<Icon class={className} />
