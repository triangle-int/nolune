import { describeSchedule } from '@nolune/core/schedule';
import { plural } from '../plural';

const p = plural('en');

/**
 * The interface in English, which every other language follows key for key. `{slots}` are filled
 * by the Rich component; see src/lib/i18n/index.ts.
 */
export const en = {
	common: {
		add: 'Add',
		cancel: 'Cancel',
		close: 'Close',
		continue: 'Continue',
		copied: 'Copied',
		copy: 'Copy',
		create: 'Create',
		default: 'Default',
		delete: 'Delete',
		done: 'Done',
		edit: 'Edit',
		more: 'More',
		newChat: 'New chat',
		remove: 'Remove',
		rename: 'Rename',
		save: 'Save',
		stop: 'Stop',
		tryAgain: 'Try again',
		checking: 'Checking…',
		starting: 'Starting…',
		uploading: 'Uploading…',
		removeFile: (name: string) => `Remove ${name}`,
		characters: (count: number, max: number) => `${count} / ${max} characters`
	},

	errors: {
		requestFailed: (status: number) => `Request failed (${status})`,
		uploadFailed: (status: number | null) =>
			`Upload failed (${status === null ? 'no connection' : status})`,
		couldNotSave: 'Could not save.',
		somethingWentWrong: 'Something went wrong.',
		thatDidntWork: 'That didn’t work.',
		tooManyFiles: (max: number) => `At most ${max} files per message`,
		tooLarge: (mb: number) => `Larger than ${mb} MB`,
		notSignedIn: 'Not signed in',
		adminsOnly: 'Admins only',
		profileNotFound: 'Profile not found',
		conversationNotFound: 'Conversation not found',
		folderNotFound: 'Folder not found',
		automationNotFound: 'Automation not found',
		messageEmpty: 'Message is empty',
		noFile: 'No file'
	},

	time: {
		justNow: 'just now',
		seconds: (s: number) => `${s}s`,
		minutesSeconds: (m: number, s: number) => `${m}m ${s}s`,
		hoursMinutes: (h: number, m: number) => `${h}h ${m}m`,
		minutesAgo: (n: number) => `${n} min ago`,
		hoursAgo: (n: number) => `${n} h ago`
	},

	units: { bytes: 'B', kilobytes: 'KB', megabytes: 'MB' },

	settings: {
		title: 'Settings',
		description: 'How nolune looks on this device.',
		theme: 'Theme',
		system: 'System',
		light: 'Light',
		dark: 'Dark',
		language: 'Language',
		languageAuto: 'Same as the browser',
		languageHint: 'For menus and buttons. nolune answers in the language you write in.',
		technical: 'Show technical details',
		technicalHint: 'Show the exact commands nolune runs, token usage and prompt caching.',
		expandSteps: 'Always show steps',
		expandStepsHint:
			'Open the list of what nolune did under each reply, instead of keeping it folded.',
		sounds: 'Sounds',
		soundsHint: 'Soft sounds where nolune moves on its own, like the welcome of a new profile.',
		logOut: 'Log out',
		deviceOnly: 'These settings are saved on this device only.'
	},

	userMenu: {
		account: 'Account',
		settings: 'Settings',
		allProfiles: 'All profiles',
		modelsAndKeys: 'Models & keys',
		logOut: 'Log out'
	},

	header: {
		openMenu: 'Open menu'
	},

	sidebar: {
		label: 'Sidebar',
		description: 'Chats, folders and pages of this profile.',
		profiles: 'Profiles',
		newProfile: 'New profile',
		openSidebar: 'Open sidebar',
		closeSidebar: 'Close sidebar',
		searchChats: 'Search chats',
		images: 'Images',
		automations: 'Automations',
		memory: 'Memory',
		skills: 'Skills',
		people: 'People & profile',
		folders: 'Folders',
		newFolder: 'New folder',
		chats: 'Chats',
		working: ', working',
		showChats: (folder: string) => `Show the chats in ${folder}`,
		hideChats: (folder: string) => `Hide the chats in ${folder}`,
		emptyFolder: "Drag chats here, or start one on the folder's page.",
		dropToTakeOut: 'Drop here to take the chat out of its folder.',
		noChats: 'Your chats will show up here.',
		searchDescription: 'Find a chat by its title',
		searchPlaceholder: 'Search chats…',
		noResults: 'No chats found.',
		deleteChatBody: 'This deletes {name} for everyone in {profile}.'
	},

	notifications: {
		title: 'Notifications',
		unseen: (n: number) => `Notifications, ${n} new`,
		clearAll: 'Clear all',
		new: 'New',
		showLess: 'Show less',
		showAll: 'Show all',
		dismiss: 'Dismiss',
		openChat: 'Open chat',
		continueInChat: 'Continue in chat',
		empty:
			'Nothing yet. Ask nolune for a reminder or a daily check, and what it finds shows up here.'
	},

	chat: {
		options: 'Chat options',
		placeholder: 'Ask nolune',
		placeholderRunning: 'Add something while nolune works…',
		disclaimer: 'nolune can make mistakes, and it can change files on this computer.',
		reconnecting: 'Reconnecting…',
		empty: 'Ask for something to get started.',
		readsAfterStep: 'nolune reads this after its current step',
		working: 'nolune is working',
		thinking: 'Thinking',
		writing: 'Writing',
		cutOff: 'The reply was cut off because it got too long.',
		refused: 'nolune declined to continue this request.',
		automation: (title: string) => `Automation · ${title}`,
		fromNolune: (to: string) => `From nolune, to ${to}`,
		finishedInBackground: (title: string) => `Finished in the background · ${title}`,
		inBackground: 'Working in the background',
		aCommand: 'A command',
		subagent: (name: string) => `Subagent ${name}`,
		stopping: 'stopping',
		errorTitle: 'Something went wrong while nolune was answering.',
		unanswered: "nolune hasn't answered this yet.",
		scrollToBottom: 'Scroll to the newest message',
		subagentBanner: 'Subagent {name}: nolune started it from {parent}, and it reports back there.',
		subagentOnly: (name: string) => `Only the agent that started ${name} writes here.`,
		subagentRetry: 'Only the agent that started it runs a subagent.',
		hiddenBanner: 'A background run from an automation. Send a message to keep it in your chats.',
		/** A subagent's parent chat that has no title yet. */
		aChat: 'a chat',
		deleteTitle: 'Delete chat?',
		deleteBody: 'This deletes {name} for everyone in the profile.',
		renameTitle: 'Rename chat',
		chatName: 'Chat name',
		couldNotRename: 'Could not rename the chat.',
		// Technical details
		// Switching the model or reasoning level
		switchTitle: (model: string) => `Switch to ${model}?`,
		anotherModel: 'another model',
		effortTitle: (level: string) => `Change reasoning to ${level}?`,
		switchCache: (model: boolean, missTokens: string | null) =>
			`nolune keeps this chat in a cache, so each reply only pays for what's new. ${model ? 'Another model' : 'Another reasoning level'} can't use it: the next reply reads the whole chat again, which takes longer and costs more${missTokens ? ` (a prompt cache miss of about ${missTokens} tokens)` : ''}.`,
		switchFiles:
			"Some pictures and PDFs in this chat don't carry over to another provider: the new model gets where their files are, and can look at them again.",
		switch: 'Switch',
		change: 'Change',
		/** The models that wrote a reply, in order. */
		models: (models: string[]) => models.join(', then '),
		usage: 'Usage',
		tokensInOut: (input: string, output: string) => `${input} tokens in, ${output} out`,
		cache: 'Cache',
		lastReply: 'Last reply',
		wholeConversation: 'Whole conversation',
		cacheSummary: (label: string, rate: string, read: string, written: string, uncached: string) =>
			`${label}: ${rate} cached (${read} read, ${written} written, ${uncached} uncached)`,
		cacheMiss: (tokens: string) => `Cache miss · ${tokens} tokens processed again`,
		cacheExpired: (ttl: '5m' | '1h') =>
			`Over ${ttl === '5m' ? '5 minutes' : 'an hour'} passed since the previous step, so the cached conversation expired and was processed again (slower and costlier).`,
		cacheBroken:
			'Context that should have come from the cache was processed again (slower and costlier). Switching the model or the reasoning level, moving the chat to another folder or changing its folder cause this once.',
		contextChip: (used: string, window: string, rate: string) =>
			`${used} / ${window} · ${rate} cached`,
		contextUsed: (used: string, window: string) =>
			`Context used by the last reply: ${used} of ${window}`,
		contextMenu: (used: string, window: string) => `Context ${used} / ${window}`,
		cachedOverall: (rate: string) => `${rate} cached overall`
	},

	steps: {
		thinking: 'Thinking',
		thinkingDots: 'Thinking…',
		running: (command: string) => `Running ${command}`,
		runningACommand: 'Running a command',
		ranACommand: 'Ran a command',
		gettingReady: 'Getting ready…',
		preparing: 'Preparing a command…',
		stopped: 'Stopped',
		thoughtFor: (duration: string) => `Thought for ${duration}`,
		thoughtForAMoment: 'Thought for a moment',
		workedFor: (duration: string) => `Worked for ${duration}`,
		workedForAMoment: 'Worked for a moment',
		ranCommands: (n: number) => p(n, { one: `Ran ${n} command`, other: `Ran ${n} commands` }),
		failedCount: (n: number) => `${n} failed`,
		blockedCount: (n: number) => `${n} blocked`,
		failed: 'failed',
		didntWork: "didn't work",
		statusStopped: 'stopped',
		statusNotRun: 'not run',
		statusBlocked: 'blocked',
		command: 'Command',
		theCommand: 'The command nolune ran',
		inFolder: (cwd: string) => `in ${cwd}`,
		noOutputYet: 'No output yet…',
		noOutput: '(no output)'
	},

	composer: {
		placeholder: 'Ask anything',
		message: 'Message',
		attach: 'Attach files',
		send: 'Send'
	},

	model: {
		model: 'Model',
		reasoning: 'Reasoning',
		efforts: {
			low: { label: 'Low', hint: 'Fastest answers' },
			medium: { label: 'Medium', hint: 'Good for most things' },
			high: { label: 'High', hint: 'Thinks longer on harder tasks' },
			xhigh: { label: 'Extra high', hint: 'Takes its time' },
			max: { label: 'Max', hint: 'Slowest, for the hardest problems' }
		}
	},

	commandMode: {
		title: 'Commands in this chat',
		auto: 'Auto',
		autoHint: 'A model checks each command before it runs.',
		unrestricted: 'Unrestricted',
		unrestrictedHint: 'Commands run without a check. Not recommended.',
		adminsOnly: 'Only an admin can turn the checks off for a chat.',
		labelAuto: 'Commands: auto mode',
		labelUnrestricted: 'Commands: unrestricted'
	},

	newChat: {
		greeting: (name: string) => `What can I help with, ${name}?`,
		greetingNoName: 'What can I help with?',
		noModels:
			'No models are set up yet. An admin can add one on the Models page or with {command}.',
		couldNotStart: 'Could not start the chat.',
		/**
		 * The general chips a profile has until nolune makes some from its memory (core's
		 * DEFAULT_SUGGESTIONS, by id): the start of a message for the person to finish.
		 */
		suggestions: {
			reminder: { label: 'Set a reminder', text: 'Remind me tomorrow at 9:00 to ' },
			weather: {
				label: 'Daily weather check',
				text: 'Every weekday at 7:30, check the weather and tell us if we need umbrellas.'
			},
			file: { label: 'Find a file', text: 'Find the file on this computer called ' },
			space: {
				label: 'Check free space',
				text: 'How much free disk space is left on this computer?'
			}
		},
		pickModel: 'Pick a model.',
		pickEffort: 'Pick a reasoning level.',
		folderGone: 'That folder was deleted. Pick another one.'
	},

	folders: {
		inFolder: (name: string) => `In the folder ${name}`,
		startInFolder: 'Start in a folder',
		startOutside: 'Start outside the folder',
		noFolder: 'No folder',
		newFolderDots: 'New folder…',
		moveTo: 'Move to folder',
		moveTechnical:
			'Moving builds the system prompt again, so the next reply re-reads the whole conversation once (a prompt cache miss).',
		move: 'After a move, the next reply takes a little longer.',
		newTitle: 'New folder',
		newDescription:
			'Keep related chats together. Every chat in a folder gets its instructions and files.',
		namePlaceholder: 'Trip to Japan',
		name: 'Folder name',
		create: 'Create folder',
		renameTitle: 'Rename folder',
		couldNotRename: 'Could not rename the folder.',
		deleteTitle: 'Delete folder?',
		deleteBody:
			'{name} is deleted for everyone in the profile. Its chats move back to your chat list, without its instructions and files. The files are moved to ~/.nolune/trash.',
		// The folder's page
		options: 'Folder options',
		newChatIn: (name: string) => `New chat in ${name}`,
		instructions: 'Instructions',
		instructionsHint: 'What nolune should know or do in every chat here.',
		instructionsPlaceholder:
			"We're planning two weeks in Japan in April with the kids (7 and 10). Keep plans relaxed and the budget under ¥600,000.",
		changesTechnical:
			'Chats in the folder get changes at their next message, which re-reads the conversation once (a prompt cache miss).',
		changes: 'Chats in the folder get changes at their next message.',
		files: 'Files',
		addFiles: 'Add files',
		filesHint:
			"Pictures, documents, anything. nolune gets where they're saved and opens them when they matter.",
		noFiles: 'No files yet',
		savedIn: (dir: string) => `Saved in ${dir}`,
		couldNotAdd: 'Could not add the files.',
		couldNotAddOffline: 'Could not add the files. Check the connection and try again.',
		chats: 'Chats',
		noChats:
			'Chats you start here show up here. You can also drag chats onto the folder in the sidebar.'
	},

	attachments: {
		open: (name: string) => `Open ${name}`,
		download: 'Download',
		onlyPath: (note: string) => `nolune got only where it's saved: ${note}`
	},

	markdown: {
		text: 'text',
		picture: 'Picture',
		notAvailable: 'Not available'
	},

	images: {
		title: 'Images',
		shapes: { square: 'Square', portrait: 'Portrait', landscape: 'Landscape', auto: 'Auto' },
		shape: 'Shape',
		groups: 'Template groups',
		noTemplates: 'No templates yet.',
		cantMakeYet: "nolune can't make pictures yet.",
		needsKeyAdmin: 'It needs an {provider} API key. {link}.',
		addKeyLink: 'Add it under Models & keys',
		needsKey: (provider: string) => `It needs an ${provider} API key. Ask an admin to add one.`,
		adminSetsUp: 'An admin sets this up on the computer nolune runs on.',
		describe: 'Describe an image',
		describeHint: 'nolune makes the picture in a new chat, where you can ask for changes.',
		onlyPictures: 'Only pictures can be used here.',
		atMostPictures: (n: number) => `At most ${n} pictures.`,
		/** `label` is the template's, like "Photo of the birthday kid". */
		changePicture: (label: string | null) => `Change ${label ?? 'the picture'}`,
		addPicture: 'Add a picture',
		draw: 'Draw',
		choosePhotoOfDrawing: 'Choose a photo of a drawing',
		usePhotoOfDrawing: 'Use a photo of a drawing',
		startDrawing: 'Start drawing',
		takePhoto: 'Take a photo',
		choosePhoto: 'Choose a photo',
		tryIt: 'Try it',
		/** `label` is the template's setting, like "Style". */
		yourOwn: (label: string) => `your own ${label.toLowerCase()}`,
		pickFromList: (label: string) => `Pick a ${label.toLowerCase()} from the list`,
		custom: 'Custom…',
		addAnything: 'Add anything else…',
		uploadingPicture: 'Uploading the picture…',
		addDrawingFirst: 'Add a drawing first.',
		addPhotoFirst: 'Add a photo first.',
		generate: 'Generate',
		noModel: 'No chat model is set up yet.',
		notPicture: (name: string) => `${name} is not a picture.`,
		templateGone: 'That template no longer exists.',
		describeFirst: 'Describe the picture first.'
	},

	drawing: {
		title: 'Drawing',
		tool: 'Tool',
		pen: 'Pen',
		eraser: 'Eraser',
		undo: 'Undo',
		penSize: 'Pen size',
		color: (color: string) => `Color ${color}`,
		use: 'Use this drawing'
	},

	emoji: {
		value: (label: string, value: string) => `${label}: ${value || 'none'}`,
		pickUpTo: (n: number) => `Pick up to ${n}`,
		removeLast: 'Remove the last emoji'
	},

	memory: {
		title: 'Memory',
		intro: (profile: string) =>
			`What nolune remembers for ${profile}, shared by everyone in it. Every chat starts with the pinned Core note; nolune reads the others when a chat needs them and saves what it learns along the way. To add something, just tell it in a chat, like "Remember that Anna is allergic to nuts".`,
		/** After the number of memories, in large type. */
		total: (memories: number, topics: number) =>
			`${memories === 1 ? 'memory' : 'memories'} in ${p(topics, { one: `${topics} topic`, other: `${topics} topics` })}`,
		newThisWeek: (n: number) => `${n} new this week`,
		updated: (ago: string) => `updated ${ago}`,
		memories: (n: number) => p(n, { one: `${n} memory`, other: `${n} memories` }),
		pinned: 'pinned, in every new chat',
		edit: (topic: string) => `Edit ${topic}`,
		forget: (topic: string) => `Forget ${topic}`,
		note: (topic: string) => `${topic} note`,
		corePlaceholder:
			'For example:\n- Anna and Ben are the parents, Mia is 7\n- We speak Russian at home\n- Mia is allergic to nuts',
		coreEmpty:
			"Nothing yet. Put here what nolune should keep in mind in every chat: who's in the family, the languages you speak, allergies. nolune adds to it too.",
		forgetTitle: (topic: string) => `Forget "${topic}"?`,
		forgetBody:
			'nolune forgets everything in {path} for everyone in {profile}. Chats that already read it keep what they read.',
		forgetButton: 'Forget',
		empty: 'The note is empty. To delete it, use Forget.',
		conflict: (problem: string) =>
			`${problem} Save again to keep your version, or cancel to see nolune's.`,
		saved: 'Saved. New chats will see the change.',
		forgot: (path: string) => `nolune forgot everything in ${path}.`,
		learn: 'Learn from chats',
		learnHint:
			"When a chat has been quiet for a couple of minutes, nolune reads it over and saves what's worth remembering. That's one short extra request to the chat's model each time. When this is off, nolune saves only what it thinks of while chatting.",
		// The grid of dots
		learned: (ago: string) => `learned ${ago}`,
		learnedAWhileAgo: 'learned a while ago',
		topicLabel: (topic: string, n: number) =>
			`${topic}: ${p(n, { one: `${n} memory`, other: `${n} memories` })}. Show the note.`,
		nothingInIt: 'Nothing in it yet',
		showFewer: 'Show fewer',
		showAllTopics: (n: number) => `Show all ${n} topics`,
		older: 'Older',
		newer: 'Newer',
		nothingYet: 'Nothing remembered yet. Each thing nolune learns becomes a dot here.',
		/** Where memory keeps things: the same in every profile. */
		categories: {
			core: 'Core',
			people: 'People',
			home: 'Home',
			health: 'Health',
			plans: 'Plans',
			routines: 'Routines',
			pets: 'Pets',
			places: 'Places',
			projects: 'Projects',
			other: 'Other',
			/** Notes from before there were categories. */
			unsorted: 'Unsorted'
		},
		unsortedHint:
			'From before memory had categories: nolune reads it, but adds nothing to it. Move it into a category to keep it growing.',
		/** On a person's note that is about a member of the profile. */
		memberNote: (name: string) => `member: ${name}`,
		move: {
			button: (topic: string) => `Move ${topic}`,
			title: (topic: string) => `Move "${topic}"`,
			body: 'Choose where it belongs. If that note is there already, the two become one.',
			to: 'Move to',
			choose: 'Choose where',
			categories: 'Categories',
			newPerson: 'Someone new…',
			newProject: 'A new project…',
			personName: 'Their name',
			projectName: 'Project name',
			moveHint: (target: string) => `It becomes ${target}.`,
			mergeHint: (from: string, into: string) =>
				`What ${from} says is added to ${into}, without repeats, and ${from} is gone. Use this for two notes about the same person.`,
			move: 'Move',
			merge: 'Merge',
			moved: (from: string, to: string) => `Moved ${from} to ${to}.`,
			merged: (from: string, into: string) => `Merged ${from} into ${into}.`
		},
		/** What nolune saved from a chat by itself: in that chat, and at the top of this page. */
		changes: {
			saved: (n: number) => `Saved ${p(n, { one: `${n} memory`, other: `${n} memories` })}`,
			added: 'Added',
			changed: 'Changed',
			before: (text: string) => `was: ${text}`,
			openNote: (topic: string) => `Open ${topic} in Memory`,
			undo: 'Undo',
			undone: 'Undone',
			undoneBy: (name: string) => `Undone by ${name}`,
			changedSince:
				"It changed since, so it can't be undone here. Edit the note on the Memory page.",
			alreadyUndone: 'It was already undone.',
			recent: 'Saved from chats',
			recentHint: 'What nolune noted by itself after chats went quiet, in the last two weeks.',
			fromChat: 'from {chat}',
			deletedChat: 'from a deleted chat',
			showAll: (n: number) => `Show all ${n}`,
			untitled: 'a chat'
		}
	},

	automations: {
		title: 'Automations',
		intro:
			'Things nolune does on its own: reminders, regular checks and replies to other apps. What it finds shows up under the bell. To add or change one, just ask nolune in a chat.',
		today: 'Today',
		previousMonth: 'Previous month',
		nextMonth: 'Next month',
		dayLabel: (day: string, runs: number) =>
			`${day}: ${runs ? p(runs, { one: `${runs} automation run`, other: `${runs} automation runs` }) : 'nothing'}`,
		nothingRan: 'Nothing ran on this day.',
		nothingRuns: 'Nothing runs on this day.',
		timeZone: (zone: string) => `Times are in the computer's time zone (${zone}).`,
		all: 'All automations',
		next: (when: string) => `next ${when}`,
		states: { paused: 'paused', done: 'done' },
		runNow: 'Run now',
		pause: 'Pause',
		resume: 'Resume',
		description: 'Description',
		descriptionPlaceholder: 'What it does, in one sentence',
		instructions: 'Instructions for nolune',
		instructionsTechnical: (model: string, effort: string) =>
			`Instructions for nolune (${model}, reasoning ${effort})`,
		script: 'Script: runs without the model and calls {command} when nolune is needed',
		webhook: 'Webhook URL. Keep it secret: anyone with it can start a run. POST JSON to it.',
		confirmDelete: (name: string) => `Delete "${name}"?`,
		recentRuns: (n: number) => `Recent runs (${n})`,
		noRuns: 'No runs yet',
		statuses: {
			pending: 'waiting',
			running: 'running',
			ok: 'done',
			notified: 'notified',
			silent: 'nothing to report',
			stopped: 'stopped',
			failed: 'failed'
		},
		sources: {
			cron: 'scheduled',
			once: 'scheduled',
			webhook: 'webhook',
			wake: 'woken by script',
			manual: 'run by hand'
		},
		scriptRun: (status: string) => `script ${status}`,
		view: 'view',
		output: 'output',
		empty: 'No automations yet. Ask nolune in a chat, for example:',
		examples: [
			'"Every weekday at 7:30, check the weather and tell us if we need umbrellas."',
			'"Remind Anna tomorrow at 17:00 to pick up the parcel."',
			'"Check my email every 10 minutes and tell me when the school writes."'
		],
		defaultModel: 'the default model',
		saved: (name: string) => `Saved "${name}".`,
		started: (name: string) => `Started "${name}". Its reply shows up under the bell.`,
		startedScript: (name: string) => `Started the script of "${name}".`,
		paused: (name: string) => `"${name}" is paused.`,
		resumed: (name: string) => `"${name}" is on again.`,
		deleted: (name: string) => `Deleted "${name}".`,
		// Schedules
		describe: describeSchedule,
		/** After an automation's name: "Check email · every 10 minutes". */
		scheduleAfterName: (schedule: string) => schedule.toLowerCase(),
		customSchedule: 'On a custom schedule',
		once: 'Once',
		onceAt: (when: string) => `Once, ${when}`,
		onWebhook: 'When another app calls its link',
		/** "tomorrow at 07:30" */
		dayAt: (day: string, time: string) => `${day} at ${time}`
	},

	skills: {
		title: 'Skills',
		intro:
			"Skills are instructions nolune follows for specific tasks. It sees the name and description of every skill that's on, and reads the rest when a task calls for it. Turning off skills this profile doesn't need keeps nolune focused. Changes apply to new chats.",
		summary: (on: number, total: number, tokens: string) =>
			`${on} of ${total} on · about ${tokens} tokens at the start of every new chat`,
		madeFor: (profile: string) => `Made for ${profile}`,
		shared: 'Shared by all profiles',
		builtIn: 'Built into nolune',
		allOff: 'Turn all off',
		allOn: 'Turn all on',
		tokens: (n: string) => `~${n} tokens`,
		use: (name: string) => `Use ${name}`,
		empty:
			'No skills yet. When nolune works out how to do something, it can save that as a skill for next time.',
		gone: 'That skill no longer exists.'
	},

	profile: {
		title: 'People & profile',
		name: 'Name',
		profileName: 'Profile name',
		folder: (slug: string) => `Folder: ~/.nolune/profiles/${slug} (doesn't change)`,
		avatar: 'Avatar',
		avatarHint:
			"How nolune looks in this profile's chats. Everyone here sees the same one, and nolune can change it when asked.",
		soul: 'Soul',
		soulHint: (profile: string) =>
			`Who nolune is for ${profile}: its character, what it cares about, how it talks. Every chat starts with it, and nolune changes it too when you ask it to be different.`,
		soulPlaceholder:
			"You're warm and a little playful, and you keep answers short. With the kids you explain things simply and never talk down to them. When you don't know something, you say so.",
		changesTechnical:
			'Chats get changes at their next message, which re-reads the conversation once (a prompt cache miss).',
		changes: 'Chats get changes at their next message.',
		members: 'Members',
		membersHint:
			'Everyone here sees and writes in the same chats, and can ask nolune for anything.',
		personToAdd: 'Person to add',
		chooseSomeone: 'Choose someone to add',
		everyoneIsMember: 'Everyone is already a member.',
		deleteTitle: 'Delete profile',
		deleteHint: 'Deletes all its chats for everyone. The folder is moved to ~/.nolune/trash.',
		deleteButton: 'Delete this profile',
		deleteConfirm: (name: string) => `Delete "${name}"?`,
		deleteBody: 'All its chats are deleted for everyone. The folder is moved to ~/.nolune/trash.',
		renamed: 'Renamed.',
		soulSaved: 'Saved the soul.',
		soulRemoved: 'Removed the soul.',
		added: (name: string) => `Added ${name}.`,
		removed: 'Removed.',
		/** Under a member: their note in memory. */
		memberNote: (note: string) => `Note: ${note}`,
		noteStarts: (note: string) => `Note: ${note}, started when there's something to write`,
		mayHaveNote: 'nolune may have a note about them already',
		chooseNote: 'Choose note',
		changeNote: 'Change note',
		linked: (name: string, note: string) => `${name}'s note is ${note} now.`,
		/** Which note in memory is about a member. */
		chooser: {
			addTitle: (name: string) => `Does nolune know ${name} already?`,
			linkTitle: (name: string) => `Which note is about ${name}?`,
			body: (name: string) =>
				`Memory has notes that may be about ${name}. Choose theirs, so nolune knows what it says is about them and adds what they tell it about themselves.`,
			linkBody: (name: string) =>
				`Choose the note about ${name}: nolune adds to it what they tell it about themselves.`,
			current: 'now',
			alsoCalled: (names: string) => `Also called: ${names}`,
			more: (n: number) => `and ${n} more`,
			newNote: 'Someone else: start a new note',
			privacy: (name: string) =>
				`${name} will be able to read everything in this profile's memory, this note too. Check that it holds nothing meant to be kept from them, like a surprise.`,
			add: (name: string) => `Add ${name}`,
			link: 'Link'
		}
	},

	/** The assistant avatars' names, as the profile page shows them. */
	avatars: {
		probe: 'Probe',
		campfire: 'Campfire',
		lantern: 'Lantern',
		planet: 'Planet',
		quantum: 'Quantum',
		comet: 'Comet',
		moon: 'Moon',
		satellite: 'Satellite'
	},

	profiles: {
		title: 'Profiles',
		intro: 'Each profile has its own chats, memory and skills, shared by its members.',
		none: "You're not in any profile yet. Create one below, or ask someone to add you to theirs.",
		new: 'New profile',
		namePlaceholder: 'e.g. Family, Grandma, Homework',
		name: 'Profile name',
		needsName: 'Give the profile a name.'
	},

	/** A new profile's welcome. nolune speaks for itself here: "I" is the assistant. */
	welcome: {
		title: "Let's make this place yours.",
		subtitle: "A few quick questions, and you'll feel at home.",
		go: "Let's go",
		takesAMinute: 'Takes about a minute',
		skipIntro: 'Skip the intro',
		skipHint: 'Click anywhere to skip',
		back: 'Back',
		soundsOn: 'Turn sounds on',
		soundsOff: 'Turn sounds off',
		progress: (n: number, total: number) => `Step ${n} of ${total}`,
		model: {
			title: 'How should I think?',
			subtitle: 'Pick what powers me. You can add more under Models & keys later.',
			askAdmin:
				'I need a model before I can chat. Ask whoever set up nolune to add one under Models & keys; everything else works in the meantime.',
			choices: {
				'claude-plan': {
					title: 'Claude plan',
					about: 'A Pro or Max plan, through Claude Code on this computer'
				},
				'chatgpt-plan': {
					title: 'ChatGPT plan',
					about: 'A Plus or Pro plan, signed in with ChatGPT'
				},
				anthropic: { title: 'Anthropic API key', about: 'Claude models, paid as you go' },
				openai: { title: 'OpenAI API key', about: 'GPT models, paid as you go' },
				openrouter: {
					title: 'OpenRouter API key',
					about: 'Claude, GPT, Gemini and more, on one key'
				}
			},
			checkingPlan: 'Checking the sign-in…',
			pasteKey: (label: string) => `Paste your ${label} key`,
			checkKey: 'Check the key',
			keyWorks: 'The key works.',
			finishOnAdmin: 'Sign in on {link}, then check again.',
			checkAgain: 'Check again',
			pickModel: 'Which model should new chats start with?',
			asking: 'Asking for the models…',
			modelId: 'Model id',
			/** Signing in with ChatGPT right in the step. */
			chatgpt: {
				title: 'Sign in with ChatGPT',
				about:
					"Sign in on ChatGPT's page and allow nolune to use your Plus or Pro plan. Chats then count toward the plan's usage, like ChatGPT's own.",
				newTab: "ChatGPT's sign-in page opens in a new tab.",
				waiting: "Once you've signed in there, this page goes on by itself.",
				starting: "Getting ChatGPT's sign-in page ready…",
				tryAgain: 'Try again',
				otherDevice: 'Signing in on another device?'
			}
		},
		avatar: {
			title: 'Who should I be?',
			subtitle: (profile: string) => `Pick a face for ${profile}.`,
			thisOne: 'This one',
			hello: 'Nice to meet you.'
		},
		memory: {
			title: 'Have we met before?',
			subtitle:
				'If you use ChatGPT, Claude or Gemini, it already knows things about you. Ask it with this prompt, then paste its answer here.',
			copyPrompt: 'Copy prompt',
			prompt: 'The prompt',
			open: (name: string) => `Open ${name}`,
			haveIt: 'I have it',
			startFresh: 'Start fresh',
			pasteTitle: 'Paste it here',
			pastePlaceholder: 'The whole answer, code block and all',
			sections: {
				instructions: 'Instructions',
				identity: 'Identity',
				career: 'Career',
				projects: 'Projects',
				preferences: 'Preferences',
				other: 'Other'
			},
			remember: (n: number) => p(n, { one: `Remember ${n} thing`, other: `Remember ${n} things` }),
			rememberThis: 'Remember this',
			reading: 'Reading it…',
			saving: 'Saving…',
			empty: 'Paste what the other assistant answered.',
			tooLong: "That's too long to be one person's memories. Paste just the answer to the prompt.",
			notAnExport:
				"That doesn't look like the answer to the prompt. Paste the whole answer, with its code block.",
			couldNotRead: (problem: string) => `Couldn't read it. ${problem}`,
			nothingFound:
				"I couldn't find anything about you in that. Paste the whole answer, with its code block."
		},
		arrival: {
			thingsIKnow: (n: number) =>
				p(n, { one: 'thing I know about you', other: 'things I know about you' })
		},
		fresh: {
			title: 'A fresh start.',
			subtitle: "I'll get to know you as we talk."
		}
	},

	login: {
		welcome: 'Welcome back',
		hint: 'Sign in with the account you were given.',
		email: 'Email address',
		password: 'Password',
		signingIn: 'Signing in…',
		forgot: 'Forgot your password? Ask whoever set up nolune to run {command}.',
		tooManyAttempts: 'Too many attempts. Wait a minute and try again.',
		wrongPassword: 'Wrong email or password.'
	},

	admin: {
		title: 'Models & keys',
		keys: 'API keys',
		keysHint:
			"Shared by every profile and kept in nolune's config file on this computer. A new key is checked with its provider before it's saved, and used right away. Add a custom provider for a model server of your own, like Ollama or LM Studio.",
		purposes: {
			anthropic: 'Runs chats and automations on Claude models.',
			openai:
				'Runs chats and automations on OpenAI models, and makes pictures for the Images page and when the agent draws.',
			openrouter:
				'Runs chats and automations on the models OpenRouter serves (Claude, GPT, Gemini, DeepSeek and many more), with one key and its credits.'
		},
		withoutIt: {
			anthropic: 'Chats and automations on Claude models stop working until a new key is added.',
			openai:
				"Chats and automations on OpenAI models stop working, and nolune can't make pictures, until a new key is added.",
			openrouter:
				'Chats and automations on OpenRouter models stop working until a new key is added.'
		},
		savedInNolune: (hint: string | null) => `Saved in nolune${hint ? ` ending in ${hint}` : ''}`,
		fromEnv: (variable: string, hint: string | null) =>
			`From the ${variable} environment variable${hint ? ` ending in ${hint}` : ''}`,
		notSet: 'Not set',
		replace: 'Replace',
		pasteKey: (provider: string) => `Paste the ${provider} API key`,
		keyLabel: (provider: string) => `${provider} API key`,
		checkingKey: 'Checking the key…',
		makeOneAt: 'Make one at {link}.',
		sameProject:
			"Use a key from the same project: pictures and PDFs already sent in chats live there, and those chats can't go on without them.",
		sameWorkspace:
			"Use a key from the same workspace: pictures and PDFs already sent in chats live there, and those chats can't go on without them.",
		removeKeyTitle: (provider: string) => `Remove the ${provider} key?`,
		useEnvInstead: (variable: string) =>
			`nolune will use the key in the ${variable} environment variable instead.`,
		plan: 'Claude plan',
		plans: 'Plans',
		plansHint:
			"Chats on a plan preset run on someone's own subscription instead of an API key: the Claude plan through Claude Code on this computer, which keeps its sign-in; the ChatGPT plan through Sign in with ChatGPT, whose sign-in nolune keeps on this computer. Plan limits assume one person's ordinary use, so keep busy automations and subagents on an API key preset.",
		claudePlanAbout: 'Pro or Max, through Claude Code.',
		chatgptPlan: 'ChatGPT plan',
		chatgptPlanAbout: 'Plus or Pro, signed in with ChatGPT.',
		chatgptSignIn: 'Continue with ChatGPT',
		chatgptContinueAs: (account: string) => `Continue as ${account}`,
		chatgptAnotherAccount: 'Use another account',
		chatgptSignInAgain: 'Sign in again',
		chatgptStarting: 'Starting…',
		signOut: 'Sign out',
		chatgptOpen: 'Open {link} and sign in, allowing nolune to use your ChatGPT plan.',
		chatgptSignInPage: "ChatGPT's sign-in page",
		chatgptHere:
			"In a browser on this computer, that's it: this page updates once you're signed in.",
		chatgptElsewhere:
			"On another device, the page ChatGPT sends you back to won't load. Copy its address (it starts with http://127.0.0.1) and paste it here:",
		chatgptFinish: 'Finish',
		chatgptSignedIn: 'Signed in. Chats on ChatGPT plan presets now use this plan.',
		chatgptUsing: 'Chats on ChatGPT plan presets use this plan. {link}',
		chatgptManageUsage: 'Manage usage',
		chatgptNobody: 'Nobody is signed in with ChatGPT.',
		chatgptSignedOutLocally:
			"Signed out here, but OpenAI couldn't be told: to be sure, disconnect nolune in ChatGPT's settings.",
		chatgptSignOutTitle: 'Sign out of ChatGPT?',
		chatgptSignOutBody: 'Chats on ChatGPT plan presets stop working until someone signs in again.',
		signedOut: 'Signed out.',
		notAt: 'Not at {path}, where {command} says it is.',
		notInstalled: 'Not installed on this computer.',
		checkSignIn: 'Check sign-in',
		install:
			"In a terminal on this computer, run {setup}: it installs Claude Code with Anthropic's installer and signs it in to your Claude account, asking first. Or install it yourself, then run {claude} and sign in:",
		copyCommand: 'Copy the command',
		signIn:
			'To sign in, run {setup} in a terminal on this computer, or run {claude} there and use {login} with your Claude account.',
		models: 'Models',
		modelsHint:
			"Presets are shared by every profile. New chats start with the default one. Editing or removing a preset doesn't change chats already on it.",
		presetDetails: (provider: string, model: string, context: string, overridden: boolean) =>
			`${provider} / ${model} · context ${context}${overridden ? ' (override)' : ''}`,
		makeDefault: 'Make default',
		editPreset: (name: string) => `Edit ${name}`,
		noPresets: 'No presets yet.',
		/** The form that adds a model preset, or edits one. */
		addModel: {
			title: 'Add a model',
			provider: 'Provider',
			model: 'Model',
			name: 'Name',
			optional: '(optional)',
			namePlaceholder: 'The model id and provider',
			contextWindow: 'Context window',
			contextTokens: 'Context window in tokens',
			auto: 'Auto',
			autoWith: (size: string) => `Auto · ${size}`,
			unknown: 'unknown',
			custom: 'Custom',
			tokensPlaceholder: 'Tokens, e.g. 272k',
			tokens: (n: number) => `${n.toLocaleString('en')} tokens.`,
			typeTokens: 'Type a token count, like 272k or 272000.',
			/** What Auto (no override) gets with each provider. */
			autoContext: {
				anthropic: 'Auto uses the window Anthropic reports for the model.',
				openai: "OpenAI doesn't report it: Auto knows only its flagships' (1.05M since GPT-5.4).",
				openrouter: 'Auto uses the window OpenRouter lists for the model and its main provider.',
				'custom-openai':
					'Auto uses the window the server lists for the model, if it lists one (vLLM does); otherwise it stays unknown.',
				'custom-anthropic':
					'Auto uses the window the server lists for the model, if it lists one (vLLM does); otherwise it stays unknown.',
				'claude-plan': "Claude Code doesn't report it: Auto knows only its 1M-context models'.",
				'chatgpt-plan':
					'Auto uses the window ChatGPT lists for the model, if it lists one; otherwise it stays unknown.'
			},
			onPlan: 'Runs on the Pro or Max plan Claude Code is signed in to.',
			noClaudeCode: "Claude Code isn't installed yet: see Claude plan, above.",
			onChatGptPlan: 'Runs on the ChatGPT plan of whoever is signed in with ChatGPT, above.',
			noChatGpt: 'Nobody is signed in with ChatGPT yet: see ChatGPT plan, above.',
			onKey: (provider: string) => `Runs on the ${provider} API key.`,
			noKey: (provider: string) => `No ${provider} API key yet: add one under API keys, above.`,
			onCustom: (api: string, url: string) =>
				`Runs on ${url}, through its ${api} API. The model must be able to call tools; pictures and PDFs reach it as their paths.`,
			customGone: 'Its custom provider was removed: pick another provider.',
			asking: (source: string) => `Asking ${source} for its models…`,
			listProblem: (problem: string) => `${problem} You can still type an id.`,
			couldNotList: (status: number) => `nolune couldn't get the models (${status}).`,
			unreachable: "nolune couldn't be reached.",
			checkingClaude: 'Checking Claude Code…',
			checkingModel: 'Checking the model…',
			saving: 'Saving…',
			pick: 'Pick a model',
			search: 'Search, or type a model id',
			use: 'Use {model}',
			typeId: "Type the model's id above.",
			added: (name: string) => `Added ${name}.`,
			saved: (name: string) => `Saved ${name}. Chats already on it keep what they had.`,
			invalidContext: 'Context window must be a token count, like 272k or 272000.'
		},
		savedWorks: 'Saved. It works.',
		savedWarning: (warning: string) => `Saved. ${warning}`,
		claudeNoAnswer: "Claude Code didn't answer.",
		removed: 'Removed.',
		newDefault: (name: string) => `New chats now start with ${name}.`,
		presetRemoved: 'Removed. Existing conversations keep working.',
		/** Custom providers: model servers of the family's own, listed with the API keys. */
		customProviders: {
			add: 'Add custom provider',
			addTitle: 'Add a custom provider',
			addHint:
				'A model server of your own, like Ollama, LM Studio or oMLX on this computer, or vLLM on a machine with GPUs. It shows up in Add a model under its name.',
			about: (api: string) => `Your own model server, through its ${api} API.`,
			apis: { openai: 'OpenAI', anthropic: 'Anthropic' },
			change: 'Change',
			name: 'Name',
			namePlaceholder: 'Ollama',
			api: 'API',
			apiHints: {
				openai:
					"OpenAI's Responses API: Ollama, LM Studio, vLLM, LiteLLM. Memory search can use its embeddings.",
				anthropic: "Anthropic's Messages API: Ollama, LM Studio, llama.cpp's server, oMLX."
			},
			address: 'Address',
			addressHint:
				'Ollama listens at http://localhost:11434 and LM Studio at http://localhost:1234. nolune asks it for its models to check it.',
			key: 'Key',
			keyOptional: '(if it needs one)',
			keyKept: 'Saved. Leave empty to keep it.',
			withKey: (hint: string | null) => (hint ? `key ending in ${hint}` : 'with a key'),
			noKey: 'no key',
			checking: 'Checking…',
			works: (n: number) => `Saved. It serves ${n} ${p(n, { one: 'model', other: 'models' })}.`,
			unchecked: (problem: string) => `Saved without checking it: ${problem}`,
			needName: 'Give it a name, like Ollama or GPU box.',
			needAddress: 'Give its address, starting with http:// or https://.',
			removeTitle: (name: string) => `Remove ${name}?`,
			removeBody:
				"Chats and automations on its models stop working until they're moved to another model.",
			usedBy: (names: string) => `These presets run on it: ${names}.`
		},
		/** Memory search by meaning: where its embeddings come from. */
		embeddings: {
			title: 'Memory search',
			hint: "nolune also finds memory facts by what they mean, not only by their words, and across languages. For that, every fact is sent once to where the embeddings are made, and each message as it's sent; a server on this computer keeps them here. Shared by every profile.",
			name: 'Search by meaning',
			using: (source: string) => `Uses ${source}.`,
			off: 'Off: memory is searched by words only.',
			noKeys: 'No OpenAI or OpenRouter key yet, so memory is searched by words only.',
			noKey: (provider: string) =>
				`No ${provider} key yet, so memory is searched by words only. Add one under API keys, above.`,
			customGone: 'Its custom provider was removed, so memory is searched by words only.',
			change: 'Change',
			source: 'Embeddings from',
			modes: {
				auto: 'Auto',
				openai: 'OpenAI',
				openrouter: 'OpenRouter',
				off: 'Off'
			},
			autoNote:
				"OpenAI's text-embedding-3-small with the OpenAI key, else the same model through OpenRouter.",
			withKey: (provider: string) => `With the ${provider} API key.`,
			customNote: (url: string) => `A model it serves at ${url}, through its OpenAI API.`,
			offNote: 'Memory is searched by words only, and no fact is sent anywhere to be embedded.',
			model: 'Model',
			customModel: 'Its name there, like nomic-embed-text',
			checking: 'Checking…',
			works: 'Saved. It works; facts are embedded in the background.',
			wordsOnly: 'Saved. Memory is searched by words only.',
			noAnswer: (problem: string) => `Saved, but it didn't answer: ${problem}`,
			needModel: 'Give the model it should use.'
		},
		commands: {
			title: 'Commands',
			hint: "The agent works by running commands on this computer, with this account's access to its files and apps. Auto mode has a model look at each command before it runs and stop what could do harm nobody asked for. Every chat goes by it unless someone sets that chat differently with the shield in its message box; only admins can turn the checks off there.",
			modes: {
				auto: 'Auto mode',
				unrestricted: 'Unrestricted'
			},
			choices: {
				auto: 'Auto (recommended)',
				unrestricted: 'Unrestricted (not recommended)'
			},
			checkedByChat: "Each command is checked by the chat's own model.",
			checkedBy: (preset: string) => `Each command is checked by ${preset}.`,
			presetGone:
				"The model chosen for the checks was removed, so each chat's own model checks its commands.",
			unrestrictedStatus: 'Commands run without a check.',
			change: 'Change',
			mode: 'Mode',
			autoNote:
				"Commands that only look at things run right away. The rest are checked first; a blocked one doesn't run, and nolune says what it wanted to do so someone can say go ahead.",
			unrestrictedNote:
				'Every command runs as the agent wrote it, with nothing to stop a mistake, or a web page or email that talks it into something. Only for people who watch closely.',
			checker: 'Checked by',
			chatModel: "The chat's own model",
			checkerNote:
				'A fast, capable model keeps chats quick: every command that does more than look costs a short call to it.',
			saved: 'Saved. It applies from the next command.'
		}
	}
};

export type Messages = typeof en;
