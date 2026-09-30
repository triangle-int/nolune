<script lang="ts">
	import type { Avatar } from '@nolune/core/avatars';
	import Phone, { type Message, type Person } from './Phone.svelte';

	// The family demo: three phones on one chat, playing a short scene while it's on screen. What
	// someone types shows on their own phone, "is typing" on the others', and what they send on
	// all three. With reduced motion, or before the script runs, it shows how the scene ends.

	interface Props {
		avatar: Avatar;
	}

	let { avatar }: Props = $props();

	const PEOPLE: Person[] = ['polina', 'timur', 'grandma'];

	const WEATHER = 'Saturday is sunny, 24° by noon. The rain is back on Sunday, so Saturday it is.';
	const PLAN =
		"Lake it is, Timur. I'll remind you on Friday evening to pump up the kayak. Grandma, your apple pie is on the list: blanket, kayak, pie.";

	/** How the scene ends: what the page shows without script and with reduced motion. */
	const ENDING: Message[] = [
		{ id: 1, from: 'polina', text: 'Picnic on Saturday? Can you check the weather?' },
		{ id: 2, from: 'nolune', text: WEATHER, worked: 'Worked for 3s' },
		{ id: 3, from: 'timur', text: 'Lake! Can we take the kayak?' },
		{ id: 4, from: 'grandma', text: "I'll bake my apple pie" },
		{ id: 5, from: 'nolune', text: PLAN, worked: 'Worked for 2s' }
	];

	const empty = (): Record<Person, string> => ({ polina: '', timur: '', grandma: '' });

	let messages = $state<Message[]>(ENDING);
	let drafts = $state(empty());
	let typing = $state<Person[]>([]);
	let working = $state(false);

	let root: HTMLElement;

	class Stopped extends Error {}

	$effect(() => {
		if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
		let run = 0;
		let visible = false;

		const wait = (ms: number, mine: number) =>
			new Promise<void>((done, fail) =>
				setTimeout(() => (mine === run ? done() : fail(new Stopped())), ms)
			);

		async function type(person: Person, text: string, mine: number) {
			typing = [...typing, person];
			for (let i = 1; i <= text.length; i++) {
				drafts[person] = text.slice(0, i);
				await wait(text[i - 1] === ' ' ? 120 : 62, mine);
			}
			await wait(350, mine);
			drafts[person] = '';
			typing = typing.filter((p) => p !== person);
			messages.push({ id: messages.length + 1, from: person, text });
		}

		async function reply(text: string, worked: string, think: number, mine: number) {
			await wait(400, mine);
			working = true;
			await wait(think, mine);
			working = false;
			const message: Message = { id: messages.length + 1, from: 'nolune', text: '', worked };
			messages.push(message);
			const words = text.split(' ');
			for (let i = 1; i <= words.length; i++) {
				messages[messages.length - 1].text = words.slice(0, i).join(' ');
				await wait(70, mine);
			}
		}

		async function play(mine: number) {
			messages = [];
			drafts = empty();
			typing = [];
			working = false;
			await wait(700, mine);
			await type('polina', ENDING[0].text, mine);
			await reply(WEATHER, 'Worked for 3s', 1500, mine);
			await wait(1400, mine);
			// Timur and Grandma write at the same time; Timur sends first.
			await Promise.all([
				type('timur', ENDING[2].text, mine),
				wait(600, mine).then(() => type('grandma', ENDING[3].text, mine))
			]);
			await reply(PLAN, 'Worked for 2s', 1300, mine);
			await wait(7000, mine);
		}

		async function loop() {
			const mine = ++run;
			try {
				while (visible && mine === run) await play(mine);
			} catch (error) {
				if (!(error instanceof Stopped)) throw error;
			}
		}

		// Plays while it's on screen, from the start each time it comes back.
		const observer = new IntersectionObserver(
			([entry]) => {
				if (entry.isIntersecting === visible) return;
				visible = entry.isIntersecting;
				if (visible) loop();
				else run++;
			},
			{ threshold: 0.35 }
		);
		observer.observe(root);
		return () => {
			observer.disconnect();
			run++;
		};
	});
</script>

<div class="phones" bind:this={root}>
	{#each PEOPLE as me (me)}
		<Phone {me} {avatar} {messages} {drafts} {typing} {working} />
	{/each}
</div>

<style>
	.phones {
		display: flex;
		gap: 36px;
		justify-content: center;
	}

	/* On a phone, the three phones side by side, to swipe between. */
	@media (max-width: 960px) {
		.phones {
			justify-content: flex-start;
			overflow-x: auto;
			scroll-snap-type: x mandatory;
			scrollbar-width: none;
			padding: 4px 0 12px;
		}
		.phones > :global(*) {
			flex-shrink: 0;
			scroll-snap-align: center;
		}
	}
</style>
