import { isMonthSpan, type Schedule } from '@nolune/core/schedule';
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
		description: 'Как nolune выглядит на этом устройстве.',
		theme: 'Тема',
		system: 'Как в системе',
		light: 'Светлая',
		dark: 'Тёмная',
		language: 'Язык',
		languageAuto: 'Как в браузере',
		languageHint: 'Для меню и кнопок. nolune отвечает на том языке, на котором вы пишете.',
		technical: 'Технические подробности',
		technicalHint:
			'Показывать точные команды, которые запускает nolune, расход токенов и кеширование промпта.',
		expandSteps: 'Всегда показывать шаги',
		expandStepsHint:
			'Раскрывать под каждым ответом список того, что сделал nolune, а не сворачивать его.',
		sounds: 'Звуки',
		soundsHint:
			'Тихие звуки там, где nolune двигается сам, например при знакомстве с новым профилем.',
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
			'Пока ничего нет. Попросите nolune о чём-нибудь напомнить или что-то проверять каждый день, и всё, что он найдёт, появится здесь.'
	},

	chat: {
		options: 'Действия с чатом',
		placeholder: 'Спросите nolune',
		placeholderRunning: 'Добавьте что-нибудь, пока nolune работает…',
		disclaimer: 'nolune может ошибаться и может менять файлы на этом компьютере.',
		reconnecting: 'Переподключение…',
		empty: 'Попросите о чём-нибудь, чтобы начать.',
		readsAfterStep: 'nolune прочитает это после текущего шага',
		working: 'nolune работает',
		thinking: 'Думает',
		writing: 'Пишет',
		cutOff: 'Ответ оборвался, потому что стал слишком длинным.',
		refused: 'nolune отказался продолжать этот запрос.',
		automation: (title: string) => `Автоматизация · ${title}`,
		fromNolune: (to: string) => `От nolune для ${to}`,
		finishedInBackground: (title: string) => `Завершено в фоне · ${title}`,
		inBackground: 'Работает в фоне',
		aCommand: 'Команда',
		subagent: (name: string) => `Субагент ${name}`,
		stopping: 'останавливается',
		errorTitle: 'Что-то пошло не так, пока nolune отвечал.',
		unanswered: 'nolune ещё не ответил на это.',
		scrollToBottom: 'К последнему сообщению',
		subagentBanner:
			'Субагент {name}: его запустил nolune из чата {parent}, и туда он присылает результаты.',
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
			`nolune хранит этот чат в кеше, чтобы каждый ответ оплачивал только новое. ${model ? 'Другая модель' : 'Другой уровень размышлений'} не может его использовать: следующий ответ заново прочитает весь чат, а это дольше и дороже${missTokens ? ` (промах кеша промпта примерно на ${missTokens} токенов)` : ''}.`,
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
		blockedCount: (n: number) => `заблокировано: ${n}`,
		failed: 'ошибка',
		didntWork: 'не получилось',
		statusStopped: 'остановлена',
		statusNotRun: 'не запускалась',
		statusBlocked: 'заблокирована',
		command: 'Команда',
		theCommand: 'Команда, которую выполнил nolune',
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

	commandMode: {
		title: 'Команды в этом чате',
		auto: 'Автоматически',
		autoHint: 'Модель проверяет каждую команду перед запуском.',
		unrestricted: 'Без ограничений',
		unrestrictedHint: 'Команды запускаются без проверки. Не рекомендуется.',
		adminsOnly: 'Отключить проверку в чате может только администратор.',
		labelAuto: 'Команды: автоматический режим',
		labelUnrestricted: 'Команды: без ограничений'
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
			'Папка {name} удалится у всех в профиле. Её чаты вернутся в общий список, но без её инструкций и файлов. Файлы переместятся в ~/.nolune/trash.',
		options: 'Действия с папкой',
		newChatIn: (name: string) => `Новый чат в папке ${name}`,
		instructions: 'Инструкции',
		instructionsHint: 'Что nolune должен знать или делать в каждом чате этой папки.',
		instructionsPlaceholder:
			'Планируем две недели в Японии в апреле с детьми (им 7 и 10). Без спешки, бюджет до 600 000 ¥.',
		changesTechnical:
			'Чаты в папке получат изменения со следующим сообщением и один раз перечитают весь чат (промах кеша промпта).',
		changes: 'Чаты в папке получат изменения со следующим сообщением.',
		files: 'Файлы',
		addFiles: 'Добавить файлы',
		filesHint:
			'Картинки, документы, что угодно. nolune знает, где они лежат, и открывает их, когда они нужны.',
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
		onlyPath: (note: string) => `nolune получил только путь к файлу: ${note}`
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
		cantMakeYet: 'nolune пока не может рисовать картинки.',
		needsKeyAdmin: 'Нужен API-ключ {provider}. {link}.',
		addKeyLink: 'Добавьте его в разделе «Модели и ключи»',
		needsKey: (provider: string) =>
			`Нужен API-ключ ${provider}. Попросите администратора добавить его.`,
		adminSetsUp: 'Это настраивает администратор на компьютере, где работает nolune.',
		describe: 'Опишите картинку',
		describeHint: 'nolune нарисует картинку в новом чате, где её можно попросить изменить.',
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
			`Что nolune помнит для профиля ${profile}. Это видят все его участники. Каждый чат начинается с закреплённой заметки «Главное»; остальные nolune читает, когда они нужны в чате, и по ходу дела записывает то, что узнаёт. Чтобы добавить что-то, просто скажите ему в чате, например: «Запомни, что у Анны аллергия на орехи».`,
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
			'Пока пусто. Напишите сюда то, что nolune должен помнить в каждом чате: кто есть кто в семье, на каких языках вы говорите, аллергии. nolune тоже будет сюда дописывать.',
		forgetTitle: (topic: string) => `Забыть «${topic}»?`,
		forgetBody:
			'nolune забудет всё из {path} для всех в профиле {profile}. Чаты, которые это уже прочитали, сохранят прочитанное.',
		forgetButton: 'Забыть',
		empty: 'Заметка пустая. Чтобы удалить её, нажмите «Забыть».',
		conflict: (problem: string) =>
			`${problem} Сохраните ещё раз, чтобы оставить свою версию, или отмените, чтобы увидеть версию nolune.`,
		saved: 'Сохранено. Новые чаты увидят изменения.',
		forgot: (path: string) => `nolune забыл всё из ${path}.`,
		learn: 'Запоминать из чатов',
		learnHint:
			'Когда в чате пару минут тихо, nolune перечитывает его и сохраняет то, что стоит запомнить. Каждый раз это один короткий дополнительный запрос к модели чата. Если выключить, nolune сохраняет только то, о чём вспомнит во время разговора.',
		learned: (ago: string) => `узнал ${ago}`,
		learnedAWhileAgo: 'узнал давно',
		topicLabel: (topic: string, n: number) => `${topic}: ${facts(n)}. Показать заметку.`,
		nothingInIt: 'Пока пусто',
		showFewer: 'Показать меньше',
		showAllTopics: (n: number) => `Показать все темы (${n})`,
		older: 'Старые',
		newer: 'Новые',
		nothingYet:
			'nolune пока ничего не запомнил. Каждый факт, который он узнаёт, становится здесь точкой.',
		categories: {
			core: 'Главное',
			people: 'Люди',
			home: 'Дом',
			health: 'Здоровье',
			plans: 'Планы',
			routines: 'Распорядок',
			pets: 'Питомцы',
			places: 'Места',
			projects: 'Проекты',
			other: 'Другое',
			unsorted: 'Без категории'
		},
		unsortedHint:
			'Эта заметка появилась, когда у памяти ещё не было категорий: nolune её читает, но ничего в неё не дописывает. Перенесите её в категорию, чтобы она пополнялась.',
		memberNote: (name: string) => `участник: ${name}`,
		move: {
			button: (topic: string) => `Перенести: ${topic}`,
			title: (topic: string) => `Перенести «${topic}»`,
			body: 'Выберите, куда она относится. Если такая заметка уже есть, они станут одной.',
			to: 'Куда перенести',
			choose: 'Выберите куда',
			categories: 'Категории',
			newPerson: 'Новый человек…',
			newProject: 'Новый проект…',
			personName: 'Имя',
			projectName: 'Название проекта',
			moveHint: (target: string) => `Заметка станет ${target}.`,
			mergeHint: (from: string, into: string) =>
				`Всё из «${from}» добавится в «${into}» без повторов, а «${from}» исчезнет. Так объединяют две заметки об одном человеке.`,
			move: 'Перенести',
			merge: 'Объединить',
			moved: (from: string, to: string) => `${from} перенесена в ${to}.`,
			merged: (from: string, into: string) => `${from} объединена с ${into}.`
		},
		/** What nolune saved from a chat by itself: in that chat, and at the top of this page. */
		changes: {
			saved: (n: number) => `Запомнил ${facts(n)}`,
			added: 'Новое',
			changed: 'Изменено',
			before: (text: string) => `было: ${text}`,
			openNote: (topic: string) => `Открыть «${topic}» в Памяти`,
			undo: 'Отменить',
			undone: 'Отменено',
			undoneBy: (name: string) => `Отменено (${name})`,
			changedSince:
				'С тех пор это изменили, поэтому здесь не отменить. Поправьте заметку на странице «Память».',
			alreadyUndone: 'Это уже отменили.',
			recent: 'Запомнено из чатов',
			recentHint: 'Что nolune записал сам, когда чаты затихали, за последние две недели.',
			fromChat: 'из чата {chat}',
			deletedChat: 'из удалённого чата',
			showAll: (n: number) => `Показать все (${n})`,
			untitled: 'без названия'
		}
	},

	automations: {
		title: 'Автоматизации',
		intro:
			'То, что nolune делает сам: напоминания, регулярные проверки и ответы другим приложениям. Всё, что он находит, появляется под колокольчиком. Чтобы добавить или изменить автоматизацию, просто попросите nolune в чате.',
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
		instructions: 'Инструкции для nolune',
		instructionsTechnical: (model: string, effort: string) =>
			`Инструкции для nolune (${model}, размышления: ${effort})`,
		script: 'Скрипт: работает без модели и вызывает {command}, когда нужен nolune',
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
		empty: 'Автоматизаций пока нет. Попросите nolune в чате, например:',
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
			'Навыки — это инструкции, по которым nolune выполняет определённые задачи. Он видит название и описание каждого включённого навыка, а остальное читает, когда этого требует задача. Если отключить навыки, которые этому профилю не нужны, nolune будет сосредоточеннее. Изменения действуют в новых чатах.',
		summary: (on: number, total: number, tokens: string) =>
			`Включено ${on} из ${total} · около ${tokens} токенов в начале каждого нового чата`,
		madeFor: (profile: string) => `Для профиля ${profile}`,
		shared: 'Общие для всех профилей',
		builtIn: 'Встроены в nolune',
		allOff: 'Выключить все',
		allOn: 'Включить все',
		tokens: (n: string) => `~${n} токенов`,
		use: (name: string) => `Использовать ${name}`,
		empty:
			'Навыков пока нет. Когда nolune разберётся, как что-то делать, он сможет сохранить это как навык на будущее.',
		gone: 'Этого навыка больше нет.'
	},

	profile: {
		title: 'Люди и профиль',
		name: 'Название',
		profileName: 'Название профиля',
		folder: (slug: string) => `Папка: ~/.nolune/profiles/${slug} (не меняется)`,
		avatar: 'Аватар',
		avatarHint:
			'Как nolune выглядит в чатах этого профиля. Все здесь видят один и тот же аватар, и nolune может сменить его, если попросить.',
		soul: 'Душа',
		soulHint: (profile: string) =>
			`Кто nolune для профиля ${profile}: его характер, что для него важно, как он говорит. С этого начинается каждый чат, и nolune сам меняет это, когда вы просите его стать другим.`,
		soulPlaceholder:
			'Ты тёплый и немного игривый, отвечаешь коротко. Детям объясняешь просто и никогда не говоришь с ними свысока. Если чего-то не знаешь, так и говоришь.',
		changesTechnical:
			'Чаты получат изменения со следующим сообщением и один раз перечитают весь чат (промах кеша промпта).',
		changes: 'Чаты получат изменения со следующим сообщением.',
		members: 'Участники',
		membersHint:
			'Все здесь видят одни и те же чаты, пишут в них и могут просить nolune о чём угодно.',
		personToAdd: 'Кого добавить',
		chooseSomeone: 'Выберите, кого добавить',
		everyoneIsMember: 'Все уже состоят в этом профиле.',
		deleteTitle: 'Удалить профиль',
		deleteHint: 'Все его чаты удалятся у всех. Папка переместится в ~/.nolune/trash.',
		deleteButton: 'Удалить этот профиль',
		deleteConfirm: (name: string) => `Удалить «${name}»?`,
		deleteBody: 'Все его чаты удалятся у всех. Папка переместится в ~/.nolune/trash.',
		renamed: 'Переименовано.',
		soulSaved: 'Душа сохранена.',
		soulRemoved: 'Душа удалена.',
		added: (name: string) => `Добавили: ${name}.`,
		removed: 'Удалено из профиля.',
		memberNote: (note: string) => `Заметка: ${note}`,
		noteStarts: (note: string) => `Заметка: ${note}, появится, когда будет что записать`,
		mayHaveNote: 'Возможно, у nolune уже есть заметка об этом человеке',
		chooseNote: 'Выбрать заметку',
		changeNote: 'Сменить заметку',
		linked: (name: string, note: string) => `Готово: ${name} → ${note}.`,
		chooser: {
			addTitle: (name: string) => `nolune уже знает человека по имени ${name}?`,
			linkTitle: (name: string) => `Какая заметка о человеке по имени ${name}?`,
			body: (name: string) =>
				`В памяти есть заметки, которые могут быть о человеке по имени ${name}. Выберите нужную: тогда nolune будет знать, что написанное там — про этого участника, и будет дописывать туда то, что участник расскажет о себе.`,
			linkBody: (name: string) =>
				`Выберите заметку о человеке по имени ${name}: nolune будет дописывать туда то, что участник расскажет о себе.`,
			current: 'сейчас',
			alsoCalled: (names: string) => `Зовут также: ${names}`,
			more: (n: number) => `и ещё ${n}`,
			newNote: 'Это другой человек: начать новую заметку',
			privacy: (name: string) =>
				`${name} сможет читать всю память профиля, и эту заметку тоже. Проверьте, что в ней нет ничего, что стоит держать в секрете от этого человека, например сюрприза.`,
			add: (name: string) => `Добавить: ${name}`,
			link: 'Привязать'
		}
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

	/** Знакомство с новым профилем. Здесь nolune говорит от себя: «я» — это ассистент. */
	welcome: {
		title: 'Сделаем это место вашим.',
		subtitle: 'Пара быстрых вопросов, и вы будете как дома.',
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
				'Чтобы общаться, мне нужна модель. Попросите того, кто установил nolune, добавить её в «Моделях и ключах»; всё остальное уже работает.',
			choices: {
				'claude-plan': {
					title: 'Подписка Claude',
					about: 'Тариф Pro или Max через Claude Code на этом компьютере'
				},
				'chatgpt-plan': {
					title: 'Подписка ChatGPT',
					about: 'Тариф Plus или Pro, вход через ChatGPT'
				},
				anthropic: {
					title: 'API-ключ Anthropic',
					about: 'Модели Claude, оплата по мере использования'
				},
				openai: { title: 'API-ключ OpenAI', about: 'Модели GPT, оплата по мере использования' },
				openrouter: {
					title: 'API-ключ OpenRouter',
					about: 'Claude, GPT, Gemini и другие по одному ключу'
				}
			},
			checkingPlan: 'Проверяю вход…',
			pasteKey: (label: string) => `Вставьте ключ ${label}`,
			checkKey: 'Проверить ключ',
			keyWorks: 'Ключ работает.',
			finishOnAdmin: 'Войдите в разделе {link}, затем проверьте ещё раз.',
			checkAgain: 'Проверить ещё раз',
			pickModel: 'С какой модели начинать новые чаты?',
			asking: 'Запрашиваю список моделей…',
			modelId: 'ID модели',
			/** Signing in with ChatGPT right in the step. */
			chatgpt: {
				title: 'Вход через ChatGPT',
				about:
					'Войдите на странице ChatGPT и разрешите nolune пользоваться вашей подпиской Plus или Pro. Чаты будут расходовать её лимиты, как и сам ChatGPT.',
				newTab: 'Страница входа ChatGPT откроется в новой вкладке.',
				waiting: 'Когда войдёте там, эта страница продолжит сама.',
				starting: 'Готовим страницу входа ChatGPT…',
				tryAgain: 'Попробовать снова',
				otherDevice: 'Входите с другого устройства?'
			}
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
		},
		fresh: {
			title: 'С чистого листа.',
			subtitle: 'Буду узнавать вас по ходу разговора.'
		}
	},

	login: {
		welcome: 'С возвращением',
		hint: 'Войдите с аккаунтом, который вам выдали.',
		email: 'Электронная почта',
		password: 'Пароль',
		signingIn: 'Входим…',
		forgot: 'Забыли пароль? Попросите того, кто настраивал nolune, выполнить {command}.',
		tooManyAttempts: 'Слишком много попыток. Подождите минуту и попробуйте снова.',
		wrongPassword: 'Неверная почта или пароль.'
	},

	admin: {
		title: 'Модели и ключи',
		keys: 'API-ключи',
		keysHint:
			'Общие для всех профилей, хранятся в файле настроек nolune на этом компьютере. Новый ключ проверяется у провайдера перед сохранением и сразу начинает работать. Для своего сервера моделей, например Ollama или LM Studio, добавьте свой провайдер.',
		purposes: {
			anthropic: 'Для чатов и автоматизаций на моделях Claude.',
			openai:
				'Для чатов и автоматизаций на моделях OpenAI, а ещё чтобы рисовать картинки на странице «Картинки» и когда рисует агент.',
			openrouter:
				'Для чатов и автоматизаций на моделях, которые даёт OpenRouter (Claude, GPT, Gemini, DeepSeek и многие другие), по одному ключу и за его кредиты.'
		},
		withoutIt: {
			anthropic:
				'Чаты и автоматизации на моделях Claude перестанут работать, пока не добавят новый ключ.',
			openai:
				'Чаты и автоматизации на моделях OpenAI перестанут работать, и nolune не сможет рисовать картинки, пока не добавят новый ключ.',
			openrouter:
				'Чаты и автоматизации на моделях OpenRouter перестанут работать, пока не добавят новый ключ.'
		},
		savedInNolune: (hint: string | null) =>
			`Сохранён в nolune${hint ? `, заканчивается на ${hint}` : ''}`,
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
			`Вместо него nolune будет использовать ключ из переменной окружения ${variable}.`,
		plan: 'Подписка Claude',
		plans: 'Подписки',
		plansHint:
			'Чаты на пресете с подпиской работают по чьей-то собственной подписке, а не по API-ключу: подписка Claude — через Claude Code на этом компьютере, который сам хранит вход; подписка ChatGPT — через «Вход с ChatGPT», и этот вход nolune хранит на этом компьютере. Лимиты подписок рассчитаны на обычное использование одним человеком, поэтому частые автоматизации и субагентов лучше оставить на пресете с API-ключом.',
		claudePlanAbout: 'Pro или Max, через Claude Code.',
		chatgptPlan: 'Подписка ChatGPT',
		chatgptPlanAbout: 'Plus или Pro, вход через ChatGPT.',
		chatgptSignIn: 'Продолжить с ChatGPT',
		chatgptContinueAs: (account: string) => `Продолжить как ${account}`,
		chatgptAnotherAccount: 'Другой аккаунт',
		chatgptSignInAgain: 'Войти заново',
		chatgptStarting: 'Запускаем…',
		signOut: 'Выйти',
		chatgptOpen: 'Откройте {link} и войдите, разрешив nolune пользоваться вашей подпиской ChatGPT.',
		chatgptSignInPage: 'страницу входа ChatGPT',
		chatgptHere:
			'Если браузер на этом компьютере, больше ничего не нужно: страница обновится, когда вход завершится.',
		chatgptElsewhere:
			'На другом устройстве страница, на которую ChatGPT вернёт вас, не откроется. Скопируйте её адрес (он начинается с http://127.0.0.1) и вставьте сюда:',
		chatgptFinish: 'Готово',
		chatgptSignedIn: 'Вход выполнен. Чаты на пресетах с подпиской ChatGPT теперь работают по ней.',
		chatgptUsing: 'Чаты на пресетах с подпиской ChatGPT работают по этой подписке. {link}',
		chatgptManageUsage: 'Управлять лимитами',
		chatgptNobody: 'Никто не вошёл через ChatGPT.',
		chatgptSignedOutLocally:
			'Выход выполнен здесь, но сообщить OpenAI не удалось: для надёжности отключите nolune в настройках ChatGPT.',
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
			'Пресеты общие для всех профилей. Новые чаты начинаются с пресета по умолчанию. Изменение или удаление пресета не затрагивает чаты, которые уже на нём.',
		presetDetails: (provider: string, model: string, context: string, overridden: boolean) =>
			`${provider} / ${model} · контекст ${context}${overridden ? ' (задан вручную)' : ''}`,
		makeDefault: 'Сделать по умолчанию',
		editPreset: (name: string) => `Изменить ${name}`,
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
				openrouter:
					'В режиме «Авто» берётся окно, которое OpenRouter указывает для модели и её основного провайдера.',
				'custom-openai':
					'В режиме «Авто» берётся окно, которое сервер указывает для модели, если указывает (vLLM указывает); иначе оно остаётся неизвестным.',
				'custom-anthropic':
					'В режиме «Авто» берётся окно, которое сервер указывает для модели, если указывает (vLLM указывает); иначе оно остаётся неизвестным.',
				'claude-plan': 'Claude Code его не сообщает: «Авто» знает только модели с контекстом 1M.',
				'chatgpt-plan':
					'«Авто» берёт окно, которое ChatGPT указывает для модели, если указывает; иначе оно остаётся неизвестным.'
			},
			onPlan: 'Работает по тарифу Pro или Max, с которым вошли в Claude Code.',
			noClaudeCode: 'Claude Code ещё не установлен: см. «Подписка Claude» выше.',
			onChatGptPlan: 'Работает по подписке ChatGPT того, кто вошёл через ChatGPT выше.',
			noChatGpt: 'Через ChatGPT ещё никто не вошёл: см. «Подписка ChatGPT» выше.',
			onKey: (provider: string) => `Работает по API-ключу ${provider}.`,
			noKey: (provider: string) =>
				`API-ключа ${provider} пока нет: добавьте его в разделе «API-ключи» выше.`,
			onCustom: (api: string, url: string) =>
				`Работает на ${url} через API в формате ${api}. Модель должна уметь вызывать инструменты; картинки и PDF она получает как пути к файлам.`,
			customGone: 'Этот свой провайдер удалён: выберите другой.',
			asking: (source: string) => `Спрашиваем у ${source} список моделей…`,
			listProblem: (problem: string) => `${problem} ID модели всё равно можно ввести вручную.`,
			couldNotList: (status: number) => `nolune не удалось получить список моделей (${status}).`,
			unreachable: 'Не удалось связаться с nolune.',
			checkingClaude: 'Проверяем Claude Code…',
			checkingModel: 'Проверяем модель…',
			saving: 'Сохраняем…',
			pick: 'Выберите модель',
			search: 'Найдите или введите ID модели',
			use: 'Использовать {model}',
			typeId: 'Введите ID модели выше.',
			added: (name: string) => `Добавлено: ${name}.`,
			saved: (name: string) => `Сохранено: ${name}. Чаты, которые уже на нём, остаются как были.`,
			invalidContext: 'Окно контекста должно быть числом токенов, например 272k или 272000.'
		},
		savedWorks: 'Сохранено. Ключ работает.',
		savedWarning: (warning: string) => `Сохранено. ${warning}`,
		claudeNoAnswer: 'Claude Code не ответил.',
		removed: 'Удалено.',
		newDefault: (name: string) => `Теперь новые чаты начинаются с ${name}.`,
		presetRemoved: 'Удалено. Существующие чаты продолжат работать.',
		/** Custom providers: model servers of the family's own, listed with the API keys. */
		customProviders: {
			add: 'Добавить свой провайдер',
			addTitle: 'Добавить свой провайдер',
			addHint:
				'Собственный сервер моделей: Ollama, LM Studio или oMLX на этом компьютере, или vLLM на машине с GPU. В «Добавить модель» он появится под своим именем.',
			about: (api: string) => `Ваш сервер моделей, через его API в формате ${api}.`,
			apis: { openai: 'OpenAI', anthropic: 'Anthropic' },
			change: 'Изменить',
			name: 'Имя',
			namePlaceholder: 'Ollama',
			api: 'API',
			apiHints: {
				openai:
					'Responses API от OpenAI: Ollama, LM Studio, vLLM, LiteLLM. Поиск по памяти может брать у него эмбеддинги.',
				anthropic: 'Messages API от Anthropic: Ollama, LM Studio, сервер llama.cpp, oMLX.'
			},
			address: 'Адрес',
			addressHint:
				'Ollama слушает на http://localhost:11434, LM Studio — на http://localhost:1234. nolune проверяет его, запрашивая список моделей.',
			key: 'Ключ',
			keyOptional: '(если он нужен)',
			keyKept: 'Сохранён. Оставьте пустым, чтобы не менять.',
			withKey: (hint: string | null) => (hint ? `ключ заканчивается на ${hint}` : 'с ключом'),
			noKey: 'без ключа',
			checking: 'Проверяем…',
			works: (n: number) =>
				`Сохранено. На сервере ${n} ${p(n, { one: 'модель', few: 'модели', many: 'моделей', other: 'модели' })}.`,
			unchecked: (problem: string) => `Сохранено без проверки: ${problem}`,
			needName: 'Дайте ему имя, например Ollama или GPU box.',
			needAddress: 'Укажите его адрес, начиная с http:// или https://.',
			removeTitle: (name: string) => `Удалить ${name}?`,
			removeBody:
				'Чаты и автоматизации на его моделях перестанут работать, пока их не переведут на другую модель.',
			usedBy: (names: string) => `На нём работают пресеты: ${names}.`
		},
		/** Memory search by meaning: where its embeddings come from. */
		embeddings: {
			title: 'Поиск по памяти',
			hint: 'nolune находит факты в памяти и по смыслу, а не только по словам, и на любом языке. Для этого каждый факт один раз отправляется туда, где считаются эмбеддинги, а каждое сообщение — при отправке; сервер на этом компьютере оставляет их здесь. Общая настройка для всех профилей.',
			name: 'Поиск по смыслу',
			using: (source: string) => `Использует ${source}.`,
			off: 'Выключен: память ищется только по словам.',
			noKeys: 'Пока нет ключа OpenAI или OpenRouter, поэтому память ищется только по словам.',
			noKey: (provider: string) =>
				`Пока нет ключа ${provider}, поэтому память ищется только по словам. Добавьте его в API-ключах выше.`,
			customGone: 'Этот свой провайдер удалён, поэтому память ищется только по словам.',
			change: 'Изменить',
			source: 'Откуда эмбеддинги',
			modes: {
				auto: 'Авто',
				openai: 'OpenAI',
				openrouter: 'OpenRouter',
				off: 'Выключен'
			},
			autoNote:
				'text-embedding-3-small от OpenAI по ключу OpenAI, а без него — та же модель через OpenRouter.',
			withKey: (provider: string) => `По API-ключу ${provider}.`,
			customNote: (url: string) =>
				`Модель, которую он отдаёт по адресу ${url}, через API в формате OpenAI.`,
			offNote: 'Память ищется только по словам, и никакие факты никуда не отправляются.',
			model: 'Модель',
			customModel: 'Как она там называется, например nomic-embed-text',
			checking: 'Проверяем…',
			works: 'Сохранено. Работает; факты обрабатываются в фоне.',
			wordsOnly: 'Сохранено. Память ищется только по словам.',
			noAnswer: (problem: string) => `Сохранено, но ответа нет: ${problem}`,
			needModel: 'Укажите, какую модель использовать.'
		},
		commands: {
			title: 'Команды',
			hint: 'Агент работает, запуская команды на этом компьютере, с доступом этой учётной записи к её файлам и программам. В автоматическом режиме модель смотрит на каждую команду перед запуском и останавливает то, что может навредить и о чём никто не просил. Так работают все чаты, если в чате не выбрано иное щитом в поле сообщения; отключить там проверку могут только администраторы.',
			modes: {
				auto: 'Автоматический режим',
				unrestricted: 'Без ограничений'
			},
			choices: {
				auto: 'Автоматический (рекомендуется)',
				unrestricted: 'Без ограничений (не рекомендуется)'
			},
			checkedByChat: 'Каждую команду проверяет модель самого чата.',
			checkedBy: (preset: string) => `Каждую команду проверяет ${preset}.`,
			presetGone:
				'Модель, выбранную для проверок, удалили, поэтому команды каждого чата проверяет его собственная модель.',
			unrestrictedStatus: 'Команды запускаются без проверки.',
			change: 'Изменить',
			mode: 'Режим',
			autoNote:
				'Команды, которые только смотрят, запускаются сразу. Остальные сначала проверяются; заблокированная команда не запускается, а nolune объясняет, что хотел сделать, чтобы кто-нибудь мог разрешить.',
			unrestrictedNote:
				'Каждая команда запускается так, как её написал агент, и ничто не остановит ошибку или веб-страницу либо письмо, которые его на что-то уговорили. Только для тех, кто внимательно следит.',
			checker: 'Проверяет',
			chatModel: 'Модель самого чата',
			checkerNote:
				'Быстрая и толковая модель не замедляет чаты: каждая команда, которая делает больше, чем просто смотрит, стоит короткого запроса к ней.',
			saved: 'Сохранено. Действует со следующей команды.'
		}
	}
};
