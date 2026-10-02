# nolune's API (DESIGN.md, The nolune plan), built from the repository's root:
#   docker build -f packages/api/Dockerfile -t nolune-api .
# packages/relay/compose.api.yaml builds it and runs it next to the relay. Only the API's own files
# go in (Dockerfile.dockerignore), and only what the build doesn't bundle comes out: pg and stripe,
# with the migrations.
FROM node:22-alpine AS build
# The version the lockfile is written with.
RUN npm install --global --no-audit --no-fund pnpm@10.34.6
WORKDIR /repo
COPY . .
RUN pnpm install --frozen-lockfile --filter @nolune/api
RUN pnpm --filter @nolune/api build
RUN pnpm --filter @nolune/api deploy --prod --legacy /deployed

FROM node:22-alpine
WORKDIR /app
ENV NODE_ENV=production
COPY --from=build /deployed/package.json ./
COPY --from=build /deployed/node_modules ./node_modules
COPY --from=build /repo/packages/api/build ./build
COPY --from=build /repo/packages/api/drizzle ./drizzle
USER node
# adapter-node's settings: chats carry their pictures inline (up to 20 MB), and behind Caddy the
# client's address is the last in X-Forwarded-For. ORIGIN and the secrets come from compose.
ENV HOST=0.0.0.0 PORT=3000 BODY_SIZE_LIMIT=32M ADDRESS_HEADER=X-Forwarded-For XFF_DEPTH=1 \
	MIGRATIONS_DIR=/app/drizzle
EXPOSE 3000
CMD ["node", "build"]
