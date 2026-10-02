# nolune relay

Gives each family's nolune a public HTTPS address, like `https://smiths.nolune.family`, without a
tunnel, port forwarding or a domain of their own. The family runs `nolune relay enable` (or answers
yes in `nolune setup`), and their gateway keeps a connection open to the relay. When anyone opens
the address, the relay passes the request down that connection and the answer back.

```
browser ──HTTPS──▶ Caddy ──▶ relay ══ WebSocket (HTTP/2 inside) ══▶ gateway ──▶ nolune's web server
                  (TLS)      one per gateway, opened by the gateway      on the family's computer
```

- **Registering.** `POST /api/gateways` with an optional `{"name": "smiths"}` answers with the name
  (a random one like `cozy-otter-42` without it), the address and a token, shown only this once.
  The relay keeps a SHA-256 of the token. `GET` and `DELETE /api/gateways/<name>` with
  `Authorization: Bearer <token>` say whether the gateway is connected and give the name back.
- **Connecting.** The gateway opens a WebSocket to `/api/connect` on the relay's host and says
  `hello` with its name and token; the relay answers `ready`. The binary messages that follow are
  an HTTP/2 connection in which the relay is the client, so many requests share the one socket and
  flow control keeps a large upload from holding up anything else. See `src/protocol.ts`.
- **Passing requests on.** A request for `<name>.<domain>` becomes an HTTP/2 stream to that gateway,
  streamed both ways (uploads, downloads, event streams). The relay sets `X-Forwarded-For`,
  `-Proto` and `-Host` itself and drops what the browser sent in them. WebSocket upgrades aren't
  passed on: nolune's pages use event streams.
- **Opened in a browser.** The relay's host and the domain without a name (`relay.nolune.dev`,
  `nolune.family`) send people to the site; `/api/health` answers `ok`, for monitoring.
- **When a gateway isn't there.** For 15 seconds after a gateway leaves (a restart, a new
  network), requests wait for it to come back. After that they get a small page, in the browser's
  language, saying nolune is offline (it reloads itself), or that nothing is at an address nobody
  registered.
- **Staying connected.** The relay pings each gateway every 30 seconds and drops one that doesn't
  answer; the gateway pings the relay too, and reconnects after a moment, then less and less often
  while the relay can't be reached. A second connection with the same token takes over from the
  first, which then stops, so two computers can't fight over one address.
- **Limits.** Anyone can register, and anyone can write a client that speaks the protocol, so the
  relay keeps what one person can take in check. Each address may pass 30 GB a month through the
  relay, both ways, counted by calendar month in UTC; a family rarely comes near it. Past it, the
  address shows a page saying its traffic is used up until the 1st, and `nolune relay status` says
  so. Each network (an IPv4 address, or an IPv6 /64) may register 10 addresses an hour and have 10
  at once. Names that belong to a site, like `www`, `api` or `login`, are reserved
  (`src/names.ts`).

## Privacy

TLS ends at the relay, as it does at any tunnel provider (ngrok, Cloudflare Tunnel): whoever runs
the relay could read what passes through. The relay keeps none of it; it logs only registrations
and gateways connecting and leaving. Families who'd rather no one in between could read their
traffic use a tunnel they run themselves, or run their own relay (`nolune relay enable --server`).

## Running it

On a small server with a public IP (1 vCPU and 1 GB is plenty for many families; traffic is what
grows). The relay runs from its TypeScript source on Node 22.18 or later, with one dependency
(`ws`).

1. **DNS.** Point `*.<domain>`, the domain itself and the relay's host at the server, with A (and
   AAAA) records. For nolune's own relay: `*.nolune.family`, `nolune.family` and
   `relay.nolune.dev`. The families' addresses have a
   domain of their own, apart from the site on `nolune.dev`: a browser blocklist that takes in one
   abused address can take its whole domain with it, and an address can set cookies for its
   domain. In Cloudflare, leave the records **DNS only** (grey cloud): proxied, Cloudflare would
   end TLS itself and limit uploads to 100 MB.
2. **A Cloudflare API token** that can edit DNS in both zones (Zone → DNS → Edit, for
   `nolune.family` and `nolune.dev`): Let's Encrypt checks the certificates with DNS records, which
   Caddy makes.
3. **Start it**, from this folder:

   ```sh
   cat > .env <<'END'
   RELAY_DOMAIN=nolune.family
   RELAY_HOST=relay.nolune.dev
   ACME_EMAIL=you@example.com
   CLOUDFLARE_API_TOKEN=...
   END
   docker compose up -d --build
   ```

   `curl https://relay.nolune.dev/api/health` should say `ok`.

`compose.yaml` runs the relay (`Dockerfile`) behind Caddy (`Caddy.Dockerfile`, Caddy with the
Cloudflare DNS module, and `Caddyfile`). The relay's settings:

| Variable                  | Meaning                                                                                                                                     |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `RELAY_DOMAIN`            | Gateways get `<name>.<domain>`. Required.                                                                                                   |
| `RELAY_HOST`              | Where gateways register and connect. The domain itself by default.                                                                          |
| `RELAY_SITE_URL`          | Where a browser that opens the relay's host, or the domain without a name, goes. `https://nolune.dev` by default; empty for a line of text. |
| `RELAY_DATA`              | The gateways file. `/data/gateways.json` in the image.                                                                                      |
| `RELAY_MONTHLY_GB`        | Traffic each address may pass in a month. 30 by default; 0: no limit.                                                                       |
| `RELAY_MAX_PER_NETWORK`   | Addresses one network may have. 10 by default; 0: no limit.                                                                                 |
| `RELAY_FORGET_AFTER_DAYS` | Days an address's nolune may stay away before its name is free again. 90 by default; 0: names stay taken.                                   |
| `RELAY_ADMIN_SOCKET`      | The operator's socket. `admin.sock` next to the gateways file.                                                                              |
| `RELAY_TRUST_PROXY`       | `1`: the client's address is the last in `X-Forwarded-For` (Caddy's).                                                                       |
| `RELAY_SCHEME`            | `http` to try it without TLS; addresses are `https` otherwise.                                                                              |
| `HOST`, `PORT`            | Where it listens. `0.0.0.0:8080` by default.                                                                                                |

To try it on your own computer, without TLS:

```sh
RELAY_DOMAIN=nolune.localhost RELAY_HOST=127.0.0.1 RELAY_SCHEME=http PORT=8090 pnpm --filter @nolune/relay start
NOLUNE_RELAY_SERVER=http://127.0.0.1:8090 pnpm nolune relay enable --name smiths
pnpm build && pnpm start
curl -H 'Host: smiths.nolune.localhost' http://127.0.0.1:8090/login
```

If Caddy's log says a certificate couldn't be had: `SERVFAIL` or `NXDOMAIN` means the name's DNS
record is missing or in the wrong zone (check it with `dig +short relay.nolune.dev`); an error from
Cloudflare's API means the token can't edit that zone. Caddy tries again by itself, every minute at
first; `docker compose restart caddy` tries at once.

## nolune's API next to it

nolune's own server also runs nolune's API (`packages/api`, the nolune plan's sign-in, limits,
models and payments; DESIGN.md, The nolune plan) behind the same Caddy, at `api.nolune.dev`.
`compose.api.yaml` adds it to `compose.yaml`: the API, its Postgres, and a service that backs the
database up each day. A relay alone is just `compose.yaml`, as before.

The API's image isn't built on the server: building it (`packages/api/Dockerfile`) wants close to
a gigabyte of memory, and a 1 GB server running the relay has about half that free, so the
kernel could stop the relay to make room. GitHub Actions builds it on each change
(`.github/workflows/api-image.yml`) into `ghcr.io/triangle-int/nolune-api`, and the server pulls
it: `latest` from `main`, a branch's own tag (`claude-nolune-plan-design`), and each commit's
(`sha-1234567`). The package is public, as the repository is, so the server pulls it without signing
in. GitHub makes a new package private: an organization owner allows public ones (the
organization's settings, Packages, Package creation), and then the package's settings change it
(Change visibility). A private one works too, once the server has signed in to the registry
with a classic token that can only read packages:
`echo <token> | docker login ghcr.io -u <user> --password-stdin`. The image is `linux/amd64`, for
the relay's server.

1. **DNS.** An A (and AAAA) record for `api.nolune.dev`, **DNS only** like the others. The
   Cloudflare token edits `nolune.dev` already, which Caddy needs for the API's certificate too
   (`api.caddy`).
2. **Settings.** Add to `.env`:

   ```sh
   COMPOSE_FILE=compose.yaml:compose.api.yaml
   API_HOST=api.nolune.dev
   API_DB_PASSWORD=...   # openssl rand -hex 24
   ```

   and put the API's secrets in `api.env` (`cp api.env.example api.env && chmod 600 api.env`):
   the session secret, Resend's key, nolune's OpenRouter key and Stripe's. `api.env.example` says
   what each is. The API won't start at an `https://` address without Resend: the sign-in codes
   would have nowhere to go.

3. **Stripe.** A webhook endpoint at `https://api.nolune.dev/stripe/webhook`, API version
   `2026-09-30.endive`, with `invoice.paid`, `checkout.session.completed`,
   `checkout.session.async_payment_succeeded` and `customer.subscription.deleted`; its signing
   secret is `STRIPE_WEBHOOK_SECRET`. A customer portal configuration of nolune's own
   (cancelling, cards and invoices, no switching) is `STRIPE_PORTAL_CONFIGURATION`. Start with
   test-mode keys and move to live ones when it's all been tried.
4. **Start it**: `docker compose up -d`, which pulls the API's image. `./check.sh` then says whether
   `https://api.nolune.dev/health` answers `ok` (the API and its Postgres), and when the last
   backup was.

**Updating it**, once GitHub Actions has built the change:
`docker compose pull api && docker compose up -d api` restarts only the API; the relay and
Postgres go on. It brings its database up to date as it starts (the migrations in
`packages/api/drizzle`). Requests on their way are cut, and a chat stream cut off in the few
minutes before may go uncharged: what it cost is still being asked of OpenRouter. To go back to
an earlier one, set `API_IMAGE` to its `sha-` tag and `docker compose up -d api`; a version
whose migrations have run can't always go back, so restore a backup from before it too.

| Variable          | Meaning                                                                       |
| ----------------- | ----------------------------------------------------------------------------- |
| `API_HOST`        | The API's address. `api.nolune.dev` by default.                               |
| `API_IMAGE`       | The API's image. `ghcr.io/triangle-int/nolune-api:latest` by default.         |
| `API_DB_PASSWORD` | Postgres's password, which the API and the backups use. Required.             |
| `API_BACKUP_DAYS` | Days of daily backups kept in `backups/`. 14 by default.                      |
| `COMPOSE_FILE`    | `compose.yaml:compose.api.yaml` runs the API; `docker compose` reads it here. |

### Its backups

The database is people's payments and what they have left, so it's backed up: a dump when the
backup service starts and each day after, in `backups/` (`nolune_api-<date>.dump`, pg_dump's
custom format), for `API_BACKUP_DAYS` days. Copy them off the server too, with rsync, rclone or
your host's snapshots: a dump on a server that's lost is lost with it.

To bring one back (over what's there):

```sh
docker compose stop api
docker compose exec -T postgres dropdb -U nolune nolune_api
docker compose exec -T postgres createdb -U nolune nolune_api
docker compose exec -T postgres pg_restore -U nolune -d nolune_api --no-owner < backups/nolune_api-<date>.dump
docker compose start api
```

Payments made since the dump are still in Stripe: sending their events again from the Dashboard
(each event's page, or the webhook's; Stripe keeps events for 30 days) grants them, once each.

## Looking after it

- **Is it all right?** `./check.sh` in this folder, on the server, answers in a few lines: whether
  the containers run, the certificates and until when, whether the relay (and the API) answers,
  the API's last backup, and the last errors in the logs.
- **The logs** are lines to read (Caddy's in its console format, the relay's as `[relay] ...`):
  `docker compose logs -f relay` for addresses coming and going, `docker compose logs -f caddy` for
  certificates, `docker compose logs --since 1h` for the last hour of both. The API's are
  `docker compose logs -f api`: it writes only what went wrong, as `[nolune api] ...`.

- **Back up** the gateways file (the `relay-data` volume). Without it, every family's nolune
  asks for its name again when it next connects, and gets it unless someone took it first.
- **Names nobody uses** are free again after 90 days without their nolune connecting
  (`RELAY_FORGET_AFTER_DAYS`); the log says `forgot <name>`. Blocked ones are kept. A nolune that
  comes back after that asks for its name again, and stops using it if someone else has it.
- **The operator's commands** talk to the running relay over a Unix socket that only its own user
  can open, never over the web:

  ```sh
  docker compose exec relay node --no-warnings src/admin.ts list
  pnpm --filter @nolune/relay admin list      # a relay run from source, from the repository
  ```

  | Command                    | What it does                                                                                                                                      |
  | -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
  | `list`                     | Every address: connected or not, its traffic this month, when it was last seen.                                                                   |
  | `show <name>`              | One address.                                                                                                                                      |
  | `block <name> [reason...]` | Takes an address off the relay: visitors see that it's blocked, and its nolune can't connect. Its owner sees the reason in `nolune relay status`. |
  | `unblock <name>`           | Lets it back.                                                                                                                                     |
  | `remove <name>`            | Forgets an address, so its name is free again. Its nolune, if it's still around, asks for it back when it next connects; `block` keeps it off.    |

- **Families behind one address.** Mobile networks and some home ISPs put many customers behind
  one IPv4 address (CGNAT), and they share its 10 addresses. If people run into it, raise
  `RELAY_MAX_PER_NETWORK`.
- Add the domain to the [Public Suffix List](https://publicsuffix.org/), as tunnel providers do,
  so browsers treat every address as a site of its own: one family's nolune then can't set cookies
  for another's, and a blocklist takes in only the address it's about. Nothing but families'
  addresses lives on `nolune.family`, so nothing else is affected.
