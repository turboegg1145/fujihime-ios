FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive
ARG GHIDRA_VERSION=12.1.4
ARG GHIDRA_DATE=20260921
ARG REA_VERSION=6.7.0

# System utilities, native toolchain, locales (Shift-JIS / Japanese support), and Node.js prerequisites
RUN apt-get update && apt-get install -y --no-install-recommends \
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
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get update && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# Install Eclipse Temurin JDK 25 (required by Ghidra 12.1.x)
RUN mkdir -p /opt/jdk \
    && curl -fsSL -o /tmp/jdk25.tar.gz \
       "https://api.adoptium.net/v3/binary/latest/25/ga/linux/x64/jdk/hotspot/normal/eclipse" \
    && tar -xzf /tmp/jdk25.tar.gz -C /opt/jdk --strip-components=1 \
    && rm -f /tmp/jdk25.tar.gz

# Install Ghidra 12.1.4
RUN curl -fsSL -o /tmp/ghidra.zip \
       "https://github.com/NationalSecurityAgency/ghidra/releases/download/Ghidra_${GHIDRA_VERSION}_build/ghidra_${GHIDRA_VERSION}_PUBLIC_${GHIDRA_DATE}.zip" \
    && unzip -q /tmp/ghidra.zip -d /opt \
    && mv "/opt/ghidra_${GHIDRA_VERSION}_PUBLIC" /opt/ghidra \
    && chmod -R a+rX /opt/ghidra \
    && rm -f /tmp/ghidra.zip

# Install REA (rea-agents) globally inside the container
RUN npm install -g "rea-agents@${REA_VERSION}" \
    && npm cache clean --force

ENV JAVA_HOME=/opt/jdk
ENV GHIDRA_INSTALL_DIR=/opt/ghidra
ENV PATH="/opt/jdk/bin:${PATH}"
ENV LANG=C.UTF-8
ENV LC_ALL=C.UTF-8
ENV REA_GHIDRA_STARTUP_TIMEOUT_MS=600000

# Prepare non-root user (ubuntu UID/GID 1000 matches typical WSL user)
RUN mkdir -p /workspace /home/ubuntu/.ghidra /tmp/rea \
    && chown -R ubuntu:ubuntu /workspace /home/ubuntu /tmp/rea

USER ubuntu
WORKDIR /workspace

CMD ["sleep", "infinity"]
