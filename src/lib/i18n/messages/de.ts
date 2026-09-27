import { isMonthSpan, type Schedule } from '@btw/core/schedule';
import { listOf, plural } from '../plural';
import type { Messages } from './en';

const p = plural('de');
const list = listOf('de');

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
		description: 'Wie btw auf diesem Gerät aussieht.',
		theme: 'Design',
		system: 'System',
		light: 'Hell',
		dark: 'Dunkel',
		language: 'Sprache',
		languageAuto: 'Wie im Browser',
		languageHint: 'Für Menüs und Schaltflächen. btw antwortet in der Sprache, in der du schreibst.',
		technical: 'Technische Details anzeigen',
		technicalHint:
			'Zeigt die genauen Befehle, die btw ausführt, den Tokenverbrauch und das Prompt-Caching.',
		expandSteps: 'Schritte immer anzeigen',
		expandStepsHint:
			'Die Liste dessen, was btw getan hat, unter jeder Antwort aufgeklappt statt eingeklappt zeigen.',
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
			'Noch nichts da. Bitte btw um eine Erinnerung oder eine tägliche Prüfung, dann erscheint hier, was es findet.'
	},

	chat: {
		options: 'Chat-Optionen',
		placeholder: 'Frag btw',
		placeholderRunning: 'Ergänze etwas, während btw arbeitet…',
		disclaimer: 'btw kann Fehler machen und Dateien auf diesem Computer ändern.',
		reconnecting: 'Verbindung wird wiederhergestellt…',
		empty: 'Bitte um etwas, um loszulegen.',
		readsAfterStep: 'btw liest das nach seinem aktuellen Schritt',
		working: 'btw arbeitet',
		thinking: 'Denkt nach',
		writing: 'Schreibt',
		cutOff: 'Die Antwort wurde abgeschnitten, weil sie zu lang wurde.',
		refused: 'btw hat es abgelehnt, diese Anfrage fortzusetzen.',
		automation: (title: string) => `Automation · ${title}`,
		fromBtw: (to: string) => `Von btw an ${to}`,
		finishedInBackground: (title: string) => `Im Hintergrund fertig · ${title}`,
		inBackground: 'Arbeitet im Hintergrund',
		aCommand: 'Ein Befehl',
		subagent: (name: string) => `Subagent ${name}`,
		stopping: 'wird gestoppt',
		errorTitle: 'Beim Antworten von btw ist etwas schiefgelaufen.',
		unanswered: 'btw hat darauf noch nicht geantwortet.',
		scrollToBottom: 'Zur neuesten Nachricht scrollen',
		subagentBanner:
			'Subagent {name}: btw hat ihn aus {parent} gestartet, und er berichtet dorthin.',
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
			`btw hält diesen Chat in einem Cache, damit jede Antwort nur für Neues bezahlt. ${model ? 'Ein anderes Modell' : 'Eine andere Denktiefe'} kann ihn nicht nutzen: Die nächste Antwort liest den ganzen Chat noch einmal, was länger dauert und mehr kostet${missTokens ? ` (ein Prompt-Cache-Fehlschlag von etwa ${missTokens} Tokens)` : ''}.`,
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
		failed: 'fehlgeschlagen',
		didntWork: 'hat nicht geklappt',
		statusStopped: 'gestoppt',
		statusNotRun: 'nicht ausgeführt',
		command: 'Befehl',
		theCommand: 'Der Befehl, den btw ausgeführt hat',
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

	newChat: {
		greeting: (name: string) => `Wobei kann ich helfen, ${name}?`,
		greetingNoName: 'Wobei kann ich helfen?',
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
			'{name} wird für alle im Profil gelöscht. Seine Chats kommen zurück in deine Chatliste, ohne seine Anweisungen und Dateien. Die Dateien werden nach ~/.btw-agent/trash verschoben.',
		options: 'Ordner-Optionen',
		newChatIn: (name: string) => `Neuer Chat in ${name}`,
		instructions: 'Anweisungen',
		instructionsHint: 'Was btw in jedem Chat hier wissen oder tun soll.',
		instructionsPlaceholder:
			'Wir planen im April zwei Wochen Japan mit den Kindern (7 und 10). Plane entspannt und bleib unter 600.000 ¥.',
		changesTechnical:
			'Chats im Ordner bekommen Änderungen mit ihrer nächsten Nachricht, die den Chat einmal neu liest (ein Prompt-Cache-Fehlschlag).',
		changes: 'Chats im Ordner bekommen Änderungen mit ihrer nächsten Nachricht.',
		files: 'Dateien',
		addFiles: 'Dateien hinzufügen',
		filesHint:
			'Bilder, Dokumente, alles Mögliche. btw weiß, wo sie gespeichert sind, und öffnet sie, wenn sie wichtig sind.',
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
		onlyPath: (note: string) => `btw hat nur den Speicherort bekommen: ${note}`
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
		cantMakeYet: 'btw kann noch keine Bilder erstellen.',
		needsKeyAdmin: 'Dafür braucht es einen {provider}-API-Schlüssel. {link}.',
		addKeyLink: 'Füge ihn unter „Modelle & Schlüssel“ hinzu',
		needsKey: (provider: string) =>
			`Dafür braucht es einen ${provider}-API-Schlüssel. Bitte einen Admin, ihn hinzuzufügen.`,
		adminSetsUp: 'Das richtet ein Admin auf dem Computer ein, auf dem btw läuft.',
		describe: 'Beschreibe ein Bild',
		describeHint:
			'btw erstellt das Bild in einem neuen Chat, in dem du um Änderungen bitten kannst.',
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
			`Was sich btw für ${profile} merkt, sichtbar für alle darin. Jeder Chat beginnt mit der angehefteten Notiz „Core“; die anderen liest btw, wenn ein Chat sie braucht, und speichert unterwegs, was es lernt. Um etwas hinzuzufügen, sag es ihm einfach in einem Chat, etwa „Merk dir, dass Anna allergisch gegen Nüsse ist“.`,
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
			'Noch leer. Schreib hier hinein, was btw in jedem Chat im Kopf haben soll: wer zur Familie gehört, welche Sprachen ihr sprecht, Allergien. btw ergänzt es auch selbst.',
		forgetTitle: (topic: string) => `„${topic}“ vergessen?`,
		forgetBody:
			'btw vergisst alles in {path} für alle in {profile}. Chats, die es schon gelesen haben, behalten, was sie gelesen haben.',
		forgetButton: 'Vergessen',
		empty: 'Die Notiz ist leer. Zum Löschen nutze „Vergessen“.',
		conflict: (problem: string) =>
			`${problem} Speichere noch einmal, um deine Version zu behalten, oder brich ab, um die von btw zu sehen.`,
		saved: 'Gespeichert. Neue Chats sehen die Änderung.',
		forgot: (path: string) => `btw hat alles in ${path} vergessen.`,
		learned: (ago: string) => `gelernt ${ago}`,
		learnedAWhileAgo: 'vor einer Weile gelernt',
		topicLabel: (topic: string, n: number) => `${topic}: ${memories(n)}. Notiz anzeigen.`,
		nothingInIt: 'Noch nichts darin',
		showFewer: 'Weniger anzeigen',
		showAllTopics: (n: number) => `Alle ${n} Themen anzeigen`,
		older: 'Älter',
		newer: 'Neuer',
		nothingYet: 'Noch nichts gemerkt. Alles, was btw lernt, wird hier zu einem Punkt.'
	},

	automations: {
		title: 'Automationen',
		intro:
			'Was btw von selbst erledigt: Erinnerungen, regelmäßige Prüfungen und Antworten an andere Apps. Was es findet, erscheint unter der Glocke. Um eine hinzuzufügen oder zu ändern, bitte btw einfach in einem Chat darum.',
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
		instructions: 'Anweisungen für btw',
		instructionsTechnical: (model: string, effort: string) =>
			`Anweisungen für btw (${model}, Denktiefe ${effort})`,
		script: 'Skript: läuft ohne das Modell und ruft {command} auf, wenn btw gebraucht wird',
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
		empty: 'Noch keine Automationen. Bitte btw in einem Chat darum, zum Beispiel:',
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
			'Fähigkeiten sind Anleitungen, denen btw bei bestimmten Aufgaben folgt. Es sieht Namen und Beschreibung jeder eingeschalteten Fähigkeit und liest den Rest, wenn eine Aufgabe es erfordert. Wenn du Fähigkeiten ausschaltest, die dieses Profil nicht braucht, bleibt btw fokussiert. Änderungen gelten für neue Chats.',
		summary: (on: number, total: number, tokens: string) =>
			`${on} von ${total} an · etwa ${tokens} Tokens am Anfang jedes neuen Chats`,
		madeFor: (profile: string) => `Für ${profile}`,
		shared: 'Von allen Profilen geteilt',
		builtIn: 'In btw eingebaut',
		allOff: 'Alle ausschalten',
		allOn: 'Alle einschalten',
		tokens: (n: string) => `~${n} Tokens`,
		use: (name: string) => `${name} verwenden`,
		empty:
			'Noch keine Fähigkeiten. Wenn btw herausfindet, wie etwas geht, kann es das als Fähigkeit für das nächste Mal speichern.',
		gone: 'Diese Fähigkeit gibt es nicht mehr.'
	},

	profile: {
		title: 'Personen & Profil',
		name: 'Name',
		profileName: 'Name des Profils',
		folder: (slug: string) => `Ordner: ~/.btw-agent/profiles/${slug} (ändert sich nicht)`,
		avatar: 'Avatar',
		avatarHint:
			'Wie btw in den Chats dieses Profils aussieht. Alle hier sehen denselben, und btw kann ihn ändern, wenn man es darum bittet.',
		soul: 'Seele',
		soulHint: (profile: string) =>
			`Wer btw für ${profile} ist: sein Charakter, was ihm wichtig ist, wie es spricht. Jeder Chat beginnt damit, und btw ändert es auch selbst, wenn du es bittest, anders zu sein.`,
		soulPlaceholder:
			'Du bist herzlich und ein bisschen verspielt und hältst Antworten kurz. Den Kindern erklärst du Dinge einfach und nie von oben herab. Wenn du etwas nicht weißt, sagst du es.',
		changesTechnical:
			'Chats bekommen Änderungen mit ihrer nächsten Nachricht, die den Chat einmal neu liest (ein Prompt-Cache-Fehlschlag).',
		changes: 'Chats bekommen Änderungen mit ihrer nächsten Nachricht.',
		members: 'Mitglieder',
		membersHint: 'Alle hier sehen dieselben Chats, schreiben darin und können btw um alles bitten.',
		personToAdd: 'Person zum Hinzufügen',
		chooseSomeone: 'Wähle jemanden zum Hinzufügen',
		everyoneIsMember: 'Alle sind schon Mitglied.',
		deleteTitle: 'Profil löschen',
		deleteHint:
			'Löscht alle seine Chats für alle. Der Ordner wird nach ~/.btw-agent/trash verschoben.',
		deleteButton: 'Dieses Profil löschen',
		deleteConfirm: (name: string) => `„${name}“ löschen?`,
		deleteBody:
			'Alle seine Chats werden für alle gelöscht. Der Ordner wird nach ~/.btw-agent/trash verschoben.',
		renamed: 'Umbenannt.',
		soulSaved: 'Seele gespeichert.',
		soulRemoved: 'Seele entfernt.',
		added: (name: string) => `${name} hinzugefügt.`,
		removed: 'Entfernt.'
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

	login: {
		welcome: 'Willkommen zurück',
		hint: 'Melde dich mit dem Konto an, das du bekommen hast.',
		email: 'E-Mail-Adresse',
		password: 'Passwort',
		signingIn: 'Anmeldung läuft…',
		forgot:
			'Passwort vergessen? Bitte die Person, die btw eingerichtet hat, {command} auszuführen.',
		tooManyAttempts: 'Zu viele Versuche. Warte eine Minute und versuch es noch einmal.',
		wrongPassword: 'E-Mail oder Passwort ist falsch.'
	},

	admin: {
		title: 'Modelle & Schlüssel',
		keys: 'API-Schlüssel',
		keysHint:
			'Von allen Profilen geteilt und in der Konfigurationsdatei von btw auf diesem Computer gespeichert. Ein neuer Schlüssel wird vor dem Speichern beim Anbieter geprüft und sofort verwendet.',
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
				'Chats und Automationen mit OpenAI-Modellen funktionieren nicht mehr, und btw kann keine Bilder erstellen, bis ein neuer Schlüssel hinzugefügt wird.',
			openrouter:
				'Chats und Automationen mit OpenRouter-Modellen funktionieren nicht mehr, bis ein neuer Schlüssel hinzugefügt wird.'
		},
		savedInBtw: (hint: string | null) => `In btw gespeichert${hint ? `, endet auf ${hint}` : ''}`,
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
			`btw verwendet dann den Schlüssel aus der Umgebungsvariable ${variable}.`,
		plan: 'Claude-Abo',
		plans: 'Abos',
		plansHint:
			'Chats mit einer Abo-Voreinstellung laufen über das eigene Abo von jemandem statt über einen API-Schlüssel, mit dem Agenten des Anbieters auf diesem Computer. btw startet ihn und sieht die Anmeldung nie. Die Abo-Limits gehen von der normalen Nutzung durch eine Person aus, also lass viel beschäftigte Automationen und Subagenten auf einer Voreinstellung mit API-Schlüssel.',
		claudePlanAbout: 'Pro oder Max, über Claude Code.',
		chatgptPlan: 'ChatGPT-Abo',
		chatgptPlanAbout: 'Plus, Pro oder Business, über Codex von OpenAI.',
		chatgptInstall:
			'Führe in einem Terminal auf diesem Computer {setup} aus: Es installiert Codex mit npm (nach Rückfrage) und meldet es bei ChatGPT an. Oder installiere es selbst und melde dich dann hier an:',
		chatgptSignIn: 'Mit ChatGPT anmelden',
		chatgptSignInAgain: 'Erneut anmelden',
		chatgptAsking: 'ChatGPT wird gefragt…',
		signOut: 'Abmelden',
		chatgptOpen: 'Öffne {link} auf einem beliebigen Gerät und melde dich bei ChatGPT an.',
		chatgptCode: 'Gib diesen Code ein: {code}',
		chatgptCodeHint:
			'Der Code gilt 15 Minuten. Diese Seite aktualisiert sich, sobald er eingegeben ist.',
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
			'Voreinstellungen werden von allen Profilen geteilt. Neue Chats starten mit der Standard-Voreinstellung. Das Entfernen einer Voreinstellung wirkt sich nicht auf bestehende Chats aus.',
		presetDetails: (provider: string, model: string, context: string, overridden: boolean) =>
			`${provider} / ${model} · Kontext ${context}${overridden ? ' (überschrieben)' : ''}`,
		makeDefault: 'Als Standard festlegen',
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
				'claude-plan': 'Claude Code meldet es nicht: Auto kennt nur seine Modelle mit 1M Kontext.',
				'chatgpt-plan': 'Codex meldet es nicht, also lässt Auto es offen.'
			},
			onPlan: 'Läuft über das Pro- oder Max-Abo, mit dem Claude Code angemeldet ist.',
			noClaudeCode: 'Claude Code ist noch nicht installiert: siehe „Claude-Abo“ oben.',
			onChatGptPlan: 'Läuft über das ChatGPT-Abo, mit dem Codex angemeldet ist.',
			noCodex: 'Codex ist noch nicht installiert: siehe „ChatGPT-Abo“ oben.',
			onKey: (provider: string) => `Läuft über den ${provider}-API-Schlüssel.`,
			noKey: (provider: string) =>
				`Noch kein ${provider}-API-Schlüssel: Füge oben unter „API-Schlüssel“ einen hinzu.`,
			asking: (source: string) => `${source} wird nach seinen Modellen gefragt…`,
			listProblem: (problem: string) => `${problem} Du kannst trotzdem eine ID eingeben.`,
			couldNotList: (status: number) => `btw konnte die Modelle nicht abrufen (${status}).`,
			unreachable: 'btw ist nicht erreichbar.',
			checkingClaude: 'Claude Code wird geprüft…',
			checkingCodex: 'Codex wird geprüft…',
			checkingModel: 'Modell wird geprüft…',
			pick: 'Modell wählen',
			search: 'Suchen oder eine Modell-ID eingeben',
			use: '{model} verwenden',
			typeId: 'Gib oben die ID des Modells ein.',
			added: (name: string) => `${name} hinzugefügt.`,
			invalidContext: 'Das Kontextfenster muss eine Tokenzahl sein, etwa 272k oder 272000.'
		},
		savedWorks: 'Gespeichert. Er funktioniert.',
		savedWarning: (warning: string) => `Gespeichert. ${warning}`,
		claudeNoAnswer: 'Claude Code hat nicht geantwortet.',
		removed: 'Entfernt.',
		newDefault: (name: string) => `Neue Chats starten jetzt mit ${name}.`,
		presetRemoved: 'Entfernt. Bestehende Chats funktionieren weiter.'
	}
};
