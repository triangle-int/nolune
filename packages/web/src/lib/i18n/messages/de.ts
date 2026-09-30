import { isMonthSpan, type Schedule } from '@nolune/core/schedule';
import { listOf, plural } from '../plural';
import type { Messages } from './en';

const p = plural('de');
const list = listOf('de');

/** ", Anna" to finish a greeting with, or nothing when there's no name. */
const to = (name: string) => (name ? `, ${name}` : '');

/** By JavaScript's numbers: 0 is Sunday. */
const WEEKDAYS = ['Sonntag', 'Montag', 'Dienstag', 'Mittwoch', 'Donnerstag', 'Freitag', 'Samstag'];
const MONTHS = [
	'',
	'Januar',
	'Februar',
	'März',
	'April',
	'Mai',
	'Juni',
	'Juli',
	'August',
	'September',
	'Oktober',
	'November',
	'Dezember'
];

const capitalize = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

/** Which days, as a sentence start ("Montags bis freitags") and as a tail. */
function days({ days, months }: Schedule): { every: string; on: string } {
	if (days.kind === 'monthDays') {
		const which = list(days.days.map((d) => `${d}.`));
		const of = months ? `im ${list(months.map((m) => MONTHS[m]))}` : 'jedes Monats';
		return { every: `Am ${which} ${of}`, on: `am ${which} ${of}` };
	}
	if (days.kind === 'date') {
		const date = `${days.day}. ${MONTHS[days.month]}`;
		return { every: `Jedes Jahr am ${date}`, on: `jedes Jahr am ${date}` };
	}
	let words: { every: string; on: string };
	if (days.kind === 'daily') words = { every: 'Täglich', on: '' };
	else if (days.kind === 'weekdays') {
		words = { every: 'Montags bis freitags', on: 'montags bis freitags' };
	} else if (days.kind === 'weekends') words = { every: 'Am Wochenende', on: 'am Wochenende' };
	else if (days.kind === 'weekdayRange') {
		const range = `${WEEKDAYS[days.from]} bis ${WEEKDAYS[days.to]}`;
		words = { every: range, on: range };
	} else {
		// "montags und mittwochs"
		const which = list(days.days.map((d) => `${WEEKDAYS[d].toLowerCase()}s`));
		words = { every: capitalize(which), on: which };
	}
	if (!months) return words;
	const inMonths = isMonthSpan(months)
		? `von ${MONTHS[months[0]]} bis ${MONTHS[months[months.length - 1]]}`
		: `im ${list(months.map((m) => MONTHS[m]))}`;
	return { every: `${words.every} ${inMonths}`, on: `${words.on || 'täglich'} ${inMonths}` };
}

function describe(schedule: Schedule): string {
	const when = days(schedule);
	const tail = when.on ? ` ${when.on}` : '';
	const { times } = schedule;
	switch (times.kind) {
		case 'minutes': {
			const every = times.step === 1 ? 'Jede Minute' : `Alle ${times.step} Minuten`;
			if (!times.between) return every + tail;
			return `${every} von ${times.between.from} bis ${times.between.to}${tail}`;
		}
		case 'hours': {
			const past = times.minute ? ` um :${String(times.minute).padStart(2, '0')}` : '';
			const every = times.step === 1 ? 'Stündlich' : `Alle ${times.step} Stunden`;
			return `${every}${past}${tail}`;
		}
		case 'hourRange':
			return `Stündlich von ${times.from} bis ${times.to}${tail}`;
		case 'at':
			return `${when.every} um ${list(times.times)} Uhr`;
	}
}

const memories = (n: number) => p(n, { one: `${n} Erinnerung`, other: `${n} Erinnerungen` });

export const de: Messages = {
	common: {
		add: 'Hinzufügen',
		cancel: 'Abbrechen',
		close: 'Schließen',
		continue: 'Weiter',
		copied: 'Kopiert',
		copy: 'Kopieren',
		create: 'Erstellen',
		default: 'Standard',
		delete: 'Löschen',
		done: 'Fertig',
		edit: 'Bearbeiten',
		more: 'Mehr',
		newChat: 'Neuer Chat',
		remove: 'Entfernen',
		rename: 'Umbenennen',
		save: 'Speichern',
		stop: 'Stoppen',
		tryAgain: 'Erneut versuchen',
		checking: 'Wird geprüft…',
		starting: 'Wird gestartet…',
		uploading: 'Wird hochgeladen…',
		removeFile: (name: string) => `${name} entfernen`,
		characters: (count: number, max: number) => `${count} / ${max} Zeichen`
	},

	errors: {
		requestFailed: (status: number) => `Anfrage fehlgeschlagen (${status})`,
		uploadFailed: (status: number | null) =>
			`Hochladen fehlgeschlagen (${status === null ? 'keine Verbindung' : status})`,
		couldNotSave: 'Konnte nicht gespeichert werden.',
		somethingWentWrong: 'Etwas ist schiefgelaufen.',
		thatDidntWork: 'Das hat nicht geklappt.',
		tooManyFiles: (max: number) => `Höchstens ${max} Dateien pro Nachricht`,
		tooLarge: (mb: number) => `Größer als ${mb} MB`,
		notSignedIn: 'Nicht angemeldet',
		adminsOnly: 'Nur für Admins',
		profileNotFound: 'Profil nicht gefunden',
		conversationNotFound: 'Chat nicht gefunden',
		folderNotFound: 'Ordner nicht gefunden',
		automationNotFound: 'Automation nicht gefunden',
		messageEmpty: 'Die Nachricht ist leer',
		noFile: 'Keine Datei'
	},

	time: {
		justNow: 'gerade eben',
		seconds: (s: number) => `${s} s`,
		minutesSeconds: (m: number, s: number) => `${m} min ${s} s`,
		hoursMinutes: (h: number, m: number) => `${h} h ${m} min`,
		minutesAgo: (n: number) => `vor ${n} min`,
		hoursAgo: (n: number) => `vor ${n} h`
	},

	units: { bytes: 'B', kilobytes: 'KB', megabytes: 'MB' },

	settings: {
		title: 'Einstellungen',
		description: 'Wie nolune auf diesem Gerät aussieht.',
		theme: 'Design',
		system: 'System',
		light: 'Hell',
		dark: 'Dunkel',
		language: 'Sprache',
		languageAuto: 'Wie im Browser',
		languageHint:
			'Für Menüs und Schaltflächen. nolune antwortet in der Sprache, in der du schreibst.',
		technical: 'Technische Details anzeigen',
		technicalHint:
			'Zeigt die genauen Befehle, die nolune ausführt, den Tokenverbrauch und das Prompt-Caching.',
		expandSteps: 'Schritte immer anzeigen',
		expandStepsHint:
			'Die Liste dessen, was nolune getan hat, unter jeder Antwort aufgeklappt statt eingeklappt zeigen.',
		sounds: 'Töne',
		soundsHint:
			'Leise Töne, wo sich nolune von selbst bewegt, etwa bei der Begrüßung eines neuen Profils.',
		logOut: 'Abmelden',
		deviceOnly: 'Diese Einstellungen werden nur auf diesem Gerät gespeichert.'
	},

	userMenu: {
		account: 'Konto',
		settings: 'Einstellungen',
		allProfiles: 'Alle Profile',
		modelsAndKeys: 'Modelle & Schlüssel',
		logOut: 'Abmelden'
	},

	header: {
		openMenu: 'Menü öffnen'
	},

	sidebar: {
		label: 'Seitenleiste',
		description: 'Chats, Ordner und Seiten dieses Profils.',
		profiles: 'Profile',
		newProfile: 'Neues Profil',
		openSidebar: 'Seitenleiste öffnen',
		closeSidebar: 'Seitenleiste schließen',
		searchChats: 'Chats durchsuchen',
		images: 'Bilder',
		automations: 'Automationen',
		memory: 'Gedächtnis',
		skills: 'Fähigkeiten',
		people: 'Personen & Profil',
		folders: 'Ordner',
		newFolder: 'Neuer Ordner',
		chats: 'Chats',
		working: ', arbeitet',
		showChats: (folder: string) => `Chats in ${folder} anzeigen`,
		hideChats: (folder: string) => `Chats in ${folder} ausblenden`,
		emptyFolder: 'Zieh Chats hierher oder starte einen auf der Seite des Ordners.',
		dropToTakeOut: 'Hier ablegen, um den Chat aus seinem Ordner zu nehmen.',
		noChats: 'Deine Chats erscheinen hier.',
		searchDescription: 'Einen Chat nach seinem Titel finden',
		searchPlaceholder: 'Chats durchsuchen…',
		noResults: 'Keine Chats gefunden.',
		deleteChatBody: 'Damit wird {name} für alle in {profile} gelöscht.'
	},

	notifications: {
		title: 'Benachrichtigungen',
		unseen: (n: number) => `Benachrichtigungen, ${n} neu`,
		clearAll: 'Alle löschen',
		new: 'Neu',
		showLess: 'Weniger anzeigen',
		showAll: 'Alles anzeigen',
		dismiss: 'Ausblenden',
		openChat: 'Chat öffnen',
		continueInChat: 'Im Chat fortsetzen',
		empty:
			'Noch nichts da. Bitte nolune um eine Erinnerung oder eine tägliche Prüfung, dann erscheint hier, was es findet.'
	},

	chat: {
		options: 'Chat-Optionen',
		placeholder: 'Frag nolune',
		placeholderRunning: 'Ergänze etwas, während nolune arbeitet…',
		disclaimer: 'nolune kann Fehler machen und Dateien auf diesem Computer ändern.',
		reconnecting: 'Verbindung wird wiederhergestellt…',
		empty: 'Bitte um etwas, um loszulegen.',
		readsAfterStep: 'nolune liest das nach seinem aktuellen Schritt',
		typing: (names: string[]) =>
			`${list(names)} ${names.length === 1 ? 'schreibt' : 'schreiben'} gerade`,
		working: 'nolune arbeitet',
		thinking: 'Denkt nach',
		writing: 'Schreibt',
		cutOff: 'Die Antwort wurde abgeschnitten, weil sie zu lang wurde.',
		refused: 'nolune hat es abgelehnt, diese Anfrage fortzusetzen.',
		automation: (title: string) => `Automation · ${title}`,
		fromNolune: (to: string) => `Von nolune an ${to}`,
		finishedInBackground: (title: string) => `Im Hintergrund fertig · ${title}`,
		inBackground: 'Arbeitet im Hintergrund',
		aCommand: 'Ein Befehl',
		subagent: (name: string) => `Subagent ${name}`,
		stopping: 'wird gestoppt',
		errorTitle: 'Beim Antworten von nolune ist etwas schiefgelaufen.',
		unanswered: 'nolune hat darauf noch nicht geantwortet.',
		scrollToBottom: 'Zur neuesten Nachricht scrollen',
		subagentBanner:
			'Subagent {name}: nolune hat ihn aus {parent} gestartet, und er berichtet dorthin.',
		subagentOnly: (name: string) => `Hier schreibt nur der Agent, der ${name} gestartet hat.`,
		subagentRetry: 'Einen Subagenten führt nur der Agent aus, der ihn gestartet hat.',
		hiddenBanner:
			'Ein Hintergrundlauf einer Automation. Sende eine Nachricht, um ihn in deinen Chats zu behalten.',
		aChat: 'einem Chat',
		deleteTitle: 'Chat löschen?',
		deleteBody: 'Damit wird {name} für alle im Profil gelöscht.',
		renameTitle: 'Chat umbenennen',
		chatName: 'Name des Chats',
		couldNotRename: 'Der Chat konnte nicht umbenannt werden.',
		switchTitle: (model: string) => `Zu ${model} wechseln?`,
		anotherModel: 'einem anderen Modell',
		effortTitle: (level: string) => `Denktiefe auf „${level}“ ändern?`,
		switchCache: (model: boolean, missTokens: string | null) =>
			`nolune hält diesen Chat in einem Cache, damit jede Antwort nur für Neues bezahlt. ${model ? 'Ein anderes Modell' : 'Eine andere Denktiefe'} kann ihn nicht nutzen: Die nächste Antwort liest den ganzen Chat noch einmal, was länger dauert und mehr kostet${missTokens ? ` (ein Prompt-Cache-Fehlschlag von etwa ${missTokens} Tokens)` : ''}.`,
		switchFiles:
			'Manche Bilder und PDFs in diesem Chat gehen nicht zu einem anderen Anbieter mit: Das neue Modell bekommt, wo ihre Dateien liegen, und kann sie sich noch einmal ansehen.',
		switch: 'Wechseln',
		change: 'Ändern',
		models: (models: string[]) => models.join(', dann '),
		usage: 'Verbrauch',
		tokensInOut: (input: string, output: string) => `${input} Tokens rein, ${output} raus`,
		cache: 'Cache',
		lastReply: 'Letzte Antwort',
		wholeConversation: 'Ganzer Chat',
		cacheSummary: (label: string, rate: string, read: string, written: string, uncached: string) =>
			`${label}: ${rate} aus dem Cache (${read} gelesen, ${written} geschrieben, ${uncached} ohne Cache)`,
		cacheMiss: (tokens: string) => `Cache-Fehlschlag · ${tokens} Tokens neu verarbeitet`,
		cacheExpired: (ttl: '5m' | '1h') =>
			`Seit dem vorigen Schritt ${ttl === '5m' ? 'sind über 5 Minuten' : 'ist über eine Stunde'} vergangen, daher war der zwischengespeicherte Chat abgelaufen und wurde neu verarbeitet (langsamer und teurer).`,
		cacheBroken:
			'Kontext, der aus dem Cache hätte kommen sollen, wurde neu verarbeitet (langsamer und teurer). Das passiert einmal, wenn das Modell oder die Denktiefe gewechselt, der Chat in einen anderen Ordner verschoben oder sein Ordner geändert wird.',
		contextChip: (used: string, window: string, rate: string) =>
			`${used} / ${window} · ${rate} aus dem Cache`,
		contextUsed: (used: string, window: string) =>
			`Kontext der letzten Antwort: ${used} von ${window}`,
		contextMenu: (used: string, window: string) => `Kontext ${used} / ${window}`,
		cachedOverall: (rate: string) => `${rate} insgesamt aus dem Cache`
	},

	steps: {
		thinking: 'Denkt nach',
		thinkingDots: 'Denkt nach…',
		running: (command: string) => `Führt ${command} aus`,
		runningACommand: 'Führt einen Befehl aus',
		ranACommand: 'Hat einen Befehl ausgeführt',
		gettingReady: 'Bereitet sich vor…',
		preparing: 'Bereitet einen Befehl vor…',
		stopped: 'Gestoppt',
		thoughtFor: (duration: string) => `${duration} nachgedacht`,
		thoughtForAMoment: 'Kurz nachgedacht',
		workedFor: (duration: string) => `${duration} gearbeitet`,
		workedForAMoment: 'Kurz gearbeitet',
		ranCommands: (n: number) =>
			p(n, { one: `${n} Befehl ausgeführt`, other: `${n} Befehle ausgeführt` }),
		failedCount: (n: number) => `${n} fehlgeschlagen`,
		blockedCount: (n: number) => `${n} blockiert`,
		failed: 'fehlgeschlagen',
		didntWork: 'hat nicht geklappt',
		statusStopped: 'gestoppt',
		statusNotRun: 'nicht ausgeführt',
		statusBlocked: 'blockiert',
		command: 'Befehl',
		theCommand: 'Der Befehl, den nolune ausgeführt hat',
		inFolder: (cwd: string) => `in ${cwd}`,
		noOutputYet: 'Noch keine Ausgabe…',
		noOutput: '(keine Ausgabe)'
	},

	composer: {
		placeholder: 'Frag irgendwas',
		message: 'Nachricht',
		attach: 'Dateien anhängen',
		send: 'Senden'
	},

	model: {
		model: 'Modell',
		reasoning: 'Denktiefe',
		efforts: {
			low: { label: 'Niedrig', hint: 'Schnellste Antworten' },
			medium: { label: 'Mittel', hint: 'Gut für das meiste' },
			high: { label: 'Hoch', hint: 'Denkt bei schwierigen Aufgaben länger nach' },
			xhigh: { label: 'Sehr hoch', hint: 'Nimmt sich Zeit' },
			max: { label: 'Maximal', hint: 'Am langsamsten, für die schwierigsten Probleme' }
		}
	},

	commandMode: {
		title: 'Befehle in diesem Chat',
		auto: 'Automatisch',
		autoHint: 'Ein Modell prüft jeden Befehl, bevor er läuft.',
		unrestricted: 'Uneingeschränkt',
		unrestrictedHint: 'Befehle laufen ohne Prüfung. Nicht empfohlen.',
		adminsOnly: 'Nur Admins können die Prüfung für einen Chat ausschalten.',
		labelAuto: 'Befehle: automatischer Modus',
		labelUnrestricted: 'Befehle: uneingeschränkt'
	},

	newChat: {
		greetings: {
			morning: [
				(name: string) => `Guten Morgen${to(name)}!`,
				(name: string) => `Guten Morgen${to(name)}. Womit fangen wir heute an?`,
				(name: string) => `Erst Kaffee oder gleich los${to(name)}?`,
				(name: string) => `Moin${to(name)}! Was steht heute an?`
			],
			afternoon: [
				(name: string) => `Guten Tag${to(name)}!`,
				(name: string) => `Hallo${to(name)}! Wie läuft dein Tag?`,
				(name: string) => `Kann ich dir heute Nachmittag etwas abnehmen${to(name)}?`,
				(name: string) => `Brauchst du heute Nachmittag Hilfe${to(name)}?`
			],
			evening: [
				(name: string) => `Guten Abend${to(name)}!`,
				(name: string) => `Guten Abend${to(name)}. Wie war dein Tag?`,
				(name: string) => `Steht heute noch etwas an${to(name)}?`,
				(name: string) => `Was hast du heute Abend vor${to(name)}?`
			],
			night: [
				(name: string) => `Noch wach${to(name)}?`,
				(name: string) => `So spät noch auf den Beinen${to(name)}?`,
				(name: string) => `Kannst du nicht schlafen${to(name)}? Ich bin da.`,
				(name: string) => `Es ist schon spät${to(name)}. Was brauchst du?`
			],
			anytime: [
				(name: string) => `Wobei kann ich helfen${to(name)}?`,
				(name: string) => `Was geht dir durch den Kopf${to(name)}?`,
				(name: string) => `Woran arbeiten wir${to(name)}?`,
				(name: string) => `Schön, dich zu sehen${to(name)}. Was gibt es Neues?`
			]
		},
		noModels:
			'Es sind noch keine Modelle eingerichtet. Ein Admin kann eines auf der Seite „Modelle & Schlüssel“ oder mit {command} hinzufügen.',
		couldNotStart: 'Der Chat konnte nicht gestartet werden.',
		suggestions: {
			reminder: { label: 'Erinnerung einrichten', text: 'Erinnere mich morgen um 9:00 daran, ' },
			weather: {
				label: 'Tägliches Wetter',
				text: 'Prüfe an jedem Werktag um 7:30 das Wetter und sag uns, ob wir Regenschirme brauchen.'
			},
			file: { label: 'Datei finden', text: 'Finde die Datei auf diesem Computer namens ' },
			space: {
				label: 'Freien Speicher prüfen',
				text: 'Wie viel freier Speicherplatz ist auf diesem Computer noch übrig?'
			}
		},
		pickModel: 'Wähle ein Modell.',
		pickEffort: 'Wähle eine Denktiefe.',
		folderGone: 'Dieser Ordner wurde gelöscht. Wähle einen anderen.'
	},

	folders: {
		inFolder: (name: string) => `Im Ordner ${name}`,
		startInFolder: 'In einem Ordner starten',
		startOutside: 'Außerhalb des Ordners starten',
		noFolder: 'Kein Ordner',
		newFolderDots: 'Neuer Ordner…',
		moveTo: 'In Ordner verschieben',
		moveTechnical:
			'Beim Verschieben wird der System-Prompt neu erstellt, daher liest die nächste Antwort einmal den ganzen Chat neu (ein Prompt-Cache-Fehlschlag).',
		move: 'Nach dem Verschieben dauert die nächste Antwort etwas länger.',
		newTitle: 'Neuer Ordner',
		newDescription:
			'Halte zusammengehörige Chats beisammen. Jeder Chat in einem Ordner bekommt seine Anweisungen und Dateien.',
		namePlaceholder: 'Reise nach Japan',
		name: 'Name des Ordners',
		create: 'Ordner erstellen',
		renameTitle: 'Ordner umbenennen',
		couldNotRename: 'Der Ordner konnte nicht umbenannt werden.',
		deleteTitle: 'Ordner löschen?',
		deleteBody:
			'{name} wird für alle im Profil gelöscht. Seine Chats kommen zurück in deine Chatliste, ohne seine Anweisungen und Dateien. Die Dateien werden nach ~/.nolune/trash verschoben.',
		options: 'Ordner-Optionen',
		newChatIn: (name: string) => `Neuer Chat in ${name}`,
		instructions: 'Anweisungen',
		instructionsHint: 'Was nolune in jedem Chat hier wissen oder tun soll.',
		instructionsPlaceholder:
			'Wir planen im April zwei Wochen Japan mit den Kindern (7 und 10). Plane entspannt und bleib unter 600.000 ¥.',
		changesTechnical:
			'Chats im Ordner bekommen Änderungen mit ihrer nächsten Nachricht, die den Chat einmal neu liest (ein Prompt-Cache-Fehlschlag).',
		changes: 'Chats im Ordner bekommen Änderungen mit ihrer nächsten Nachricht.',
		files: 'Dateien',
		addFiles: 'Dateien hinzufügen',
		filesHint:
			'Bilder, Dokumente, alles Mögliche. nolune weiß, wo sie gespeichert sind, und öffnet sie, wenn sie wichtig sind.',
		noFiles: 'Noch keine Dateien',
		savedIn: (dir: string) => `Gespeichert in ${dir}`,
		couldNotAdd: 'Die Dateien konnten nicht hinzugefügt werden.',
		couldNotAddOffline:
			'Die Dateien konnten nicht hinzugefügt werden. Prüfe die Verbindung und versuch es noch einmal.',
		chats: 'Chats',
		noChats:
			'Chats, die du hier startest, erscheinen hier. Du kannst Chats auch in der Seitenleiste auf den Ordner ziehen.'
	},

	attachments: {
		open: (name: string) => `${name} öffnen`,
		download: 'Herunterladen',
		onlyPath: (note: string) => `nolune hat nur den Speicherort bekommen: ${note}`
	},

	markdown: {
		text: 'Text',
		picture: 'Bild',
		notAvailable: 'Nicht verfügbar'
	},

	images: {
		title: 'Bilder',
		shapes: { square: 'Quadrat', portrait: 'Hochformat', landscape: 'Querformat', auto: 'Auto' },
		shape: 'Format',
		groups: 'Vorlagengruppen',
		noTemplates: 'Noch keine Vorlagen.',
		cantMakeYet: 'nolune kann noch keine Bilder erstellen.',
		needsKeyAdmin: 'Dafür braucht es einen {provider}-API-Schlüssel. {link}.',
		addKeyLink: 'Füge ihn unter „Modelle & Schlüssel“ hinzu',
		needsKey: (provider: string) =>
			`Dafür braucht es einen ${provider}-API-Schlüssel. Bitte einen Admin, ihn hinzuzufügen.`,
		adminSetsUp: 'Das richtet ein Admin auf dem Computer ein, auf dem nolune läuft.',
		describe: 'Beschreibe ein Bild',
		describeHint:
			'nolune erstellt das Bild in einem neuen Chat, in dem du um Änderungen bitten kannst.',
		onlyPictures: 'Hier können nur Bilder verwendet werden.',
		atMostPictures: (n: number) => `Höchstens ${n} Bilder.`,
		changePicture: (label: string | null) => (label ? `${label} ändern` : 'Bild ändern'),
		addPicture: 'Bild hinzufügen',
		draw: 'Zeichnen',
		choosePhotoOfDrawing: 'Foto einer Zeichnung wählen',
		usePhotoOfDrawing: 'Foto einer Zeichnung verwenden',
		startDrawing: 'Zeichnen beginnen',
		takePhoto: 'Foto aufnehmen',
		choosePhoto: 'Foto wählen',
		tryIt: 'Ausprobieren',
		yourOwn: (label: string) => `eigene Angabe: ${label}`,
		pickFromList: (label: string) => `${label} aus der Liste wählen`,
		custom: 'Eigene…',
		addAnything: 'Sonst noch etwas…',
		uploadingPicture: 'Bild wird hochgeladen…',
		addDrawingFirst: 'Füge zuerst eine Zeichnung hinzu.',
		addPhotoFirst: 'Füge zuerst ein Foto hinzu.',
		generate: 'Erstellen',
		noModel: 'Es ist noch kein Chatmodell eingerichtet.',
		notPicture: (name: string) => `${name} ist kein Bild.`,
		templateGone: 'Diese Vorlage gibt es nicht mehr.',
		describeFirst: 'Beschreibe zuerst das Bild.'
	},

	drawing: {
		title: 'Zeichnung',
		tool: 'Werkzeug',
		pen: 'Stift',
		eraser: 'Radierer',
		undo: 'Rückgängig',
		penSize: 'Stiftgröße',
		color: (color: string) => `Farbe ${color}`,
		use: 'Diese Zeichnung verwenden'
	},

	emoji: {
		value: (label: string, value: string) => `${label}: ${value || 'keins'}`,
		pickUpTo: (n: number) => `Wähle bis zu ${n}`,
		removeLast: 'Letztes Emoji entfernen'
	},

	memory: {
		title: 'Gedächtnis',
		intro: (profile: string) =>
			`Was sich nolune für ${profile} merkt, sichtbar für alle darin. Jeder Chat beginnt mit der angehefteten Notiz „Das Wichtigste“; die anderen liest nolune, wenn ein Chat sie braucht, und speichert unterwegs, was es lernt. Um etwas hinzuzufügen, sag es ihm einfach in einem Chat, etwa „Merk dir, dass Anna allergisch gegen Nüsse ist“.`,
		total: (count: number, topics: number) =>
			`${count === 1 ? 'Erinnerung' : 'Erinnerungen'} in ${p(topics, { one: `${topics} Thema`, other: `${topics} Themen` })}`,
		newThisWeek: (n: number) => `${n} neu diese Woche`,
		updated: (ago: string) => `aktualisiert ${ago}`,
		memories,
		pinned: 'angeheftet, in jedem neuen Chat',
		edit: (topic: string) => `${topic} bearbeiten`,
		forget: (topic: string) => `${topic} vergessen`,
		note: (topic: string) => `Notiz ${topic}`,
		corePlaceholder:
			'Zum Beispiel:\n- Anna und Ben sind die Eltern, Mia ist 7\n- Zu Hause sprechen wir Russisch\n- Mia ist allergisch gegen Nüsse',
		coreEmpty:
			'Noch leer. Schreib hier hinein, was nolune in jedem Chat im Kopf haben soll: wer zur Familie gehört, welche Sprachen ihr sprecht, Allergien. nolune ergänzt es auch selbst.',
		forgetTitle: (topic: string) => `„${topic}“ vergessen?`,
		forgetBody:
			'nolune vergisst alles in {path} für alle in {profile}. Chats, die es schon gelesen haben, behalten, was sie gelesen haben.',
		forgetButton: 'Vergessen',
		empty: 'Die Notiz ist leer. Zum Löschen nutze „Vergessen“.',
		conflict: (problem: string) =>
			`${problem} Speichere noch einmal, um deine Version zu behalten, oder brich ab, um die von nolune zu sehen.`,
		saved: 'Gespeichert. Neue Chats sehen die Änderung.',
		forgot: (path: string) => `nolune hat alles in ${path} vergessen.`,
		learn: 'Aus Chats lernen',
		learnHint:
			'Wenn ein Chat ein paar Minuten ruht, liest nolune ihn noch einmal durch und merkt sich, was sich zu merken lohnt. Das ist jedes Mal eine kurze zusätzliche Anfrage an das Modell des Chats. Ausgeschaltet merkt sich nolune nur, woran es während des Chats selbst denkt.',
		learned: (ago: string) => `gelernt ${ago}`,
		learnedAWhileAgo: 'vor einer Weile gelernt',
		topicLabel: (topic: string, n: number) => `${topic}: ${memories(n)}. Notiz anzeigen.`,
		nothingInIt: 'Noch nichts darin',
		showFewer: 'Weniger anzeigen',
		showAllTopics: (n: number) => `Alle ${n} Themen anzeigen`,
		older: 'Älter',
		newer: 'Neuer',
		nothingYet: 'Noch nichts gemerkt. Alles, was nolune lernt, wird hier zu einem Punkt.',
		categories: {
			core: 'Das Wichtigste',
			people: 'Menschen',
			home: 'Zuhause',
			health: 'Gesundheit',
			plans: 'Pläne',
			routines: 'Routinen',
			pets: 'Haustiere',
			places: 'Orte',
			projects: 'Projekte',
			other: 'Sonstiges',
			unsorted: 'Ohne Kategorie'
		},
		unsortedHint:
			'Aus der Zeit, bevor das Gedächtnis Kategorien hatte: nolune liest sie, fügt aber nichts hinzu. Verschiebe sie in eine Kategorie, damit sie weiterwächst.',
		memberNote: (name: string) => `Mitglied: ${name}`,
		move: {
			button: (topic: string) => `${topic} verschieben`,
			title: (topic: string) => `„${topic}“ verschieben`,
			body: 'Wähle, wohin sie gehört. Gibt es diese Notiz schon, werden beide zu einer.',
			to: 'Verschieben nach',
			choose: 'Wohin?',
			categories: 'Kategorien',
			newPerson: 'Jemand Neues…',
			newProject: 'Ein neues Projekt…',
			personName: 'Name',
			projectName: 'Projektname',
			moveHint: (target: string) => `Sie wird zu ${target}.`,
			mergeHint: (from: string, into: string) =>
				`Was in „${from}“ steht, kommt ohne Wiederholungen zu „${into}“, und „${from}“ verschwindet. So werden zwei Notizen über dieselbe Person eins.`,
			move: 'Verschieben',
			merge: 'Zusammenführen',
			moved: (from: string, to: string) => `${from} nach ${to} verschoben.`,
			merged: (from: string, into: string) => `${from} mit ${into} zusammengeführt.`
		},
		/** What nolune saved from a chat by itself: in that chat, and at the top of this page. */
		changes: {
			saved: (n: number) => `${memories(n)} gespeichert`,
			added: 'Neu',
			changed: 'Geändert',
			before: (text: string) => `vorher: ${text}`,
			openNote: (topic: string) => `${topic} im Gedächtnis öffnen`,
			undo: 'Rückgängig',
			undone: 'Rückgängig gemacht',
			undoneBy: (name: string) => `Rückgängig gemacht von ${name}`,
			changedSince:
				'Das wurde inzwischen geändert und lässt sich hier nicht rückgängig machen. Bearbeite die Notiz auf der Gedächtnis-Seite.',
			alreadyUndone: 'Das wurde schon rückgängig gemacht.',
			recent: 'Aus Chats gespeichert',
			recentHint:
				'Was nolune sich von selbst notiert hat, wenn Chats still wurden, in den letzten zwei Wochen.',
			fromChat: 'aus {chat}',
			deletedChat: 'aus einem gelöschten Chat',
			showAll: (n: number) => `Alle ${n} zeigen`,
			untitled: 'einem Chat'
		}
	},

	automations: {
		title: 'Automationen',
		intro:
			'Was nolune von selbst erledigt: Erinnerungen, regelmäßige Prüfungen und Antworten an andere Apps. Was es findet, erscheint unter der Glocke. Um eine hinzuzufügen oder zu ändern, bitte nolune einfach in einem Chat darum.',
		today: 'Heute',
		previousMonth: 'Vorheriger Monat',
		nextMonth: 'Nächster Monat',
		dayLabel: (day: string, runs: number) =>
			`${day}: ${runs ? p(runs, { one: `${runs} Automationslauf`, other: `${runs} Automationsläufe` }) : 'nichts'}`,
		nothingRan: 'An diesem Tag lief nichts.',
		nothingRuns: 'An diesem Tag läuft nichts.',
		timeZone: (zone: string) => `Die Zeiten sind in der Zeitzone des Computers (${zone}).`,
		all: 'Alle Automationen',
		next: (when: string) => `nächster Lauf ${when}`,
		states: { paused: 'pausiert', done: 'erledigt' },
		runNow: 'Jetzt ausführen',
		pause: 'Pausieren',
		resume: 'Fortsetzen',
		description: 'Beschreibung',
		descriptionPlaceholder: 'Was sie tut, in einem Satz',
		instructions: 'Anweisungen für nolune',
		instructionsTechnical: (model: string, effort: string) =>
			`Anweisungen für nolune (${model}, Denktiefe ${effort})`,
		script: 'Skript: läuft ohne das Modell und ruft {command} auf, wenn nolune gebraucht wird',
		webhook:
			'Webhook-URL. Halte sie geheim: Jeder, der sie hat, kann einen Lauf starten. Sende JSON per POST an sie.',
		confirmDelete: (name: string) => `„${name}“ löschen?`,
		recentRuns: (n: number) => `Letzte Läufe (${n})`,
		noRuns: 'Noch keine Läufe',
		statuses: {
			pending: 'wartet',
			running: 'läuft',
			ok: 'erledigt',
			notified: 'benachrichtigt',
			silent: 'nichts zu melden',
			stopped: 'gestoppt',
			failed: 'fehlgeschlagen'
		},
		sources: {
			cron: 'geplant',
			once: 'geplant',
			webhook: 'Webhook',
			wake: 'von einem Skript geweckt',
			manual: 'von Hand gestartet'
		},
		scriptRun: (status: string) => `Skript ${status}`,
		view: 'ansehen',
		output: 'Ausgabe',
		empty: 'Noch keine Automationen. Bitte nolune in einem Chat darum, zum Beispiel:',
		examples: [
			'„Prüfe an jedem Werktag um 7:30 das Wetter und sag uns, ob wir Regenschirme brauchen.“',
			'„Erinnere Anna morgen um 17:00, das Paket abzuholen.“',
			'„Schau alle 10 Minuten in meine E-Mails und sag mir, wenn die Schule schreibt.“'
		],
		defaultModel: 'das Standardmodell',
		saved: (name: string) => `„${name}“ gespeichert.`,
		started: (name: string) => `„${name}“ gestartet. Die Antwort erscheint unter der Glocke.`,
		startedScript: (name: string) => `Das Skript von „${name}“ wurde gestartet.`,
		paused: (name: string) => `„${name}“ ist pausiert.`,
		resumed: (name: string) => `„${name}“ läuft wieder.`,
		deleted: (name: string) => `„${name}“ gelöscht.`,
		describe,
		scheduleAfterName: (schedule: string) => schedule,
		customSchedule: 'Nach eigenem Zeitplan',
		once: 'Einmalig',
		onceAt: (when: string) => `Einmalig, ${when}`,
		onWebhook: 'Wenn eine andere App ihren Link aufruft',
		dayAt: (day: string, time: string) => `${day} um ${time} Uhr`
	},

	skills: {
		title: 'Fähigkeiten',
		intro:
			'Fähigkeiten sind Anleitungen, denen nolune bei bestimmten Aufgaben folgt. Es sieht Namen und Beschreibung jeder eingeschalteten Fähigkeit und liest den Rest, wenn eine Aufgabe es erfordert. Wenn du Fähigkeiten ausschaltest, die dieses Profil nicht braucht, bleibt nolune fokussiert. Änderungen gelten für neue Chats.',
		summary: (on: number, total: number, tokens: string) =>
			`${on} von ${total} an · etwa ${tokens} Tokens am Anfang jedes neuen Chats`,
		madeFor: (profile: string) => `Für ${profile}`,
		shared: 'Von allen Profilen geteilt',
		builtIn: 'In nolune eingebaut',
		allOff: 'Alle ausschalten',
		allOn: 'Alle einschalten',
		tokens: (n: string) => `~${n} Tokens`,
		use: (name: string) => `${name} verwenden`,
		empty:
			'Noch keine Fähigkeiten. Wenn nolune herausfindet, wie etwas geht, kann es das als Fähigkeit für das nächste Mal speichern.',
		gone: 'Diese Fähigkeit gibt es nicht mehr.'
	},

	profile: {
		title: 'Personen & Profil',
		name: 'Name',
		profileName: 'Name des Profils',
		folder: (slug: string) => `Ordner: ~/.nolune/profiles/${slug} (ändert sich nicht)`,
		avatar: 'Avatar',
		avatarHint:
			'Wie nolune in den Chats dieses Profils aussieht. Alle hier sehen denselben, und nolune kann ihn ändern, wenn man es darum bittet.',
		soul: 'Seele',
		soulHint: (profile: string) =>
			`Wer nolune für ${profile} ist: sein Charakter, was ihm wichtig ist, wie es spricht. Jeder Chat beginnt damit, und nolune ändert es auch selbst, wenn du es bittest, anders zu sein.`,
		soulPlaceholder:
			'Du bist herzlich und ein bisschen verspielt und hältst Antworten kurz. Den Kindern erklärst du Dinge einfach und nie von oben herab. Wenn du etwas nicht weißt, sagst du es.',
		changesTechnical:
			'Chats bekommen Änderungen mit ihrer nächsten Nachricht, die den Chat einmal neu liest (ein Prompt-Cache-Fehlschlag).',
		changes: 'Chats bekommen Änderungen mit ihrer nächsten Nachricht.',
		members: 'Mitglieder',
		membersHint:
			'Alle hier sehen dieselben Chats, schreiben darin und können nolune um alles bitten.',
		personToAdd: 'Person zum Hinzufügen',
		chooseSomeone: 'Wähle jemanden zum Hinzufügen',
		everyoneIsMember: 'Alle sind schon Mitglied.',
		deleteTitle: 'Profil löschen',
		deleteHint:
			'Löscht alle seine Chats für alle. Der Ordner wird nach ~/.nolune/trash verschoben.',
		deleteButton: 'Dieses Profil löschen',
		deleteConfirm: (name: string) => `„${name}“ löschen?`,
		deleteBody:
			'Alle seine Chats werden für alle gelöscht. Der Ordner wird nach ~/.nolune/trash verschoben.',
		renamed: 'Umbenannt.',
		soulSaved: 'Seele gespeichert.',
		soulRemoved: 'Seele entfernt.',
		added: (name: string) => `${name} hinzugefügt.`,
		removed: 'Entfernt.',
		memberNote: (note: string) => `Notiz: ${note}`,
		noteStarts: (note: string) => `Notiz: ${note}, entsteht, sobald es etwas zu notieren gibt`,
		mayHaveNote: 'nolune hat vielleicht schon eine Notiz über diese Person',
		chooseNote: 'Notiz wählen',
		changeNote: 'Notiz ändern',
		linked: (name: string, note: string) => `Die Notiz von ${name} ist jetzt ${note}.`,
		chooser: {
			addTitle: (name: string) => `Kennt nolune ${name} schon?`,
			linkTitle: (name: string) => `Welche Notiz ist über ${name}?`,
			body: (name: string) =>
				`Im Gedächtnis gibt es Notizen, die über ${name} sein könnten. Wähle die richtige, damit nolune weiß, dass es darin um diese Person geht, und dort ergänzt, was sie über sich erzählt.`,
			linkBody: (name: string) =>
				`Wähle die Notiz über ${name}: nolune ergänzt dort, was diese Person über sich erzählt.`,
			current: 'jetzt',
			alsoCalled: (names: string) => `Auch genannt: ${names}`,
			more: (n: number) => `und ${n} weitere`,
			newNote: 'Jemand anderes: neue Notiz beginnen',
			privacy: (name: string) =>
				`${name} kann dann das ganze Gedächtnis dieses Profils lesen, auch diese Notiz. Prüfe, dass nichts darin steht, was vor dieser Person geheim bleiben soll, etwa eine Überraschung.`,
			add: (name: string) => `${name} hinzufügen`,
			link: 'Verknüpfen'
		}
	},

	avatars: {
		probe: 'Sonde',
		campfire: 'Lagerfeuer',
		lantern: 'Laterne',
		planet: 'Planet',
		quantum: 'Quant',
		comet: 'Komet',
		moon: 'Mond',
		satellite: 'Satellit'
	},

	profiles: {
		title: 'Profile',
		intro:
			'Jedes Profil hat eigene Chats, ein eigenes Gedächtnis und eigene Fähigkeiten, geteilt von seinen Mitgliedern.',
		none: 'Du bist noch in keinem Profil. Erstelle unten eines oder bitte jemanden, dich zu seinem hinzuzufügen.',
		new: 'Neues Profil',
		namePlaceholder: 'z. B. Familie, Oma, Hausaufgaben',
		name: 'Name des Profils',
		needsName: 'Gib dem Profil einen Namen.'
	},

	/** Die Begrüßung eines neuen Profils. Hier spricht nolune selbst: „ich“ ist der Assistent. */
	welcome: {
		title: 'Machen wir es dir hier gemütlich.',
		subtitle: 'Ein paar kurze Fragen, dann fühlst du dich wie zu Hause.',
		go: "Los geht's",
		takesAMinute: 'Dauert etwa eine Minute',
		skipIntro: 'Intro überspringen',
		skipHint: 'Irgendwo klicken, um zu überspringen',
		back: 'Zurück',
		soundsOn: 'Töne einschalten',
		soundsOff: 'Töne ausschalten',
		progress: (n: number, total: number) => `Schritt ${n} von ${total}`,
		model: {
			title: 'Womit soll ich denken?',
			subtitle:
				'Wähle, was mich antreibt. Mehr kannst du später unter Modelle & Schlüssel hinzufügen.',
			askAdmin:
				'Zum Chatten brauche ich ein Modell. Bitte die Person, die nolune eingerichtet hat, eines unter Modelle & Schlüssel hinzuzufügen; alles andere funktioniert schon.',
			choices: {
				'claude-plan': {
					title: 'Claude-Abo',
					about: 'Ein Pro- oder Max-Abo, über Claude Code auf diesem Computer'
				},
				'chatgpt-plan': {
					title: 'ChatGPT-Abo',
					about: 'Ein Plus- oder Pro-Abo, angemeldet mit ChatGPT'
				},
				anthropic: {
					title: 'Anthropic-API-Schlüssel',
					about: 'Claude-Modelle, nach Verbrauch bezahlt'
				},
				openai: { title: 'OpenAI-API-Schlüssel', about: 'GPT-Modelle, nach Verbrauch bezahlt' },
				openrouter: {
					title: 'OpenRouter-API-Schlüssel',
					about: 'Claude, GPT, Gemini und mehr mit einem Schlüssel'
				}
			},
			checkingPlan: 'Anmeldung wird geprüft…',
			pasteKey: (label: string) => `Füge deinen ${label}-Schlüssel ein`,
			checkKey: 'Schlüssel prüfen',
			keyWorks: 'Der Schlüssel funktioniert.',
			finishOnAdmin: 'Melde dich unter {link} an und prüfe dann noch einmal.',
			checkAgain: 'Noch einmal prüfen',
			pickModel: 'Mit welchem Modell sollen neue Chats beginnen?',
			asking: 'Frage nach den Modellen…',
			modelId: 'Modell-ID',
			/** Signing in with ChatGPT right in the step. */
			chatgpt: {
				title: 'Mit ChatGPT anmelden',
				about:
					'Melde dich auf der Seite von ChatGPT an und erlaube nolune, dein Plus- oder Pro-Abo zu nutzen. Chats zählen dann zur Nutzung des Abos, wie bei ChatGPT selbst.',
				newTab: 'Die Anmeldeseite von ChatGPT öffnet sich in einem neuen Tab.',
				waiting: 'Sobald du dort angemeldet bist, geht es hier von selbst weiter.',
				starting: 'Die Anmeldeseite von ChatGPT wird vorbereitet…',
				tryAgain: 'Erneut versuchen',
				otherDevice: 'Meldest du dich auf einem anderen Gerät an?'
			}
		},
		avatar: {
			title: 'Wer soll ich sein?',
			subtitle: (profile: string) => `Wähle ein Gesicht für ${profile}.`,
			thisOne: 'Dieses',
			hello: 'Schön, dich kennenzulernen.'
		},
		memory: {
			title: 'Kennen wir uns schon?',
			subtitle:
				'Wenn du ChatGPT, Claude oder Gemini nutzt, weiß es schon einiges über dich. Frag es mit diesem Prompt und füge die Antwort hier ein.',
			copyPrompt: 'Prompt kopieren',
			prompt: 'Der Prompt',
			open: (name: string) => `${name} öffnen`,
			haveIt: 'Hab ich',
			startFresh: 'Neu anfangen',
			pasteTitle: 'Hier einfügen',
			pastePlaceholder: 'Die ganze Antwort, samt Codeblock',
			sections: {
				instructions: 'Anweisungen',
				identity: 'Über dich',
				career: 'Beruf',
				projects: 'Projekte',
				preferences: 'Vorlieben',
				other: 'Sonstiges'
			},
			remember: (n: number) => p(n, { one: `${n} Sache merken`, other: `${n} Dinge merken` }),
			rememberThis: 'Merken',
			reading: 'Wird gelesen…',
			saving: 'Wird gespeichert…',
			empty: 'Füge ein, was der andere Assistent geantwortet hat.',
			tooLong:
				'Das ist zu lang für die Erinnerungen eines Menschen. Füge nur die Antwort auf den Prompt ein.',
			notAnExport:
				'Das sieht nicht nach der Antwort auf den Prompt aus. Füge die ganze Antwort samt Codeblock ein.',
			couldNotRead: (problem: string) => `Konnte es nicht lesen. ${problem}`,
			nothingFound:
				'Darin habe ich nichts über dich gefunden. Füge die ganze Antwort samt Codeblock ein.'
		},
		arrival: {
			thingsIKnow: (n: number) =>
				p(n, { one: 'Sache, die ich über dich weiß', other: 'Dinge, die ich über dich weiß' })
		},
		fresh: {
			title: 'Ein neuer Anfang.',
			subtitle: 'Ich lerne dich kennen, während wir reden.'
		}
	},

	login: {
		welcome: 'Willkommen zurück',
		hint: 'Melde dich mit dem Konto an, das du bekommen hast.',
		email: 'E-Mail-Adresse',
		password: 'Passwort',
		signingIn: 'Anmeldung läuft…',
		forgot:
			'Passwort vergessen? Bitte die Person, die nolune eingerichtet hat, {command} auszuführen.',
		tooManyAttempts: 'Zu viele Versuche. Warte eine Minute und versuch es noch einmal.',
		wrongPassword: 'E-Mail oder Passwort ist falsch.'
	},

	admin: {
		title: 'Modelle & Schlüssel',
		keys: 'API-Schlüssel',
		keysHint:
			'Von allen Profilen geteilt und in der Konfigurationsdatei von nolune auf diesem Computer gespeichert. Ein neuer Schlüssel wird vor dem Speichern beim Anbieter geprüft und sofort verwendet. Für einen eigenen Modellserver wie Ollama oder LM Studio füge einen eigenen Anbieter hinzu.',
		purposes: {
			anthropic: 'Für Chats und Automationen mit Claude-Modellen.',
			openai:
				'Für Chats und Automationen mit OpenAI-Modellen und zum Erstellen von Bildern auf der Seite „Bilder“ und wenn der Agent zeichnet.',
			openrouter:
				'Für Chats und Automationen mit den Modellen, die OpenRouter anbietet (Claude, GPT, Gemini, DeepSeek und viele mehr), mit einem Schlüssel und dessen Guthaben.'
		},
		withoutIt: {
			anthropic:
				'Chats und Automationen mit Claude-Modellen funktionieren nicht mehr, bis ein neuer Schlüssel hinzugefügt wird.',
			openai:
				'Chats und Automationen mit OpenAI-Modellen funktionieren nicht mehr, und nolune kann keine Bilder erstellen, bis ein neuer Schlüssel hinzugefügt wird.',
			openrouter:
				'Chats und Automationen mit OpenRouter-Modellen funktionieren nicht mehr, bis ein neuer Schlüssel hinzugefügt wird.'
		},
		savedInNolune: (hint: string | null) =>
			`In nolune gespeichert${hint ? `, endet auf ${hint}` : ''}`,
		fromEnv: (variable: string, hint: string | null) =>
			`Aus der Umgebungsvariable ${variable}${hint ? `, endet auf ${hint}` : ''}`,
		notSet: 'Nicht gesetzt',
		replace: 'Ersetzen',
		pasteKey: (provider: string) => `${provider}-API-Schlüssel einfügen`,
		keyLabel: (provider: string) => `${provider}-API-Schlüssel`,
		checkingKey: 'Schlüssel wird geprüft…',
		makeOneAt: 'Erstelle einen auf {link}.',
		sameProject:
			'Nimm einen Schlüssel aus demselben Projekt: Bilder und PDFs, die schon in Chats gesendet wurden, liegen dort, und ohne sie können diese Chats nicht weitergehen.',
		sameWorkspace:
			'Nimm einen Schlüssel aus demselben Workspace: Bilder und PDFs, die schon in Chats gesendet wurden, liegen dort, und ohne sie können diese Chats nicht weitergehen.',
		removeKeyTitle: (provider: string) => `${provider}-Schlüssel entfernen?`,
		useEnvInstead: (variable: string) =>
			`nolune verwendet dann den Schlüssel aus der Umgebungsvariable ${variable}.`,
		plan: 'Claude-Abo',
		plans: 'Abos',
		plansHint:
			'Chats mit einer Abo-Voreinstellung laufen über das eigene Abo von jemandem statt über einen API-Schlüssel: das Claude-Abo über Claude Code auf diesem Computer, das die Anmeldung selbst verwahrt; das ChatGPT-Abo über „Mit ChatGPT anmelden“, dessen Anmeldung nolune auf diesem Computer verwahrt. Die Abo-Limits gehen von der normalen Nutzung durch eine Person aus, also lass viel beschäftigte Automationen und Subagenten auf einer Voreinstellung mit API-Schlüssel.',
		claudePlanAbout: 'Pro oder Max, über Claude Code.',
		chatgptPlan: 'ChatGPT-Abo',
		chatgptPlanAbout: 'Plus oder Pro, angemeldet mit ChatGPT.',
		chatgptSignIn: 'Weiter mit ChatGPT',
		chatgptContinueAs: (account: string) => `Weiter als ${account}`,
		chatgptAnotherAccount: 'Anderes Konto',
		chatgptSignInAgain: 'Erneut anmelden',
		chatgptStarting: 'Wird gestartet…',
		signOut: 'Abmelden',
		chatgptOpen:
			'Öffne {link} und melde dich an; erlaube nolune dabei, dein ChatGPT-Abo zu nutzen.',
		chatgptSignInPage: 'die Anmeldeseite von ChatGPT',
		chatgptHere:
			'In einem Browser auf diesem Computer war’s das: Diese Seite aktualisiert sich, sobald du angemeldet bist.',
		chatgptElsewhere:
			'Auf einem anderen Gerät lädt die Seite nicht, zu der ChatGPT dich zurückschickt. Kopiere ihre Adresse (sie beginnt mit http://127.0.0.1) und füge sie hier ein:',
		chatgptFinish: 'Fertig',
		chatgptSignedIn: 'Angemeldet. Chats mit ChatGPT-Abo-Voreinstellungen nutzen jetzt dieses Abo.',
		chatgptUsing: 'Chats mit ChatGPT-Abo-Voreinstellungen nutzen dieses Abo. {link}',
		chatgptManageUsage: 'Nutzung verwalten',
		chatgptNobody: 'Niemand ist mit ChatGPT angemeldet.',
		chatgptSignedOutLocally:
			'Hier abgemeldet, aber OpenAI konnte nicht benachrichtigt werden: Trenne nolune zur Sicherheit in den Einstellungen von ChatGPT.',
		chatgptSignOutTitle: 'Von ChatGPT abmelden?',
		chatgptSignOutBody:
			'Chats mit ChatGPT-Abo-Voreinstellungen funktionieren erst wieder, wenn sich jemand erneut anmeldet.',
		signedOut: 'Abgemeldet.',
		notAt: 'Nicht unter {path}, wo es laut {command} sein sollte.',
		notInstalled: 'Nicht auf diesem Computer installiert.',
		checkSignIn: 'Anmeldung prüfen',
		install:
			'Führe in einem Terminal auf diesem Computer {setup} aus: Das installiert Claude Code mit dem Installer von Anthropic und meldet es bei deinem Claude-Konto an, jeweils nach Rückfrage. Oder installiere es selbst, starte dann {claude} und melde dich an:',
		copyCommand: 'Befehl kopieren',
		signIn:
			'Zum Anmelden führe {setup} in einem Terminal auf diesem Computer aus, oder starte dort {claude} und nutze {login} mit deinem Claude-Konto.',
		models: 'Modelle',
		modelsHint:
			'Voreinstellungen werden von allen Profilen geteilt. Neue Chats starten mit der Standard-Voreinstellung. Das Bearbeiten oder Entfernen einer Voreinstellung ändert nichts an Chats, die sie schon verwenden.',
		presetDetails: (provider: string, model: string, context: string, overridden: boolean) =>
			`${provider} / ${model} · Kontext ${context}${overridden ? ' (überschrieben)' : ''}`,
		makeDefault: 'Als Standard festlegen',
		editPreset: (name: string) => `${name} bearbeiten`,
		noPresets: 'Noch keine Voreinstellungen.',
		addModel: {
			title: 'Modell hinzufügen',
			provider: 'Anbieter',
			model: 'Modell',
			name: 'Name',
			optional: '(optional)',
			namePlaceholder: 'Modell-ID und Anbieter',
			contextWindow: 'Kontextfenster',
			contextTokens: 'Kontextfenster in Tokens',
			auto: 'Auto',
			autoWith: (size: string) => `Auto · ${size}`,
			unknown: 'unbekannt',
			custom: 'Eigenes',
			tokensPlaceholder: 'Tokens, z. B. 272k',
			tokens: (n: number) => `${n.toLocaleString('de')} Tokens.`,
			typeTokens: 'Gib eine Tokenzahl ein, etwa 272k oder 272000.',
			autoContext: {
				anthropic: 'Auto verwendet das Fenster, das Anthropic für das Modell meldet.',
				openai: 'OpenAI meldet es nicht: Auto kennt nur das der Flaggschiffe (1,05M seit GPT-5.4).',
				openrouter:
					'Auto verwendet das Fenster, das OpenRouter für das Modell und seinen Hauptanbieter angibt.',
				'custom-openai':
					'Auto verwendet das Fenster, das der Server für das Modell angibt, falls er eines angibt (vLLM tut es); sonst bleibt es offen.',
				'custom-anthropic':
					'Auto verwendet das Fenster, das der Server für das Modell angibt, falls er eines angibt (vLLM tut es); sonst bleibt es offen.',
				'claude-plan': 'Claude Code meldet es nicht: Auto kennt nur seine Modelle mit 1M Kontext.',
				'chatgpt-plan':
					'Auto nutzt das Fenster, das ChatGPT für das Modell angibt, falls es eines angibt; sonst bleibt es offen.'
			},
			onPlan: 'Läuft über das Pro- oder Max-Abo, mit dem Claude Code angemeldet ist.',
			noClaudeCode: 'Claude Code ist noch nicht installiert: siehe „Claude-Abo“ oben.',
			onChatGptPlan: 'Läuft über das ChatGPT-Abo der Person, die oben mit ChatGPT angemeldet ist.',
			noChatGpt: 'Noch ist niemand mit ChatGPT angemeldet: siehe „ChatGPT-Abo“ oben.',
			onKey: (provider: string) => `Läuft über den ${provider}-API-Schlüssel.`,
			noKey: (provider: string) =>
				`Noch kein ${provider}-API-Schlüssel: Füge oben unter „API-Schlüssel“ einen hinzu.`,
			onCustom: (api: string, url: string) =>
				`Läuft auf ${url}, über seine ${api}-API. Das Modell muss Werkzeuge aufrufen können; Bilder und PDFs erhält es als Pfade.`,
			customGone: 'Dieser eigene Anbieter wurde entfernt: Wähle einen anderen.',
			asking: (source: string) => `${source} wird nach seinen Modellen gefragt…`,
			listProblem: (problem: string) => `${problem} Du kannst trotzdem eine ID eingeben.`,
			couldNotList: (status: number) => `nolune konnte die Modelle nicht abrufen (${status}).`,
			unreachable: 'nolune ist nicht erreichbar.',
			checkingClaude: 'Claude Code wird geprüft…',
			checkingModel: 'Modell wird geprüft…',
			saving: 'Wird gespeichert…',
			pick: 'Modell wählen',
			search: 'Suchen oder eine Modell-ID eingeben',
			use: '{model} verwenden',
			typeId: 'Gib oben die ID des Modells ein.',
			added: (name: string) => `${name} hinzugefügt.`,
			saved: (name: string) =>
				`${name} gespeichert. Chats, die sie schon verwenden, bleiben, wie sie waren.`,
			invalidContext: 'Das Kontextfenster muss eine Tokenzahl sein, etwa 272k oder 272000.'
		},
		savedWorks: 'Gespeichert. Er funktioniert.',
		savedWarning: (warning: string) => `Gespeichert. ${warning}`,
		claudeNoAnswer: 'Claude Code hat nicht geantwortet.',
		removed: 'Entfernt.',
		newDefault: (name: string) => `Neue Chats starten jetzt mit ${name}.`,
		presetRemoved: 'Entfernt. Bestehende Chats funktionieren weiter.',
		/** Custom providers: model servers of the family's own, listed with the API keys. */
		customProviders: {
			add: 'Eigenen Anbieter hinzufügen',
			addTitle: 'Eigenen Anbieter hinzufügen',
			addHint:
				'Ein eigener Modellserver, etwa Ollama, LM Studio oder oMLX auf diesem Computer oder vLLM auf einem Rechner mit GPUs. Unter „Modell hinzufügen“ erscheint er mit seinem Namen.',
			about: (api: string) => `Dein eigener Modellserver, über seine ${api}-API.`,
			apis: { openai: 'OpenAI', anthropic: 'Anthropic' },
			change: 'Ändern',
			name: 'Name',
			namePlaceholder: 'Ollama',
			api: 'API',
			apiHints: {
				openai:
					'Die Responses API von OpenAI: Ollama, LM Studio, vLLM, LiteLLM. Die Gedächtnissuche kann seine Embeddings nutzen.',
				anthropic:
					'Die Messages API von Anthropic: Ollama, LM Studio, der Server von llama.cpp, oMLX.'
			},
			address: 'Adresse',
			addressHint:
				'Ollama lauscht auf http://localhost:11434, LM Studio auf http://localhost:1234. nolune fragt ihn nach seinen Modellen, um ihn zu prüfen.',
			key: 'Schlüssel',
			keyOptional: '(falls er einen braucht)',
			keyKept: 'Gespeichert. Leer lassen, um ihn zu behalten.',
			withKey: (hint: string | null) => (hint ? `Schlüssel endet auf ${hint}` : 'mit Schlüssel'),
			noKey: 'ohne Schlüssel',
			checking: 'Wird geprüft…',
			works: (n: number) =>
				`Gespeichert. Er bietet ${n} ${p(n, { one: 'Modell', other: 'Modelle' })}.`,
			unchecked: (problem: string) => `Ohne Prüfung gespeichert: ${problem}`,
			needName: 'Gib ihm einen Namen, etwa Ollama oder GPU box.',
			needAddress: 'Gib seine Adresse an, beginnend mit http:// oder https://.',
			removeTitle: (name: string) => `${name} entfernen?`,
			removeBody:
				'Chats und Automationen mit seinen Modellen funktionieren nicht mehr, bis sie auf ein anderes Modell umgestellt sind.',
			usedBy: (names: string) => `Diese Voreinstellungen laufen darauf: ${names}.`
		},
		/** Memory search by meaning: where its embeddings come from. */
		embeddings: {
			title: 'Gedächtnissuche',
			hint: 'nolune findet Fakten im Gedächtnis auch nach ihrer Bedeutung, nicht nur nach ihren Wörtern, und über Sprachen hinweg. Dafür wird jeder Fakt einmal dorthin geschickt, wo die Embeddings entstehen, und jede Nachricht beim Senden; ein Server auf diesem Computer behält sie hier. Gilt für alle Profile.',
			name: 'Suche nach Bedeutung',
			using: (source: string) => `Verwendet ${source}.`,
			off: 'Aus: Das Gedächtnis wird nur nach Wörtern durchsucht.',
			noKeys:
				'Noch kein OpenAI- oder OpenRouter-Schlüssel, daher wird das Gedächtnis nur nach Wörtern durchsucht.',
			noKey: (provider: string) =>
				`Noch kein ${provider}-Schlüssel, daher wird das Gedächtnis nur nach Wörtern durchsucht. Füge oben unter API-Schlüssel einen hinzu.`,
			customGone:
				'Dieser eigene Anbieter wurde entfernt, daher wird das Gedächtnis nur nach Wörtern durchsucht.',
			change: 'Ändern',
			source: 'Embeddings von',
			modes: {
				auto: 'Auto',
				openai: 'OpenAI',
				openrouter: 'OpenRouter',
				off: 'Aus'
			},
			autoNote:
				'text-embedding-3-small von OpenAI mit dem OpenAI-Schlüssel, sonst dasselbe Modell über OpenRouter.',
			withKey: (provider: string) => `Mit dem ${provider}-API-Schlüssel.`,
			customNote: (url: string) =>
				`Ein Modell, das er unter ${url} anbietet, über seine OpenAI-API.`,
			offNote:
				'Das Gedächtnis wird nur nach Wörtern durchsucht, und kein Fakt wird irgendwohin geschickt.',
			model: 'Modell',
			customModel: 'Sein Name dort, etwa nomic-embed-text',
			checking: 'Wird geprüft…',
			works: 'Gespeichert. Es funktioniert; die Fakten werden im Hintergrund verarbeitet.',
			wordsOnly: 'Gespeichert. Das Gedächtnis wird nur nach Wörtern durchsucht.',
			noAnswer: (problem: string) => `Gespeichert, aber keine Antwort: ${problem}`,
			needModel: 'Gib das Modell an, das verwendet werden soll.'
		},
		commands: {
			title: 'Befehle',
			hint: 'Der Agent arbeitet, indem er Befehle auf diesem Computer ausführt, mit dem Zugriff dieses Kontos auf seine Dateien und Apps. Im automatischen Modus prüft ein Modell jeden Befehl, bevor er läuft, und stoppt, was Schaden anrichten könnte, ohne dass jemand darum gebeten hat. Jeder Chat richtet sich danach, außer jemand stellt ihn mit dem Schild im Eingabefeld anders ein; die Prüfung ausschalten können dort nur Admins.',
			modes: {
				auto: 'Automatischer Modus',
				unrestricted: 'Uneingeschränkt'
			},
			choices: {
				auto: 'Automatisch (empfohlen)',
				unrestricted: 'Uneingeschränkt (nicht empfohlen)'
			},
			checkedByChat: 'Jeden Befehl prüft das eigene Modell des Chats.',
			checkedBy: (preset: string) => `Jeden Befehl prüft ${preset}.`,
			presetGone:
				'Das für die Prüfungen gewählte Modell wurde entfernt, deshalb prüft jeder Chat seine Befehle mit seinem eigenen Modell.',
			unrestrictedStatus: 'Befehle laufen ohne Prüfung.',
			change: 'Ändern',
			mode: 'Modus',
			autoNote:
				'Befehle, die nur nachsehen, laufen sofort. Die übrigen werden zuerst geprüft; ein blockierter Befehl läuft nicht, und nolune sagt, was es vorhatte, damit jemand zustimmen kann.',
			unrestrictedNote:
				'Jeder Befehl läuft so, wie der Agent ihn geschrieben hat. Nichts hält einen Fehler auf oder eine Webseite oder E-Mail, die ihn zu etwas überredet. Nur für Leute, die genau hinsehen.',
			checker: 'Geprüft von',
			chatModel: 'Das eigene Modell des Chats',
			checkerNote:
				'Ein schnelles, fähiges Modell hält Chats flott: Jeder Befehl, der mehr tut als nachzusehen, kostet eine kurze Anfrage an das Modell.',
			saved: 'Gespeichert. Gilt ab dem nächsten Befehl.'
		}
	}
};
