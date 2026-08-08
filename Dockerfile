# Multi-stage build: Angular frontend + Rust backend in one image (the backend
# serves the bundle and the API). Mirrors the fleet's xinutec/<app>:latest
# convention — see kubes/tasks/k8s.
#
# The image carries the application and never the work. Tasks live in MariaDB,
# which is the sidecar the Dhall model declares; nothing about anybody's list is
# baked in, published to Docker Hub, or committed. That is what makes this repo
# safe to have public.

# --- frontend ---
FROM node:24-alpine AS frontend
WORKDIR /fe
# pnpm-workspace.yaml belongs in this layer, not with the sources: it carries the
# install-script allowlist, and without it esbuild never unpacks its binary and
# the ui-harness never builds itself from the TypeScript it ships.
COPY frontend/package.json frontend/pnpm-lock.yaml frontend/pnpm-workspace.yaml ./
# git: the shared layout harness is a git dependency (github:xinutec/ui-harness),
# so the install clones it — node:alpine ships no git.
#
# pnpm is taken unpinned. The host gets its copy from the flake, and pinning a
# second version here would be two numbers held level by hand; the lockfile is
# the thing that has to match, and --frozen-lockfile fails rather than drift.
RUN apk add --no-cache git ca-certificates \
    && npm install -g pnpm \
    && pnpm install --frozen-lockfile
COPY frontend/ .
# Stamp the build into the bundle (frontend/scripts/stamp-version.mjs), so the
# page can say which build it is. The context has no .git, so the commit comes
# from GIT_SHA — passed by CI, and 'dev' for a plain local build.
ARG GIT_SHA=dev
RUN GIT_SHA="$GIT_SHA" node scripts/stamp-version.mjs
# ⚠ `ng build` directly, so npm's prebuild hook does not run — which is why the
# stamp is its own step above.
RUN pnpm exec ng build --configuration production

# --- backend (deps cached in their own layer) ---
FROM rust:1-bookworm AS backend
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
# A stub source is enough to prime the dependency cache. Both targets, because
# the manifest names a `default-run` and a manifest whose default target does
# not exist fails to parse before any of this compiles.
RUN mkdir -p src/bin \
    && echo 'fn main() {}' > src/main.rs && echo '' > src/lib.rs \
    && echo 'fn main() {}' > src/bin/task.rs \
    && cargo build --release && rm -rf src
COPY src/ src/
# `migrations/` is compiled IN, not read at runtime: `sqlx::migrate!()` embeds
# the directory at build time, so a missing copy here is a binary that starts
# against an empty database and creates nothing.
COPY migrations/ migrations/
# `touch` so the real sources are newer than the primed artefacts and actually
# rebuild.
RUN touch src/main.rs src/lib.rs src/bin/task.rs && cargo build --release

# --- runtime ---
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
# 65532 is the conventional "nonroot" id, matched by the Dhall model's `uid`.
RUN groupadd --gid 65532 tasks \
    && useradd --uid 65532 --gid tasks --no-create-home --shell /usr/sbin/nologin tasks
WORKDIR /app
COPY --from=backend /app/target/release/tasks /usr/local/bin/tasks
# The CLI ships too. It is the surface a Claude session uses, and a session runs
# on the Mac — but having it in the image means a `kubectl exec` can read and
# move a task when the Mac cannot be reached, which is exactly the situation
# where somebody wants to.
COPY --from=backend /app/target/release/task /usr/local/bin/task
COPY --from=frontend /fe/dist/tasks-web/browser ./public
ENV STATIC_DIR=/app/public \
    BIND_ADDR=0.0.0.0:8092
USER tasks
EXPOSE 8092
CMD ["tasks"]
