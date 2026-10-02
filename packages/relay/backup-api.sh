#!/bin/sh
# The backup service of compose.api.yaml: a dump of nolune's API's database (pg_dump's custom
# format, for pg_restore) when it starts and every 24 hours after, in /backups, keeping KEEP_DAYS
# days of them. A dump is written under another name and renamed when it's whole.

set -u
while true; do
	name="/backups/nolune_api-$(date -u +%Y-%m-%dT%H%MZ).dump"
	if pg_dump --format=custom --file="$name.part" && mv "$name.part" "$name"; then
		echo "[backup] $name ($(du -h "$name" | cut -f1))"
	else
		rm -f "$name.part"
		echo "[backup] ERROR: the dump failed; trying again in an hour"
		sleep 3600
		continue
	fi
	find /backups -name 'nolune_api-*.dump' -mtime +"$KEEP_DAYS" -delete
	sleep 86400
done
