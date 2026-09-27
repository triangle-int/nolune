import { isMonthSpan, type Schedule } from '@btw/core/schedule';
import { listOf, plural } from '../plural';
import type { Messages } from './en';

const p = plural('ru');
const list = listOf('ru');

// Weekdays by JavaScript's numbers (0 is Sunday), in the cases the schedules need.
/** "с понедельника" */
const WEEKDAY_FROM = [
	'воскресенья',
	'понедельника',
	'вторника',
	'среды',
	'четверга',
	'пятницы',
	'субботы'
];
/** "по четверг" */
const WEEKDAY_TO = [
	'воскресенье',
	'понедельник',
	'вторник',
	'среду',
	'четверг',
	'пятницу',
	'субботу'
];
/** "по понедельникам" */
const WEEKDAYS_ON = [
	'воскресеньям',
	'понедельникам',
	'вторникам',
	'средам',
	'четвергам',
	'пятницам',
	'субботам'
];
// Months, 1 to 12.
/** "5 марта", "с января" */
const MONTH_OF = [
	'',
	'января',
	'февраля',
	'марта',
	'апреля',
	'мая',
	'июня',
	'июля',
	'августа',
	'сентября',
	'октября',
	'ноября',
	'декабря'
];
/** "по март" */
const MONTH_TO = [
	'',
	'январь',
	'февраль',
	'март',
	'апрель',
	'май',
	'июнь',
	'июль',
	'август',
	'сентябрь',
	'октябрь',
	'ноябрь',
	'декабрь'
];
/** "в марте" */
const MONTH_IN = [
	'',
	'январе',
	'феврале',
	'марте',
	'апреле',
	'мае',
	'июне',
	'июле',
	'августе',
	'сентябре',
	'октябре',
	'ноябре',
	'декабре'
];

/** Which days, as a sentence start ("По будням") and as a tail ("по будням"). */
function days({ days, months }: Schedule): { every: string; on: string } {
	if (days.kind === 'monthDays') {
		const which = `${list(days.days.map(String))} числа`;
		const text = months
			? `${which} в ${list(months.map((m) => MONTH_IN[m]))}`
			: `${which} каждого месяца`;
		return { every: text, on: text };
	}
	if (days.kind === 'date') {
		const date = `${days.day} ${MONTH_OF[days.month]}`;
		return { every: `Каждый год ${date}`, on: `каждый год ${date}` };
	}
	let words: { every: string; on: string };
	if (days.kind === 'daily') words = { every: 'Каждый день', on: '' };
	else if (days.kind === 'weekdays') words = { every: 'По будням', on: 'по будням' };
	else if (days.kind === 'weekends') words = { every: 'По выходным', on: 'по выходным' };
	else if (days.kind === 'weekdayRange') {
		const range = `${WEEKDAY_FROM[days.from]} по ${WEEKDAY_TO[days.to]}`;
		words = { every: `С ${range}`, on: `с ${range}` };
	} else {
		const which = list(days.days.map((d) => WEEKDAYS_ON[d]));
		words = { every: `По ${which}`, on: `по ${which}` };
	}
	if (!months) return words;
	const inMonths = isMonthSpan(months)
		? `с ${MONTH_OF[months[0]]} по ${MONTH_TO[months[months.length - 1]]}`
		: `в ${list(months.map((m) => MONTH_IN[m]))}`;
	return { every: `${words.every} ${inMonths}`, on: `${words.on || 'каждый день'} ${inMonths}` };
}

function describe(schedule: Schedule): string {
	const when = days(schedule);
	const tail = when.on ? ` ${when.on}` : '';
	const { times } = schedule;
	switch (times.kind) {
		case 'minutes': {
			const n = times.step;
			const every =
				n === 1
					? 'Каждую минуту'
					: `Каждые ${n} ${p(n, { one: 'минуту', few: 'минуты', many: 'минут', other: 'минуты' })}`;
			if (!times.between) return every + tail;
			return `${every} с ${times.between.from} до ${times.between.to}${tail}`;
		}
		case 'hours': {
			const n = times.step;
			const past = times.minute ? ` в :${String(times.minute).padStart(2, '0')}` : '';
			const every =
				n === 1
					? 'Каждый час'
					: `Каждые ${n} ${p(n, { one: 'час', few: 'часа', many: 'часов', other: 'часа' })}`;
			return `${every}${past}${tail}`;
		}
		case 'hourRange':
			return `Каждый час с ${times.from} до ${times.to}${tail}`;
		case 'at':
			return `${when.every} в ${list(times.times)}`;
	}
}

const facts = (n: number) =>
	p(n, { one: `${n} факт`, few: `${n} факта`, many: `${n} фактов`, other: `${n} факта` });

export const ru: Messages = {
	common: {
		add: 'Добавить',
		cancel: 'Отмена',
		close: 'Закрыть',
		continue: 'Продолжить',
		copied: 'Скопировано',
		copy: 'Копировать',
		create: 'Создать',
		default: 'По умолчанию',
		delete: 'Удалить',
		done: 'Готово',
		edit: 'Изменить',
		more: 'Ещё',
		newChat: 'Новый чат',
		remove: 'Убрать',
		rename: 'Переименовать',
		save: 'Сохранить',
		stop: 'Остановить',
		tryAgain: 'Повторить',
		checking: 'Проверка…',
		starting: 'Запуск…',
		uploading: 'Загрузка…',
		removeFile: (name: string) => `Убрать ${name}`,
		characters: (count: number, max: number) => `${count} / ${max} символов`
	},

	errors: {
		requestFailed: (status: number) => `Запрос не удался (${status})`,
		uploadFailed: (status: number | null) =>
			`Не удалось загрузить (${status === null ? 'нет соединения' : status})`,
		couldNotSave: 'Не удалось сохранить.',
		somethingWentWrong: 'Что-то пошло не так.',
		thatDidntWork: 'Не получилось.',
		tooManyFiles: (max: number) => `Не больше ${max} файлов в одном сообщении`,
		tooLarge: (mb: number) => `Больше ${mb} МБ`,
		notSignedIn: 'Вы не вошли',
		adminsOnly: 'Только для администраторов',
		profileNotFound: 'Профиль не найден',
		conversationNotFound: 'Чат не найден',
		folderNotFound: 'Папка не найдена',
		automationNotFound: 'Автоматизация не найдена',
		messageEmpty: 'Сообщение пустое',
		noFile: 'Нет файла'
	},

	time: {
		justNow: 'только что',
		seconds: (s: number) => `${s} с`,
		minutesSeconds: (m: number, s: number) => `${m} мин ${s} с`,
		hoursMinutes: (h: number, m: number) => `${h} ч ${m} мин`,
		minutesAgo: (n: number) => `${n} мин назад`,
		hoursAgo: (n: number) => `${n} ч назад`
	},

	units: { bytes: 'Б', kilobytes: 'КБ', megabytes: 'МБ' },

	settings: {
		title: 'Настройки',
		description: 'Как btw выглядит на этом устройстве.',
		theme: 'Тема',
		system: 'Как в системе',
		light: 'Светлая',
		dark: 'Тёмная',
		language: 'Язык',
		languageAuto: 'Как в браузере',
		languageHint: 'Для меню и кнопок. btw отвечает на том языке, на котором вы пишете.',
		technical: 'Технические подробности',
		technicalHint:
			'Показывать точные команды, которые запускает btw, расход токенов и кеширование промпта.',
		expandSteps: 'Всегда показывать шаги',
		expandStepsHint:
			'Раскрывать под каждым ответом список того, что сделал btw, а не сворачивать его.',
		sounds: 'Звуки',
		soundsHint: 'Тихие звуки там, где btw двигается сам, например при знакомстве с новым профилем.',
		logOut: 'Выйти',
		deviceOnly: 'Эти настройки сохраняются только на этом устройстве.'
	},

	userMenu: {
		account: 'Аккаунт',
		settings: 'Настройки',
		allProfiles: 'Все профили',
		modelsAndKeys: 'Модели и ключи',
		logOut: 'Выйти'
	},

	header: {
		openMenu: 'Открыть меню'
	},

	sidebar: {
		label: 'Боковая панель',
		description: 'Чаты, папки и страницы этого профиля.',
		profiles: 'Профили',
		newProfile: 'Новый профиль',
		openSidebar: 'Открыть боковую панель',
		closeSidebar: 'Закрыть боковую панель',
		searchChats: 'Поиск по чатам',
		images: 'Картинки',
		automations: 'Автоматизации',
		memory: 'Память',
		skills: 'Навыки',
		people: 'Люди и профиль',
		folders: 'Папки',
		newFolder: 'Новая папка',
		chats: 'Чаты',
		working: ', работает',
		showChats: (folder: string) => `Показать чаты в папке ${folder}`,
		hideChats: (folder: string) => `Скрыть чаты в папке ${folder}`,
		emptyFolder: 'Перетащите сюда чаты или начните новый на странице папки.',
		dropToTakeOut: 'Отпустите здесь, чтобы вынуть чат из папки.',
		noChats: 'Здесь появятся ваши чаты.',
		searchDescription: 'Найти чат по названию',
		searchPlaceholder: 'Поиск по чатам…',
		noResults: 'Чаты не найдены.',
		deleteChatBody: 'Чат {name} удалится у всех в профиле {profile}.'
	},

	notifications: {
		title: 'Уведомления',
		unseen: (n: number) => `Уведомления, новых: ${n}`,
		clearAll: 'Очистить все',
		new: 'Новое',
		showLess: 'Свернуть',
		showAll: 'Показать полностью',
		dismiss: 'Скрыть',
		openChat: 'Открыть чат',
		continueInChat: 'Продолжить в чате',
		empty:
			'Пока ничего нет. Попросите btw о чём-нибудь напомнить или что-то проверять каждый день, и всё, что он найдёт, появится здесь.'
	},

	chat: {
		options: 'Действия с чатом',
		placeholder: 'Спросите btw',
		placeholderRunning: 'Добавьте что-нибудь, пока btw работает…',
		disclaimer: 'btw может ошибаться и может менять файлы на этом компьютере.',
		reconnecting: 'Переподключение…',
		empty: 'Попросите о чём-нибудь, чтобы начать.',
		readsAfterStep: 'btw прочитает это после текущего шага',
		working: 'btw работает',
		thinking: 'Думает',
		writing: 'Пишет',
		cutOff: 'Ответ оборвался, потому что стал слишком длинным.',
		refused: 'btw отказался продолжать этот запрос.',
		automation: (title: string) => `Автоматизация · ${title}`,
		fromBtw: (to: string) => `От btw для ${to}`,
		finishedInBackground: (title: string) => `Завершено в фоне · ${title}`,
		inBackground: 'Работает в фоне',
		aCommand: 'Команда',
		subagent: (name: string) => `Субагент ${name}`,
		stopping: 'останавливается',
		errorTitle: 'Что-то пошло не так, пока btw отвечал.',
		unanswered: 'btw ещё не ответил на это.',
		scrollToBottom: 'К последнему сообщению',
		subagentBanner:
			'Субагент {name}: его запустил btw из чата {parent}, и туда он присылает результаты.',
		subagentOnly: (name: string) => `Сюда пишет только агент, который запустил ${name}.`,
		subagentRetry: 'Субагента запускает только агент, который его создал.',
		hiddenBanner:
			'Фоновый запуск автоматизации. Отправьте сообщение, чтобы оставить его среди своих чатов.',
		aChat: 'без названия',
		deleteTitle: 'Удалить чат?',
		deleteBody: 'Чат {name} удалится у всех в профиле.',
		renameTitle: 'Переименовать чат',
		chatName: 'Название чата',
		couldNotRename: 'Не удалось переименовать чат.',
		switchTitle: (model: string) => `Переключиться на ${model}?`,
		anotherModel: 'другую модель',
		effortTitle: (level: string) => `Изменить уровень размышлений на «${level}»?`,
		switchCache: (model: boolean, missTokens: string | null) =>
			`btw хранит этот чат в кеше, чтобы каждый ответ оплачивал только новое. ${model ? 'Другая модель' : 'Другой уровень размышлений'} не может его использовать: следующий ответ заново прочитает весь чат, а это дольше и дороже${missTokens ? ` (промах кеша промпта примерно на ${missTokens} токенов)` : ''}.`,
		switchFiles:
			'Некоторые картинки и PDF из этого чата не перейдут к другому провайдеру: новая модель получит пути к их файлам и сможет посмотреть их снова.',
		switch: 'Переключить',
		change: 'Изменить',
		models: (models: string[]) => models.join(', затем '),
		usage: 'Расход',
		tokensInOut: (input: string, output: string) =>
			`Токенов: ${input} на входе, ${output} на выходе`,
		cache: 'Кеш',
		lastReply: 'Последний ответ',
		wholeConversation: 'Весь чат',
		cacheSummary: (label: string, rate: string, read: string, written: string, uncached: string) =>
			`${label}: ${rate} из кеша (прочитано ${read}, записано ${written}, без кеша ${uncached})`,
		cacheMiss: (tokens: string) => `Промах кеша · ${tokens} токенов обработано заново`,
		cacheExpired: (ttl: '5m' | '1h') =>
			`С предыдущего шага прошло больше ${ttl === '5m' ? '5 минут' : 'часа'}, поэтому кеш чата истёк и его обработали заново (медленнее и дороже).`,
		cacheBroken:
			'Контекст, который должен был взяться из кеша, обработан заново (медленнее и дороже). Так бывает один раз после смены модели или уровня размышлений, переноса чата в другую папку или изменений в его папке.',
		contextChip: (used: string, window: string, rate: string) =>
			`${used} / ${window} · ${rate} из кеша`,
		contextUsed: (used: string, window: string) =>
			`Контекст последнего ответа: ${used} из ${window}`,
		contextMenu: (used: string, window: string) => `Контекст ${used} / ${window}`,
		cachedOverall: (rate: string) => `${rate} из кеша за весь чат`
	},

	steps: {
		thinking: 'Думает',
		thinkingDots: 'Думает…',
		running: (command: string) => `Выполняет ${command}`,
		runningACommand: 'Выполняет команду',
		ranACommand: 'Выполнил команду',
		gettingReady: 'Готовится…',
		preparing: 'Готовит команду…',
		stopped: 'Остановлено',
		thoughtFor: (duration: string) => `Думал ${duration}`,
		thoughtForAMoment: 'Думал недолго',
		workedFor: (duration: string) => `Работал ${duration}`,
		workedForAMoment: 'Работал недолго',
		ranCommands: (n: number) =>
			p(n, {
				one: `Выполнил ${n} команду`,
				few: `Выполнил ${n} команды`,
				many: `Выполнил ${n} команд`,
				other: `Выполнил ${n} команды`
			}),
		failedCount: (n: number) => `с ошибкой: ${n}`,
		failed: 'ошибка',
		didntWork: 'не получилось',
		statusStopped: 'остановлена',
		statusNotRun: 'не запускалась',
		command: 'Команда',
		theCommand: 'Команда, которую выполнил btw',
		inFolder: (cwd: string) => `в ${cwd}`,
		noOutputYet: 'Пока нет вывода…',
		noOutput: '(нет вывода)'
	},

	composer: {
		placeholder: 'Спросите что угодно',
		message: 'Сообщение',
		attach: 'Прикрепить файлы',
		send: 'Отправить'
	},

	model: {
		model: 'Модель',
		reasoning: 'Размышления',
		efforts: {
			low: { label: 'Низкий', hint: 'Самые быстрые ответы' },
			medium: { label: 'Средний', hint: 'Подходит для большинства задач' },
			high: { label: 'Высокий', hint: 'Дольше думает над сложными задачами' },
			xhigh: { label: 'Очень высокий', hint: 'Не торопится' },
			max: { label: 'Максимальный', hint: 'Медленнее всего, для самых трудных задач' }
		}
	},

	newChat: {
		greeting: (name: string) => `Чем помочь, ${name}?`,
		greetingNoName: 'Чем помочь?',
		noModels:
			'Модели ещё не настроены. Администратор может добавить модель на странице «Модели и ключи» или командой {command}.',
		couldNotStart: 'Не удалось начать чат.',
		suggestions: {
			reminder: { label: 'Напоминание', text: 'Напомни мне завтра в 9:00 ' },
			weather: {
				label: 'Погода каждый день',
				text: 'Каждый будний день в 7:30 проверяй погоду и говори нам, нужен ли зонт.'
			},
			file: { label: 'Найти файл', text: 'Найди на этом компьютере файл под названием ' },
			space: {
				label: 'Свободное место',
				text: 'Сколько свободного места осталось на диске этого компьютера?'
			}
		},
		pickModel: 'Выберите модель.',
		pickEffort: 'Выберите уровень размышлений.',
		folderGone: 'Эту папку удалили. Выберите другую.'
	},

	folders: {
		inFolder: (name: string) => `В папке ${name}`,
		startInFolder: 'Начать в папке',
		startOutside: 'Начать вне папки',
		noFolder: 'Без папки',
		newFolderDots: 'Новая папка…',
		moveTo: 'Переместить в папку',
		moveTechnical:
			'После перемещения системный промпт собирается заново, поэтому следующий ответ один раз перечитает весь чат (промах кеша промпта).',
		move: 'После перемещения следующий ответ займёт чуть больше времени.',
		newTitle: 'Новая папка',
		newDescription:
			'Держите связанные чаты вместе. Каждый чат в папке получает её инструкции и файлы.',
		namePlaceholder: 'Поездка в Японию',
		name: 'Название папки',
		create: 'Создать папку',
		renameTitle: 'Переименовать папку',
		couldNotRename: 'Не удалось переименовать папку.',
		deleteTitle: 'Удалить папку?',
		deleteBody:
			'Папка {name} удалится у всех в профиле. Её чаты вернутся в общий список, но без её инструкций и файлов. Файлы переместятся в ~/.btw-agent/trash.',
		options: 'Действия с папкой',
		newChatIn: (name: string) => `Новый чат в папке ${name}`,
		instructions: 'Инструкции',
		instructionsHint: 'Что btw должен знать или делать в каждом чате этой папки.',
		instructionsPlaceholder:
			'Планируем две недели в Японии в апреле с детьми (им 7 и 10). Без спешки, бюджет до 600 000 ¥.',
		changesTechnical:
			'Чаты в папке получат изменения со следующим сообщением и один раз перечитают весь чат (промах кеша промпта).',
		changes: 'Чаты в папке получат изменения со следующим сообщением.',
		files: 'Файлы',
		addFiles: 'Добавить файлы',
		filesHint:
			'Картинки, документы, что угодно. btw знает, где они лежат, и открывает их, когда они нужны.',
		noFiles: 'Файлов пока нет',
		savedIn: (dir: string) => `Хранятся в ${dir}`,
		couldNotAdd: 'Не удалось добавить файлы.',
		couldNotAddOffline: 'Не удалось добавить файлы. Проверьте соединение и попробуйте ещё раз.',
		chats: 'Чаты',
		noChats:
			'Здесь появятся чаты, которые вы начнёте в этой папке. Чаты можно и перетащить на папку в боковой панели.'
	},

	attachments: {
		open: (name: string) => `Открыть ${name}`,
		download: 'Скачать',
		onlyPath: (note: string) => `btw получил только путь к файлу: ${note}`
	},

	markdown: {
		text: 'текст',
		picture: 'Картинка',
		notAvailable: 'Недоступно'
	},

	images: {
		title: 'Картинки',
		shapes: {
			square: 'Квадрат',
			portrait: 'Вертикально',
			landscape: 'Горизонтально',
			auto: 'Авто'
		},
		shape: 'Формат',
		groups: 'Группы шаблонов',
		noTemplates: 'Шаблонов пока нет.',
		cantMakeYet: 'btw пока не может рисовать картинки.',
		needsKeyAdmin: 'Нужен API-ключ {provider}. {link}.',
		addKeyLink: 'Добавьте его в разделе «Модели и ключи»',
		needsKey: (provider: string) =>
			`Нужен API-ключ ${provider}. Попросите администратора добавить его.`,
		adminSetsUp: 'Это настраивает администратор на компьютере, где работает btw.',
		describe: 'Опишите картинку',
		describeHint: 'btw нарисует картинку в новом чате, где её можно попросить изменить.',
		onlyPictures: 'Сюда можно добавить только картинки.',
		atMostPictures: (n: number) => `Максимум картинок: ${n}.`,
		changePicture: (label: string | null) => (label ? `Заменить: ${label}` : 'Заменить картинку'),
		addPicture: 'Добавить картинку',
		draw: 'Нарисовать',
		choosePhotoOfDrawing: 'Выбрать фото рисунка',
		usePhotoOfDrawing: 'Взять фото рисунка',
		startDrawing: 'Начать рисовать',
		takePhoto: 'Сделать фото',
		choosePhoto: 'Выбрать фото',
		tryIt: 'Попробовать',
		yourOwn: (label: string) => `свой вариант: ${label.toLowerCase()}`,
		pickFromList: (label: string) => `Выбрать из списка: ${label.toLowerCase()}`,
		custom: 'Свой вариант…',
		addAnything: 'Добавьте что-нибудь ещё…',
		uploadingPicture: 'Картинка загружается…',
		addDrawingFirst: 'Сначала добавьте рисунок.',
		addPhotoFirst: 'Сначала добавьте фото.',
		generate: 'Создать',
		noModel: 'Модель для чатов ещё не настроена.',
		notPicture: (name: string) => `${name} не картинка.`,
		templateGone: 'Этого шаблона больше нет.',
		describeFirst: 'Сначала опишите картинку.'
	},

	drawing: {
		title: 'Рисунок',
		tool: 'Инструмент',
		pen: 'Карандаш',
		eraser: 'Ластик',
		undo: 'Отменить',
		penSize: 'Толщина линии',
		color: (color: string) => `Цвет ${color}`,
		use: 'Использовать рисунок'
	},

	emoji: {
		value: (label: string, value: string) => `${label}: ${value || 'не выбрано'}`,
		pickUpTo: (n: number) => `Можно выбрать до ${n}`,
		removeLast: 'Убрать последний эмодзи'
	},

	memory: {
		title: 'Память',
		intro: (profile: string) =>
			`Что btw помнит для профиля ${profile}. Это видят все его участники. Каждый чат начинается с закреплённой заметки Core; остальные btw читает, когда они нужны в чате, и по ходу дела записывает то, что узнаёт. Чтобы добавить что-то, просто скажите ему в чате, например: «Запомни, что у Анны аллергия на орехи».`,
		total: (memories: number, topics: number) =>
			`${p(memories, { one: 'факт', few: 'факта', many: 'фактов', other: 'факта' })} ${p(topics, { one: `в ${topics} теме`, other: `в ${topics} темах` })}`,
		newThisWeek: (n: number) =>
			p(n, { one: `${n} новый за неделю`, other: `${n} новых за неделю` }),
		updated: (ago: string) => `обновлено ${ago}`,
		memories: facts,
		pinned: 'закреплена, есть в каждом новом чате',
		edit: (topic: string) => `Изменить: ${topic}`,
		forget: (topic: string) => `Забыть: ${topic}`,
		note: (topic: string) => `Заметка ${topic}`,
		corePlaceholder:
			'Например:\n- Анна и Борис родители, Мие 7 лет\n- Дома говорим по-русски\n- У Мии аллергия на орехи',
		coreEmpty:
			'Пока пусто. Напишите сюда то, что btw должен помнить в каждом чате: кто есть кто в семье, на каких языках вы говорите, аллергии. btw тоже будет сюда дописывать.',
		forgetTitle: (topic: string) => `Забыть «${topic}»?`,
		forgetBody:
			'btw забудет всё из {path} для всех в профиле {profile}. Чаты, которые это уже прочитали, сохранят прочитанное.',
		forgetButton: 'Забыть',
		empty: 'Заметка пустая. Чтобы удалить её, нажмите «Забыть».',
		conflict: (problem: string) =>
			`${problem} Сохраните ещё раз, чтобы оставить свою версию, или отмените, чтобы увидеть версию btw.`,
		saved: 'Сохранено. Новые чаты увидят изменения.',
		forgot: (path: string) => `btw забыл всё из ${path}.`,
		learned: (ago: string) => `узнал ${ago}`,
		learnedAWhileAgo: 'узнал давно',
		topicLabel: (topic: string, n: number) => `${topic}: ${facts(n)}. Показать заметку.`,
		nothingInIt: 'Пока пусто',
		showFewer: 'Показать меньше',
		showAllTopics: (n: number) => `Показать все темы (${n})`,
		older: 'Старые',
		newer: 'Новые',
		nothingYet:
			'btw пока ничего не запомнил. Каждый факт, который он узнаёт, становится здесь точкой.'
	},

	automations: {
		title: 'Автоматизации',
		intro:
			'То, что btw делает сам: напоминания, регулярные проверки и ответы другим приложениям. Всё, что он находит, появляется под колокольчиком. Чтобы добавить или изменить автоматизацию, просто попросите btw в чате.',
		today: 'Сегодня',
		previousMonth: 'Предыдущий месяц',
		nextMonth: 'Следующий месяц',
		dayLabel: (day: string, runs: number) =>
			`${day}: ${runs ? p(runs, { one: `${runs} запуск`, few: `${runs} запуска`, many: `${runs} запусков`, other: `${runs} запуска` }) : 'ничего'}`,
		nothingRan: 'В этот день ничего не запускалось.',
		nothingRuns: 'На этот день ничего не запланировано.',
		timeZone: (zone: string) => `Время указано в часовом поясе компьютера (${zone}).`,
		all: 'Все автоматизации',
		next: (when: string) => `следующий запуск ${when}`,
		states: { paused: 'на паузе', done: 'завершена' },
		runNow: 'Запустить сейчас',
		pause: 'Поставить на паузу',
		resume: 'Возобновить',
		description: 'Описание',
		descriptionPlaceholder: 'Что она делает, одним предложением',
		instructions: 'Инструкции для btw',
		instructionsTechnical: (model: string, effort: string) =>
			`Инструкции для btw (${model}, размышления: ${effort})`,
		script: 'Скрипт: работает без модели и вызывает {command}, когда нужен btw',
		webhook:
			'URL вебхука. Держите его в секрете: любой, у кого он есть, может запустить автоматизацию. Отправляйте на него JSON методом POST.',
		confirmDelete: (name: string) => `Удалить «${name}»?`,
		recentRuns: (n: number) => `Последние запуски (${n})`,
		noRuns: 'Запусков пока не было',
		statuses: {
			pending: 'ожидает',
			running: 'выполняется',
			ok: 'готово',
			notified: 'уведомление отправлено',
			silent: 'нечего сообщить',
			stopped: 'остановлено',
			failed: 'ошибка'
		},
		sources: {
			cron: 'по расписанию',
			once: 'по расписанию',
			webhook: 'вебхук',
			wake: 'разбужено скриптом',
			manual: 'вручную'
		},
		scriptRun: (status: string) => `скрипт: ${status}`,
		view: 'открыть',
		output: 'вывод',
		empty: 'Автоматизаций пока нет. Попросите btw в чате, например:',
		examples: [
			'«Каждый будний день в 7:30 проверяй погоду и говори нам, нужен ли зонт».',
			'«Напомни Анне завтра в 17:00 забрать посылку».',
			'«Проверяй мою почту каждые 10 минут и скажи, когда напишут из школы».'
		],
		defaultModel: 'модель по умолчанию',
		saved: (name: string) => `Сохранено: «${name}».`,
		started: (name: string) => `Запущено: «${name}». Ответ появится под колокольчиком.`,
		startedScript: (name: string) => `Запущен скрипт «${name}».`,
		paused: (name: string) => `«${name}» на паузе.`,
		resumed: (name: string) => `«${name}» снова работает.`,
		deleted: (name: string) => `Удалено: «${name}».`,
		describe,
		scheduleAfterName: (schedule: string) => schedule.charAt(0).toLowerCase() + schedule.slice(1),
		customSchedule: 'По особому расписанию',
		once: 'Однократно',
		onceAt: (when: string) => `Однократно, ${when}`,
		onWebhook: 'Когда другое приложение открывает её ссылку',
		dayAt: (day: string, time: string) => `${day} в ${time}`
	},

	skills: {
		title: 'Навыки',
		intro:
			'Навыки — это инструкции, по которым btw выполняет определённые задачи. Он видит название и описание каждого включённого навыка, а остальное читает, когда этого требует задача. Если отключить навыки, которые этому профилю не нужны, btw будет сосредоточеннее. Изменения действуют в новых чатах.',
		summary: (on: number, total: number, tokens: string) =>
			`Включено ${on} из ${total} · около ${tokens} токенов в начале каждого нового чата`,
		madeFor: (profile: string) => `Для профиля ${profile}`,
		shared: 'Общие для всех профилей',
		builtIn: 'Встроены в btw',
		allOff: 'Выключить все',
		allOn: 'Включить все',
		tokens: (n: string) => `~${n} токенов`,
		use: (name: string) => `Использовать ${name}`,
		empty:
			'Навыков пока нет. Когда btw разберётся, как что-то делать, он сможет сохранить это как навык на будущее.',
		gone: 'Этого навыка больше нет.'
	},

	profile: {
		title: 'Люди и профиль',
		name: 'Название',
		profileName: 'Название профиля',
		folder: (slug: string) => `Папка: ~/.btw-agent/profiles/${slug} (не меняется)`,
		avatar: 'Аватар',
		avatarHint:
			'Как btw выглядит в чатах этого профиля. Все здесь видят один и тот же аватар, и btw может сменить его, если попросить.',
		soul: 'Душа',
		soulHint: (profile: string) =>
			`Кто btw для профиля ${profile}: его характер, что для него важно, как он говорит. С этого начинается каждый чат, и btw сам меняет это, когда вы просите его стать другим.`,
		soulPlaceholder:
			'Ты тёплый и немного игривый, отвечаешь коротко. Детям объясняешь просто и никогда не говоришь с ними свысока. Если чего-то не знаешь, так и говоришь.',
		changesTechnical:
			'Чаты получат изменения со следующим сообщением и один раз перечитают весь чат (промах кеша промпта).',
		changes: 'Чаты получат изменения со следующим сообщением.',
		members: 'Участники',
		membersHint: 'Все здесь видят одни и те же чаты, пишут в них и могут просить btw о чём угодно.',
		personToAdd: 'Кого добавить',
		chooseSomeone: 'Выберите, кого добавить',
		everyoneIsMember: 'Все уже состоят в этом профиле.',
		deleteTitle: 'Удалить профиль',
		deleteHint: 'Все его чаты удалятся у всех. Папка переместится в ~/.btw-agent/trash.',
		deleteButton: 'Удалить этот профиль',
		deleteConfirm: (name: string) => `Удалить «${name}»?`,
		deleteBody: 'Все его чаты удалятся у всех. Папка переместится в ~/.btw-agent/trash.',
		renamed: 'Переименовано.',
		soulSaved: 'Душа сохранена.',
		soulRemoved: 'Душа удалена.',
		added: (name: string) => `Добавили: ${name}.`,
		removed: 'Удалено из профиля.'
	},

	avatars: {
		probe: 'Зонд',
		campfire: 'Костёр',
		lantern: 'Фонарь',
		planet: 'Планета',
		quantum: 'Квант',
		comet: 'Комета',
		moon: 'Луна',
		satellite: 'Спутник'
	},

	profiles: {
		title: 'Профили',
		intro: 'У каждого профиля свои чаты, память и навыки, общие для его участников.',
		none: 'Вы пока не состоите ни в одном профиле. Создайте профиль ниже или попросите кого-нибудь добавить вас в свой.',
		new: 'Новый профиль',
		namePlaceholder: 'Например: Семья, Бабушка, Уроки',
		name: 'Название профиля',
		needsName: 'Назовите профиль.'
	},

	/** Знакомство с новым профилем. Здесь btw говорит от себя: «я» — это ассистент. */
	welcome: {
		title: 'Сделаем это место вашим.',
		subtitle: 'Пара быстрых вопросов, и я буду как дома.',
		go: 'Поехали',
		takesAMinute: 'Займёт около минуты',
		skipIntro: 'Пропустить заставку',
		skipHint: 'Нажмите в любом месте, чтобы пропустить',
		back: 'Назад',
		soundsOn: 'Включить звуки',
		soundsOff: 'Выключить звуки',
		progress: (n: number, total: number) => `Шаг ${n} из ${total}`,
		model: {
			title: 'На чём мне думать?',
			subtitle:
				'Выберите, на чём я буду работать. Другие варианты можно добавить позже в «Моделях и ключах».',
			askAdmin:
				'Чтобы общаться, мне нужна модель. Попросите того, кто установил btw, добавить её в «Моделях и ключах»; всё остальное уже работает.',
			choices: {
				'claude-plan': {
					title: 'Подписка Claude',
					about: 'Тариф Pro или Max через Claude Code на этом компьютере'
				},
				'chatgpt-plan': {
					title: 'Подписка ChatGPT',
					about: 'Тариф Plus, Pro или Business через Codex на этом компьютере'
				},
				anthropic: {
					title: 'API-ключ Anthropic',
					about: 'Модели Claude, оплата по мере использования'
				},
				openai: { title: 'API-ключ OpenAI', about: 'Модели GPT, оплата по мере использования' }
			},
			checkingPlan: 'Проверяю вход…',
			pasteKey: (label: string) => `Вставьте ключ ${label}`,
			checkKey: 'Проверить ключ',
			keyWorks: 'Ключ работает.',
			finishOnAdmin: 'Войдите в разделе {link}, затем проверьте ещё раз.',
			checkAgain: 'Проверить ещё раз',
			pickModel: 'С какой модели начинать новые чаты?',
			asking: 'Запрашиваю список моделей…',
			modelId: 'ID модели'
		},
		avatar: {
			title: 'Кем мне быть?',
			subtitle: (profile: string) => `Выберите лицо для профиля «${profile}».`,
			thisOne: 'Вот этот',
			hello: 'Приятно познакомиться.'
		},
		memory: {
			title: 'Мы уже знакомы?',
			subtitle:
				'Если вы пользуетесь ChatGPT, Claude или Gemini, они уже кое-что о вас знают. Спросите их этим запросом и вставьте ответ сюда.',
			copyPrompt: 'Скопировать запрос',
			prompt: 'Запрос',
			open: (name: string) => `Открыть ${name}`,
			haveIt: 'Ответ есть',
			startFresh: 'Начать с чистого листа',
			pasteTitle: 'Вставьте ответ сюда',
			pastePlaceholder: 'Весь ответ целиком, вместе с блоком кода',
			sections: {
				instructions: 'Инструкции',
				identity: 'О вас',
				career: 'Карьера',
				projects: 'Проекты',
				preferences: 'Предпочтения',
				other: 'Другое'
			},
			remember: (n: number) =>
				p(n, {
					one: `Запомнить ${n} факт`,
					few: `Запомнить ${n} факта`,
					many: `Запомнить ${n} фактов`,
					other: `Запомнить ${n} факта`
				}),
			rememberThis: 'Запомнить',
			reading: 'Читаю…',
			saving: 'Сохраняю…',
			empty: 'Вставьте ответ другого ассистента.',
			tooLong: 'Слишком длинно для воспоминаний одного человека. Вставьте только ответ на запрос.',
			notAnExport: 'Это не похоже на ответ на запрос. Вставьте весь ответ вместе с блоком кода.',
			couldNotRead: (problem: string) => `Не удалось прочитать. ${problem}`,
			nothingFound: 'Ничего о вас здесь не нашлось. Вставьте весь ответ вместе с блоком кода.'
		},
		arrival: {
			thingsIKnow: (n: number) =>
				p(n, {
					one: 'факт, который я о вас знаю',
					few: 'факта, которые я о вас знаю',
					many: 'фактов, которые я о вас знаю',
					other: 'факта, которые я о вас знаю'
				})
		}
	},

	login: {
		welcome: 'С возвращением',
		hint: 'Войдите с аккаунтом, который вам выдали.',
		email: 'Электронная почта',
		password: 'Пароль',
		signingIn: 'Входим…',
		forgot: 'Забыли пароль? Попросите того, кто настраивал btw, выполнить {command}.',
		tooManyAttempts: 'Слишком много попыток. Подождите минуту и попробуйте снова.',
		wrongPassword: 'Неверная почта или пароль.'
	},

	admin: {
		title: 'Модели и ключи',
		keys: 'API-ключи',
		keysHint:
			'Общие для всех профилей, хранятся в файле настроек btw на этом компьютере. Новый ключ проверяется у провайдера перед сохранением и сразу начинает работать.',
		purposes: {
			anthropic: 'Для чатов и автоматизаций на моделях Claude.',
			openai:
				'Для чатов и автоматизаций на моделях OpenAI, а ещё чтобы рисовать картинки на странице «Картинки» и когда рисует агент.'
		},
		withoutIt: {
			anthropic:
				'Чаты и автоматизации на моделях Claude перестанут работать, пока не добавят новый ключ.',
			openai:
				'Чаты и автоматизации на моделях OpenAI перестанут работать, и btw не сможет рисовать картинки, пока не добавят новый ключ.'
		},
		savedInBtw: (hint: string | null) =>
			`Сохранён в btw${hint ? `, заканчивается на ${hint}` : ''}`,
		fromEnv: (variable: string, hint: string | null) =>
			`Из переменной окружения ${variable}${hint ? `, заканчивается на ${hint}` : ''}`,
		notSet: 'Не задан',
		replace: 'Заменить',
		pasteKey: (provider: string) => `Вставьте API-ключ ${provider}`,
		keyLabel: (provider: string) => `API-ключ ${provider}`,
		checkingKey: 'Проверяем ключ…',
		makeOneAt: 'Создать ключ можно на {link}.',
		sameProject:
			'Возьмите ключ из того же проекта: там хранятся картинки и PDF, уже отправленные в чатах, и без них эти чаты не смогут продолжиться.',
		sameWorkspace:
			'Возьмите ключ из того же рабочего пространства: там хранятся картинки и PDF, уже отправленные в чатах, и без них эти чаты не смогут продолжиться.',
		removeKeyTitle: (provider: string) => `Удалить ключ ${provider}?`,
		useEnvInstead: (variable: string) =>
			`Вместо него btw будет использовать ключ из переменной окружения ${variable}.`,
		plan: 'Подписка Claude',
		plans: 'Подписки',
		plansHint:
			'Чаты на пресете с подпиской работают по чьей-то собственной подписке, а не по API-ключу, через агента её создателя на этом компьютере. btw запускает его и никогда не видит данные для входа. Лимиты подписок рассчитаны на обычное использование одним человеком, поэтому частые автоматизации и субагентов лучше оставить на пресете с API-ключом.',
		claudePlanAbout: 'Pro или Max, через Claude Code.',
		chatgptPlan: 'Подписка ChatGPT',
		chatgptPlanAbout: 'Plus, Pro или Business, через Codex от OpenAI.',
		chatgptInstall:
			'В терминале на этом компьютере выполните {setup}: команда установит Codex через npm, спросив перед этим, и войдёт в ChatGPT. Или установите его сами, а затем войдите здесь:',
		chatgptSignIn: 'Войти через ChatGPT',
		chatgptSignInAgain: 'Войти заново',
		chatgptAsking: 'Запрашиваем ChatGPT…',
		signOut: 'Выйти',
		chatgptOpen: 'Откройте {link} на любом устройстве и войдите в ChatGPT.',
		chatgptCode: 'Введите этот код: {code}',
		chatgptCodeHint: 'Код действует 15 минут. Страница обновится, когда его введут.',
		chatgptSignOutTitle: 'Выйти из ChatGPT?',
		chatgptSignOutBody:
			'Чаты на пресетах с подпиской ChatGPT перестанут работать, пока кто-нибудь снова не войдёт.',
		signedOut: 'Выход выполнен.',
		notAt: 'Не найден по пути {path}, который указан в {command}.',
		notInstalled: 'Не установлен на этом компьютере.',
		checkSignIn: 'Проверить вход',
		install:
			'В терминале на этом компьютере выполните {setup}: команда установит Claude Code установщиком Anthropic и войдёт в ваш аккаунт Claude, спросив разрешения. Или установите его сами, затем запустите {claude} и войдите:',
		copyCommand: 'Скопировать команду',
		signIn:
			'Чтобы войти, выполните {setup} в терминале на этом компьютере или запустите там {claude} и используйте {login} с вашим аккаунтом Claude.',
		models: 'Модели',
		modelsHint:
			'Пресеты общие для всех профилей. Новые чаты начинаются с пресета по умолчанию. Удаление пресета не влияет на существующие чаты.',
		presetDetails: (provider: string, model: string, context: string, overridden: boolean) =>
			`${provider} / ${model} · контекст ${context}${overridden ? ' (задан вручную)' : ''}`,
		makeDefault: 'Сделать по умолчанию',
		noPresets: 'Пресетов пока нет.',
		addModel: {
			title: 'Добавить модель',
			provider: 'Провайдер',
			model: 'Модель',
			name: 'Название',
			optional: '(необязательно)',
			namePlaceholder: 'ID модели и провайдер',
			contextWindow: 'Окно контекста',
			contextTokens: 'Окно контекста в токенах',
			auto: 'Авто',
			autoWith: (size: string) => `Авто · ${size}`,
			unknown: 'неизвестно',
			custom: 'Своё',
			tokensPlaceholder: 'Токены, например 272k',
			tokens: (n: number) =>
				`${n.toLocaleString('ru')} ${p(n, { one: 'токен', few: 'токена', many: 'токенов', other: 'токена' })}.`,
			typeTokens: 'Введите число токенов, например 272k или 272000.',
			autoContext: {
				anthropic: 'В режиме «Авто» берётся окно, которое Anthropic сообщает для модели.',
				openai:
					'OpenAI его не сообщает: «Авто» знает только окна флагманов (1,05M начиная с GPT-5.4).',
				'claude-plan': 'Claude Code его не сообщает: «Авто» знает только модели с контекстом 1M.',
				'chatgpt-plan': 'Codex его не сообщает, поэтому «Авто» оставляет его неизвестным.'
			},
			onPlan: 'Работает по тарифу Pro или Max, с которым вошли в Claude Code.',
			noClaudeCode: 'Claude Code ещё не установлен: см. «Подписка Claude» выше.',
			onChatGptPlan: 'Работает по подписке ChatGPT, с которой вошли в Codex.',
			noCodex: 'Codex ещё не установлен: см. «Подписка ChatGPT» выше.',
			onKey: (provider: string) => `Работает по API-ключу ${provider}.`,
			noKey: (provider: string) =>
				`API-ключа ${provider} пока нет: добавьте его в разделе «API-ключи» выше.`,
			asking: (source: string) => `Спрашиваем у ${source} список моделей…`,
			listProblem: (problem: string) => `${problem} ID модели всё равно можно ввести вручную.`,
			couldNotList: (status: number) => `btw не удалось получить список моделей (${status}).`,
			unreachable: 'Не удалось связаться с btw.',
			checkingClaude: 'Проверяем Claude Code…',
			checkingCodex: 'Проверяем Codex…',
			checkingModel: 'Проверяем модель…',
			pick: 'Выберите модель',
			search: 'Найдите или введите ID модели',
			use: 'Использовать {model}',
			typeId: 'Введите ID модели выше.',
			added: (name: string) => `Добавлено: ${name}.`,
			invalidContext: 'Окно контекста должно быть числом токенов, например 272k или 272000.'
		},
		savedWorks: 'Сохранено. Ключ работает.',
		savedWarning: (warning: string) => `Сохранено. ${warning}`,
		claudeNoAnswer: 'Claude Code не ответил.',
		removed: 'Удалено.',
		newDefault: (name: string) => `Теперь новые чаты начинаются с ${name}.`,
		presetRemoved: 'Удалено. Существующие чаты продолжат работать.'
	}
};
