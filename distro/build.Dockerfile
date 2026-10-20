# Host toolchain image for one-command Buildroot builds.
FROM debian:bookworm-slim

RUN apt-get update \
	&& DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
		bc \
		bison \
		build-essential \
		ca-certificates \
		cpio \
		file \
		flex \
		gawk \
		git \
		libelf-dev \
		libncurses-dev \
		python3 \
		rsync \
		unzip \
		wget \
		xz-utils \
	&& rm -rf /var/lib/apt/lists/*

WORKDIR /work
