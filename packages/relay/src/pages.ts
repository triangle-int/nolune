/*
 * The pages the relay shows in place of a family's nolune, in the languages nolune's interface
 * speaks (the browser's first one that matches, else English).
 */

export type PageKind = 'offline' | 'unknown' | 'failed' | 'quota' | 'blocked';

type Texts = Record<PageKind, { title: string; body: string }>;

const TEXTS: Record<string, Texts> = {
	en: {
		offline: {
			title: 'nolune is offline',
			body: "The computer nolune runs on isn't connected right now. Check that it's on and awake, and that nolune is running on it. This page reloads by itself."
		},
		unknown: {
			title: 'No nolune here',
			body: 'Nothing is registered at this address. Check the address you were given.'
		},
		failed: {
			title: "nolune didn't answer",
			body: 'Something went wrong on the way to nolune. Try again in a moment.'
		},
		quota: {
			title: "This address has used this month's traffic",
			body: 'This nolune has passed the traffic its address gets each month. It works again from the 1st of next month.'
		},
		blocked: {
			title: 'This address is blocked',
			body: 'It was blocked for breaking the rules of the service that runs it.'
		}
	},
	ru: {
		offline: {
			title: 'nolune сейчас не на связи',
			body: 'Компьютер, на котором работает nolune, сейчас не подключён. Проверьте, что он включён и не спит, а nolune на нём запущен. Страница обновится сама.'
		},
		unknown: {
			title: 'Здесь нет nolune',
			body: 'По этому адресу ничего не зарегистрировано. Проверьте адрес, который вам дали.'
		},
		failed: {
			title: 'nolune не ответил',
			body: 'Что-то пошло не так по пути к nolune. Попробуйте ещё раз чуть позже.'
		},
		quota: {
			title: 'У этого адреса закончился трафик на этот месяц',
			body: 'Этот nolune израсходовал трафик, который его адресу положен на месяц. Он снова заработает с 1-го числа следующего месяца.'
		},
		blocked: {
			title: 'Этот адрес заблокирован',
			body: 'Его заблокировали за нарушение правил сервиса, через который он работает.'
		}
	},
	de: {
		offline: {
			title: 'nolune ist gerade offline',
			body: 'Der Computer, auf dem nolune läuft, ist gerade nicht verbunden. Prüfe, ob er eingeschaltet und wach ist und nolune darauf läuft. Diese Seite lädt sich von selbst neu.'
		},
		unknown: {
			title: 'Hier gibt es kein nolune',
			body: 'Unter dieser Adresse ist nichts registriert. Prüfe die Adresse, die du bekommen hast.'
		},
		failed: {
			title: 'nolune hat nicht geantwortet',
			body: 'Auf dem Weg zu nolune ist etwas schiefgegangen. Versuche es gleich noch einmal.'
		},
		quota: {
			title: 'Diese Adresse hat ihr Datenvolumen für diesen Monat verbraucht',
			body: 'Dieses nolune hat das monatliche Datenvolumen seiner Adresse überschritten. Ab dem 1. des nächsten Monats funktioniert es wieder.'
		},
		blocked: {
			title: 'Diese Adresse ist gesperrt',
			body: 'Sie wurde gesperrt, weil sie gegen die Regeln des Dienstes verstoßen hat, über den sie läuft.'
		}
	},
	es: {
		offline: {
			title: 'nolune está desconectado',
			body: 'El ordenador en el que funciona nolune no está conectado ahora mismo. Comprueba que esté encendido y sin suspender, y que nolune se esté ejecutando en él. Esta página se recargará sola.'
		},
		unknown: {
			title: 'Aquí no hay ningún nolune',
			body: 'No hay nada registrado en esta dirección. Comprueba la dirección que te dieron.'
		},
		failed: {
			title: 'nolune no respondió',
			body: 'Algo salió mal por el camino hacia nolune. Inténtalo de nuevo en un momento.'
		},
		quota: {
			title: 'Esta dirección ha gastado el tráfico de este mes',
			body: 'Este nolune ha superado el tráfico mensual de su dirección. Volverá a funcionar a partir del día 1 del mes que viene.'
		},
		blocked: {
			title: 'Esta dirección está bloqueada',
			body: 'Se bloqueó por incumplir las normas del servicio a través del que funciona.'
		}
	},
	fr: {
		offline: {
			title: 'nolune est hors ligne',
			body: "L'ordinateur sur lequel tourne nolune n'est pas connecté pour le moment. Vérifiez qu'il est allumé et pas en veille, et que nolune y fonctionne. Cette page se recharge toute seule."
		},
		unknown: {
			title: 'Aucun nolune ici',
			body: "Rien n'est enregistré à cette adresse. Vérifiez l'adresse qu'on vous a donnée."
		},
		failed: {
			title: "nolune n'a pas répondu",
			body: "Quelque chose s'est mal passé en chemin vers nolune. Réessayez dans un instant."
		},
		quota: {
			title: 'Cette adresse a épuisé son trafic du mois',
			body: 'Ce nolune a dépassé le trafic mensuel de son adresse. Il fonctionnera de nouveau à partir du 1er du mois prochain.'
		},
		blocked: {
			title: 'Cette adresse est bloquée',
			body: 'Elle a été bloquée pour non-respect des règles du service par lequel elle passe.'
		}
	}
};

/** The first language in Accept-Language, by weight, that the pages have; else English. */
export function pageLanguage(acceptLanguage: string | undefined): string {
	const ranked = (acceptLanguage ?? '')
		.split(',')
		.map((part, index) => {
			const [tag, ...params] = part.trim().split(';');
			const q = params.map((p) => /^\s*q=([\d.]+)\s*$/.exec(p)?.[1]).find(Boolean);
			return { language: tag.split('-')[0].toLowerCase(), q: q ? Number(q) : 1, index };
		})
		.filter(({ q }) => q > 0)
		.sort((a, b) => b.q - a.q || a.index - b.index);
	return ranked.find(({ language }) => Object.hasOwn(TEXTS, language))?.language ?? 'en';
}

export function page(kind: PageKind, acceptLanguage: string | undefined): string {
	const language = pageLanguage(acceptLanguage);
	const { title, body } = TEXTS[language][kind];
	// An offline nolune may come back: check again now and then.
	const refresh = kind === 'offline' ? '\n<meta http-equiv="refresh" content="20">' : '';
	return `<!doctype html>
<html lang="${language}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">${refresh}
<title>${title}</title>
<style>
	:root { color-scheme: light dark; --bg: #ffffff; --fg: #212121; --muted: #6b6b6b; }
	@media (prefers-color-scheme: dark) { :root { --bg: #212121; --fg: #ececec; --muted: #a3a3a3; } }
	body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: var(--bg);
		color: var(--fg); font: 16px/1.5 system-ui, -apple-system, 'Segoe UI', sans-serif; }
	main { max-width: 28rem; padding: 2rem 1rem; text-align: center; }
	h1 { font-size: 1.375rem; font-weight: 600; margin: 0 0 0.5rem; }
	p { margin: 0; color: var(--muted); }
</style>
</head>
<body>
<main>
<h1>${title}</h1>
<p>${body}</p>
</main>
</body>
</html>
`;
}
