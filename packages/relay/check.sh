#!/bin/sh
# Is the relay all right? On the server, in this folder: ./check.sh
# It looks from outside, as a browser would: whether the containers run, the certificates, whether
# the relay answers, and the last problems in the logs.

cd "$(dirname "$0")" || exit 1

# A setting from .env, else its default, as compose.yaml has it.
setting() {
	value=$(sed -n "s/^$1=//p" .env 2>/dev/null | tail -n 1)
	echo "${value:-$2}"
}
domain=$(setting RELAY_DOMAIN nolune.family)
host=$(setting RELAY_HOST relay.nolune.dev)

echo "Containers"
docker compose ps --all --format '  {{.Service}}: {{.State}} ({{.Status}})'

echo
echo "Certificates"
# check.<domain> stands for every family's address: they share the wildcard certificate.
for name in "$host" "$domain" "check.$domain"; do
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

echo
echo "Errors in the last 15 minutes"
# Errors only: Caddy's (in its console format, or JSON from before it), and the relay's failed
# requests. Caddy's notes that merely mention a failure, like the one about UDP buffers, aren't.
problems=$(docker compose logs --since 15m --no-log-prefix caddy relay 2>/dev/null |
	grep -E '[[:space:]]ERROR[[:space:]]|"level":"error"|^\[relay\] request failed' |
	tail -n 5 | cut -c1-240)
if [ -n "$problems" ]; then
	echo "$problems" | sed 's/^/  /'
else
	echo "  none"
fi
