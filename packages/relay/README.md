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

1. **DNS.** Point `*.<domain>` and the relay's host at the server, with A (and AAAA) records. For
   nolune's own relay: `*.nolune.family` and `relay.nolune.dev`. The families' addresses have a
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

   `curl https://relay.nolune.dev` should say `nolune relay`.

`compose.yaml` runs the relay (`Dockerfile`) behind Caddy (`Caddy.Dockerfile`, Caddy with the
Cloudflare DNS module, and `Caddyfile`). The relay's settings:

| Variable                | Meaning                                                               |
| ----------------------- | --------------------------------------------------------------------- |
| `RELAY_DOMAIN`          | Gateways get `<name>.<domain>`. Required.                             |
| `RELAY_HOST`            | Where gateways register and connect. The domain itself by default.    |
| `RELAY_DATA`            | The gateways file. `/data/gateways.json` in the image.                |
| `RELAY_MONTHLY_GB`      | Traffic each address may pass in a month. 30 by default; 0: no limit. |
| `RELAY_MAX_PER_NETWORK` | Addresses one network may have. 10 by default; 0: no limit.           |
| `RELAY_ADMIN_SOCKET`    | The operator's socket. `admin.sock` next to the gateways file.        |
| `RELAY_TRUST_PROXY`     | `1`: the client's address is the last in `X-Forwarded-For` (Caddy's). |
| `RELAY_SCHEME`          | `http` to try it without TLS; addresses are `https` otherwise.        |
| `HOST`, `PORT`          | Where it listens. `0.0.0.0:8080` by default.                          |

To try it on your own computer, without TLS:

```sh
RELAY_DOMAIN=nolune.localhost RELAY_HOST=127.0.0.1 RELAY_SCHEME=http PORT=8090 pnpm --filter @nolune/relay start
NOLUNE_RELAY_SERVER=http://127.0.0.1:8090 pnpm nolune relay enable --name smiths
pnpm build && pnpm start
curl -H 'Host: smiths.nolune.localhost' http://127.0.0.1:8090/login
```

## Looking after it

- **Back up** the gateways file (the `relay-data` volume). Without it, every family would have to
  run `nolune relay enable` again (it asks for the same name back) and restart nolune; until then
  their gateways try every 5 minutes, in case the file comes back.
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
  | `remove <name>`            | Forgets an address, so its name is free again.                                                                                                    |

- **Families behind one address.** Mobile networks and some home ISPs put many customers behind
  one IPv4 address (CGNAT), and they share its 10 addresses. If people run into it, raise
  `RELAY_MAX_PER_NETWORK`.
- Add the domain to the [Public Suffix List](https://publicsuffix.org/), as tunnel providers do,
  so browsers treat every address as a site of its own: one family's nolune then can't set cookies
  for another's, and a blocklist takes in only the address it's about. Nothing but families'
  addresses lives on `nolune.family`, so nothing else is affected.
