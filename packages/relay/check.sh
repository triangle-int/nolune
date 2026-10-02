#!/bin/sh
# Is the relay all right? On the server, in this folder: ./check.sh
# It looks from outside, as a browser would: whether the containers run, the certificates, whether
# the relay (and nolune's API, when compose.api.yaml runs it here) answers, the API's last backup,
# and the last problems in the logs.

cd "$(dirname "$0")" || exit 1

# A setting from the environment, else .env, else its default, as docker compose reads them.
setting() {
	value=$(printenv "$1")
	[ -n "$value" ] || value=$(sed -n "s/^$1=//p" .env 2>/dev/null | tail -n 1)
	echo "${value:-$2}"
}
domain=$(setting RELAY_DOMAIN nolune.family)
host=$(setting RELAY_HOST relay.nolune.dev)
api=""
case "$(setting COMPOSE_FILE compose.yaml)" in
*compose.api.yaml*) api=$(setting API_HOST api.nolune.dev) ;;
esac

echo "Containers"
docker compose ps --all --format '  {{.Service}}: {{.State}} ({{.Status}})'

echo
echo "Certificates"
# check.<domain> stands for every family's address: they share the wildcard certificate.
for name in "$host" "$domain" "check.$domain" $api; do
	until=$(echo | openssl s_client -connect "$name:443" -servername "$name" 2>/dev/null |
		openssl x509 -noout -enddate 2>/dev/null | sed 's/^notAfter=//')
	if [ -n "$until" ]; then
		echo "  ok       $name, until $until"
	else
		echo "  MISSING  $name: see the errors below"
	fi
done

echo
echo "Relay"
if [ "$(curl -s -m 10 "https://$host/api/health")" = "ok" ]; then
	echo "  ok       https://$host answers"
else
	echo "  DOWN     https://$host/api/health doesn't answer ok"
fi

if [ -n "$api" ]; then
	echo
	echo "nolune's API"
	if [ "$(curl -s -m 10 "https://$api/health")" = "ok" ]; then
		echo "  ok       https://$api answers, and its database too"
	else
		echo "  DOWN     https://$api/health doesn't answer ok"
	fi
	# The backup service dumps the database each day: the newest dump should be a day old at most.
	newest=$(ls -t backups/nolune_api-*.dump 2>/dev/null | head -n 1)
	if [ -z "$newest" ]; then
		echo "  MISSING  no backup in backups/"
	elif [ -n "$(find "$newest" -mmin +1560)" ]; then
		echo "  OLD      the newest backup is $newest: see the backup service's log"
	else
		echo "  ok       backup $newest ($(du -h "$newest" | cut -f1))"
	fi
fi

echo
echo "Errors in the last 15 minutes"
# Errors only: Caddy's (in its console format, or JSON from before it), the relay's failed
# requests, and the API's (everything it logs is a problem) and its backups'. Caddy's notes that
# merely mention a failure, like the one about UDP buffers, aren't.
problems=$(docker compose logs --since 15m --no-log-prefix caddy relay ${api:+api backup} 2>/dev/null |
	grep -E '[[:space:]]ERROR[[:space:]]|"level":"error"|^\[relay\] request failed|^\[nolune api\]|^\[backup\] ERROR' |
	tail -n 5 | cut -c1-240)
if [ -n "$problems" ]; then
	echo "$problems" | sed 's/^/  /'
else
	echo "  none"
fi
