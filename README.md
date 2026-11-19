# 腐姫～euthanasia～ 移植项目

本项目用于对《腐姫～euthanasia～》（DL 版）进行完全原版体验的逆向分析与移植工程。

## 目录结构

- `AGENTS.md`：项目最高行为准则（100% 原版体验、有据可依、严禁擅自降级或瞎编）
- `docker/Dockerfile` & `docker-compose.yml`：隔离逆向与开发容器（内置 Ghidra 12.1.4、Temurin JDK 25、Node.js 22、`rea-agents@6.7.0` 及 C/C++/Python 基础工具链）
- `.mcp.json`：项目级 REA MCP 配置（通过 `./scripts/rea-docker-mcp.sh` 连接容器内 `rea mcp`）
- `scripts/`：容器启动与 REA 调用脚本
- `game/腐姫ＤＬ/`：复制入库的游戏本体目录（已加入 `.gitignore`，完整性记录于 `docs/game-assets-manifest.sha256`）
- `re/evidence/`：REA 导出的逆向证据归档

## 快速开始

```bash
# 构建并启动隔离容器，校验 REA + Ghidra 就绪状态
./scripts/docker-up.sh

# 在隔离容器中执行 REA CLI
./scripts/rea-docker.sh doctor --json
./scripts/rea-docker.sh inspect-pe-resources /workspace/game/腐姫ＤＬ/KhimeDL.exe --json
```
