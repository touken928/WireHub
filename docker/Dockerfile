# Build frontend on the native CI arch (avoid slow npm under QEMU).
FROM --platform=$BUILDPLATFORM node:22-alpine AS frontend
WORKDIR /app/web
COPY web/package*.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# Cross-compile Go binary for TARGETOS/TARGETARCH on the build host.
FROM --platform=$BUILDPLATFORM golang:1.26-alpine AS backend
ARG TARGETOS
ARG TARGETARCH
WORKDIR /app
RUN apk add --no-cache git
COPY go.mod go.sum ./
RUN go mod download
COPY . .
COPY --from=frontend /app/internal/static/dist ./internal/static/dist
RUN CGO_ENABLED=0 GOOS=${TARGETOS} GOARCH=${TARGETARCH} \
    go build -ldflags="-s -w" -o /wirehub ./cmd/wirehub

FROM alpine:3.20
RUN apk add --no-cache ca-certificates
WORKDIR /app
COPY --from=backend /wirehub /usr/local/bin/wirehub
RUN mkdir -p /app/data
VOLUME /app/data
EXPOSE 8443 8443/udp
ENTRYPOINT ["wirehub"]
