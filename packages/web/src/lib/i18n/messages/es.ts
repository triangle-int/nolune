import { isMonthSpan, type Schedule } from '@nolune/core/schedule';
import { listOf, plural } from '../plural';
import type { Messages } from './en';

const p = plural('es');
const list = listOf('es');

/** ", Anna" to finish a greeting with, or nothing when there's no name. */
const to = (name: string) => (name ? `, ${name}` : '');

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
		description: 'Tu nombre y tu foto, y cómo se ve nolune en este dispositivo.',
		theme: 'Tema',
		system: 'Sistema',
		light: 'Claro',
		dark: 'Oscuro',
		language: 'Idioma',
		languageAuto: 'El del navegador',
		languageHint: 'Para menús y botones. nolune responde en el idioma en que escribas.',
		technical: 'Mostrar detalles técnicos',
		technicalHint:
			'Muestra los comandos exactos que ejecuta nolune, el uso de tokens y la caché del prompt.',
		expandSteps: 'Mostrar siempre los pasos',
		expandStepsHint:
			'Abre la lista de lo que hizo nolune debajo de cada respuesta, en lugar de dejarla plegada.',
		sounds: 'Sonidos',
		soundsHint: 'Sonidos suaves donde nolune se mueve solo, como la bienvenida de un perfil nuevo.',
		logOut: 'Cerrar sesión',
		deviceOnly: 'Estos ajustes solo se guardan en este dispositivo.'
	},

	account: {
		name: 'Tu nombre',
		hint: 'Todos en tus perfiles ven tu nombre y tu foto, y nolune lee tu nombre con cada mensaje que envías.',
		addPicture: 'Añadir una foto',
		changePicture: 'Cambiar foto',
		removePicture: 'Quitar foto',
		cropHint: 'Arrastra la foto para colocarla en el círculo.',
		zoom: 'Zoom',
		usePicture: 'Usar esta foto',
		cantOpen: 'No se pudo abrir esa imagen. Prueba con JPEG o PNG.',
		nameRequired: 'Escribe un nombre.',
		nameTooLong: (max: number) => `Un nombre puede tener como mucho ${max} caracteres.`,
		nameHasAt: 'Un nombre no puede llevar @.',
		nameTaken: (name: string) => `Ya hay alguien que se llama ${name}.`,
		emailInvalid: 'Escribe un correo electrónico, como anna@example.com.',
		emailTaken: 'Alguien ya inicia sesión con ese correo.',
		passwordTooShort: (min: number) => `Una contraseña necesita al menos ${min} caracteres.`,
		passwordTooSimple:
			'Usa 3 de: minúsculas, mayúsculas, dígitos y símbolos. O que tenga 20 caracteres o más.',
		passwordTooRepetitive: 'Esa contraseña repite demasiado los mismos caracteres.',
		notPicture: 'nolune no puede usar esa imagen.',
		pictureTooLarge: 'Esa imagen es demasiado grande.'
	},

	userMenu: {
		account: 'Cuenta',
		settings: 'Ajustes',
		allProfiles: 'Todos los perfiles',
		modelsAndKeys: 'Modelos y claves',
		people: 'Personas',
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
			'Nada todavía. Pídele a nolune un recordatorio o una comprobación diaria, y lo que encuentre aparecerá aquí.'
	},

	chat: {
		options: 'Opciones del chat',
		placeholder: 'Pregúntale a nolune',
		placeholderRunning: 'Añade algo mientras nolune trabaja…',
		disclaimer: 'nolune puede equivocarse y puede cambiar archivos en este ordenador.',
		reconnecting: 'Reconectando…',
		empty: 'Pide algo para empezar.',
		readsAfterStep: 'nolune lo leerá después de su paso actual',
		typing: (names: string[]) =>
			`${list(names)} ${names.length === 1 ? 'está' : 'están'} escribiendo`,
		working: 'nolune está trabajando',
		thinking: 'Pensando',
		writing: 'Escribiendo',
		cutOff: 'La respuesta se cortó porque se hizo demasiado larga.',
		refused: 'nolune se negó a continuar con esta solicitud.',
		automation: (title: string) => `Automatización · ${title}`,
		fromNolune: (to: string) => `De nolune, para ${to}`,
		finishedInBackground: (title: string) => `Terminado en segundo plano · ${title}`,
		inBackground: 'Trabajando en segundo plano',
		aCommand: 'Un comando',
		subagent: (name: string) => `Subagente ${name}`,
		stopping: 'deteniéndose',
		errorTitle: 'Algo salió mal mientras nolune respondía.',
		unanswered: 'nolune aún no ha respondido a esto.',
		scrollToBottom: 'Ir al mensaje más reciente',
		subagentBanner: 'Subagente {name}: nolune lo inició desde {parent} y le informa allí.',
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
			`nolune guarda este chat en una caché para que cada respuesta solo pague por lo nuevo. ${model ? 'Otro modelo' : 'Otro nivel de razonamiento'} no puede usarla: la próxima respuesta vuelve a leer todo el chat, lo que tarda más y cuesta más${missTokens ? ` (un fallo de la caché del prompt de unos ${missTokens} tokens)` : ''}.`,
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
		blockedCount: (n: number) => p(n, { one: `${n} bloqueado`, other: `${n} bloqueados` }),
		failed: 'falló',
		didntWork: 'no funcionó',
		statusStopped: 'detenido',
		statusNotRun: 'no ejecutado',
		statusBlocked: 'bloqueado',
		command: 'Comando',
		theCommand: 'El comando que ejecutó nolune',
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

	commandMode: {
		title: 'Comandos en este chat',
		auto: 'Automático',
		autoHint: 'Un modelo revisa cada comando antes de que se ejecute.',
		unrestricted: 'Sin restricciones',
		unrestrictedHint: 'Los comandos se ejecutan sin revisión. No recomendado.',
		adminsOnly: 'Solo un administrador puede desactivar la revisión en un chat.',
		labelAuto: 'Comandos: modo automático',
		labelUnrestricted: 'Comandos: sin restricciones'
	},

	newChat: {
		greetings: {
			morning: [
				(name: string) => `¡Buenos días${to(name)}!`,
				(name: string) => `Buenos días${to(name)}. ¿Por dónde empezamos hoy?`,
				(name: string) => `¿Primero un café o vamos al grano${to(name)}?`,
				(name: string) => `¿Qué hay para hoy${to(name)}?`
			],
			afternoon: [
				(name: string) => `¡Buenas tardes${to(name)}!`,
				(name: string) => `Buenas tardes${to(name)}. ¿Qué tal va el día?`,
				(name: string) => `¿Qué te quito de encima esta tarde${to(name)}?`,
				(name: string) => `¿Te echo una mano esta tarde${to(name)}?`
			],
			evening: [
				(name: string) => `¿Qué tal tu día${to(name)}?`,
				(name: string) => `¿Queda algo pendiente para hoy${to(name)}?`,
				(name: string) => `¿Qué planes hay para esta noche${to(name)}?`,
				(name: string) => `¿En qué te ayudo esta noche${to(name)}?`
			],
			night: [
				(name: string) => `¿Trasnochando${to(name)}?`,
				(name: string) => `¿No puedes dormir${to(name)}? Aquí estoy.`,
				(name: string) => `Se está haciendo tarde${to(name)}. ¿Qué necesitas?`,
				(name: string) => `¿Una noche larga${to(name)}?`
			],
			anytime: [
				(name: string) => `¿En qué te ayudo${to(name)}?`,
				(name: string) => `¿Qué tienes en mente${to(name)}?`,
				(name: string) => `¿En qué trabajamos${to(name)}?`,
				(name: string) => `¡Qué gusto verte${to(name)}! ¿Qué hay de nuevo?`
			]
		},
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
			'{name} se elimina para todos en el perfil. Sus chats vuelven a tu lista de chats, sin sus instrucciones ni archivos. Los archivos se mueven a ~/.nolune/trash.',
		options: 'Opciones de la carpeta',
		newChatIn: (name: string) => `Nuevo chat en ${name}`,
		instructions: 'Instrucciones',
		instructionsHint: 'Lo que nolune debe saber o hacer en cada chat de aquí.',
		instructionsPlaceholder:
			'Estamos planeando dos semanas en Japón en abril con los niños (7 y 10 años). Planes tranquilos y un presupuesto de menos de 600 000 ¥.',
		changesTechnical:
			'Los chats de la carpeta reciben los cambios con su próximo mensaje, que vuelve a leer la conversación una vez (un fallo de la caché del prompt).',
		changes: 'Los chats de la carpeta reciben los cambios con su próximo mensaje.',
		files: 'Archivos',
		addFiles: 'Añadir archivos',
		filesHint:
			'Fotos, documentos, lo que sea. nolune sabe dónde están guardados y los abre cuando hace falta.',
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
		onlyPath: (note: string) => `nolune solo recibió dónde está guardado: ${note}`
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
		cantMakeYet: 'nolune todavía no puede crear imágenes.',
		needsKeyAdmin: 'Necesita una clave de API de {provider}. {link}.',
		addKeyLink: 'Añádela en «Modelos y claves»',
		needsKey: (provider: string) =>
			`Necesita una clave de API de ${provider}. Pide a un administrador que la añada.`,
		adminSetsUp: 'Un administrador lo configura en el ordenador donde funciona nolune.',
		describe: 'Describe una imagen',
		describeHint: 'nolune crea la imagen en un chat nuevo, donde puedes pedir cambios.',
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
			`Lo que nolune recuerda para ${profile}, compartido por todos en él. Cada chat empieza con la nota fijada Lo esencial; nolune lee las demás cuando un chat las necesita y guarda lo que aprende por el camino. Para añadir algo, díselo en un chat, por ejemplo: «Recuerda que Anna es alérgica a los frutos secos».`,
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
			'Nada todavía. Pon aquí lo que nolune debe tener presente en cada chat: quién es quién en la familia, los idiomas que habláis, alergias. nolune también lo irá completando.',
		forgetTitle: (topic: string) => `¿Olvidar «${topic}»?`,
		forgetBody:
			'nolune olvida todo lo que hay en {path} para todos en {profile}. Los chats que ya lo leyeron conservan lo que leyeron.',
		forgetButton: 'Olvidar',
		empty: 'La nota está vacía. Para eliminarla, usa Olvidar.',
		conflict: (problem: string) =>
			`${problem} Guarda de nuevo para conservar tu versión, o cancela para ver la de nolune.`,
		saved: 'Guardado. Los chats nuevos verán el cambio.',
		forgot: (path: string) => `nolune olvidó todo lo que había en ${path}.`,
		learn: 'Aprender de los chats',
		learnHint:
			'Cuando un chat lleva un par de minutos en silencio, nolune lo repasa y guarda lo que vale la pena recordar. Cada vez es una breve petición extra al modelo del chat. Si lo desactivas, nolune solo guarda lo que se le ocurra durante la conversación.',
		learned: (ago: string) => `aprendido ${ago}`,
		learnedAWhileAgo: 'aprendido hace tiempo',
		topicLabel: (topic: string, n: number) => `${topic}: ${memories(n)}. Mostrar la nota.`,
		nothingInIt: 'Todavía vacía',
		showFewer: 'Mostrar menos',
		showAllTopics: (n: number) => `Mostrar los ${n} temas`,
		older: 'Más antiguo',
		newer: 'Más reciente',
		nothingYet:
			'Todavía no recuerda nada. Cada cosa que nolune aprende se convierte aquí en un punto.',
		categories: {
			core: 'Lo esencial',
			people: 'Personas',
			home: 'Casa',
			health: 'Salud',
			plans: 'Planes',
			routines: 'Rutinas',
			pets: 'Mascotas',
			places: 'Lugares',
			projects: 'Proyectos',
			other: 'Otros',
			unsorted: 'Sin categoría'
		},
		unsortedHint:
			'De antes de que la memoria tuviera categorías: nolune la lee, pero no le añade nada. Muévela a una categoría para que siga creciendo.',
		memberNote: (name: string) => `miembro: ${name}`,
		move: {
			button: (topic: string) => `Mover ${topic}`,
			title: (topic: string) => `Mover «${topic}»`,
			body: 'Elige dónde va. Si esa nota ya existe, las dos se convierten en una.',
			to: 'Mover a',
			choose: 'Elige dónde',
			categories: 'Categorías',
			newPerson: 'Alguien nuevo…',
			newProject: 'Un proyecto nuevo…',
			personName: 'Su nombre',
			projectName: 'Nombre del proyecto',
			moveHint: (target: string) => `Pasará a ser ${target}.`,
			mergeHint: (from: string, into: string) =>
				`Lo que dice «${from}» se añade a «${into}», sin repeticiones, y «${from}» desaparece. Sirve para dos notas sobre la misma persona.`,
			move: 'Mover',
			merge: 'Unir',
			moved: (from: string, to: string) => `${from} movida a ${to}.`,
			merged: (from: string, into: string) => `${from} unida a ${into}.`
		},
		/** What nolune saved from a chat by itself: in that chat, and at the top of this page. */
		changes: {
			saved: (n: number) => `Guardó ${memories(n)}`,
			added: 'Nuevo',
			changed: 'Cambiado',
			before: (text: string) => `antes: ${text}`,
			openNote: (topic: string) => `Abrir ${topic} en Memoria`,
			undo: 'Deshacer',
			undone: 'Deshecho',
			undoneBy: (name: string) => `Deshecho por ${name}`,
			changedSince:
				'Cambió desde entonces, así que no se puede deshacer aquí. Edita la nota en la página Memoria.',
			alreadyUndone: 'Ya se deshizo.',
			recent: 'Guardado de los chats',
			recentHint:
				'Lo que nolune anotó por su cuenta cuando los chats quedaron en silencio, en las últimas dos semanas.',
			fromChat: 'de {chat}',
			deletedChat: 'de un chat eliminado',
			showAll: (n: number) => `Mostrar los ${n}`,
			untitled: 'un chat'
		}
	},

	automations: {
		title: 'Automatizaciones',
		intro:
			'Lo que nolune hace por su cuenta: recordatorios, comprobaciones periódicas y respuestas a otras apps. Lo que encuentra aparece bajo la campana. Para añadir o cambiar una, pídeselo a nolune en un chat.',
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
		instructions: 'Instrucciones para nolune',
		instructionsTechnical: (model: string, effort: string) =>
			`Instrucciones para nolune (${model}, razonamiento ${effort})`,
		script: 'Script: funciona sin el modelo y llama a {command} cuando hace falta nolune',
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
		empty: 'Todavía no hay automatizaciones. Pídeselo a nolune en un chat, por ejemplo:',
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
			'Las habilidades son instrucciones que nolune sigue para tareas concretas. Ve el nombre y la descripción de cada habilidad activada y lee el resto cuando una tarea lo necesita. Desactivar las que este perfil no necesita ayuda a nolune a centrarse. Los cambios se aplican a los chats nuevos.',
		summary: (on: number, total: number, tokens: string) =>
			`${on} de ${total} activadas · unos ${tokens} tokens al inicio de cada chat nuevo`,
		madeFor: (profile: string) => `Hechas para ${profile}`,
		shared: 'Compartidas por todos los perfiles',
		builtIn: 'Incluidas en nolune',
		allOff: 'Desactivar todas',
		allOn: 'Activar todas',
		tokens: (n: string) => `~${n} tokens`,
		use: (name: string) => `Usar ${name}`,
		empty:
			'Todavía no hay habilidades. Cuando nolune descubre cómo hacer algo, puede guardarlo como habilidad para la próxima vez.',
		gone: 'Esa habilidad ya no existe.'
	},

	profile: {
		title: 'Personas y perfil',
		name: 'Nombre',
		profileName: 'Nombre del perfil',
		folder: (slug: string) => `Carpeta: ~/.nolune/profiles/${slug} (no cambia)`,
		avatar: 'Avatar',
		avatarHint:
			'Cómo se ve nolune en los chats de este perfil. Todos aquí ven el mismo, y nolune puede cambiarlo si se lo pides.',
		soul: 'Alma',
		soulHint: (profile: string) =>
			`Quién es nolune para ${profile}: su carácter, lo que le importa, cómo habla. Cada chat empieza con ella, y nolune también la cambia cuando le pides que sea distinto.`,
		soulPlaceholder:
			'Eres cálido y un poco juguetón, y das respuestas cortas. A los niños les explicas las cosas de forma sencilla y nunca con condescendencia. Cuando no sabes algo, lo dices.',
		changesTechnical:
			'Los chats reciben los cambios con su próximo mensaje, que vuelve a leer la conversación una vez (un fallo de la caché del prompt).',
		changes: 'Los chats reciben los cambios con su próximo mensaje.',
		members: 'Miembros',
		membersHint:
			'Todos aquí ven y escriben en los mismos chats, y pueden pedirle a nolune cualquier cosa.',
		personToAdd: 'Persona que añadir',
		chooseSomeone: 'Elige a quién añadir',
		everyoneIsMember: 'Todos son ya miembros.',
		deleteTitle: 'Eliminar el perfil',
		deleteHint: 'Elimina todos sus chats para todos. La carpeta se mueve a ~/.nolune/trash.',
		deleteButton: 'Eliminar este perfil',
		deleteConfirm: (name: string) => `¿Eliminar «${name}»?`,
		deleteBody: 'Todos sus chats se eliminan para todos. La carpeta se mueve a ~/.nolune/trash.',
		renamed: 'Nombre cambiado.',
		soulSaved: 'Alma guardada.',
		soulRemoved: 'Alma eliminada.',
		added: (name: string) => `${name} añadido.`,
		removed: 'Quitado.',
		memberNote: (note: string) => `Nota: ${note}`,
		noteStarts: (note: string) => `Nota: ${note}, se crea cuando haya algo que apuntar`,
		mayHaveNote: 'Puede que nolune ya tenga una nota sobre esta persona',
		chooseNote: 'Elegir nota',
		changeNote: 'Cambiar nota',
		linked: (name: string, note: string) => `La nota de ${name} ahora es ${note}.`,
		chooser: {
			addTitle: (name: string) => `¿nolune ya conoce a ${name}?`,
			linkTitle: (name: string) => `¿Qué nota es sobre ${name}?`,
			body: (name: string) =>
				`La memoria tiene notas que podrían ser sobre ${name}. Elige la suya, para que nolune sepa que lo que dice es sobre esta persona y añada ahí lo que cuente de sí misma.`,
			linkBody: (name: string) =>
				`Elige la nota sobre ${name}: nolune añadirá ahí lo que esta persona cuente de sí misma.`,
			current: 'ahora',
			alsoCalled: (names: string) => `Otros nombres: ${names}`,
			more: (n: number) => `y ${n} más`,
			newNote: 'Es otra persona: empezar una nota nueva',
			privacy: (name: string) =>
				`${name} podrá leer toda la memoria de este perfil, también esta nota. Comprueba que no tenga nada que deba quedar en secreto para esta persona, como una sorpresa.`,
			add: (name: string) => `Añadir a ${name}`,
			link: 'Vincular'
		}
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

	/** La bienvenida de un perfil nuevo. Aquí habla nolune: «yo» es el asistente. */
	welcome: {
		title: 'Hagamos de este lugar el tuyo.',
		subtitle: 'Unas preguntas rápidas y te sentirás como en casa.',
		go: 'Vamos',
		takesAMinute: 'Lleva cerca de un minuto',
		skipIntro: 'Saltar la intro',
		skipHint: 'Haz clic en cualquier sitio para saltar',
		back: 'Atrás',
		soundsOn: 'Activar sonidos',
		soundsOff: 'Desactivar sonidos',
		progress: (n: number, total: number) => `Paso ${n} de ${total}`,
		model: {
			title: '¿Con qué debo pensar?',
			subtitle: 'Elige qué me mueve. Puedes añadir más luego en Modelos y claves.',
			askAdmin:
				'Necesito un modelo para chatear. Pide a quien instaló nolune que añada uno en Modelos y claves; lo demás ya funciona.',
			choices: {
				'claude-plan': {
					title: 'Plan de Claude',
					about: 'Un plan Pro o Max, con Claude Code en este ordenador'
				},
				'chatgpt-plan': {
					title: 'Plan de ChatGPT',
					about: 'Un plan Plus o Pro, con inicio de sesión de ChatGPT'
				},
				anthropic: { title: 'Clave de API de Anthropic', about: 'Modelos Claude, pago por uso' },
				openai: { title: 'Clave de API de OpenAI', about: 'Modelos GPT, pago por uso' },
				openrouter: {
					title: 'Clave de API de OpenRouter',
					about: 'Claude, GPT, Gemini y más con una sola clave'
				},
				xai: { title: 'Clave de API de xAI', about: 'Modelos Grok, pago por uso' }
			},
			checkingPlan: 'Comprobando el inicio de sesión…',
			pasteKey: (label: string) => `Pega tu clave de ${label}`,
			checkKey: 'Comprobar la clave',
			keyWorks: 'La clave funciona.',
			finishOnAdmin: 'Inicia sesión en {link} y vuelve a comprobar.',
			checkAgain: 'Volver a comprobar',
			pickModel: '¿Con qué modelo deben empezar los chats nuevos?',
			asking: 'Pidiendo los modelos…',
			modelId: 'ID del modelo',
			/** Signing in with ChatGPT right in the step. */
			chatgpt: {
				title: 'Iniciar sesión con ChatGPT',
				about:
					'Inicia sesión en la página de ChatGPT y permite que nolune use tu plan Plus o Pro. Los chats contarán para el uso del plan, como en el propio ChatGPT.',
				newTab: 'La página de inicio de sesión de ChatGPT se abre en una pestaña nueva.',
				waiting: 'En cuanto inicies sesión allí, esta página seguirá sola.',
				starting: 'Preparando la página de inicio de sesión de ChatGPT…',
				tryAgain: 'Intentarlo de nuevo',
				otherDevice: '¿Inicias sesión en otro dispositivo?'
			}
		},
		avatar: {
			title: '¿Quién debo ser?',
			subtitle: (profile: string) => `Elige una cara para ${profile}.`,
			thisOne: 'Este',
			hello: 'Un placer conocerte.'
		},
		memory: {
			title: '¿Ya nos conocemos?',
			subtitle:
				'Si usas ChatGPT, Claude o Gemini, ya sabe cosas de ti. Pregúntale con este prompt y pega aquí su respuesta.',
			copyPrompt: 'Copiar prompt',
			prompt: 'El prompt',
			open: (name: string) => `Abrir ${name}`,
			haveIt: 'Ya la tengo',
			startFresh: 'Empezar de cero',
			pasteTitle: 'Pégala aquí',
			pastePlaceholder: 'La respuesta completa, con el bloque de código',
			sections: {
				instructions: 'Instrucciones',
				identity: 'Sobre ti',
				career: 'Carrera',
				projects: 'Proyectos',
				preferences: 'Preferencias',
				other: 'Otros'
			},
			remember: (n: number) => p(n, { one: `Recordar ${n} cosa`, other: `Recordar ${n} cosas` }),
			rememberThis: 'Recordar esto',
			reading: 'Leyendo…',
			saving: 'Guardando…',
			empty: 'Pega lo que respondió el otro asistente.',
			tooLong:
				'Es demasiado largo para ser los recuerdos de una persona. Pega solo la respuesta al prompt.',
			notAnExport:
				'No parece la respuesta al prompt. Pega la respuesta completa, con su bloque de código.',
			couldNotRead: (problem: string) => `No se pudo leer. ${problem}`,
			nothingFound:
				'No encontré nada sobre ti ahí. Pega la respuesta completa, con su bloque de código.'
		},
		arrival: {
			thingsIKnow: (n: number) => p(n, { one: 'cosa que sé de ti', other: 'cosas que sé de ti' })
		},
		fresh: {
			title: 'Un nuevo comienzo.',
			subtitle: 'Te iré conociendo mientras hablamos.'
		}
	},

	login: {
		welcome: 'Hola de nuevo',
		hint: 'Inicia sesión con la cuenta que te dieron.',
		email: 'Correo electrónico',
		password: 'Contraseña',
		signingIn: 'Iniciando sesión…',
		forgot:
			'¿Olvidaste la contraseña? Pide a un administrador que la restablezca en Personas o que ejecute {command}.',
		tooManyAttempts: 'Demasiados intentos. Espera un minuto y vuelve a intentarlo.',
		wrongPassword: 'Correo o contraseña incorrectos.'
	},

	admin: {
		title: 'Modelos y claves',
		keys: 'Claves de API',
		keysHint:
			'Compartidas por todos los perfiles y guardadas en el archivo de configuración de nolune en este ordenador. Una clave nueva se comprueba con su proveedor antes de guardarla, y se usa enseguida. Para un servidor de modelos propio, como Ollama o LM Studio, añade un proveedor propio.',
		purposes: {
			anthropic: 'Hace funcionar los chats y las automatizaciones con modelos de Claude.',
			openai:
				'Hace funcionar los chats y las automatizaciones con modelos de OpenAI, y crea las imágenes de la página Imágenes y las que dibuja el agente.',
			openrouter:
				'Hace funcionar los chats y las automatizaciones con los modelos que ofrece OpenRouter (Claude, GPT, Gemini, DeepSeek y muchos más), con una sola clave y sus créditos.',
			xai: 'Hace funcionar los chats y las automatizaciones con los modelos Grok de xAI.'
		},
		withoutIt: {
			anthropic:
				'Los chats y las automatizaciones con modelos de Claude dejan de funcionar hasta que se añada una clave nueva.',
			openai:
				'Los chats y las automatizaciones con modelos de OpenAI dejan de funcionar, y nolune no puede crear imágenes, hasta que se añada una clave nueva.',
			openrouter:
				'Los chats y las automatizaciones con modelos de OpenRouter dejan de funcionar hasta que se añada una clave nueva.',
			xai: 'Los chats y las automatizaciones con modelos Grok dejan de funcionar hasta que se añada una clave nueva.'
		},
		savedInNolune: (hint: string | null) =>
			`Guardada en nolune${hint ? `, termina en ${hint}` : ''}`,
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
			`nolune usará en su lugar la clave de la variable de entorno ${variable}.`,
		plan: 'Plan de Claude',
		plans: 'Planes',
		plansHint:
			'Los chats con un preajuste de plan funcionan con la suscripción propia de alguien en lugar de con una clave de API: el plan de Claude a través de Claude Code en este ordenador, que guarda su inicio de sesión; el plan de ChatGPT a través de «Iniciar sesión con ChatGPT», cuyo inicio de sesión guarda nolune en este ordenador. Los límites de los planes suponen el uso normal de una persona, así que deja las automatizaciones intensivas y los subagentes en un preajuste con clave de API.',
		claudePlanAbout: 'Pro o Max, a través de Claude Code.',
		chatgptPlan: 'Plan de ChatGPT',
		chatgptPlanAbout: 'Plus o Pro, con inicio de sesión de ChatGPT.',
		chatgptSignIn: 'Continuar con ChatGPT',
		chatgptContinueAs: (account: string) => `Continuar como ${account}`,
		chatgptAnotherAccount: 'Usar otra cuenta',
		chatgptSignInAgain: 'Volver a iniciar sesión',
		chatgptStarting: 'Iniciando…',
		signOut: 'Cerrar sesión',
		chatgptOpen: 'Abre {link} e inicia sesión, permitiendo que nolune use tu plan de ChatGPT.',
		chatgptSignInPage: 'la página de inicio de sesión de ChatGPT',
		chatgptHere:
			'En un navegador de este ordenador, no hace falta nada más: esta página se actualiza en cuanto inicies sesión.',
		chatgptElsewhere:
			'En otro dispositivo, la página a la que ChatGPT te devuelve no cargará. Copia su dirección (empieza por http://127.0.0.1) y pégala aquí:',
		chatgptFinish: 'Terminar',
		chatgptSignedIn:
			'Sesión iniciada. Los chats con preajustes del plan de ChatGPT ahora usan este plan.',
		chatgptUsing: 'Los chats con preajustes del plan de ChatGPT usan este plan. {link}',
		chatgptManageUsage: 'Gestionar el uso',
		chatgptNobody: 'Nadie ha iniciado sesión con ChatGPT.',
		chatgptSignedOutLocally:
			'Sesión cerrada aquí, pero no se pudo avisar a OpenAI: para asegurarte, desconecta nolune en los ajustes de ChatGPT.',
		chatgptSignOutTitle: '¿Cerrar la sesión de ChatGPT?',
		chatgptSignOutBody:
			'Los chats con preajustes del plan de ChatGPT dejarán de funcionar hasta que alguien vuelva a iniciar sesión.',
		signedOut: 'Sesión cerrada.',
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
			'Los preajustes se comparten entre todos los perfiles. Los chats nuevos empiezan con el predeterminado. Editar o quitar un preajuste no cambia los chats que ya lo usan.',
		presetDetails: (provider: string, model: string, context: string, overridden: boolean) =>
			`${provider} / ${model} · contexto ${context}${overridden ? ' (personalizado)' : ''}`,
		makeDefault: 'Hacer predeterminado',
		editPreset: (name: string) => `Editar ${name}`,
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
				openrouter:
					'Auto usa la ventana que OpenRouter indica para el modelo y su proveedor principal.',
				xai: 'Auto usa la ventana que xAI indica para el modelo.',
				'custom-openai':
					'Auto usa la ventana que el servidor indica para el modelo, si la indica (vLLM lo hace); si no, queda como desconocida.',
				'custom-anthropic':
					'Auto usa la ventana que el servidor indica para el modelo, si la indica (vLLM lo hace); si no, queda como desconocida.',
				'claude-plan': 'Claude Code no la indica: Auto solo conoce sus modelos de 1M de contexto.',
				'chatgpt-plan':
					'Auto usa la ventana que ChatGPT indica para el modelo, si la indica; si no, queda como desconocida.'
			},
			onPlan: 'Funciona con el plan Pro o Max con el que inició sesión Claude Code.',
			noClaudeCode: 'Claude Code aún no está instalado: consulta «Plan de Claude», arriba.',
			onChatGptPlan:
				'Funciona con el plan de ChatGPT de quien haya iniciado sesión con ChatGPT, arriba.',
			noChatGpt: 'Aún nadie ha iniciado sesión con ChatGPT: consulta «Plan de ChatGPT», arriba.',
			onKey: (provider: string) => `Funciona con la clave de API de ${provider}.`,
			noKey: (provider: string) =>
				`Todavía no hay clave de API de ${provider}: añade una en «Claves de API», arriba.`,
			onCustom: (api: string, url: string) =>
				`Funciona en ${url}, a través de su API de ${api}. El modelo debe poder llamar herramientas; las imágenes y los PDF le llegan como rutas.`,
			customGone: 'Este proveedor propio se quitó: elige otro.',
			asking: (source: string) => `Preguntando a ${source} por sus modelos…`,
			listProblem: (problem: string) => `${problem} Aun así puedes escribir un ID.`,
			couldNotList: (status: number) => `nolune no pudo obtener los modelos (${status}).`,
			unreachable: 'No se pudo contactar con nolune.',
			checkingClaude: 'Comprobando Claude Code…',
			checkingModel: 'Comprobando el modelo…',
			saving: 'Guardando…',
			pick: 'Elige un modelo',
			search: 'Busca o escribe un ID de modelo',
			use: 'Usar {model}',
			typeId: 'Escribe arriba el ID del modelo.',
			added: (name: string) => `${name} añadido.`,
			saved: (name: string) => `${name} guardado. Los chats que ya lo usan siguen como estaban.`,
			invalidContext: 'La ventana de contexto debe ser un número de tokens, como 272k o 272000.'
		},
		savedWorks: 'Guardada. Funciona.',
		savedWarning: (warning: string) => `Guardada. ${warning}`,
		claudeNoAnswer: 'Claude Code no respondió.',
		removed: 'Quitada.',
		newDefault: (name: string) => `Los chats nuevos empiezan ahora con ${name}.`,
		presetRemoved: 'Quitado. Las conversaciones existentes siguen funcionando.',
		/** Custom providers: model servers of the family's own, listed with the API keys. */
		customProviders: {
			add: 'Añadir un proveedor propio',
			addTitle: 'Añadir un proveedor propio',
			addHint:
				'Un servidor de modelos propio, como Ollama, LM Studio u oMLX en este ordenador, o vLLM en una máquina con GPU. Aparece con su nombre en Añadir un modelo.',
			about: (api: string) => `Tu propio servidor de modelos, a través de su API de ${api}.`,
			apis: { openai: 'OpenAI', anthropic: 'Anthropic' },
			change: 'Cambiar',
			name: 'Nombre',
			namePlaceholder: 'Ollama',
			api: 'API',
			apiHints: {
				openai:
					'La Responses API de OpenAI: Ollama, LM Studio, vLLM, LiteLLM. La búsqueda en la memoria puede usar sus embeddings.',
				anthropic:
					'La Messages API de Anthropic: Ollama, LM Studio, el servidor de llama.cpp, oMLX.'
			},
			address: 'Dirección',
			addressHint:
				'Ollama escucha en http://localhost:11434 y LM Studio en http://localhost:1234. nolune le pide sus modelos para comprobarlo.',
			key: 'Clave',
			keyOptional: '(si la necesita)',
			keyKept: 'Guardada. Déjala vacía para conservarla.',
			withKey: (hint: string | null) => (hint ? `clave termina en ${hint}` : 'con clave'),
			noKey: 'sin clave',
			checking: 'Comprobando…',
			works: (n: number) => `Guardado. Ofrece ${n} ${p(n, { one: 'modelo', other: 'modelos' })}.`,
			unchecked: (problem: string) => `Guardado sin comprobarlo: ${problem}`,
			needName: 'Ponle un nombre, como Ollama o GPU box.',
			needAddress: 'Indica su dirección, empezando por http:// o https://.',
			removeTitle: (name: string) => `¿Quitar ${name}?`,
			removeBody:
				'Los chats y automatizaciones con sus modelos dejan de funcionar hasta que se pasen a otro modelo.',
			usedBy: (names: string) => `Estos preajustes funcionan en él: ${names}.`
		},
		/** Memory search by meaning: where its embeddings come from. */
		embeddings: {
			title: 'Búsqueda en la memoria',
			hint: 'nolune también encuentra datos de la memoria por lo que significan, no solo por sus palabras, y entre idiomas. Para ello, cada dato se envía una vez adonde se calculan los embeddings, y cada mensaje al enviarse; un servidor en este ordenador los mantiene aquí. Compartido por todos los perfiles.',
			name: 'Búsqueda por significado',
			using: (source: string) => `Usa ${source}.`,
			off: 'Desactivada: la memoria se busca solo por palabras.',
			noKeys:
				'Aún no hay clave de OpenAI ni de OpenRouter, así que la memoria se busca solo por palabras.',
			noKey: (provider: string) =>
				`Aún no hay clave de ${provider}, así que la memoria se busca solo por palabras. Añade una en Claves de API, arriba.`,
			customGone: 'Este proveedor propio se quitó, así que la memoria se busca solo por palabras.',
			change: 'Cambiar',
			source: 'Embeddings de',
			modes: {
				auto: 'Auto',
				openai: 'OpenAI',
				openrouter: 'OpenRouter',
				off: 'Desactivada'
			},
			autoNote:
				'text-embedding-3-small de OpenAI con la clave de OpenAI; si no, el mismo modelo a través de OpenRouter.',
			withKey: (provider: string) => `Con la clave de API de ${provider}.`,
			customNote: (url: string) => `Un modelo que ofrece en ${url}, a través de su API de OpenAI.`,
			offNote: 'La memoria se busca solo por palabras, y ningún dato se envía a ninguna parte.',
			model: 'Modelo',
			customModel: 'Su nombre allí, como nomic-embed-text',
			checking: 'Comprobando…',
			works: 'Guardado. Funciona; los datos se procesan en segundo plano.',
			wordsOnly: 'Guardado. La memoria se busca solo por palabras.',
			noAnswer: (problem: string) => `Guardado, pero no hubo respuesta: ${problem}`,
			needModel: 'Indica el modelo que debe usar.'
		},
		commands: {
			title: 'Comandos',
			hint: 'El agente trabaja ejecutando comandos en este ordenador, con el acceso de esta cuenta a sus archivos y apps. En modo automático, un modelo revisa cada comando antes de que se ejecute y detiene lo que podría hacer daño sin que nadie lo haya pedido. Todos los chats lo siguen, salvo que alguien cambie uno con el escudo de su cuadro de mensaje; allí solo los administradores pueden desactivar la revisión.',
			modes: {
				auto: 'Modo automático',
				unrestricted: 'Sin restricciones'
			},
			choices: {
				auto: 'Automático (recomendado)',
				unrestricted: 'Sin restricciones (no recomendado)'
			},
			checkedByChat: 'Cada comando lo revisa el propio modelo del chat.',
			checkedBy: (preset: string) => `Cada comando lo revisa ${preset}.`,
			presetGone:
				'Se eliminó el modelo elegido para las revisiones, así que cada chat revisa sus comandos con su propio modelo.',
			unrestrictedStatus: 'Los comandos se ejecutan sin revisión.',
			change: 'Cambiar',
			mode: 'Modo',
			autoNote:
				'Los comandos que solo miran se ejecutan al momento. Los demás se revisan antes; uno bloqueado no se ejecuta, y nolune explica qué quería hacer para que alguien pueda dar el visto bueno.',
			unrestrictedNote:
				'Cada comando se ejecuta tal como lo escribió el agente, sin nada que frene un error, ni una página web o un correo que lo convenza de algo. Solo para quien lo vigila de cerca.',
			checker: 'Lo revisa',
			chatModel: 'El propio modelo del chat',
			checkerNote:
				'Un modelo rápido y capaz mantiene ágiles los chats: cada comando que hace algo más que mirar le cuesta una breve consulta.',
			saved: 'Guardado. Se aplica desde el próximo comando.'
		}
	},

	people: {
		title: 'Personas',
		hint: 'Todos los que pueden iniciar sesión en nolune. Quien tiene una cuenta puede pedirle al agente cualquier cosa en este ordenador, así que añade solo a personas de confianza. Para compartir un perfil con alguien, añádelo en Personas y perfil de ese perfil.',
		accounts: 'Cuentas',
		you: 'Tú',
		admin: 'Admin',
		resetPassword: 'Restablecer contraseña',
		makeAdmin: 'Hacer administrador',
		removeAdmin: 'Quitar administrador',
		resetTitle: (name: string) => `¿Restablecer la contraseña de ${name}?`,
		resetBody:
			'nolune crea una contraseña nueva y la muestra una vez. La anterior deja de funcionar; los dispositivos que ya iniciaron sesión siguen conectados.',
		reset: 'Restablecer',
		newPassword: (name: string) =>
			`La nueva contraseña de ${name}. Solo se muestra esta vez: envíasela.`,
		nowAdmin: (name: string) => `${name} ahora es administrador.`,
		noLongerAdmin: (name: string) => `${name} ya no es administrador.`,
		removeTitle: (name: string) => `¿Quitar a ${name}?`,
		removeBody:
			'Se cierra su sesión y ya no puede volver a entrar. Sus mensajes quedan en los chats compartidos, y un perfil en el que solo está esa persona queda en este ordenador sin nadie. No se puede deshacer.',
		removed: (name: string) => `Se quitó a ${name}.`,
		notYourself: 'No puedes hacerle eso a tu propia cuenta. Pídeselo a otro administrador.',
		gone: 'Esa cuenta ya no existe.',
		add: 'Añadir una persona',
		addHint:
			'nolune crea su contraseña y la muestra una vez, para que se la envíes con la dirección. O envía un enlace de invitación, abajo, y elige la suya.',
		name: 'Nombre',
		namePlaceholder: 'Anna',
		email: 'Correo',
		emailPlaceholder: 'anna@example.com',
		makeThemAdmin: 'Administrador',
		adminHint: 'Los administradores gestionan modelos, claves y personas.',
		adding: 'Añadiendo…',
		added: (name: string) =>
			`Se añadió a ${name}. Envíale la dirección y esta contraseña; solo se muestra esta vez.`,
		address: 'Dirección',
		password: 'Contraseña',
		copyAddress: 'Copiar la dirección',
		copyPassword: 'Copiar la contraseña',
		invites: 'Enlaces de invitación',
		invitesHint: (days: number) =>
			`Con un enlace, alguien crea su propia cuenta: elige su nombre, correo y contraseña, así que no hay que enviar ninguna contraseña. Cada enlace sirve una vez, durante ${days} días, y crea una cuenta normal.`,
		newInvite: 'Crear enlace de invitación',
		inviteFor: 'Para quién',
		optional: '(opcional)',
		inviteForPlaceholder: 'Abuela',
		inviteForHint: 'Su nombre aparece ya en el formulario. Puede cambiarlo.',
		create: 'Crear',
		creating: 'Creando…',
		inviteCreated: (name: string | null) =>
			`${name ? `Envía este enlace a ${name}` : 'Envía este enlace a esa persona'}. Solo se muestra esta vez.`,
		copyLink: 'Copiar el enlace',
		noInvites: 'No hay enlaces de invitación pendientes.',
		inviteTitle: (name: string | null) => (name ? `Para ${name}` : 'Enlace de invitación'),
		inviteDetails: (by: string | null, until: string) =>
			`${by ? `Creado por ${by}` : 'Creado en este ordenador'} · válido hasta el ${until}`,
		revoke: 'Retirar',
		revoked: 'Retirado. El enlace ya no funciona.'
	},

	invite: {
		title: 'Únete a nolune',
		from: (name: string) => `${name} te ha invitado.`,
		hint: 'Crea tu cuenta: iniciarás sesión con este correo y esta contraseña.',
		name: 'Tu nombre',
		nameHint: 'Todos en tus perfiles lo ven, y nolune lo lee con cada mensaje que envías.',
		email: 'Correo electrónico',
		password: 'Contraseña',
		passwordAgain: 'Repite la contraseña',
		passwordHint: (min: number) =>
			`Al menos ${min} caracteres, con 3 de: minúsculas, mayúsculas, dígitos y símbolos. O 20 caracteres o más de lo que sea.`,
		mismatch: 'Las contraseñas no coinciden.',
		create: 'Crear cuenta',
		creating: 'Creando tu cuenta…',
		until: (date: string) => `Este enlace es válido hasta el ${date}.`,
		goneTitle: 'Este enlace ya no funciona',
		goneBody: 'Ya se usó, caducó o se retiró. Pide uno nuevo a quien te lo envió.',
		signIn: 'Iniciar sesión',
		signedInTitle: (name: string) => `Has iniciado sesión como ${name}`,
		signedInBody:
			'Este enlace sirve para crear una cuenta nueva. Envíalo a la persona a la que va dirigido, o cierra sesión antes para usarlo.',
		open: 'Abrir nolune'
	}
};
