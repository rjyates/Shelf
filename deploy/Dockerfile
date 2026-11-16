# Shelf server image: Rust binary + Book Orbit reader + in-process ghost-story (CTC).
# One container = full Combine/readalong. No sidecar, no Whisper fallback.
#
# GPU variants (Storyteller-style tags):
#   SHELF_GPU_VARIANT=cpu  → ghcr.io/.../shelf-server:latest (default)
#   SHELF_GPU_VARIANT=cuda → ...:cuda  (NVIDIA ORT EP; pass nvidia devices)
#   SHELF_GPU_VARIANT=rocm → ...:rocm  (AMD WebGPU/Mesa; pass /dev/dri + /dev/kfd)
# ghost-story advertise cpu|cuda|webgpu — there is no separate ROCm ORT EP in upstream;
# the :rocm tag means AMD GPU via WebGPU + Mesa Vulkan, matching Storyteller's AMD compose devices.
# :cuda/:rocm pull mesa-vulkan-drivers from bookworm-backports (Mesa ≥25) so RDNA3.5
# iGPUs (gfx1150/1151/1152 — 860M/880M/890M) work; bookworm main Mesa 22.3 cannot.
ARG SHELF_GPU_VARIANT=cpu

FROM node:24-bookworm AS reader
WORKDIR /src/web/orbit-reader
COPY web/orbit-reader/package.json web/orbit-reader/package-lock.json ./
RUN npm ci
COPY web/orbit-reader/ ./
RUN npm run build

FROM rust:1.99.1-bookworm AS build
WORKDIR /src
COPY server/Cargo.toml server/Cargo.lock server/
# Dependency crates stay cached when only the application sources change.
RUN mkdir server/src \
    && printf 'fn main() {}\n' > server/src/main.rs \
    && : > server/src/lib.rs \
    && cargo build --manifest-path server/Cargo.toml --locked --release
COPY server/ server/
COPY web/ web/
COPY --from=reader /src/web/orbit-reader/dist web/orbit-reader/dist
ARG VERSION=dev
ENV SHELF_VERSION=${VERSION}
RUN find server/src server/schema.sql web -type f -exec touch {} + \
    && cargo build --manifest-path server/Cargo.toml --locked --release

FROM node:24-bookworm-slim
ARG SHELF_GPU_VARIANT=cpu
LABEL org.opencontainers.image.source="https://github.com/rjyates/Shelf"
LABEL org.opencontainers.image.licenses="AGPL-3.0-only"
LABEL org.opencontainers.image.title="shelf-server"
LABEL shelf.gpu.variant="${SHELF_GPU_VARIANT}"

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl ffmpeg util-linux \
    && case "${SHELF_GPU_VARIANT}" in \
         cuda|rocm) \
           printf 'deb http://deb.debian.org/debian bookworm-backports main\n' \
             >/etc/apt/sources.list.d/backports.list \
           && apt-get update \
           && apt-get install -y --no-install-recommends -t bookworm-backports \
                libvulkan1 mesa-vulkan-drivers vulkan-tools ;; \
       esac \
    && rm -rf /var/lib/apt/lists/* \
    && npm install -g @storyteller-platform/ghost-story@^0.3.4 \
    && groupadd --system --gid 10001 shelf \
    && useradd --system --uid 10001 --gid 10001 --create-home shelf \
    && case "${SHELF_GPU_VARIANT}" in \
         cuda|rocm) \
           groupadd -f -g 987 render; groupadd -f -g 983 video ;; \
       esac \
    && mkdir -p /data /books && chown shelf:shelf /data /books

COPY --from=build /src/server/target/release/shelf-server /usr/local/bin/shelf-server
COPY LICENSE /usr/share/licenses/shelf/LICENSE
COPY web/fonts/*LICENSE.txt /usr/share/licenses/shelf/fonts/
COPY web/orbit-reader/vendor/bookorbit/ATTRIBUTION.md /usr/share/licenses/shelf/reader/BOOKORBIT-ATTRIBUTION.md
COPY third_party/storyteller-align/NOTICE.md /usr/share/licenses/shelf/align/STORYTELLER-ALIGN-NOTICE.md
COPY third_party/storyteller-align/LICENSE.txt /usr/share/licenses/shelf/align/STORYTELLER-ALIGN-LICENSE.txt
COPY deploy/entrypoint.sh /usr/local/bin/shelf-entrypoint
RUN chmod 755 /usr/local/bin/shelf-entrypoint

# CTC emit talks to ghost-story on loopback inside this container (started by entrypoint).
ENV SHELF_DATA=/data \
    SHELF_LISTEN=0.0.0.0:8080 \
    SHELF_CTC_EMIT_URL=http://127.0.0.1:8765 \
    SHELF_CTC_EMIT_API_KEY=shelf-internal \
    GHOST_STORY_API_KEY=shelf-internal \
    GHOST_STORY_HOST=127.0.0.1 \
    GHOST_STORY_PORT=8765 \
    SHELF_GPU_VARIANT=${SHELF_GPU_VARIANT} \
    NVIDIA_VISIBLE_DEVICES=all \
    NVIDIA_DRIVER_CAPABILITIES=compute,utility,graphics

RUN case "${SHELF_GPU_VARIANT}" in \
      cuda) echo cuda >/etc/shelf-gpu-default-device ;; \
      rocm) echo webgpu >/etc/shelf-gpu-default-device ;; \
      *) echo cpu >/etc/shelf-gpu-default-device ;; \
    esac

EXPOSE 8080
# First boot may download the MMS CTC model into /data/ghost-story (~1.2GB).
HEALTHCHECK --interval=30s --timeout=3s --start-period=180s \
  CMD curl --fail --silent http://127.0.0.1:8080/api/v1/health || exit 1
ENTRYPOINT ["/usr/local/bin/shelf-entrypoint"]
