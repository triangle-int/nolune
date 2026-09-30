# The nolune relay (README.md). Built from this folder: docker build -t nolune-relay .
FROM node:22-alpine
WORKDIR /app
ENV NODE_ENV=production
COPY package.json ./
RUN npm install --omit=dev --no-package-lock --no-audit --no-fund
COPY src ./src
RUN mkdir /data && chown node:node /data
USER node
ENV HOST=0.0.0.0 PORT=8080 RELAY_DATA=/data/gateways.json RELAY_TRUST_PROXY=1
VOLUME /data
EXPOSE 8080
CMD ["node", "--no-warnings", "src/main.ts"]
