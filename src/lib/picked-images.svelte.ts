/**
 * Pictures picked for a form: previews, removing one, and dropping files. The form still posts
 * them the ordinary way, through its file input, which is kept in sync here.
 */
export class PickedImages {
	items = $state<{ file: File; url: string }[]>([]);
	error = $state<string | null>(null);
	/** The form's `<input type="file">`. */
	input = $state<HTMLInputElement | null>(null);

	private limits: () => { max: number; maxBytes: number };

	/** `limits` is read on every change, so it can follow the template that's open. */
	constructor(limits: () => { max: number; maxBytes: number }) {
		this.limits = limits;
	}

	get full(): boolean {
		return this.items.length >= this.limits().max;
	}

	/** Adds pictures; with room for only one, a new one replaces the old. */
	add(files: FileList | File[] | null | undefined) {
		const { max, maxBytes } = this.limits();
		this.error = null;
		for (const file of files ?? []) {
			// HEIC from a Mac often has no type; the server checks the content anyway.
			if (!file.type.startsWith('image/') && !/\.(heic|heif)$/i.test(file.name)) {
				this.error = `${file.name} is not a picture.`;
				continue;
			}
			if (file.size > maxBytes) {
				this.error = `${file.name} is larger than ${Math.round(maxBytes / (1024 * 1024))} MB.`;
				continue;
			}
			if (max === 1) this.clear();
			else if (this.items.length >= max) {
				this.error = `At most ${max} pictures.`;
				break;
			}
			this.items.push({ file, url: URL.createObjectURL(file) });
		}
		this.sync();
	}

	remove(index: number) {
		const [item] = this.items.splice(index, 1);
		if (item) URL.revokeObjectURL(item.url);
		this.error = null;
		this.sync();
	}

	clear() {
		for (const item of this.items) URL.revokeObjectURL(item.url);
		this.items = [];
		this.error = null;
		this.sync();
	}

	pick() {
		this.input?.click();
	}

	/** Puts the picked files in the input, so the form posts exactly these. */
	private sync() {
		if (this.input) {
			const transfer = new DataTransfer();
			for (const { file } of this.items) transfer.items.add(file);
			this.input.files = transfer.files;
		}
	}

	/** For the input's change event: the files just chosen join the ones already picked. */
	readonly onInputChange = (event: Event & { currentTarget: HTMLInputElement }) => {
		const chosen = [...(event.currentTarget.files ?? [])];
		// The input now holds only the new choice; add() puts the whole list back.
		this.add(chosen.filter((f) => !this.items.some((i) => i.file === f)));
	};

	/** Attach to an element to accept pictures dropped on it. */
	readonly dropTarget = (node: HTMLElement) => {
		const over = (event: DragEvent) => {
			if (!event.dataTransfer?.types.includes('Files')) return;
			event.preventDefault();
			node.dataset.dragging = '';
		};
		const leave = () => delete node.dataset.dragging;
		const drop = (event: DragEvent) => {
			if (!event.dataTransfer?.files.length) return;
			event.preventDefault();
			leave();
			this.add(event.dataTransfer.files);
		};
		node.addEventListener('dragover', over);
		node.addEventListener('dragleave', leave);
		node.addEventListener('drop', drop);
		return () => {
			node.removeEventListener('dragover', over);
			node.removeEventListener('dragleave', leave);
			node.removeEventListener('drop', drop);
		};
	};
}
