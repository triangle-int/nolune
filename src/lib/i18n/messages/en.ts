import { describeSchedule } from '@btw/core/schedule';
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
		description: 'How btw looks on this device.',
		theme: 'Theme',
		system: 'System',
		light: 'Light',
		dark: 'Dark',
		language: 'Language',
		languageAuto: 'Same as the browser',
		languageHint: 'For menus and buttons. btw answers in the language you write in.',
		technical: 'Show technical details',
		technicalHint: 'Show the exact commands btw runs, token usage and prompt caching.',
		expandSteps: 'Always show steps',
		expandStepsHint:
			'Open the list of what btw did under each reply, instead of keeping it folded.',
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
		empty: 'Nothing yet. Ask btw for a reminder or a daily check, and what it finds shows up here.'
	},

	chat: {
		options: 'Chat options',
		placeholder: 'Ask btw',
		placeholderRunning: 'Add something while btw works…',
		disclaimer: 'btw can make mistakes, and it can change files on this computer.',
		reconnecting: 'Reconnecting…',
		empty: 'Ask for something to get started.',
		readsAfterStep: 'btw reads this after its current step',
		working: 'btw is working',
		thinking: 'Thinking',
		writing: 'Writing',
		cutOff: 'The reply was cut off because it got too long.',
		refused: 'btw declined to continue this request.',
		automation: (title: string) => `Automation · ${title}`,
		fromBtw: (to: string) => `From btw, to ${to}`,
		finishedInBackground: (title: string) => `Finished in the background · ${title}`,
		inBackground: 'Working in the background',
		aCommand: 'A command',
		subagent: (name: string) => `Subagent ${name}`,
		stopping: 'stopping',
		errorTitle: 'Something went wrong while btw was answering.',
		unanswered: "btw hasn't answered this yet.",
		scrollToBottom: 'Scroll to the newest message',
		subagentBanner: 'Subagent {name}: btw started it from {parent}, and it reports back there.',
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
			`btw keeps this chat in a cache, so each reply only pays for what's new. ${model ? 'Another model' : 'Another reasoning level'} can't use it: the next reply reads the whole chat again, which takes longer and costs more${missTokens ? ` (a prompt cache miss of about ${missTokens} tokens)` : ''}.`,
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
		failed: 'failed',
		didntWork: "didn't work",
		statusStopped: 'stopped',
		statusNotRun: 'not run',
		command: 'Command',
		theCommand: 'The command btw ran',
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

	newChat: {
		greeting: (name: string) => `What can I help with, ${name}?`,
		greetingNoName: 'What can I help with?',
		noModels:
			'No models are set up yet. An admin can add one on the Models page or with {command}.',
		couldNotStart: 'Could not start the chat.',
		/**
		 * The general chips a profile has until btw makes some from its memory (core's
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
			'{name} is deleted for everyone in the profile. Its chats move back to your chat list, without its instructions and files. The files are moved to ~/.btw-agent/trash.',
		// The folder's page
		options: 'Folder options',
		newChatIn: (name: string) => `New chat in ${name}`,
		instructions: 'Instructions',
		instructionsHint: 'What btw should know or do in every chat here.',
		instructionsPlaceholder:
			"We're planning two weeks in Japan in April with the kids (7 and 10). Keep plans relaxed and the budget under ¥600,000.",
		changesTechnical:
			'Chats in the folder get changes at their next message, which re-reads the conversation once (a prompt cache miss).',
		changes: 'Chats in the folder get changes at their next message.',
		files: 'Files',
		addFiles: 'Add files',
		filesHint:
			"Pictures, documents, anything. btw gets where they're saved and opens them when they matter.",
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
		onlyPath: (note: string) => `btw got only where it's saved: ${note}`
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
		cantMakeYet: "btw can't make pictures yet.",
		needsKeyAdmin: 'It needs an {provider} API key. {link}.',
		addKeyLink: 'Add it under Models & keys',
		needsKey: (provider: string) => `It needs an ${provider} API key. Ask an admin to add one.`,
		adminSetsUp: 'An admin sets this up on the computer btw runs on.',
		describe: 'Describe an image',
		describeHint: 'btw makes the picture in a new chat, where you can ask for changes.',
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
			`What btw remembers for ${profile}, shared by everyone in it. Every chat starts with the pinned Core note; btw reads the others when a chat needs them and saves what it learns along the way. To add something, just tell it in a chat, like "Remember that Anna is allergic to nuts".`,
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
			"Nothing yet. Put here what btw should keep in mind in every chat: who's in the family, the languages you speak, allergies. btw adds to it too.",
		forgetTitle: (topic: string) => `Forget "${topic}"?`,
		forgetBody:
			'btw forgets everything in {path} for everyone in {profile}. Chats that already read it keep what they read.',
		forgetButton: 'Forget',
		empty: 'The note is empty. To delete it, use Forget.',
		conflict: (problem: string) =>
			`${problem} Save again to keep your version, or cancel to see btw's.`,
		saved: 'Saved. New chats will see the change.',
		forgot: (path: string) => `btw forgot everything in ${path}.`,
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
		nothingYet: 'Nothing remembered yet. Each thing btw learns becomes a dot here.'
	},

	automations: {
		title: 'Automations',
		intro:
			'Things btw does on its own: reminders, regular checks and replies to other apps. What it finds shows up under the bell. To add or change one, just ask btw in a chat.',
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
		instructions: 'Instructions for btw',
		instructionsTechnical: (model: string, effort: string) =>
			`Instructions for btw (${model}, reasoning ${effort})`,
		script: 'Script: runs without the model and calls {command} when btw is needed',
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
		empty: 'No automations yet. Ask btw in a chat, for example:',
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
			"Skills are instructions btw follows for specific tasks. It sees the name and description of every skill that's on, and reads the rest when a task calls for it. Turning off skills this profile doesn't need keeps btw focused. Changes apply to new chats.",
		summary: (on: number, total: number, tokens: string) =>
			`${on} of ${total} on · about ${tokens} tokens at the start of every new chat`,
		madeFor: (profile: string) => `Made for ${profile}`,
		shared: 'Shared by all profiles',
		builtIn: 'Built into btw',
		allOff: 'Turn all off',
		allOn: 'Turn all on',
		tokens: (n: string) => `~${n} tokens`,
		use: (name: string) => `Use ${name}`,
		empty:
			'No skills yet. When btw works out how to do something, it can save that as a skill for next time.',
		gone: 'That skill no longer exists.'
	},

	profile: {
		title: 'People & profile',
		name: 'Name',
		profileName: 'Profile name',
		folder: (slug: string) => `Folder: ~/.btw-agent/profiles/${slug} (doesn't change)`,
		avatar: 'Avatar',
		avatarHint:
			"How btw looks in this profile's chats. Everyone here sees the same one, and btw can change it when asked.",
		soul: 'Soul',
		soulHint: (profile: string) =>
			`Who btw is for ${profile}: its character, what it cares about, how it talks. Every chat starts with it, and btw changes it too when you ask it to be different.`,
		soulPlaceholder:
			"You're warm and a little playful, and you keep answers short. With the kids you explain things simply and never talk down to them. When you don't know something, you say so.",
		changesTechnical:
			'Chats get changes at their next message, which re-reads the conversation once (a prompt cache miss).',
		changes: 'Chats get changes at their next message.',
		members: 'Members',
		membersHint: 'Everyone here sees and writes in the same chats, and can ask btw for anything.',
		personToAdd: 'Person to add',
		chooseSomeone: 'Choose someone to add',
		everyoneIsMember: 'Everyone is already a member.',
		deleteTitle: 'Delete profile',
		deleteHint: 'Deletes all its chats for everyone. The folder is moved to ~/.btw-agent/trash.',
		deleteButton: 'Delete this profile',
		deleteConfirm: (name: string) => `Delete "${name}"?`,
		deleteBody:
			'All its chats are deleted for everyone. The folder is moved to ~/.btw-agent/trash.',
		renamed: 'Renamed.',
		soulSaved: 'Saved the soul.',
		soulRemoved: 'Removed the soul.',
		added: (name: string) => `Added ${name}.`,
		removed: 'Removed.'
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

	login: {
		welcome: 'Welcome back',
		hint: 'Sign in with the account you were given.',
		email: 'Email address',
		password: 'Password',
		signingIn: 'Signing in…',
		forgot: 'Forgot your password? Ask whoever set up btw to run {command}.',
		tooManyAttempts: 'Too many attempts. Wait a minute and try again.',
		wrongPassword: 'Wrong email or password.'
	},

	admin: {
		title: 'Models & keys',
		keys: 'API keys',
		keysHint:
			"Shared by every profile and kept in btw's config file on this computer. A new key is checked with its provider before it's saved, and used right away.",
		purposes: {
			anthropic: 'Runs chats and automations on Claude models.',
			openai:
				'Runs chats and automations on OpenAI models, and makes pictures for the Images page and when the agent draws.'
		},
		withoutIt: {
			anthropic: 'Chats and automations on Claude models stop working until a new key is added.',
			openai:
				"Chats and automations on OpenAI models stop working, and btw can't make pictures, until a new key is added."
		},
		savedInBtw: (hint: string | null) => `Saved in btw${hint ? ` ending in ${hint}` : ''}`,
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
			`btw will use the key in the ${variable} environment variable instead.`,
		plan: 'Claude plan',
		planHint:
			"Chats on a Claude plan preset run on the Pro or Max plan someone signed in to Claude Code with on this computer, instead of an API key. btw runs Claude Code and never sees the sign-in. Plan limits assume one person's ordinary use, so keep busy automations and subagents on an API key preset.",
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
			"Presets are shared by every profile. New chats start with the default one. Removing a preset doesn't affect existing chats.",
		presetDetails: (provider: string, model: string, context: string, overridden: boolean) =>
			`${provider} / ${model} · context ${context}${overridden ? ' (override)' : ''}`,
		makeDefault: 'Make default',
		noPresets: 'No presets yet.',
		/** The form that adds a model preset. */
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
				'claude-plan': "Claude Code doesn't report it: Auto knows only its 1M-context models'."
			},
			onPlan: 'Runs on the Pro or Max plan Claude Code is signed in to.',
			noClaudeCode: "Claude Code isn't installed yet: see Claude plan, above.",
			onKey: (provider: string) => `Runs on the ${provider} API key.`,
			noKey: (provider: string) => `No ${provider} API key yet: add one under API keys, above.`,
			asking: (source: string) => `Asking ${source} for its models…`,
			listProblem: (problem: string) => `${problem} You can still type an id.`,
			couldNotList: (status: number) => `btw couldn't get the models (${status}).`,
			unreachable: "btw couldn't be reached.",
			checkingClaude: 'Checking Claude Code…',
			checkingModel: 'Checking the model…',
			pick: 'Pick a model',
			search: 'Search, or type a model id',
			use: 'Use {model}',
			typeId: "Type the model's id above.",
			added: (name: string) => `Added ${name}.`,
			invalidContext: 'Context window must be a token count, like 272k or 272000.'
		},
		savedWorks: 'Saved. It works.',
		savedWarning: (warning: string) => `Saved. ${warning}`,
		claudeNoAnswer: "Claude Code didn't answer.",
		removed: 'Removed.',
		newDefault: (name: string) => `New chats now start with ${name}.`,
		presetRemoved: 'Removed. Existing conversations keep working.'
	}
};

export type Messages = typeof en;
