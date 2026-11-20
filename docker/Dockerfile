FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive
ARG GHIDRA_VERSION=12.1.4
ARG GHIDRA_DATE=20260921
ARG REA_VERSION=6.7.0

# System utilities, native toolchain, locales (Shift-JIS / Japanese support), and Node.js prerequisites
RUN sed -i 's|http://archive.ubuntu.com/ubuntu|http://mirrors.tuna.tsinghua.edu.cn/ubuntu|g; s|http://security.ubuntu.com/ubuntu|http://mirrors.tuna.tsinghua.edu.cn/ubuntu|g' /etc/apt/sources.list.d/ubuntu.sources \
    && apt-get -o Acquire::Retries=5 update && apt-get -o Acquire::Retries=5 install -y --no-install-recommends \
    ca-certificates \
    curl \
    wget \
    unzip \
    xz-utils \
    git \
    ripgrep \
    jq \
    file \
    binutils \
    gdb \
    python3 \
    python3-pip \
    python3-venv \
    build-essential \
    cmake \
    ninja-build \
    pkg-config \
    locales \
    && locale-gen en_US.UTF-8 ja_JP.UTF-8 \
    && rm -rf /var/lib/apt/lists/*

# Install Node.js 22.x LTS (required by rea-agents)
RUN curl --retry 5 --retry-delay 2 -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get -o Acquire::Retries=5 update && apt-get -o Acquire::Retries=5 install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# Install Eclipse Temurin JDK 25 (required by Ghidra 12.1.x)
RUN mkdir -p /opt/jdk \
    && curl --retry 5 --retry-delay 2 --retry-all-errors -fsSL -o /tmp/jdk25.tar.gz \
       "https://api.adoptium.net/v3/binary/latest/25/ga/linux/x64/jdk/hotspot/normal/eclipse" \
    && tar -xzf /tmp/jdk25.tar.gz -C /opt/jdk --strip-components=1 \
    && rm -f /tmp/jdk25.tar.gz

# Install Ghidra 12.1.4
RUN curl --retry 5 --retry-delay 2 --retry-all-errors -fsSL -o /tmp/ghidra.zip \
       "https://github.com/NationalSecurityAgency/ghidra/releases/download/Ghidra_${GHIDRA_VERSION}_build/ghidra_${GHIDRA_VERSION}_PUBLIC_${GHIDRA_DATE}.zip" \
    && unzip -q /tmp/ghidra.zip -d /opt \
    && mv "/opt/ghidra_${GHIDRA_VERSION}_PUBLIC" /opt/ghidra \
    && chmod -R a+rX /opt/ghidra \
    && rm -f /tmp/ghidra.zip

# Install REA (rea-agents) globally inside the container
RUN npm install -g "rea-agents@${REA_VERSION}" \
    && npm cache clean --force

# Install Rust (rustup minimal profile) + rustfmt/clippy + aarch64-apple-ios + cbindgen
# chown inside the same RUN layer to avoid duplicating /opt/rustup and /opt/cargo in OverlayFS
ENV RUSTUP_HOME=/opt/rustup
ENV CARGO_HOME=/opt/cargo
ENV PATH="/opt/cargo/bin:/opt/jdk/bin:${PATH}"

ENV RUSTUP_DIST_SERVER=https://rsproxy.cn
ENV RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup

RUN mkdir -p /opt/cargo \
    && printf '[source.crates-io]\nreplace-with = "rsproxy-sparse"\n\n[source.rsproxy-sparse]\nregistry = "sparse+https://rsproxy.cn/index/"\n\n[net]\ngit-fetch-with-cli = true\n' > /opt/cargo/config.toml \
    && curl --proto '=https' --tlsv1.2 -fSL \
      --retry 5 --retry-delay 2 --connect-timeout 15 --speed-limit 1024 --speed-time 30 \
      -o /tmp/rustup-init \
      "https://rsproxy.cn/rustup/dist/x86_64-unknown-linux-gnu/rustup-init" \
    && chmod +x /tmp/rustup-init \
    && /tmp/rustup-init -y --default-toolchain stable --profile minimal --no-modify-path \
    && rm -f /tmp/rustup-init \
    && rustup component add rustfmt clippy \
    && rustup target add aarch64-apple-ios \
    && cargo install --locked cbindgen \
    && rm -rf /opt/cargo/registry/cache /opt/cargo/registry/src \
    && chown -R ubuntu:ubuntu /opt/rustup /opt/cargo

ENV JAVA_HOME=/opt/jdk
ENV GHIDRA_INSTALL_DIR=/opt/ghidra
ENV LANG=C.UTF-8
ENV LC_ALL=C.UTF-8
ENV REA_GHIDRA_STARTUP_TIMEOUT_MS=600000

# Prepare non-root user (ubuntu UID/GID 1000 matches typical WSL user)
RUN mkdir -p /workspace /home/ubuntu/.ghidra /tmp/rea \
    && chown -R ubuntu:ubuntu /workspace /home/ubuntu /tmp/rea

USER ubuntu
WORKDIR /workspace

CMD ["sleep", "infinity"]
