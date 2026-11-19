# AGENTS.md — 《腐姫～euthanasia～》原版体验移植项目规范

## 1. 项目总目标（最高优先级）

本项目旨在对视觉小说《腐姫～euthanasia～》（DL 版，原始文件位于 `game/腐姫ＤＬ`）进行**完全原版体验的移植**。

“完全原版体验”意味着：
- 1:1 还原原版引擎的逻辑、剧本分支、标志位（Flags）、存档数据结构、文字排版与字体渲染、图像解码与混合/转场特效、音频解码与循环/音量控制、MPG 视频播放、窗口尺寸与交互逻辑、时序与帧率控制。
- **任何细节都不允许擅自妥协**。

---

## 2. 铁律：有据可依，严禁未经同意的降级或瞎编

1. **严禁擅自降级或省略（Zero Silent Downgrade）**
   - 未经用户明确同意，**绝对禁止**简化、裁减、替换或跳过任何原版功能、特效、转场、音画精度、UI 布局或边缘逻辑。
   - 严禁以“现代习惯”“实现太麻烦”“先跑起来再说”为由使用 placeholder / stub / mock / 硬编码假数据替代真实还原。
2. **严禁瞎编与脑补（Evidence-First, No Guessing）**
   - 关于二进制行为、资源封包格式（`.xfl`）、图像格式（`.wcg` / `.lwg`）、音频（`vorbis.acm` / `.wav`）、脚本字节码、存档格式（`KhDLsave.dat`）、Win32 API 调用及渲染管线的一切结论，**必须有可核查的实证依据**：
     - Ghidra / REA 反编译或反汇编结果（标注虚拟地址 `0x...`、函数名、交叉引用）
     - 原始二进制或资源文件的字节级解析验证（标注文件偏移与实际字节/校验和）
     - 原版程序运行观测或对照证据
   - **不知道就是不知道**：尚未通过逆向或实测证实的内容，必须明确记录为 `UNKNOWN / 待逆向确认`，不得凭经验或猜测填充。
3. **遇阻必先汇报与征求同意**
   - 若遇到客观技术瓶颈、不可抗力或目标平台限制导致某项原版体验确实无法按原方式实现，**必须立即停下来向用户如实说明**：
     1. 遇到的具体技术障碍与实证依据；
     2. 原方案为何受阻；
     3. 备选方案及其对原版体验的具体影响。
   - 只有在获得用户明确批准后，才可执行替代/折中方案。
4. **重大技术决策须经用户确认**
   - 移植的目标语言、框架、图形/音频后端等架构选型，必须与用户确认后再落定，不得擅自拍板。

---

## 3. 原始游戏资产与汉化补丁的严格区分

> 以下事实已通过目录盘点与 SHA-256 实测确认（完整校验清单见 `docs/game-assets-manifest.sha256`）：

- `game/` 为**只读原始资产基准目录**，严禁修改、覆写或删除其中的原始文件。
- `game/腐姫ＤＬ/` 内混合了原版日文文件与 `V1.0 汉化补丁` 文件：
  - **原版日文主程序**：`game/腐姫ＤＬ/KhimeDL.exe`（与 `game/腐姫ＤＬ/补丁/原版备份/KhimeDL.exe` SHA-256 完全一致：`2dfc7b78d32867716ab0406ccbf176b55a76faea1cb9c40feac25c90775222fe`）。
  - **汉化版主程序**：`game/腐姫ＤＬ/KhimeDL_CHS.exe`（`522bec3e93340ffd3bdb24ba98cfb775f80df9d85460819da840d824ad4822a8`）。
  - **原版 vs 汉化覆盖的资源**：根目录 `scr.xfl`（`131c3fe1...`）和 `grpo_tp.xfl` 以及散落目录 `grpe/`、`grpo_ex/`、`grps/` 属于汉化补丁覆盖/附加内容；**原版日文对应的 `scr.xfl`（`c2ec1d54...`）与 `grpo_tp.xfl` 保存在 `game/腐姫ＤＬ/补丁/原版备份/`**。
- **基准原则**：还原原版引擎与原版体验时，必须以原版日文 `KhimeDL.exe` 及原版资源（含 `补丁/原版备份/`）为 Ground Truth；`KhimeDL_CHS.exe` 与汉化文件仅作为差异对照或可选本地化参考，不得将汉化补丁的改动误当作原版引擎行为。

---

## 4. Docker 隔离环境与 REA / Ghidra 工作流

本项目所有逆向分析、脚本解包、工具链构建与测试均必须在 Docker 隔离容器（`fujihime-re`）内完成：

- **隔离边界**：容器仅将本仓库根目录挂载到 `/workspace`，不挂载宿主机用户目录或外部磁盘；所有工具与缓存均隔离在容器及项目卷内。
- **容器内已预装工具**：
  - `REA`（`rea-agents@6.7.0`）
  - `Ghidra 12.1.4`（`GHIDRA_INSTALL_DIR=/opt/ghidra`）
  - `Eclipse Temurin JDK 25`（`JAVA_HOME=/opt/jdk`）
  - `Node.js 22.x`、`Python 3`、`GCC / CMake / Ninja / GDB / binutils / file / jq / ripgrep`

### 在容器中使用 REA

1. **CLI 方式（容器内隔离执行）**：
   使用封装脚本 `./scripts/rea-docker.sh`（会自动拉起 `fujihime-re` 容器，所有文件参数使用容器内 `/workspace/...` 路径）：
   ```bash
   ./scripts/rea-docker.sh doctor --json
   ./scripts/rea-docker.sh inspect-pe-resources /workspace/game/腐姫ＤＬ/KhimeDL.exe --json
   ./scripts/rea-docker.sh analyze /workspace/game/腐姫ＤＬ/KhimeDL.exe --provider ghidra --json
   ./scripts/rea-docker.sh decompile /workspace/game/腐姫ＤＬ/KhimeDL.exe --address 0x00401000 --provider ghidra --json
   ```
2. **MCP 方式（项目级隔离）**：
   项目根目录 `.mcp.json` 已将 `"rea"` 指向 `./scripts/rea-docker-mcp.sh`，通过 stdio 桥接运行在 `fujihime-re` 容器内的 `rea mcp`。使用项目级 MCP 时，目标路径同样使用 `/workspace/...`。

### 与本项目直接相关的 REA 能力范围

- **Win32 PE32 原生逆向（Ghidra + 内置 PE 分析）**：
  - `inspect_pe_resources`：离线检查 PE32 资源树（版本信息、对话框、菜单、图标、字符串表、清单等）。
  - `open_binary`（`provider_id: "ghidra"`）配合 `binary_overview`、`list_segments`、`list_names`、`list_procedures`、`search_procedures`、`list_strings`、`search_strings`、`procedure_pseudo_code`、`procedure_assembly`、`read_function_instructions`、`analyze_function`、`xrefs`、`procedure_references`、`procedure_callees`、`procedure_callers`、`read_bytes`、`search_bytes`、`address_to_file_offset`、`inspect_native_data_type`、`annotate_native_function`、`set_address_name`、`export_evidence_bundle`、`close_binary`。
- **原版与汉化版对比**：
  - `compare_artifacts`、`match_native_functions`、`compare_functions`。
- **还原度追踪与证据留存**：
  - 关键逆向证据导出至 `re/evidence/`（可追溯 JSON），并配合 `build_reconstruction_obligation_ledger`、`evaluate_reconstruction_coverage`、`verify_reconstruction`、`record_unknown`、`update_unknown`、`verify_unknown_resolution` 追踪每个子系统的还原状态与未解未知项。

---

## 5. Git 与目录约定

- `AGENTS.md`：本协作与还原规范（最高约束）。
- `docker/`、`docker-compose.yml`、`scripts/`：Docker 隔离与 REA/Ghidra 运行脚本。
- `game/`：原版游戏本体与补丁备份（体积约 942MB，已在 `.gitignore` 中忽略，严禁提交大体积二进制，使用 `docs/game-assets-manifest.sha256` 追踪完整性）。
- `docs/`：逆向文档、格式规范与资源 SHA-256 清单。
- `re/`：逆向分析脚本、结构化证据（`re/evidence/`）；Ghidra 临时工程与缓存（`re/ghidra/`）不入库。
- `src/`：后续移植实现代码（经用户确认架构后再开展）。
