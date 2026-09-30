import { isMonthSpan, type Schedule } from '@nolune/core/schedule';
import { listOf, plural } from '../plural';
import type { Messages } from './en';

const p = plural('fr');
const list = listOf('fr');

/** By JavaScript's numbers: 0 is Sunday. */
const WEEKDAYS = ['dimanche', 'lundi', 'mardi', 'mercredi', 'jeudi', 'vendredi', 'samedi'];
const MONTHS = [
	'',
	'janvier',
	'février',
	'mars',
	'avril',
	'mai',
	'juin',
	'juillet',
	'août',
	'septembre',
	'octobre',
	'novembre',
	'décembre'
];

const capitalize = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);
const lowerFirst = (text: string) => text.charAt(0).toLowerCase() + text.slice(1);
/** "1er", "15". */
const day = (n: number) => (n === 1 ? '1er' : String(n));
/** "de mars", "d’avril". */
const of = (word: string) => (/^[aeiouéh]/i.test(word) ? `d’${word}` : `de ${word}`);

/** Which days, as a sentence start ("Du lundi au vendredi") and as a tail. */
function days({ days, months }: Schedule): { every: string; on: string } {
	if (days.kind === 'monthDays') {
		const which =
			days.days.length === 1 ? `le ${day(days.days[0])}` : `les ${list(days.days.map(day))}`;
		const text = months
			? `${which}, en ${list(months.map((m) => MONTHS[m]))}`
			: `${which} de chaque mois`;
		return { every: capitalize(text), on: text };
	}
	if (days.kind === 'date') {
		const date = `${day(days.day)} ${MONTHS[days.month]}`;
		return { every: `Chaque année le ${date}`, on: `chaque année le ${date}` };
	}
	let words: { every: string; on: string };
	if (days.kind === 'daily') words = { every: 'Tous les jours', on: '' };
	else if (days.kind === 'weekdays') {
		words = { every: 'Du lundi au vendredi', on: 'du lundi au vendredi' };
	} else if (days.kind === 'weekends') words = { every: 'Le week-end', on: 'le week-end' };
	else if (days.kind === 'weekdayRange') {
		const range = `du ${WEEKDAYS[days.from]} au ${WEEKDAYS[days.to]}`;
		words = { every: capitalize(range), on: range };
	} else {
		// "les lundis et mercredis"
		const which = `les ${list(days.days.map((d) => `${WEEKDAYS[d]}s`))}`;
		words = { every: capitalize(which), on: which };
	}
	if (!months) return words;
	const inMonths = isMonthSpan(months)
		? `${of(MONTHS[months[0]])} à ${MONTHS[months[months.length - 1]]}`
		: `en ${list(months.map((m) => MONTHS[m]))}`;
	return {
		every: `${words.every} ${inMonths}`,
		on: `${words.on || 'tous les jours'} ${inMonths}`
	};
}

function describe(schedule: Schedule): string {
	const when = days(schedule);
	const tail = when.on ? ` ${when.on}` : '';
	const { times } = schedule;
	switch (times.kind) {
		case 'minutes': {
			const every = times.step === 1 ? 'Toutes les minutes' : `Toutes les ${times.step} minutes`;
			if (!times.between) return every + tail;
			return `${every} de ${times.between.from} à ${times.between.to}${tail}`;
		}
		case 'hours': {
			const past = times.minute ? ` à la minute ${times.minute}` : '';
			const every = times.step === 1 ? 'Toutes les heures' : `Toutes les ${times.step} heures`;
			return `${every}${past}${tail}`;
		}
		case 'hourRange':
			return `Toutes les heures de ${times.from} à ${times.to}${tail}`;
		case 'at':
			return `${when.every} à ${list(times.times)}`;
	}
}

const memories = (n: number) => p(n, { one: `${n} souvenir`, other: `${n} souvenirs` });

export const fr: Messages = {
	common: {
		add: 'Ajouter',
		cancel: 'Annuler',
		close: 'Fermer',
		continue: 'Continuer',
		copied: 'Copié',
		copy: 'Copier',
		create: 'Créer',
		default: 'Par défaut',
		delete: 'Supprimer',
		done: 'Terminé',
		edit: 'Modifier',
		more: 'Plus',
		newChat: 'Nouvelle discussion',
		remove: 'Retirer',
		rename: 'Renommer',
		save: 'Enregistrer',
		stop: 'Arrêter',
		tryAgain: 'Réessayer',
		checking: 'Vérification…',
		starting: 'Démarrage…',
		uploading: 'Envoi…',
		removeFile: (name: string) => `Retirer ${name}`,
		characters: (count: number, max: number) => `${count} / ${max} caractères`
	},

	errors: {
		requestFailed: (status: number) => `La requête a échoué (${status})`,
		uploadFailed: (status: number | null) =>
			`L’envoi a échoué (${status === null ? 'pas de connexion' : status})`,
		couldNotSave: 'Impossible d’enregistrer.',
		somethingWentWrong: 'Un problème est survenu.',
		thatDidntWork: 'Cela n’a pas fonctionné.',
		tooManyFiles: (max: number) => `${max} fichiers au maximum par message`,
		tooLarge: (mb: number) => `Plus de ${mb} Mo`,
		notSignedIn: 'Vous n’êtes pas connecté',
		adminsOnly: 'Réservé aux administrateurs',
		profileNotFound: 'Profil introuvable',
		conversationNotFound: 'Discussion introuvable',
		folderNotFound: 'Dossier introuvable',
		automationNotFound: 'Automatisation introuvable',
		messageEmpty: 'Le message est vide',
		noFile: 'Aucun fichier'
	},

	time: {
		justNow: 'à l’instant',
		seconds: (s: number) => `${s} s`,
		minutesSeconds: (m: number, s: number) => `${m} min ${s} s`,
		hoursMinutes: (h: number, m: number) => `${h} h ${m} min`,
		minutesAgo: (n: number) => `il y a ${n} min`,
		hoursAgo: (n: number) => `il y a ${n} h`
	},

	units: { bytes: 'o', kilobytes: 'Ko', megabytes: 'Mo' },

	settings: {
		title: 'Paramètres',
		description: 'L’apparence de nolune sur cet appareil.',
		theme: 'Thème',
		system: 'Système',
		light: 'Clair',
		dark: 'Sombre',
		language: 'Langue',
		languageAuto: 'Celle du navigateur',
		languageHint:
			'Pour les menus et les boutons. nolune répond dans la langue dans laquelle vous écrivez.',
		technical: 'Afficher les détails techniques',
		technicalHint:
			'Afficher les commandes exactes que nolune exécute, la consommation de tokens et le cache du prompt.',
		expandSteps: 'Toujours afficher les étapes',
		expandStepsHint:
			'Déplier sous chaque réponse la liste de ce que nolune a fait, au lieu de la garder repliée.',
		sounds: 'Sons',
		soundsHint:
			'Des sons discrets quand nolune bouge tout seul, comme à l’accueil d’un nouveau profil.',
		logOut: 'Se déconnecter',
		deviceOnly: 'Ces paramètres ne sont enregistrés que sur cet appareil.'
	},

	userMenu: {
		account: 'Compte',
		settings: 'Paramètres',
		allProfiles: 'Tous les profils',
		modelsAndKeys: 'Modèles et clés',
		logOut: 'Se déconnecter'
	},

	header: {
		openMenu: 'Ouvrir le menu'
	},

	sidebar: {
		label: 'Barre latérale',
		description: 'Discussions, dossiers et pages de ce profil.',
		profiles: 'Profils',
		newProfile: 'Nouveau profil',
		openSidebar: 'Ouvrir la barre latérale',
		closeSidebar: 'Fermer la barre latérale',
		searchChats: 'Rechercher des discussions',
		images: 'Images',
		automations: 'Automatisations',
		memory: 'Mémoire',
		skills: 'Compétences',
		people: 'Personnes et profil',
		folders: 'Dossiers',
		newFolder: 'Nouveau dossier',
		chats: 'Discussions',
		working: ', en cours',
		showChats: (folder: string) => `Afficher les discussions de ${folder}`,
		hideChats: (folder: string) => `Masquer les discussions de ${folder}`,
		emptyFolder: 'Glissez des discussions ici, ou commencez-en une sur la page du dossier.',
		dropToTakeOut: 'Déposez ici pour sortir la discussion de son dossier.',
		noChats: 'Vos discussions apparaîtront ici.',
		searchDescription: 'Trouver une discussion par son titre',
		searchPlaceholder: 'Rechercher des discussions…',
		noResults: 'Aucune discussion trouvée.',
		deleteChatBody: 'Cela supprime {name} pour tout le monde dans {profile}.'
	},

	notifications: {
		title: 'Notifications',
		unseen: (n: number) =>
			`Notifications, ${p(n, { one: `${n} nouvelle`, other: `${n} nouvelles` })}`,
		clearAll: 'Tout effacer',
		new: 'Nouveau',
		showLess: 'Afficher moins',
		showAll: 'Tout afficher',
		dismiss: 'Ignorer',
		openChat: 'Ouvrir la discussion',
		continueInChat: 'Continuer dans une discussion',
		empty:
			'Rien pour l’instant. Demandez à nolune un rappel ou une vérification quotidienne, et ce qu’il trouve apparaîtra ici.'
	},

	chat: {
		options: 'Options de la discussion',
		placeholder: 'Demandez à nolune',
		placeholderRunning: 'Ajoutez quelque chose pendant que nolune travaille…',
		disclaimer: 'nolune peut se tromper, et il peut modifier des fichiers sur cet ordinateur.',
		reconnecting: 'Reconnexion…',
		empty: 'Demandez quelque chose pour commencer.',
		readsAfterStep: 'nolune lira ceci après son étape en cours',
		working: 'nolune travaille',
		thinking: 'Réflexion',
		writing: 'Rédaction',
		cutOff: 'La réponse a été coupée parce qu’elle devenait trop longue.',
		refused: 'nolune a refusé de poursuivre cette demande.',
		automation: (title: string) => `Automatisation · ${title}`,
		fromNolune: (to: string) => `De nolune, pour ${to}`,
		finishedInBackground: (title: string) => `Terminé en arrière-plan · ${title}`,
		inBackground: 'Travail en arrière-plan',
		aCommand: 'Une commande',
		subagent: (name: string) => `Sous-agent ${name}`,
		stopping: 'en cours d’arrêt',
		errorTitle: 'Un problème est survenu pendant que nolune répondait.',
		unanswered: 'nolune n’a pas encore répondu.',
		scrollToBottom: 'Aller au message le plus récent',
		subagentBanner:
			'Sous-agent {name} : nolune l’a lancé depuis {parent}, et il en rend compte là-bas.',
		subagentOnly: (name: string) => `Seul l’agent qui a lancé ${name} écrit ici.`,
		subagentRetry: 'Seul l’agent qui l’a lancé fait tourner un sous-agent.',
		hiddenBanner:
			'Une exécution en arrière-plan d’une automatisation. Envoyez un message pour la garder dans vos discussions.',
		aChat: 'une discussion',
		deleteTitle: 'Supprimer la discussion ?',
		deleteBody: 'Cela supprime {name} pour tout le monde dans le profil.',
		renameTitle: 'Renommer la discussion',
		chatName: 'Nom de la discussion',
		couldNotRename: 'Impossible de renommer la discussion.',
		switchTitle: (model: string) => `Passer à ${model} ?`,
		anotherModel: 'un autre modèle',
		effortTitle: (level: string) => `Passer la réflexion à « ${level} » ?`,
		switchCache: (model: boolean, missTokens: string | null) =>
			`nolune garde cette discussion en cache, pour que chaque réponse ne paie que ce qui est nouveau. ${model ? 'Un autre modèle' : 'Un autre niveau de réflexion'} ne peut pas s’en servir : la prochaine réponse relit toute la discussion, ce qui prend plus de temps et coûte plus cher${missTokens ? ` (un défaut du cache du prompt d’environ ${missTokens} tokens)` : ''}.`,
		switchFiles:
			'Certaines images et certains PDF de cette discussion ne passent pas à un autre fournisseur : le nouveau modèle reçoit l’emplacement de leurs fichiers et peut les regarder à nouveau.',
		switch: 'Changer',
		change: 'Modifier',
		models: (models: string[]) => models.join(', puis '),
		usage: 'Consommation',
		tokensInOut: (input: string, output: string) =>
			`${input} tokens en entrée, ${output} en sortie`,
		cache: 'Cache',
		lastReply: 'Dernière réponse',
		wholeConversation: 'Toute la discussion',
		cacheSummary: (label: string, rate: string, read: string, written: string, uncached: string) =>
			`${label} : ${rate} en cache (${read} lus, ${written} écrits, ${uncached} hors cache)`,
		cacheMiss: (tokens: string) => `Défaut de cache · ${tokens} tokens traités à nouveau`,
		cacheExpired: (ttl: '5m' | '1h') =>
			`${ttl === '5m' ? 'Plus de 5 minutes se sont écoulées' : 'Plus d’une heure s’est écoulée'} depuis l’étape précédente : la discussion en cache a expiré et a été traitée à nouveau (plus lent et plus coûteux).`,
		cacheBroken:
			'Du contexte qui aurait dû venir du cache a été traité à nouveau (plus lent et plus coûteux). Cela arrive une fois quand on change de modèle ou de niveau de réflexion, qu’on déplace la discussion dans un autre dossier ou qu’on modifie son dossier.',
		contextChip: (used: string, window: string, rate: string) =>
			`${used} / ${window} · ${rate} en cache`,
		contextUsed: (used: string, window: string) =>
			`Contexte utilisé par la dernière réponse : ${used} sur ${window}`,
		contextMenu: (used: string, window: string) => `Contexte ${used} / ${window}`,
		cachedOverall: (rate: string) => `${rate} en cache au total`
	},

	steps: {
		thinking: 'Réflexion',
		thinkingDots: 'Réflexion…',
		running: (command: string) => `Exécution de ${command}`,
		runningACommand: 'Exécution d’une commande',
		ranACommand: 'A exécuté une commande',
		gettingReady: 'Préparation…',
		preparing: 'Préparation d’une commande…',
		stopped: 'Arrêté',
		thoughtFor: (duration: string) => `A réfléchi ${duration}`,
		thoughtForAMoment: 'A réfléchi un instant',
		workedFor: (duration: string) => `A travaillé ${duration}`,
		workedForAMoment: 'A travaillé un instant',
		ranCommands: (n: number) =>
			p(n, { one: `A exécuté ${n} commande`, other: `A exécuté ${n} commandes` }),
		failedCount: (n: number) => `${n} en échec`,
		blockedCount: (n: number) => p(n, { one: `${n} bloquée`, other: `${n} bloquées` }),
		failed: 'échec',
		didntWork: 'n’a pas fonctionné',
		statusStopped: 'arrêtée',
		statusNotRun: 'non exécutée',
		statusBlocked: 'bloquée',
		command: 'Commande',
		theCommand: 'La commande exécutée par nolune',
		inFolder: (cwd: string) => `dans ${cwd}`,
		noOutputYet: 'Pas encore de sortie…',
		noOutput: '(aucune sortie)'
	},

	composer: {
		placeholder: 'Demandez ce que vous voulez',
		message: 'Message',
		attach: 'Joindre des fichiers',
		send: 'Envoyer'
	},

	model: {
		model: 'Modèle',
		reasoning: 'Réflexion',
		efforts: {
			low: { label: 'Faible', hint: 'Les réponses les plus rapides' },
			medium: { label: 'Moyenne', hint: 'Convient à la plupart des choses' },
			high: { label: 'Élevée', hint: 'Réfléchit plus longtemps aux tâches difficiles' },
			xhigh: { label: 'Très élevée', hint: 'Prend son temps' },
			max: { label: 'Maximale', hint: 'La plus lente, pour les problèmes les plus durs' }
		}
	},

	commandMode: {
		title: 'Commandes dans cette discussion',
		auto: 'Automatique',
		autoHint: 'Un modèle vérifie chaque commande avant qu’elle ne s’exécute.',
		unrestricted: 'Sans restriction',
		unrestrictedHint: 'Les commandes s’exécutent sans vérification. Déconseillé.',
		adminsOnly: 'Seul un administrateur peut désactiver la vérification pour une discussion.',
		labelAuto: 'Commandes : mode automatique',
		labelUnrestricted: 'Commandes : sans restriction'
	},

	newChat: {
		greeting: (name: string) => `Comment puis-je vous aider, ${name} ?`,
		greetingNoName: 'Comment puis-je vous aider ?',
		noModels:
			'Aucun modèle n’est encore configuré. Un administrateur peut en ajouter un sur la page « Modèles et clés » ou avec {command}.',
		couldNotStart: 'Impossible de commencer la discussion.',
		suggestions: {
			reminder: { label: 'Créer un rappel', text: 'Rappelle-moi demain à 9 h de ' },
			weather: {
				label: 'Météo du jour',
				text: 'Chaque jour de semaine à 7 h 30, regarde la météo et dis-nous s’il faut prendre un parapluie.'
			},
			file: { label: 'Trouver un fichier', text: 'Trouve sur cet ordinateur le fichier nommé ' },
			space: {
				label: 'Espace disponible',
				text: 'Combien d’espace disque reste-t-il sur cet ordinateur ?'
			}
		},
		pickModel: 'Choisissez un modèle.',
		pickEffort: 'Choisissez un niveau de réflexion.',
		folderGone: 'Ce dossier a été supprimé. Choisissez-en un autre.'
	},

	folders: {
		inFolder: (name: string) => `Dans le dossier ${name}`,
		startInFolder: 'Commencer dans un dossier',
		startOutside: 'Commencer hors du dossier',
		noFolder: 'Aucun dossier',
		newFolderDots: 'Nouveau dossier…',
		moveTo: 'Déplacer vers un dossier',
		moveTechnical:
			'Le déplacement reconstruit le prompt système : la prochaine réponse relit donc une fois toute la discussion (un défaut du cache du prompt).',
		move: 'Après un déplacement, la prochaine réponse prend un peu plus de temps.',
		newTitle: 'Nouveau dossier',
		newDescription:
			'Regroupez les discussions liées. Chaque discussion d’un dossier reçoit ses instructions et ses fichiers.',
		namePlaceholder: 'Voyage au Japon',
		name: 'Nom du dossier',
		create: 'Créer le dossier',
		renameTitle: 'Renommer le dossier',
		couldNotRename: 'Impossible de renommer le dossier.',
		deleteTitle: 'Supprimer le dossier ?',
		deleteBody:
			'{name} est supprimé pour tout le monde dans le profil. Ses discussions reviennent dans votre liste, sans ses instructions ni ses fichiers. Les fichiers sont déplacés dans ~/.nolune/trash.',
		options: 'Options du dossier',
		newChatIn: (name: string) => `Nouvelle discussion dans ${name}`,
		instructions: 'Instructions',
		instructionsHint: 'Ce que nolune doit savoir ou faire dans chaque discussion ici.',
		instructionsPlaceholder:
			'Nous préparons deux semaines au Japon en avril avec les enfants (7 et 10 ans). Des programmes tranquilles et un budget sous 600 000 ¥.',
		changesTechnical:
			'Les discussions du dossier reçoivent les changements à leur prochain message, qui relit une fois la discussion (un défaut du cache du prompt).',
		changes: 'Les discussions du dossier reçoivent les changements à leur prochain message.',
		files: 'Fichiers',
		addFiles: 'Ajouter des fichiers',
		filesHint:
			'Photos, documents, tout ce que vous voulez. nolune sait où ils sont enregistrés et les ouvre quand ils comptent.',
		noFiles: 'Aucun fichier pour l’instant',
		savedIn: (dir: string) => `Enregistrés dans ${dir}`,
		couldNotAdd: 'Impossible d’ajouter les fichiers.',
		couldNotAddOffline: 'Impossible d’ajouter les fichiers. Vérifiez la connexion et réessayez.',
		chats: 'Discussions',
		noChats:
			'Les discussions commencées ici apparaissent ici. Vous pouvez aussi glisser des discussions sur le dossier dans la barre latérale.'
	},

	attachments: {
		open: (name: string) => `Ouvrir ${name}`,
		download: 'Télécharger',
		onlyPath: (note: string) => `nolune n’a reçu que l’emplacement du fichier : ${note}`
	},

	markdown: {
		text: 'texte',
		picture: 'Image',
		notAvailable: 'Indisponible'
	},

	images: {
		title: 'Images',
		shapes: { square: 'Carré', portrait: 'Portrait', landscape: 'Paysage', auto: 'Auto' },
		shape: 'Format',
		groups: 'Groupes de modèles',
		noTemplates: 'Aucun modèle pour l’instant.',
		cantMakeYet: 'nolune ne peut pas encore créer d’images.',
		needsKeyAdmin: 'Il faut une clé d’API {provider}. {link}.',
		addKeyLink: 'Ajoutez-la dans « Modèles et clés »',
		needsKey: (provider: string) =>
			`Il faut une clé d’API ${provider}. Demandez à un administrateur de l’ajouter.`,
		adminSetsUp: 'Un administrateur le configure sur l’ordinateur où tourne nolune.',
		describe: 'Décrivez une image',
		describeHint:
			'nolune crée l’image dans une nouvelle discussion, où vous pouvez demander des changements.',
		onlyPictures: 'Seules des images peuvent être utilisées ici.',
		atMostPictures: (n: number) => `${n} images au maximum.`,
		changePicture: (label: string | null) => (label ? `Changer : ${label}` : 'Changer l’image'),
		addPicture: 'Ajouter une image',
		draw: 'Dessiner',
		choosePhotoOfDrawing: 'Choisir une photo d’un dessin',
		usePhotoOfDrawing: 'Utiliser une photo d’un dessin',
		startDrawing: 'Commencer à dessiner',
		takePhoto: 'Prendre une photo',
		choosePhoto: 'Choisir une photo',
		tryIt: 'Essayer',
		yourOwn: (label: string) => `votre choix : ${label.toLowerCase()}`,
		pickFromList: (label: string) => `Choisir dans la liste : ${label.toLowerCase()}`,
		custom: 'Personnalisé…',
		addAnything: 'Ajoutez autre chose…',
		uploadingPicture: 'Envoi de l’image…',
		addDrawingFirst: 'Ajoutez d’abord un dessin.',
		addPhotoFirst: 'Ajoutez d’abord une photo.',
		generate: 'Générer',
		noModel: 'Aucun modèle de discussion n’est encore configuré.',
		notPicture: (name: string) => `${name} n’est pas une image.`,
		templateGone: 'Ce modèle n’existe plus.',
		describeFirst: 'Décrivez d’abord l’image.'
	},

	drawing: {
		title: 'Dessin',
		tool: 'Outil',
		pen: 'Crayon',
		eraser: 'Gomme',
		undo: 'Annuler',
		penSize: 'Épaisseur du crayon',
		color: (color: string) => `Couleur ${color}`,
		use: 'Utiliser ce dessin'
	},

	emoji: {
		value: (label: string, value: string) => `${label} : ${value || 'aucun'}`,
		pickUpTo: (n: number) => `Choisissez-en jusqu’à ${n}`,
		removeLast: 'Retirer le dernier emoji'
	},

	memory: {
		title: 'Mémoire',
		intro: (profile: string) =>
			`Ce dont nolune se souvient pour ${profile}, partagé par tous ses membres. Chaque discussion commence avec la note épinglée L’essentiel ; nolune lit les autres quand une discussion en a besoin et enregistre ce qu’il apprend en chemin. Pour ajouter quelque chose, dites-le-lui simplement dans une discussion, par exemple « Retiens qu’Anna est allergique aux fruits à coque ».`,
		total: (count: number, topics: number) =>
			`${count === 1 ? 'souvenir' : 'souvenirs'} dans ${p(topics, { one: `${topics} sujet`, other: `${topics} sujets` })}`,
		newThisWeek: (n: number) =>
			p(n, { one: `${n} nouveau cette semaine`, other: `${n} nouveaux cette semaine` }),
		updated: (ago: string) => `mis à jour ${ago}`,
		memories,
		pinned: 'épinglée, dans chaque nouvelle discussion',
		edit: (topic: string) => `Modifier ${topic}`,
		forget: (topic: string) => `Oublier ${topic}`,
		note: (topic: string) => `Note ${topic}`,
		corePlaceholder:
			'Par exemple :\n- Anna et Ben sont les parents, Mia a 7 ans\n- À la maison, on parle russe\n- Mia est allergique aux fruits à coque',
		coreEmpty:
			'Rien pour l’instant. Écrivez ici ce que nolune doit garder en tête dans chaque discussion : qui est qui dans la famille, les langues que vous parlez, les allergies. nolune la complète aussi.',
		forgetTitle: (topic: string) => `Oublier « ${topic} » ?`,
		forgetBody:
			'nolune oublie tout ce qu’il y a dans {path} pour tout le monde dans {profile}. Les discussions qui l’ont déjà lu gardent ce qu’elles ont lu.',
		forgetButton: 'Oublier',
		empty: 'La note est vide. Pour la supprimer, utilisez Oublier.',
		conflict: (problem: string) =>
			`${problem} Enregistrez à nouveau pour garder votre version, ou annulez pour voir celle de nolune.`,
		saved: 'Enregistré. Les nouvelles discussions verront le changement.',
		forgot: (path: string) => `nolune a oublié tout ce qu’il y avait dans ${path}.`,
		learn: 'Apprendre des discussions',
		learnHint:
			'Quand une discussion est calme depuis quelques minutes, nolune la relit et garde ce qui mérite d’être retenu. C’est chaque fois une courte requête de plus au modèle de la discussion. Désactivé, nolune ne garde que ce à quoi il pense pendant la discussion.',
		learned: (ago: string) => `appris ${ago}`,
		learnedAWhileAgo: 'appris il y a un moment',
		topicLabel: (topic: string, n: number) => `${topic} : ${memories(n)}. Afficher la note.`,
		nothingInIt: 'Encore vide',
		showFewer: 'Afficher moins',
		showAllTopics: (n: number) => `Afficher les ${n} sujets`,
		older: 'Plus ancien',
		newer: 'Plus récent',
		nothingYet:
			'Rien de mémorisé pour l’instant. Chaque chose que nolune apprend devient ici un point.',
		categories: {
			core: 'L’essentiel',
			people: 'Personnes',
			home: 'Maison',
			health: 'Santé',
			plans: 'Agenda',
			routines: 'Habitudes',
			pets: 'Animaux',
			places: 'Lieux',
			projects: 'Projets',
			other: 'Autre',
			unsorted: 'Sans catégorie'
		},
		unsortedHint:
			'D’avant que la mémoire ait des catégories : nolune la lit, mais n’y ajoute rien. Déplacez-la dans une catégorie pour qu’elle continue de s’enrichir.',
		memberNote: (name: string) => `membre : ${name}`,
		move: {
			button: (topic: string) => `Déplacer ${topic}`,
			title: (topic: string) => `Déplacer « ${topic} »`,
			body: 'Choisissez où elle va. Si cette note existe déjà, les deux n’en font plus qu’une.',
			to: 'Déplacer vers',
			choose: 'Choisir où',
			categories: 'Catégories',
			newPerson: 'Quelqu’un de nouveau…',
			newProject: 'Un nouveau projet…',
			personName: 'Son nom',
			projectName: 'Nom du projet',
			moveHint: (target: string) => `Elle devient ${target}.`,
			mergeHint: (from: string, into: string) =>
				`Ce que dit « ${from} » est ajouté à « ${into} », sans répétitions, et « ${from} » disparaît. Utile pour deux notes sur la même personne.`,
			move: 'Déplacer',
			merge: 'Fusionner',
			moved: (from: string, to: string) => `${from} déplacée vers ${to}.`,
			merged: (from: string, into: string) => `${from} fusionnée avec ${into}.`
		},
		/** What nolune saved from a chat by itself: in that chat, and at the top of this page. */
		changes: {
			saved: (n: number) => `${memories(n)} enregistré${n === 1 ? '' : 's'}`,
			added: 'Nouveau',
			changed: 'Modifié',
			before: (text: string) => `avant : ${text}`,
			openNote: (topic: string) => `Ouvrir ${topic} dans Mémoire`,
			undo: 'Annuler',
			undone: 'Annulé',
			undoneBy: (name: string) => `Annulé par ${name}`,
			changedSince:
				'Cela a changé depuis, donc impossible d’annuler ici. Modifiez la note sur la page Mémoire.',
			alreadyUndone: 'C’est déjà annulé.',
			recent: 'Enregistré depuis les discussions',
			recentHint:
				'Ce que nolune a noté de lui-même quand les discussions se sont calmées, ces deux dernières semaines.',
			fromChat: 'de {chat}',
			deletedChat: 'd’une discussion supprimée',
			showAll: (n: number) => `Tout afficher (${n})`,
			untitled: 'une discussion'
		}
	},

	automations: {
		title: 'Automatisations',
		intro:
			'Ce que nolune fait de lui-même : rappels, vérifications régulières et réponses à d’autres applis. Ce qu’il trouve apparaît sous la cloche. Pour en ajouter ou en modifier une, demandez-le simplement à nolune dans une discussion.',
		today: 'Aujourd’hui',
		previousMonth: 'Mois précédent',
		nextMonth: 'Mois suivant',
		dayLabel: (day: string, runs: number) =>
			`${day} : ${runs ? p(runs, { one: `${runs} exécution`, other: `${runs} exécutions` }) : 'rien'}`,
		nothingRan: 'Rien ne s’est exécuté ce jour-là.',
		nothingRuns: 'Rien n’est prévu ce jour-là.',
		timeZone: (zone: string) => `Les heures sont dans le fuseau horaire de l’ordinateur (${zone}).`,
		all: 'Toutes les automatisations',
		next: (when: string) => `prochaine ${when}`,
		states: { paused: 'en pause', done: 'terminée' },
		runNow: 'Exécuter maintenant',
		pause: 'Mettre en pause',
		resume: 'Reprendre',
		description: 'Description',
		descriptionPlaceholder: 'Ce qu’elle fait, en une phrase',
		instructions: 'Instructions pour nolune',
		instructionsTechnical: (model: string, effort: string) =>
			`Instructions pour nolune (${model}, réflexion ${effort})`,
		script: 'Script : tourne sans le modèle et appelle {command} quand nolune est nécessaire',
		webhook:
			'URL du webhook. Gardez-la secrète : toute personne qui l’a peut lancer une exécution. Envoyez-lui du JSON en POST.',
		confirmDelete: (name: string) => `Supprimer « ${name} » ?`,
		recentRuns: (n: number) => `Exécutions récentes (${n})`,
		noRuns: 'Aucune exécution pour l’instant',
		statuses: {
			pending: 'en attente',
			running: 'en cours',
			ok: 'terminé',
			notified: 'notifié',
			silent: 'rien à signaler',
			stopped: 'arrêté',
			failed: 'échec'
		},
		sources: {
			cron: 'planifiée',
			once: 'planifiée',
			webhook: 'webhook',
			wake: 'réveillée par un script',
			manual: 'lancée à la main'
		},
		scriptRun: (status: string) => `script : ${status}`,
		view: 'voir',
		output: 'sortie',
		empty:
			'Aucune automatisation pour l’instant. Demandez à nolune dans une discussion, par exemple :',
		examples: [
			'« Chaque jour de semaine à 7 h 30, regarde la météo et dis-nous s’il faut prendre un parapluie. »',
			'« Rappelle à Anna demain à 17 h d’aller chercher le colis. »',
			'« Vérifie mes e-mails toutes les 10 minutes et préviens-moi quand l’école écrit. »'
		],
		defaultModel: 'le modèle par défaut',
		saved: (name: string) => `« ${name} » enregistrée.`,
		started: (name: string) => `« ${name} » lancée. Sa réponse apparaîtra sous la cloche.`,
		startedScript: (name: string) => `Le script de « ${name} » a été lancé.`,
		paused: (name: string) => `« ${name} » est en pause.`,
		resumed: (name: string) => `« ${name} » est de nouveau active.`,
		deleted: (name: string) => `« ${name} » supprimée.`,
		describe,
		scheduleAfterName: lowerFirst,
		customSchedule: 'Selon un calendrier personnalisé',
		once: 'Une fois',
		onceAt: (when: string) => `Une fois, ${when}`,
		onWebhook: 'Quand une autre appli appelle son lien',
		dayAt: (day: string, time: string) => `${day} à ${time}`
	},

	skills: {
		title: 'Compétences',
		intro:
			'Les compétences sont des instructions que nolune suit pour des tâches précises. Il voit le nom et la description de chaque compétence activée, et lit le reste quand une tâche le demande. Désactiver celles dont ce profil n’a pas besoin aide nolune à rester concentré. Les changements s’appliquent aux nouvelles discussions.',
		summary: (on: number, total: number, tokens: string) =>
			`${on} sur ${total} activées · environ ${tokens} tokens au début de chaque nouvelle discussion`,
		madeFor: (profile: string) => `Faites pour ${profile}`,
		shared: 'Partagées par tous les profils',
		builtIn: 'Intégrées à nolune',
		allOff: 'Tout désactiver',
		allOn: 'Tout activer',
		tokens: (n: string) => `~${n} tokens`,
		use: (name: string) => `Utiliser ${name}`,
		empty:
			'Aucune compétence pour l’instant. Quand nolune trouve comment faire quelque chose, il peut l’enregistrer comme compétence pour la prochaine fois.',
		gone: 'Cette compétence n’existe plus.'
	},

	profile: {
		title: 'Personnes et profil',
		name: 'Nom',
		profileName: 'Nom du profil',
		folder: (slug: string) => `Dossier : ~/.nolune/profiles/${slug} (ne change pas)`,
		avatar: 'Avatar',
		avatarHint:
			'L’apparence de nolune dans les discussions de ce profil. Tout le monde ici voit le même, et nolune peut le changer si on le lui demande.',
		soul: 'Âme',
		soulHint: (profile: string) =>
			`Qui est nolune pour ${profile} : son caractère, ce qui compte pour lui, sa façon de parler. Chaque discussion commence avec elle, et nolune la modifie aussi quand vous lui demandez d’être différent.`,
		soulPlaceholder:
			'Tu es chaleureux et un peu espiègle, et tu réponds brièvement. Avec les enfants, tu expliques simplement, sans jamais les prendre de haut. Quand tu ne sais pas, tu le dis.',
		changesTechnical:
			'Les discussions reçoivent les changements à leur prochain message, qui relit une fois la discussion (un défaut du cache du prompt).',
		changes: 'Les discussions reçoivent les changements à leur prochain message.',
		members: 'Membres',
		membersHint:
			'Tout le monde ici voit les mêmes discussions, y écrit, et peut demander n’importe quoi à nolune.',
		personToAdd: 'Personne à ajouter',
		chooseSomeone: 'Choisissez quelqu’un à ajouter',
		everyoneIsMember: 'Tout le monde est déjà membre.',
		deleteTitle: 'Supprimer le profil',
		deleteHint:
			'Supprime toutes ses discussions pour tout le monde. Le dossier est déplacé dans ~/.nolune/trash.',
		deleteButton: 'Supprimer ce profil',
		deleteConfirm: (name: string) => `Supprimer « ${name} » ?`,
		deleteBody:
			'Toutes ses discussions sont supprimées pour tout le monde. Le dossier est déplacé dans ~/.nolune/trash.',
		renamed: 'Renommé.',
		soulSaved: 'Âme enregistrée.',
		soulRemoved: 'Âme supprimée.',
		added: (name: string) => `${name} a été ajouté.`,
		removed: 'Retiré.',
		memberNote: (note: string) => `Note : ${note}`,
		noteStarts: (note: string) => `Note : ${note}, créée dès qu’il y a quelque chose à noter`,
		mayHaveNote: 'nolune a peut-être déjà une note sur cette personne',
		chooseNote: 'Choisir la note',
		changeNote: 'Changer de note',
		linked: (name: string, note: string) => `La note de ${name} est maintenant ${note}.`,
		chooser: {
			addTitle: (name: string) => `nolune connaît-il déjà ${name} ?`,
			linkTitle: (name: string) => `Quelle note parle de ${name} ?`,
			body: (name: string) =>
				`La mémoire a des notes qui pourraient parler de ${name}. Choisissez la sienne : nolune saura que ce qu’elle dit concerne cette personne et y ajoutera ce qu’elle raconte d’elle-même.`,
			linkBody: (name: string) =>
				`Choisissez la note sur ${name} : nolune y ajoutera ce que cette personne raconte d’elle-même.`,
			current: 'actuelle',
			alsoCalled: (names: string) => `Autres noms : ${names}`,
			more: (n: number) => `et ${n} de plus`,
			newNote: 'Quelqu’un d’autre : commencer une nouvelle note',
			privacy: (name: string) =>
				`${name} pourra lire toute la mémoire de ce profil, y compris cette note. Vérifiez qu’elle ne contient rien à garder secret pour cette personne, comme une surprise.`,
			add: (name: string) => `Ajouter ${name}`,
			link: 'Associer'
		}
	},

	avatars: {
		probe: 'Sonde',
		campfire: 'Feu de camp',
		lantern: 'Lanterne',
		planet: 'Planète',
		quantum: 'Quantique',
		comet: 'Comète',
		moon: 'Lune',
		satellite: 'Satellite'
	},

	profiles: {
		title: 'Profils',
		intro:
			'Chaque profil a ses propres discussions, sa mémoire et ses compétences, partagées par ses membres.',
		none: 'Vous n’êtes encore dans aucun profil. Créez-en un ci-dessous, ou demandez à quelqu’un de vous ajouter au sien.',
		new: 'Nouveau profil',
		namePlaceholder: 'p. ex. Famille, Mamie, Devoirs',
		name: 'Nom du profil',
		needsName: 'Donnez un nom au profil.'
	},

	/** L’accueil d’un nouveau profil. Ici nolune parle en son nom : « je », c’est l’assistant. */
	welcome: {
		title: 'Faisons de cet endroit le vôtre.',
		subtitle: 'Quelques questions rapides, et vous vous sentirez chez vous.',
		go: 'C’est parti',
		takesAMinute: 'Environ une minute',
		skipIntro: 'Passer l’intro',
		skipHint: 'Cliquez n’importe où pour passer',
		back: 'Retour',
		soundsOn: 'Activer les sons',
		soundsOff: 'Couper les sons',
		progress: (n: number, total: number) => `Étape ${n} sur ${total}`,
		model: {
			title: 'Avec quoi dois-je réfléchir ?',
			subtitle:
				'Choisissez ce qui me fait tourner. Vous pourrez en ajouter d’autres dans Modèles et clés.',
			askAdmin:
				'Il me faut un modèle pour discuter. Demandez à la personne qui a installé nolune d’en ajouter un dans Modèles et clés ; tout le reste fonctionne déjà.',
			choices: {
				'claude-plan': {
					title: 'Abonnement Claude',
					about: 'Un abonnement Pro ou Max, via Claude Code sur cet ordinateur'
				},
				'chatgpt-plan': {
					title: 'Abonnement ChatGPT',
					about: 'Un abonnement Plus ou Pro, connecté avec ChatGPT'
				},
				anthropic: { title: 'Clé d’API Anthropic', about: 'Modèles Claude, payés à l’usage' },
				openai: { title: 'Clé d’API OpenAI', about: 'Modèles GPT, payés à l’usage' },
				openrouter: {
					title: 'Clé d’API OpenRouter',
					about: 'Claude, GPT, Gemini et d’autres avec une seule clé'
				}
			},
			checkingPlan: 'Vérification de la connexion…',
			pasteKey: (label: string) => `Collez votre clé ${label}`,
			checkKey: 'Vérifier la clé',
			keyWorks: 'La clé fonctionne.',
			finishOnAdmin: 'Connectez-vous dans {link}, puis vérifiez à nouveau.',
			checkAgain: 'Vérifier à nouveau',
			pickModel: 'Avec quel modèle les nouvelles discussions doivent-elles commencer ?',
			asking: 'Récupération des modèles…',
			modelId: 'Identifiant du modèle'
		},
		avatar: {
			title: 'Qui dois-je être ?',
			subtitle: (profile: string) => `Choisissez un visage pour ${profile}.`,
			thisOne: 'Celui-ci',
			hello: 'Faisons connaissance.'
		},
		memory: {
			title: 'On se connaît déjà ?',
			subtitle:
				'Si vous utilisez ChatGPT, Claude ou Gemini, il sait déjà des choses sur vous. Posez-lui la question avec ce prompt, puis collez sa réponse ici.',
			copyPrompt: 'Copier le prompt',
			prompt: 'Le prompt',
			open: (name: string) => `Ouvrir ${name}`,
			haveIt: 'Je l’ai',
			startFresh: 'Repartir de zéro',
			pasteTitle: 'Collez-la ici',
			pastePlaceholder: 'La réponse entière, bloc de code compris',
			sections: {
				instructions: 'Instructions',
				identity: 'Identité',
				career: 'Carrière',
				projects: 'Projets',
				preferences: 'Préférences',
				other: 'Autre'
			},
			remember: (n: number) => p(n, { one: `Retenir ${n} chose`, other: `Retenir ${n} choses` }),
			rememberThis: 'Retenir ceci',
			reading: 'Lecture…',
			saving: 'Enregistrement…',
			empty: 'Collez ce que l’autre assistant a répondu.',
			tooLong:
				'C’est trop long pour les souvenirs d’une seule personne. Collez seulement la réponse au prompt.',
			notAnExport:
				'Cela ne ressemble pas à la réponse au prompt. Collez la réponse entière, avec son bloc de code.',
			couldNotRead: (problem: string) => `Lecture impossible. ${problem}`,
			nothingFound:
				'Je n’ai rien trouvé sur vous là-dedans. Collez la réponse entière, avec son bloc de code.'
		},
		arrival: {
			thingsIKnow: (n: number) =>
				p(n, { one: 'chose que je sais de vous', other: 'choses que je sais de vous' })
		},
		fresh: {
			title: 'Un nouveau départ.',
			subtitle: "J'apprendrai à vous connaître au fil de nos échanges."
		}
	},

	login: {
		welcome: 'Bon retour',
		hint: 'Connectez-vous avec le compte qu’on vous a donné.',
		email: 'Adresse e-mail',
		password: 'Mot de passe',
		signingIn: 'Connexion…',
		forgot:
			'Mot de passe oublié ? Demandez à la personne qui a installé nolune d’exécuter {command}.',
		tooManyAttempts: 'Trop de tentatives. Patientez une minute et réessayez.',
		wrongPassword: 'Adresse e-mail ou mot de passe incorrect.'
	},

	admin: {
		title: 'Modèles et clés',
		keys: 'Clés d’API',
		keysHint:
			'Partagées par tous les profils et conservées dans le fichier de configuration de nolune sur cet ordinateur. Une nouvelle clé est vérifiée auprès de son fournisseur avant d’être enregistrée, puis utilisée tout de suite. Pour un serveur de modèles à vous, comme Ollama ou LM Studio, ajoutez un fournisseur personnalisé.',
		purposes: {
			anthropic: 'Fait tourner les discussions et les automatisations sur les modèles Claude.',
			openai:
				'Fait tourner les discussions et les automatisations sur les modèles OpenAI, et crée les images de la page Images et celles que dessine l’agent.',
			openrouter:
				'Fait tourner les discussions et les automatisations sur les modèles que propose OpenRouter (Claude, GPT, Gemini, DeepSeek et bien d’autres), avec une seule clé et ses crédits.'
		},
		withoutIt: {
			anthropic:
				'Les discussions et les automatisations sur les modèles Claude cessent de fonctionner jusqu’à l’ajout d’une nouvelle clé.',
			openai:
				'Les discussions et les automatisations sur les modèles OpenAI cessent de fonctionner, et nolune ne peut plus créer d’images, jusqu’à l’ajout d’une nouvelle clé.',
			openrouter:
				'Les discussions et les automatisations sur les modèles OpenRouter cessent de fonctionner jusqu’à l’ajout d’une nouvelle clé.'
		},
		savedInNolune: (hint: string | null) =>
			`Enregistrée dans nolune${hint ? `, se termine par ${hint}` : ''}`,
		fromEnv: (variable: string, hint: string | null) =>
			`Depuis la variable d’environnement ${variable}${hint ? `, se termine par ${hint}` : ''}`,
		notSet: 'Non définie',
		replace: 'Remplacer',
		pasteKey: (provider: string) => `Collez la clé d’API ${provider}`,
		keyLabel: (provider: string) => `Clé d’API ${provider}`,
		checkingKey: 'Vérification de la clé…',
		makeOneAt: 'Créez-en une sur {link}.',
		sameProject:
			'Utilisez une clé du même projet : les images et les PDF déjà envoyés dans les discussions y sont stockés, et ces discussions ne peuvent pas continuer sans eux.',
		sameWorkspace:
			'Utilisez une clé du même espace de travail : les images et les PDF déjà envoyés dans les discussions y sont stockés, et ces discussions ne peuvent pas continuer sans eux.',
		removeKeyTitle: (provider: string) => `Retirer la clé ${provider} ?`,
		useEnvInstead: (variable: string) =>
			`nolune utilisera à la place la clé de la variable d’environnement ${variable}.`,
		plan: 'Abonnement Claude',
		plans: 'Abonnements',
		plansHint:
			'Les discussions avec un préréglage d’abonnement utilisent l’abonnement de quelqu’un au lieu d’une clé d’API : l’abonnement Claude via Claude Code sur cet ordinateur, qui garde sa connexion ; l’abonnement ChatGPT via « Se connecter avec ChatGPT », dont nolune garde la connexion sur cet ordinateur. Les limites des abonnements supposent l’usage ordinaire d’une seule personne : gardez donc les automatisations chargées et les sous-agents sur un préréglage avec clé d’API.',
		claudePlanAbout: 'Pro ou Max, via Claude Code.',
		chatgptPlan: 'Abonnement ChatGPT',
		chatgptPlanAbout: 'Plus ou Pro, connecté avec ChatGPT.',
		chatgptSignIn: 'Continuer avec ChatGPT',
		chatgptContinueAs: (account: string) => `Continuer en tant que ${account}`,
		chatgptAnotherAccount: 'Utiliser un autre compte',
		chatgptSignInAgain: 'Se reconnecter',
		chatgptStarting: 'Démarrage…',
		signOut: 'Se déconnecter',
		chatgptOpen:
			'Ouvrez {link} et connectez-vous en autorisant nolune à utiliser votre abonnement ChatGPT.',
		chatgptSignInPage: 'la page de connexion de ChatGPT',
		chatgptHere:
			'Dans un navigateur sur cet ordinateur, c’est tout : cette page se met à jour dès que vous êtes connecté.',
		chatgptElsewhere:
			'Sur un autre appareil, la page vers laquelle ChatGPT vous renvoie ne se chargera pas. Copiez son adresse (elle commence par http://127.0.0.1) et collez-la ici :',
		chatgptFinish: 'Terminer',
		chatgptSignedIn:
			'Connecté. Les discussions avec un préréglage d’abonnement ChatGPT utilisent maintenant cet abonnement.',
		chatgptUsing:
			'Les discussions avec un préréglage d’abonnement ChatGPT utilisent cet abonnement. {link}',
		chatgptManageUsage: 'Gérer l’utilisation',
		chatgptNobody: 'Personne n’est connecté avec ChatGPT.',
		chatgptSignedOutLocally:
			'Déconnecté ici, mais OpenAI n’a pas pu être prévenu : par précaution, déconnectez nolune dans les réglages de ChatGPT.',
		chatgptSignOutTitle: 'Se déconnecter de ChatGPT ?',
		chatgptSignOutBody:
			'Les discussions avec un préréglage d’abonnement ChatGPT ne fonctionneront plus tant que personne ne se sera reconnecté.',
		signedOut: 'Déconnecté.',
		notAt: 'Introuvable à {path}, où {command} indique qu’il se trouve.',
		notInstalled: 'Pas installé sur cet ordinateur.',
		checkSignIn: 'Vérifier la connexion',
		install:
			'Dans un terminal sur cet ordinateur, exécutez {setup} : cela installe Claude Code avec l’installateur d’Anthropic et le connecte à votre compte Claude, en demandant d’abord. Ou installez-le vous-même, puis lancez {claude} et connectez-vous :',
		copyCommand: 'Copier la commande',
		signIn:
			'Pour vous connecter, exécutez {setup} dans un terminal sur cet ordinateur, ou lancez-y {claude} et utilisez {login} avec votre compte Claude.',
		models: 'Modèles',
		modelsHint:
			'Les préréglages sont partagés par tous les profils. Les nouvelles discussions commencent avec celui par défaut. Modifier ou retirer un préréglage ne change pas les discussions qui l’utilisent déjà.',
		presetDetails: (provider: string, model: string, context: string, overridden: boolean) =>
			`${provider} / ${model} · contexte ${context}${overridden ? ' (personnalisé)' : ''}`,
		makeDefault: 'Définir par défaut',
		editPreset: (name: string) => `Modifier ${name}`,
		noPresets: 'Aucun préréglage pour l’instant.',
		addModel: {
			title: 'Ajouter un modèle',
			provider: 'Fournisseur',
			model: 'Modèle',
			name: 'Nom',
			optional: '(facultatif)',
			namePlaceholder: 'L’identifiant du modèle et le fournisseur',
			contextWindow: 'Fenêtre de contexte',
			contextTokens: 'Fenêtre de contexte en tokens',
			auto: 'Auto',
			autoWith: (size: string) => `Auto · ${size}`,
			unknown: 'inconnue',
			custom: 'Personnalisée',
			tokensPlaceholder: 'Tokens, p. ex. 272k',
			tokens: (n: number) => `${n.toLocaleString('fr')} tokens.`,
			typeTokens: 'Saisissez un nombre de tokens, comme 272k ou 272000.',
			autoContext: {
				anthropic: 'Auto utilise la fenêtre qu’Anthropic indique pour le modèle.',
				openai:
					'OpenAI ne l’indique pas : Auto ne connaît que celle de ses modèles phares (1,05M depuis GPT-5.4).',
				openrouter:
					'Auto utilise la fenêtre qu’OpenRouter indique pour le modèle et son fournisseur principal.',
				'custom-openai':
					'Auto utilise la fenêtre que le serveur indique pour le modèle, s’il en indique une (vLLM le fait) ; sinon elle reste inconnue.',
				'custom-anthropic':
					'Auto utilise la fenêtre que le serveur indique pour le modèle, s’il en indique une (vLLM le fait) ; sinon elle reste inconnue.',
				'claude-plan':
					'Claude Code ne l’indique pas : Auto ne connaît que ses modèles à 1M de contexte.',
				'chatgpt-plan':
					'Auto utilise la fenêtre que ChatGPT indique pour le modèle, s’il en indique une ; sinon elle reste inconnue.'
			},
			onPlan: 'Utilise l’abonnement Pro ou Max auquel Claude Code est connecté.',
			noClaudeCode: 'Claude Code n’est pas encore installé : voir « Abonnement Claude » plus haut.',
			onChatGptPlan:
				'Utilise l’abonnement ChatGPT de la personne connectée avec ChatGPT, plus haut.',
			noChatGpt:
				'Personne n’est encore connecté avec ChatGPT : voir « Abonnement ChatGPT » plus haut.',
			onKey: (provider: string) => `Utilise la clé d’API ${provider}.`,
			noKey: (provider: string) =>
				`Pas encore de clé d’API ${provider} : ajoutez-en une dans « Clés d’API » plus haut.`,
			onCustom: (api: string, url: string) =>
				`Utilise ${url}, par son API ${api}. Le modèle doit savoir appeler des outils ; les images et les PDF lui parviennent sous forme de chemins.`,
			customGone: 'Ce fournisseur personnalisé a été retiré : choisissez-en un autre.',
			asking: (source: string) => `Demande de ses modèles à ${source}…`,
			listProblem: (problem: string) => `${problem} Vous pouvez quand même saisir un identifiant.`,
			couldNotList: (status: number) => `nolune n’a pas pu obtenir les modèles (${status}).`,
			unreachable: 'nolune est injoignable.',
			checkingClaude: 'Vérification de Claude Code…',
			checkingModel: 'Vérification du modèle…',
			saving: 'Enregistrement…',
			pick: 'Choisissez un modèle',
			search: 'Recherchez ou saisissez un identifiant de modèle',
			use: 'Utiliser {model}',
			typeId: 'Saisissez l’identifiant du modèle ci-dessus.',
			added: (name: string) => `${name} a été ajouté.`,
			saved: (name: string) =>
				`${name} a été enregistré. Les discussions qui l’utilisent déjà restent comme elles étaient.`,
			invalidContext: 'La fenêtre de contexte doit être un nombre de tokens, comme 272k ou 272000.'
		},
		savedWorks: 'Enregistrée. Elle fonctionne.',
		savedWarning: (warning: string) => `Enregistrée. ${warning}`,
		claudeNoAnswer: 'Claude Code n’a pas répondu.',
		removed: 'Retirée.',
		newDefault: (name: string) => `Les nouvelles discussions commencent maintenant avec ${name}.`,
		presetRemoved: 'Retiré. Les discussions existantes continuent de fonctionner.',
		/** Custom providers: model servers of the family's own, listed with the API keys. */
		customProviders: {
			add: 'Ajouter un fournisseur personnalisé',
			addTitle: 'Ajouter un fournisseur personnalisé',
			addHint:
				'Un serveur de modèles à vous, comme Ollama, LM Studio ou oMLX sur cet ordinateur, ou vLLM sur une machine avec des GPU. Il apparaît sous son nom dans Ajouter un modèle.',
			about: (api: string) => `Votre propre serveur de modèles, par son API ${api}.`,
			apis: { openai: 'OpenAI', anthropic: 'Anthropic' },
			change: 'Modifier',
			name: 'Nom',
			namePlaceholder: 'Ollama',
			api: 'API',
			apiHints: {
				openai:
					'La Responses API d’OpenAI : Ollama, LM Studio, vLLM, LiteLLM. La recherche dans la mémoire peut prendre ses embeddings.',
				anthropic: 'La Messages API d’Anthropic : Ollama, LM Studio, le serveur de llama.cpp, oMLX.'
			},
			address: 'Adresse',
			addressHint:
				'Ollama écoute sur http://localhost:11434 et LM Studio sur http://localhost:1234. nolune lui demande ses modèles pour le vérifier.',
			key: 'Clé',
			keyOptional: '(s’il en demande une)',
			keyKept: 'Enregistrée. Laissez vide pour la garder.',
			withKey: (hint: string | null) => (hint ? `clé se terminant par ${hint}` : 'avec une clé'),
			noKey: 'sans clé',
			checking: 'Vérification…',
			works: (n: number) =>
				`Enregistré. Il propose ${n} ${p(n, { one: 'modèle', other: 'modèles' })}.`,
			unchecked: (problem: string) => `Enregistré sans vérification : ${problem}`,
			needName: 'Donnez-lui un nom, comme Ollama ou GPU box.',
			needAddress: 'Indiquez son adresse, en commençant par http:// ou https://.',
			removeTitle: (name: string) => `Retirer ${name} ?`,
			removeBody:
				'Les discussions et automatisations sur ses modèles cessent de fonctionner jusqu’à ce qu’elles passent à un autre modèle.',
			usedBy: (names: string) => `Ces préréglages l’utilisent : ${names}.`
		},
		/** Memory search by meaning: where its embeddings come from. */
		embeddings: {
			title: 'Recherche dans la mémoire',
			hint: 'nolune retrouve aussi les faits de la mémoire par leur sens, pas seulement par leurs mots, et d’une langue à l’autre. Pour cela, chaque fait est envoyé une fois là où sont calculés les embeddings, et chaque message à son envoi ; un serveur sur cet ordinateur les garde ici. Commun à tous les profils.',
			name: 'Recherche par le sens',
			using: (source: string) => `Utilise ${source}.`,
			off: 'Désactivée : la mémoire est cherchée par mots seulement.',
			noKeys:
				'Pas encore de clé OpenAI ni OpenRouter : la mémoire est cherchée par mots seulement.',
			noKey: (provider: string) =>
				`Pas encore de clé ${provider} : la mémoire est cherchée par mots seulement. Ajoutez-en une dans Clés d’API, plus haut.`,
			customGone:
				'Ce fournisseur personnalisé a été retiré : la mémoire est cherchée par mots seulement.',
			change: 'Modifier',
			source: 'Embeddings de',
			modes: {
				auto: 'Auto',
				openai: 'OpenAI',
				openrouter: 'OpenRouter',
				off: 'Désactivée'
			},
			autoNote:
				'text-embedding-3-small d’OpenAI avec la clé OpenAI, sinon le même modèle via OpenRouter.',
			withKey: (provider: string) => `Avec la clé d’API ${provider}.`,
			customNote: (url: string) => `Un modèle qu’il propose à ${url}, par son API OpenAI.`,
			offNote: 'La mémoire est cherchée par mots seulement, et aucun fait n’est envoyé nulle part.',
			model: 'Modèle',
			customModel: 'Son nom là-bas, comme nomic-embed-text',
			checking: 'Vérification…',
			works: 'Enregistré. Ça fonctionne ; les faits sont traités en arrière-plan.',
			wordsOnly: 'Enregistré. La mémoire est cherchée par mots seulement.',
			noAnswer: (problem: string) => `Enregistré, mais pas de réponse : ${problem}`,
			needModel: 'Indiquez le modèle à utiliser.'
		},
		commands: {
			title: 'Commandes',
			hint: 'L’agent travaille en lançant des commandes sur cet ordinateur, avec l’accès de ce compte à ses fichiers et à ses apps. En mode automatique, un modèle examine chaque commande avant qu’elle ne s’exécute et arrête ce qui pourrait nuire sans que personne l’ait demandé. Toutes les discussions le suivent, sauf si quelqu’un en règle une autrement avec le bouclier de sa zone de message ; seuls les administrateurs peuvent y désactiver la vérification.',
			modes: {
				auto: 'Mode automatique',
				unrestricted: 'Sans restriction'
			},
			choices: {
				auto: 'Automatique (recommandé)',
				unrestricted: 'Sans restriction (déconseillé)'
			},
			checkedByChat: 'Chaque commande est vérifiée par le modèle de la discussion.',
			checkedBy: (preset: string) => `Chaque commande est vérifiée par ${preset}.`,
			presetGone:
				'Le modèle choisi pour les vérifications a été supprimé : chaque discussion vérifie donc ses commandes avec son propre modèle.',
			unrestrictedStatus: 'Les commandes s’exécutent sans vérification.',
			change: 'Modifier',
			mode: 'Mode',
			autoNote:
				'Les commandes qui ne font que regarder s’exécutent tout de suite. Les autres sont d’abord vérifiées ; une commande bloquée ne s’exécute pas, et nolune explique ce qu’il voulait faire pour que quelqu’un puisse donner son accord.',
			unrestrictedNote:
				'Chaque commande s’exécute telle que l’agent l’a écrite, sans rien pour arrêter une erreur, ni une page web ou un e-mail qui le pousse à faire quelque chose. Seulement pour qui surveille de près.',
			checker: 'Vérifiée par',
			chatModel: 'Le modèle de la discussion',
			checkerNote:
				'Un modèle rapide et compétent garde les discussions fluides : chaque commande qui fait plus que regarder lui coûte une courte requête.',
			saved: 'Enregistré. S’applique dès la prochaine commande.'
		}
	}
};
