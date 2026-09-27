import { isMonthSpan, type Schedule } from '@btw/core/schedule';
import { listOf, plural } from '../plural';
import type { Messages } from './en';

const p = plural('es');
const list = listOf('es');

/** By JavaScript's numbers: 0 is Sunday. */
const WEEKDAYS = ['domingo', 'lunes', 'martes', 'miércoles', 'jueves', 'viernes', 'sábado'];
/** "los lunes" */
const WEEKDAYS_PLURAL = [
	'domingos',
	'lunes',
	'martes',
	'miércoles',
	'jueves',
	'viernes',
	'sábados'
];
const MONTHS = [
	'',
	'enero',
	'febrero',
	'marzo',
	'abril',
	'mayo',
	'junio',
	'julio',
	'agosto',
	'septiembre',
	'octubre',
	'noviembre',
	'diciembre'
];

const capitalize = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);
const lowerFirst = (text: string) => text.charAt(0).toLowerCase() + text.slice(1);

/** "a las 07:30", "a la 01:15". */
function at(times: string[]): string {
	return times.length === 1 && times[0].startsWith('01:')
		? `a la ${times[0]}`
		: `a las ${list(times)}`;
}

/** Which days, as a sentence start ("De lunes a viernes") and as a tail. */
function days({ days, months }: Schedule): { every: string; on: string } {
	if (days.kind === 'monthDays') {
		const which =
			days.days.length === 1 ? `El día ${days.days[0]}` : `Los días ${list(days.days.map(String))}`;
		const of = months ? `de ${list(months.map((m) => MONTHS[m]))}` : 'de cada mes';
		return { every: `${which} ${of}`, on: lowerFirst(`${which} ${of}`) };
	}
	if (days.kind === 'date') {
		const date = `${days.day} de ${MONTHS[days.month]}`;
		return { every: `Cada año el ${date}`, on: `cada año el ${date}` };
	}
	let words: { every: string; on: string };
	if (days.kind === 'daily') words = { every: 'Todos los días', on: '' };
	else if (days.kind === 'weekdays') {
		words = { every: 'De lunes a viernes', on: 'de lunes a viernes' };
	} else if (days.kind === 'weekends') {
		words = { every: 'Los fines de semana', on: 'los fines de semana' };
	} else if (days.kind === 'weekdayRange') {
		const range = `de ${WEEKDAYS[days.from]} a ${WEEKDAYS[days.to]}`;
		words = { every: capitalize(range), on: range };
	} else {
		const which = `los ${list(days.days.map((d) => WEEKDAYS_PLURAL[d]))}`;
		words = { every: capitalize(which), on: which };
	}
	if (!months) return words;
	const inMonths = isMonthSpan(months)
		? `de ${MONTHS[months[0]]} a ${MONTHS[months[months.length - 1]]}`
		: `en ${list(months.map((m) => MONTHS[m]))}`;
	return {
		every: `${words.every} ${inMonths}`,
		on: `${words.on || 'todos los días'} ${inMonths}`
	};
}

function describe(schedule: Schedule): string {
	const when = days(schedule);
	const tail = when.on ? ` ${when.on}` : '';
	const { times } = schedule;
	switch (times.kind) {
		case 'minutes': {
			const every = times.step === 1 ? 'Cada minuto' : `Cada ${times.step} minutos`;
			if (!times.between) return every + tail;
			return `${every} de ${times.between.from} a ${times.between.to}${tail}`;
		}
		case 'hours': {
			const past = times.minute ? ` en el minuto ${times.minute}` : '';
			const every = times.step === 1 ? 'Cada hora' : `Cada ${times.step} horas`;
			return `${every}${past}${tail}`;
		}
		case 'hourRange':
			return `Cada hora de ${times.from} a ${times.to}${tail}`;
		case 'at':
			return `${when.every} ${at(times.times)}`;
	}
}

const memories = (n: number) => p(n, { one: `${n} recuerdo`, other: `${n} recuerdos` });

export const es: Messages = {
	common: {
		add: 'Añadir',
		cancel: 'Cancelar',
		close: 'Cerrar',
		continue: 'Continuar',
		copied: 'Copiado',
		copy: 'Copiar',
		create: 'Crear',
		default: 'Predeterminado',
		delete: 'Eliminar',
		done: 'Listo',
		edit: 'Editar',
		more: 'Más',
		newChat: 'Nuevo chat',
		remove: 'Quitar',
		rename: 'Cambiar nombre',
		save: 'Guardar',
		stop: 'Detener',
		tryAgain: 'Reintentar',
		checking: 'Comprobando…',
		starting: 'Empezando…',
		uploading: 'Subiendo…',
		removeFile: (name: string) => `Quitar ${name}`,
		characters: (count: number, max: number) => `${count} / ${max} caracteres`
	},

	errors: {
		requestFailed: (status: number) => `La solicitud falló (${status})`,
		uploadFailed: (status: number | null) =>
			`No se pudo subir (${status === null ? 'sin conexión' : status})`,
		couldNotSave: 'No se pudo guardar.',
		somethingWentWrong: 'Algo salió mal.',
		thatDidntWork: 'Eso no funcionó.',
		tooManyFiles: (max: number) => `Como máximo ${max} archivos por mensaje`,
		tooLarge: (mb: number) => `Más de ${mb} MB`,
		notSignedIn: 'No has iniciado sesión',
		adminsOnly: 'Solo para administradores',
		profileNotFound: 'No se encontró el perfil',
		conversationNotFound: 'No se encontró el chat',
		folderNotFound: 'No se encontró la carpeta',
		automationNotFound: 'No se encontró la automatización',
		messageEmpty: 'El mensaje está vacío',
		noFile: 'No hay archivo'
	},

	time: {
		justNow: 'ahora mismo',
		seconds: (s: number) => `${s} s`,
		minutesSeconds: (m: number, s: number) => `${m} min ${s} s`,
		hoursMinutes: (h: number, m: number) => `${h} h ${m} min`,
		minutesAgo: (n: number) => `hace ${n} min`,
		hoursAgo: (n: number) => `hace ${n} h`
	},

	units: { bytes: 'B', kilobytes: 'KB', megabytes: 'MB' },

	settings: {
		title: 'Ajustes',
		description: 'Cómo se ve btw en este dispositivo.',
		theme: 'Tema',
		system: 'Sistema',
		light: 'Claro',
		dark: 'Oscuro',
		language: 'Idioma',
		languageAuto: 'El del navegador',
		languageHint: 'Para menús y botones. btw responde en el idioma en que escribas.',
		technical: 'Mostrar detalles técnicos',
		technicalHint:
			'Muestra los comandos exactos que ejecuta btw, el uso de tokens y la caché del prompt.',
		expandSteps: 'Mostrar siempre los pasos',
		expandStepsHint:
			'Abre la lista de lo que hizo btw debajo de cada respuesta, en lugar de dejarla plegada.',
		logOut: 'Cerrar sesión',
		deviceOnly: 'Estos ajustes solo se guardan en este dispositivo.'
	},

	userMenu: {
		account: 'Cuenta',
		settings: 'Ajustes',
		allProfiles: 'Todos los perfiles',
		modelsAndKeys: 'Modelos y claves',
		logOut: 'Cerrar sesión'
	},

	header: {
		openMenu: 'Abrir menú'
	},

	sidebar: {
		label: 'Barra lateral',
		description: 'Chats, carpetas y páginas de este perfil.',
		profiles: 'Perfiles',
		newProfile: 'Nuevo perfil',
		openSidebar: 'Abrir la barra lateral',
		closeSidebar: 'Cerrar la barra lateral',
		searchChats: 'Buscar chats',
		images: 'Imágenes',
		automations: 'Automatizaciones',
		memory: 'Memoria',
		skills: 'Habilidades',
		people: 'Personas y perfil',
		folders: 'Carpetas',
		newFolder: 'Nueva carpeta',
		chats: 'Chats',
		working: ', trabajando',
		showChats: (folder: string) => `Mostrar los chats de ${folder}`,
		hideChats: (folder: string) => `Ocultar los chats de ${folder}`,
		emptyFolder: 'Arrastra chats aquí o empieza uno en la página de la carpeta.',
		dropToTakeOut: 'Suelta aquí para sacar el chat de su carpeta.',
		noChats: 'Tus chats aparecerán aquí.',
		searchDescription: 'Encuentra un chat por su título',
		searchPlaceholder: 'Buscar chats…',
		noResults: 'No se encontraron chats.',
		deleteChatBody: 'Esto elimina {name} para todos en {profile}.'
	},

	notifications: {
		title: 'Notificaciones',
		unseen: (n: number) => `Notificaciones, ${n} nuevas`,
		clearAll: 'Borrar todo',
		new: 'Nueva',
		showLess: 'Mostrar menos',
		showAll: 'Mostrar todo',
		dismiss: 'Descartar',
		openChat: 'Abrir chat',
		continueInChat: 'Seguir en un chat',
		empty:
			'Nada todavía. Pídele a btw un recordatorio o una comprobación diaria, y lo que encuentre aparecerá aquí.'
	},

	chat: {
		options: 'Opciones del chat',
		placeholder: 'Pregúntale a btw',
		placeholderRunning: 'Añade algo mientras btw trabaja…',
		disclaimer: 'btw puede equivocarse y puede cambiar archivos en este ordenador.',
		reconnecting: 'Reconectando…',
		empty: 'Pide algo para empezar.',
		readsAfterStep: 'btw lo leerá después de su paso actual',
		working: 'btw está trabajando',
		thinking: 'Pensando',
		writing: 'Escribiendo',
		cutOff: 'La respuesta se cortó porque se hizo demasiado larga.',
		refused: 'btw se negó a continuar con esta solicitud.',
		automation: (title: string) => `Automatización · ${title}`,
		fromBtw: (to: string) => `De btw, para ${to}`,
		finishedInBackground: (title: string) => `Terminado en segundo plano · ${title}`,
		inBackground: 'Trabajando en segundo plano',
		aCommand: 'Un comando',
		subagent: (name: string) => `Subagente ${name}`,
		stopping: 'deteniéndose',
		errorTitle: 'Algo salió mal mientras btw respondía.',
		unanswered: 'btw aún no ha respondido a esto.',
		scrollToBottom: 'Ir al mensaje más reciente',
		subagentBanner: 'Subagente {name}: btw lo inició desde {parent} y le informa allí.',
		subagentOnly: (name: string) => `Aquí solo escribe el agente que inició a ${name}.`,
		subagentRetry: 'Solo el agente que lo inició ejecuta un subagente.',
		hiddenBanner:
			'Una ejecución en segundo plano de una automatización. Envía un mensaje para conservarla en tus chats.',
		aChat: 'un chat',
		deleteTitle: '¿Eliminar el chat?',
		deleteBody: 'Esto elimina {name} para todos en el perfil.',
		renameTitle: 'Cambiar el nombre del chat',
		chatName: 'Nombre del chat',
		couldNotRename: 'No se pudo cambiar el nombre del chat.',
		switchTitle: (model: string) => `¿Cambiar a ${model}?`,
		anotherModel: 'otro modelo',
		effortTitle: (level: string) => `¿Cambiar el razonamiento a «${level}»?`,
		switchCache: (model: boolean, missTokens: string | null) =>
			`btw guarda este chat en una caché para que cada respuesta solo pague por lo nuevo. ${model ? 'Otro modelo' : 'Otro nivel de razonamiento'} no puede usarla: la próxima respuesta vuelve a leer todo el chat, lo que tarda más y cuesta más${missTokens ? ` (un fallo de la caché del prompt de unos ${missTokens} tokens)` : ''}.`,
		switchFiles:
			'Algunas imágenes y PDF de este chat no pasan a otro proveedor: el nuevo modelo recibe dónde están sus archivos y puede volver a mirarlos.',
		switch: 'Cambiar',
		change: 'Cambiar',
		models: (models: string[]) => models.join(', luego '),
		usage: 'Uso',
		tokensInOut: (input: string, output: string) =>
			`${input} tokens de entrada, ${output} de salida`,
		cache: 'Caché',
		lastReply: 'Última respuesta',
		wholeConversation: 'Todo el chat',
		cacheSummary: (label: string, rate: string, read: string, written: string, uncached: string) =>
			`${label}: ${rate} en caché (${read} leídos, ${written} escritos, ${uncached} sin caché)`,
		cacheMiss: (tokens: string) => `Fallo de caché · ${tokens} tokens procesados de nuevo`,
		cacheExpired: (ttl: '5m' | '1h') =>
			`Pasaron más de ${ttl === '5m' ? '5 minutos' : 'una hora'} desde el paso anterior, así que la conversación en caché caducó y se procesó de nuevo (más lento y más caro).`,
		cacheBroken:
			'El contexto que debía salir de la caché se procesó de nuevo (más lento y más caro). Pasa una vez al cambiar el modelo o el nivel de razonamiento, mover el chat a otra carpeta o cambiar su carpeta.',
		contextChip: (used: string, window: string, rate: string) =>
			`${used} / ${window} · ${rate} en caché`,
		contextUsed: (used: string, window: string) =>
			`Contexto usado por la última respuesta: ${used} de ${window}`,
		contextMenu: (used: string, window: string) => `Contexto ${used} / ${window}`,
		cachedOverall: (rate: string) => `${rate} en caché en total`
	},

	steps: {
		thinking: 'Pensando',
		thinkingDots: 'Pensando…',
		running: (command: string) => `Ejecutando ${command}`,
		runningACommand: 'Ejecutando un comando',
		ranACommand: 'Ejecutó un comando',
		gettingReady: 'Preparándose…',
		preparing: 'Preparando un comando…',
		stopped: 'Detenido',
		thoughtFor: (duration: string) => `Pensó durante ${duration}`,
		thoughtForAMoment: 'Pensó un momento',
		workedFor: (duration: string) => `Trabajó durante ${duration}`,
		workedForAMoment: 'Trabajó un momento',
		ranCommands: (n: number) =>
			p(n, { one: `Ejecutó ${n} comando`, other: `Ejecutó ${n} comandos` }),
		failedCount: (n: number) => p(n, { one: `${n} falló`, other: `${n} fallaron` }),
		failed: 'falló',
		didntWork: 'no funcionó',
		statusStopped: 'detenido',
		statusNotRun: 'no ejecutado',
		command: 'Comando',
		theCommand: 'El comando que ejecutó btw',
		inFolder: (cwd: string) => `en ${cwd}`,
		noOutputYet: 'Todavía sin salida…',
		noOutput: '(sin salida)'
	},

	composer: {
		placeholder: 'Pregunta lo que quieras',
		message: 'Mensaje',
		attach: 'Adjuntar archivos',
		send: 'Enviar'
	},

	model: {
		model: 'Modelo',
		reasoning: 'Razonamiento',
		efforts: {
			low: { label: 'Bajo', hint: 'Las respuestas más rápidas' },
			medium: { label: 'Medio', hint: 'Bien para casi todo' },
			high: { label: 'Alto', hint: 'Piensa más en las tareas difíciles' },
			xhigh: { label: 'Muy alto', hint: 'Se toma su tiempo' },
			max: { label: 'Máximo', hint: 'El más lento, para los problemas más difíciles' }
		}
	},

	newChat: {
		greeting: (name: string) => `¿En qué te ayudo, ${name}?`,
		greetingNoName: '¿En qué te ayudo?',
		noModels:
			'Todavía no hay modelos configurados. Un administrador puede añadir uno en la página «Modelos y claves» o con {command}.',
		couldNotStart: 'No se pudo empezar el chat.',
		suggestions: {
			reminder: { label: 'Crear un recordatorio', text: 'Recuérdame mañana a las 9:00 que ' },
			weather: {
				label: 'El tiempo cada día',
				text: 'Cada día entre semana a las 7:30, mira el tiempo y dinos si necesitamos paraguas.'
			},
			file: { label: 'Buscar un archivo', text: 'Busca en este ordenador el archivo llamado ' },
			space: {
				label: 'Ver el espacio libre',
				text: '¿Cuánto espacio libre queda en el disco de este ordenador?'
			}
		},
		pickModel: 'Elige un modelo.',
		pickEffort: 'Elige un nivel de razonamiento.',
		folderGone: 'Esa carpeta se eliminó. Elige otra.'
	},

	folders: {
		inFolder: (name: string) => `En la carpeta ${name}`,
		startInFolder: 'Empezar en una carpeta',
		startOutside: 'Empezar fuera de la carpeta',
		noFolder: 'Sin carpeta',
		newFolderDots: 'Nueva carpeta…',
		moveTo: 'Mover a una carpeta',
		moveTechnical:
			'Al moverlo se vuelve a construir el prompt del sistema, así que la próxima respuesta vuelve a leer toda la conversación una vez (un fallo de la caché del prompt).',
		move: 'Después de moverlo, la próxima respuesta tarda un poco más.',
		newTitle: 'Nueva carpeta',
		newDescription:
			'Mantén juntos los chats relacionados. Cada chat de una carpeta recibe sus instrucciones y archivos.',
		namePlaceholder: 'Viaje a Japón',
		name: 'Nombre de la carpeta',
		create: 'Crear carpeta',
		renameTitle: 'Cambiar el nombre de la carpeta',
		couldNotRename: 'No se pudo cambiar el nombre de la carpeta.',
		deleteTitle: '¿Eliminar la carpeta?',
		deleteBody:
			'{name} se elimina para todos en el perfil. Sus chats vuelven a tu lista de chats, sin sus instrucciones ni archivos. Los archivos se mueven a ~/.btw-agent/trash.',
		options: 'Opciones de la carpeta',
		newChatIn: (name: string) => `Nuevo chat en ${name}`,
		instructions: 'Instrucciones',
		instructionsHint: 'Lo que btw debe saber o hacer en cada chat de aquí.',
		instructionsPlaceholder:
			'Estamos planeando dos semanas en Japón en abril con los niños (7 y 10 años). Planes tranquilos y un presupuesto de menos de 600 000 ¥.',
		changesTechnical:
			'Los chats de la carpeta reciben los cambios con su próximo mensaje, que vuelve a leer la conversación una vez (un fallo de la caché del prompt).',
		changes: 'Los chats de la carpeta reciben los cambios con su próximo mensaje.',
		files: 'Archivos',
		addFiles: 'Añadir archivos',
		filesHint:
			'Fotos, documentos, lo que sea. btw sabe dónde están guardados y los abre cuando hace falta.',
		noFiles: 'Todavía no hay archivos',
		savedIn: (dir: string) => `Guardados en ${dir}`,
		couldNotAdd: 'No se pudieron añadir los archivos.',
		couldNotAddOffline:
			'No se pudieron añadir los archivos. Comprueba la conexión y vuelve a intentarlo.',
		chats: 'Chats',
		noChats:
			'Los chats que empieces aquí aparecen aquí. También puedes arrastrar chats a la carpeta en la barra lateral.'
	},

	attachments: {
		open: (name: string) => `Abrir ${name}`,
		download: 'Descargar',
		onlyPath: (note: string) => `btw solo recibió dónde está guardado: ${note}`
	},

	markdown: {
		text: 'texto',
		picture: 'Imagen',
		notAvailable: 'No disponible'
	},

	images: {
		title: 'Imágenes',
		shapes: { square: 'Cuadrada', portrait: 'Vertical', landscape: 'Horizontal', auto: 'Auto' },
		shape: 'Formato',
		groups: 'Grupos de plantillas',
		noTemplates: 'Todavía no hay plantillas.',
		cantMakeYet: 'btw todavía no puede crear imágenes.',
		needsKeyAdmin: 'Necesita una clave de API de {provider}. {link}.',
		addKeyLink: 'Añádela en «Modelos y claves»',
		needsKey: (provider: string) =>
			`Necesita una clave de API de ${provider}. Pide a un administrador que la añada.`,
		adminSetsUp: 'Un administrador lo configura en el ordenador donde funciona btw.',
		describe: 'Describe una imagen',
		describeHint: 'btw crea la imagen en un chat nuevo, donde puedes pedir cambios.',
		onlyPictures: 'Aquí solo se pueden usar imágenes.',
		atMostPictures: (n: number) => `Como máximo ${n} imágenes.`,
		changePicture: (label: string | null) => (label ? `Cambiar: ${label}` : 'Cambiar la imagen'),
		addPicture: 'Añadir una imagen',
		draw: 'Dibujar',
		choosePhotoOfDrawing: 'Elegir una foto de un dibujo',
		usePhotoOfDrawing: 'Usar una foto de un dibujo',
		startDrawing: 'Empezar a dibujar',
		takePhoto: 'Hacer una foto',
		choosePhoto: 'Elegir una foto',
		tryIt: 'Probar',
		yourOwn: (label: string) => `tu propio valor: ${label.toLowerCase()}`,
		pickFromList: (label: string) => `Elegir de la lista: ${label.toLowerCase()}`,
		custom: 'Personalizado…',
		addAnything: 'Añade lo que quieras…',
		uploadingPicture: 'Subiendo la imagen…',
		addDrawingFirst: 'Primero añade un dibujo.',
		addPhotoFirst: 'Primero añade una foto.',
		generate: 'Generar',
		noModel: 'Todavía no hay ningún modelo de chat configurado.',
		notPicture: (name: string) => `${name} no es una imagen.`,
		templateGone: 'Esa plantilla ya no existe.',
		describeFirst: 'Primero describe la imagen.'
	},

	drawing: {
		title: 'Dibujo',
		tool: 'Herramienta',
		pen: 'Lápiz',
		eraser: 'Goma',
		undo: 'Deshacer',
		penSize: 'Grosor del lápiz',
		color: (color: string) => `Color ${color}`,
		use: 'Usar este dibujo'
	},

	emoji: {
		value: (label: string, value: string) => `${label}: ${value || 'ninguno'}`,
		pickUpTo: (n: number) => `Elige hasta ${n}`,
		removeLast: 'Quitar el último emoji'
	},

	memory: {
		title: 'Memoria',
		intro: (profile: string) =>
			`Lo que btw recuerda para ${profile}, compartido por todos en él. Cada chat empieza con la nota fijada Core; btw lee las demás cuando un chat las necesita y guarda lo que aprende por el camino. Para añadir algo, díselo en un chat, por ejemplo: «Recuerda que Anna es alérgica a los frutos secos».`,
		total: (count: number, topics: number) =>
			`${count === 1 ? 'recuerdo' : 'recuerdos'} en ${p(topics, { one: `${topics} tema`, other: `${topics} temas` })}`,
		newThisWeek: (n: number) =>
			p(n, { one: `${n} nuevo esta semana`, other: `${n} nuevos esta semana` }),
		updated: (ago: string) => `actualizado ${ago}`,
		memories,
		pinned: 'fijada, en cada chat nuevo',
		edit: (topic: string) => `Editar ${topic}`,
		forget: (topic: string) => `Olvidar ${topic}`,
		note: (topic: string) => `Nota ${topic}`,
		corePlaceholder:
			'Por ejemplo:\n- Anna y Ben son los padres, Mia tiene 7 años\n- En casa hablamos ruso\n- Mia es alérgica a los frutos secos',
		coreEmpty:
			'Nada todavía. Pon aquí lo que btw debe tener presente en cada chat: quién es quién en la familia, los idiomas que habláis, alergias. btw también lo irá completando.',
		forgetTitle: (topic: string) => `¿Olvidar «${topic}»?`,
		forgetBody:
			'btw olvida todo lo que hay en {path} para todos en {profile}. Los chats que ya lo leyeron conservan lo que leyeron.',
		forgetButton: 'Olvidar',
		empty: 'La nota está vacía. Para eliminarla, usa Olvidar.',
		conflict: (problem: string) =>
			`${problem} Guarda de nuevo para conservar tu versión, o cancela para ver la de btw.`,
		saved: 'Guardado. Los chats nuevos verán el cambio.',
		forgot: (path: string) => `btw olvidó todo lo que había en ${path}.`,
		learned: (ago: string) => `aprendido ${ago}`,
		learnedAWhileAgo: 'aprendido hace tiempo',
		topicLabel: (topic: string, n: number) => `${topic}: ${memories(n)}. Mostrar la nota.`,
		nothingInIt: 'Todavía vacía',
		showFewer: 'Mostrar menos',
		showAllTopics: (n: number) => `Mostrar los ${n} temas`,
		older: 'Más antiguo',
		newer: 'Más reciente',
		nothingYet: 'Todavía no recuerda nada. Cada cosa que btw aprende se convierte aquí en un punto.'
	},

	automations: {
		title: 'Automatizaciones',
		intro:
			'Lo que btw hace por su cuenta: recordatorios, comprobaciones periódicas y respuestas a otras apps. Lo que encuentra aparece bajo la campana. Para añadir o cambiar una, pídeselo a btw en un chat.',
		today: 'Hoy',
		previousMonth: 'Mes anterior',
		nextMonth: 'Mes siguiente',
		dayLabel: (day: string, runs: number) =>
			`${day}: ${runs ? p(runs, { one: `${runs} ejecución`, other: `${runs} ejecuciones` }) : 'nada'}`,
		nothingRan: 'Ese día no se ejecutó nada.',
		nothingRuns: 'Ese día no se ejecuta nada.',
		timeZone: (zone: string) => `Las horas están en la zona horaria del ordenador (${zone}).`,
		all: 'Todas las automatizaciones',
		next: (when: string) => `próxima ${when}`,
		states: { paused: 'en pausa', done: 'terminada' },
		runNow: 'Ejecutar ahora',
		pause: 'Pausar',
		resume: 'Reanudar',
		description: 'Descripción',
		descriptionPlaceholder: 'Qué hace, en una frase',
		instructions: 'Instrucciones para btw',
		instructionsTechnical: (model: string, effort: string) =>
			`Instrucciones para btw (${model}, razonamiento ${effort})`,
		script: 'Script: funciona sin el modelo y llama a {command} cuando hace falta btw',
		webhook:
			'URL del webhook. Mantenla en secreto: cualquiera que la tenga puede iniciar una ejecución. Envíale JSON con POST.',
		confirmDelete: (name: string) => `¿Eliminar «${name}»?`,
		recentRuns: (n: number) => `Ejecuciones recientes (${n})`,
		noRuns: 'Todavía no hay ejecuciones',
		statuses: {
			pending: 'esperando',
			running: 'en curso',
			ok: 'hecho',
			notified: 'notificado',
			silent: 'nada que contar',
			stopped: 'detenido',
			failed: 'falló'
		},
		sources: {
			cron: 'programada',
			once: 'programada',
			webhook: 'webhook',
			wake: 'despertada por un script',
			manual: 'a mano'
		},
		scriptRun: (status: string) => `script: ${status}`,
		view: 'ver',
		output: 'salida',
		empty: 'Todavía no hay automatizaciones. Pídeselo a btw en un chat, por ejemplo:',
		examples: [
			'«Cada día entre semana a las 7:30, mira el tiempo y dinos si necesitamos paraguas».',
			'«Recuérdale a Anna mañana a las 17:00 que recoja el paquete».',
			'«Revisa mi correo cada 10 minutos y avísame cuando escriba el colegio».'
		],
		defaultModel: 'el modelo predeterminado',
		saved: (name: string) => `«${name}» guardada.`,
		started: (name: string) => `«${name}» iniciada. Su respuesta aparecerá bajo la campana.`,
		startedScript: (name: string) => `Se inició el script de «${name}».`,
		paused: (name: string) => `«${name}» está en pausa.`,
		resumed: (name: string) => `«${name}» vuelve a estar activa.`,
		deleted: (name: string) => `«${name}» eliminada.`,
		describe,
		scheduleAfterName: lowerFirst,
		customSchedule: 'Con un horario personalizado',
		once: 'Una vez',
		onceAt: (when: string) => `Una vez, ${when}`,
		onWebhook: 'Cuando otra app llama a su enlace',
		dayAt: (day: string, time: string) => `${day} ${at([time])}`
	},

	skills: {
		title: 'Habilidades',
		intro:
			'Las habilidades son instrucciones que btw sigue para tareas concretas. Ve el nombre y la descripción de cada habilidad activada y lee el resto cuando una tarea lo necesita. Desactivar las que este perfil no necesita ayuda a btw a centrarse. Los cambios se aplican a los chats nuevos.',
		summary: (on: number, total: number, tokens: string) =>
			`${on} de ${total} activadas · unos ${tokens} tokens al inicio de cada chat nuevo`,
		madeFor: (profile: string) => `Hechas para ${profile}`,
		shared: 'Compartidas por todos los perfiles',
		builtIn: 'Incluidas en btw',
		allOff: 'Desactivar todas',
		allOn: 'Activar todas',
		tokens: (n: string) => `~${n} tokens`,
		use: (name: string) => `Usar ${name}`,
		empty:
			'Todavía no hay habilidades. Cuando btw descubre cómo hacer algo, puede guardarlo como habilidad para la próxima vez.',
		gone: 'Esa habilidad ya no existe.'
	},

	profile: {
		title: 'Personas y perfil',
		name: 'Nombre',
		profileName: 'Nombre del perfil',
		folder: (slug: string) => `Carpeta: ~/.btw-agent/profiles/${slug} (no cambia)`,
		avatar: 'Avatar',
		avatarHint:
			'Cómo se ve btw en los chats de este perfil. Todos aquí ven el mismo, y btw puede cambiarlo si se lo pides.',
		soul: 'Alma',
		soulHint: (profile: string) =>
			`Quién es btw para ${profile}: su carácter, lo que le importa, cómo habla. Cada chat empieza con ella, y btw también la cambia cuando le pides que sea distinto.`,
		soulPlaceholder:
			'Eres cálido y un poco juguetón, y das respuestas cortas. A los niños les explicas las cosas de forma sencilla y nunca con condescendencia. Cuando no sabes algo, lo dices.',
		changesTechnical:
			'Los chats reciben los cambios con su próximo mensaje, que vuelve a leer la conversación una vez (un fallo de la caché del prompt).',
		changes: 'Los chats reciben los cambios con su próximo mensaje.',
		members: 'Miembros',
		membersHint:
			'Todos aquí ven y escriben en los mismos chats, y pueden pedirle a btw cualquier cosa.',
		personToAdd: 'Persona que añadir',
		chooseSomeone: 'Elige a quién añadir',
		everyoneIsMember: 'Todos son ya miembros.',
		deleteTitle: 'Eliminar el perfil',
		deleteHint: 'Elimina todos sus chats para todos. La carpeta se mueve a ~/.btw-agent/trash.',
		deleteButton: 'Eliminar este perfil',
		deleteConfirm: (name: string) => `¿Eliminar «${name}»?`,
		deleteBody: 'Todos sus chats se eliminan para todos. La carpeta se mueve a ~/.btw-agent/trash.',
		renamed: 'Nombre cambiado.',
		soulSaved: 'Alma guardada.',
		soulRemoved: 'Alma eliminada.',
		added: (name: string) => `${name} añadido.`,
		removed: 'Quitado.'
	},

	avatars: {
		probe: 'Sonda',
		campfire: 'Fogata',
		lantern: 'Farol',
		planet: 'Planeta',
		quantum: 'Cuanto',
		comet: 'Cometa',
		moon: 'Luna',
		satellite: 'Satélite'
	},

	profiles: {
		title: 'Perfiles',
		intro:
			'Cada perfil tiene sus propios chats, memoria y habilidades, compartidos por sus miembros.',
		none: 'Todavía no estás en ningún perfil. Crea uno abajo o pide a alguien que te añada al suyo.',
		new: 'Nuevo perfil',
		namePlaceholder: 'p. ej. Familia, Abuela, Deberes',
		name: 'Nombre del perfil',
		needsName: 'Ponle un nombre al perfil.'
	},

	login: {
		welcome: 'Hola de nuevo',
		hint: 'Inicia sesión con la cuenta que te dieron.',
		email: 'Correo electrónico',
		password: 'Contraseña',
		signingIn: 'Iniciando sesión…',
		forgot: '¿Olvidaste la contraseña? Pide a quien configuró btw que ejecute {command}.',
		tooManyAttempts: 'Demasiados intentos. Espera un minuto y vuelve a intentarlo.',
		wrongPassword: 'Correo o contraseña incorrectos.'
	},

	admin: {
		title: 'Modelos y claves',
		keys: 'Claves de API',
		keysHint:
			'Compartidas por todos los perfiles y guardadas en el archivo de configuración de btw en este ordenador. Una clave nueva se comprueba con su proveedor antes de guardarla, y se usa enseguida.',
		purposes: {
			anthropic: 'Hace funcionar los chats y las automatizaciones con modelos de Claude.',
			openai:
				'Hace funcionar los chats y las automatizaciones con modelos de OpenAI, y crea las imágenes de la página Imágenes y las que dibuja el agente.'
		},
		withoutIt: {
			anthropic:
				'Los chats y las automatizaciones con modelos de Claude dejan de funcionar hasta que se añada una clave nueva.',
			openai:
				'Los chats y las automatizaciones con modelos de OpenAI dejan de funcionar, y btw no puede crear imágenes, hasta que se añada una clave nueva.'
		},
		savedInBtw: (hint: string | null) => `Guardada en btw${hint ? `, termina en ${hint}` : ''}`,
		fromEnv: (variable: string, hint: string | null) =>
			`De la variable de entorno ${variable}${hint ? `, termina en ${hint}` : ''}`,
		notSet: 'Sin configurar',
		replace: 'Reemplazar',
		pasteKey: (provider: string) => `Pega la clave de API de ${provider}`,
		keyLabel: (provider: string) => `Clave de API de ${provider}`,
		checkingKey: 'Comprobando la clave…',
		makeOneAt: 'Crea una en {link}.',
		sameProject:
			'Usa una clave del mismo proyecto: las imágenes y los PDF ya enviados en los chats están allí, y esos chats no pueden seguir sin ellos.',
		sameWorkspace:
			'Usa una clave del mismo espacio de trabajo: las imágenes y los PDF ya enviados en los chats están allí, y esos chats no pueden seguir sin ellos.',
		removeKeyTitle: (provider: string) => `¿Quitar la clave de ${provider}?`,
		useEnvInstead: (variable: string) =>
			`btw usará en su lugar la clave de la variable de entorno ${variable}.`,
		plan: 'Plan de Claude',
		planHint:
			'Los chats con un preajuste del plan de Claude funcionan con el plan Pro o Max con el que alguien inició sesión en Claude Code en este ordenador, en lugar de con una clave de API. btw ejecuta Claude Code y nunca ve el inicio de sesión. Los límites del plan suponen el uso normal de una persona, así que deja las automatizaciones intensivas y los subagentes en un preajuste con clave de API.',
		notAt: 'No está en {path}, donde {command} dice que está.',
		notInstalled: 'No está instalado en este ordenador.',
		checkSignIn: 'Comprobar la sesión',
		install:
			'En una terminal de este ordenador, ejecuta {setup}: instala Claude Code con el instalador de Anthropic y lo conecta a tu cuenta de Claude, preguntando antes. O instálalo tú, luego ejecuta {claude} e inicia sesión:',
		copyCommand: 'Copiar el comando',
		signIn:
			'Para iniciar sesión, ejecuta {setup} en una terminal de este ordenador, o ejecuta {claude} allí y usa {login} con tu cuenta de Claude.',
		models: 'Modelos',
		modelsHint:
			'Los preajustes se comparten entre todos los perfiles. Los chats nuevos empiezan con el predeterminado. Quitar un preajuste no afecta a los chats existentes.',
		presetDetails: (provider: string, model: string, context: string, overridden: boolean) =>
			`${provider} / ${model} · contexto ${context}${overridden ? ' (personalizado)' : ''}`,
		makeDefault: 'Hacer predeterminado',
		noPresets: 'Todavía no hay preajustes.',
		addModel: {
			title: 'Añadir un modelo',
			provider: 'Proveedor',
			model: 'Modelo',
			name: 'Nombre',
			optional: '(opcional)',
			namePlaceholder: 'El ID del modelo y el proveedor',
			contextWindow: 'Ventana de contexto',
			contextTokens: 'Ventana de contexto en tokens',
			auto: 'Auto',
			autoWith: (size: string) => `Auto · ${size}`,
			unknown: 'desconocida',
			custom: 'Personalizada',
			tokensPlaceholder: 'Tokens, p. ej. 272k',
			tokens: (n: number) => `${n.toLocaleString('es')} tokens.`,
			typeTokens: 'Escribe un número de tokens, como 272k o 272000.',
			autoContext: {
				anthropic: 'Auto usa la ventana que Anthropic indica para el modelo.',
				openai:
					'OpenAI no la indica: Auto solo conoce la de sus modelos insignia (1,05M desde GPT-5.4).',
				'claude-plan': 'Claude Code no la indica: Auto solo conoce sus modelos de 1M de contexto.'
			},
			onPlan: 'Funciona con el plan Pro o Max con el que inició sesión Claude Code.',
			noClaudeCode: 'Claude Code aún no está instalado: consulta «Plan de Claude», arriba.',
			onKey: (provider: string) => `Funciona con la clave de API de ${provider}.`,
			noKey: (provider: string) =>
				`Todavía no hay clave de API de ${provider}: añade una en «Claves de API», arriba.`,
			asking: (source: string) => `Preguntando a ${source} por sus modelos…`,
			listProblem: (problem: string) => `${problem} Aun así puedes escribir un ID.`,
			couldNotList: (status: number) => `btw no pudo obtener los modelos (${status}).`,
			unreachable: 'No se pudo contactar con btw.',
			checkingClaude: 'Comprobando Claude Code…',
			checkingModel: 'Comprobando el modelo…',
			pick: 'Elige un modelo',
			search: 'Busca o escribe un ID de modelo',
			use: 'Usar {model}',
			typeId: 'Escribe arriba el ID del modelo.',
			added: (name: string) => `${name} añadido.`,
			invalidContext: 'La ventana de contexto debe ser un número de tokens, como 272k o 272000.'
		},
		savedWorks: 'Guardada. Funciona.',
		savedWarning: (warning: string) => `Guardada. ${warning}`,
		claudeNoAnswer: 'Claude Code no respondió.',
		removed: 'Quitada.',
		newDefault: (name: string) => `Los chats nuevos empiezan ahora con ${name}.`,
		presetRemoved: 'Quitado. Las conversaciones existentes siguen funcionando.'
	}
};
