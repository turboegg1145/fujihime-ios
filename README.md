# 腐姫～euthanasia～ iOS 14.4+ 移植版 (`fujihime-ios`)

[![CI and iOS 14.4 IPA Build](https://github.com/turboegg1145/fujihime-ios/actions/workflows/ci-and-ios-ipa.yml/badge.svg)](https://github.com/turboegg1145/fujihime-ios/actions/workflows/ci-and-ios-ipa.yml)
![Platform](https://img.shields.io/badge/Platform-iOS%2014.4%2B%20(arm64)-007AFF?logo=apple)
![Language](https://img.shields.io/badge/Core-Rust%20%2B%20Swift%205.0%20%2B%20Metal-DEA584)
![License](https://img.shields.io/badge/License-MIT-green)

> **1:1 faithful iOS 14.4+ (`arm64`) engine reimplementation of *Kusarihime ~euthanasia~* (*《腐姫～euthanasia～》*, 2007 DL Edition)** — built in Rust, FreeType 2.13.2, `pl_mpeg`, Metal, UIKit, and `AVAudioEngine` based on full Ghidra/REA reverse engineering of Liar-soft's `RScript` / `LiosGameClass` (`KhimeDL.exe`).

本项目是对 Liar-soft 经典视觉小说 **《腐姫～euthanasia～》（2007 DL 版）** 的 **1:1 原版体验 iOS 14.4+ (`arm64`) 移植引擎**。通过对原版日文主程序 `KhimeDL.exe`（`RScript` / `LiosGameClass`）的逐函数逆向与全量资产字节级校验，以 Rust + C11 FFI + Swift/Metal/AVAudioEngine 从零重构全部子系统，不依赖模拟器或兼容层，精准还原原版画面、音效、演出、剧本逻辑与存档格式。

---

## 核心特性

### 1. 100% 原版引擎还原
- **`.xfl` 虚拟文件系统（VFS）**：完整支持原版 `.xfl` 封包、散落目录覆盖及 `Documents/override/` 热覆盖机制。
- **图像解码与图形管线**：完整实现 `.wcg`、`.lwg`（含内嵌多图层）、`.msk`、`.bmp`（含 32bpp 补丁贴图）解码，还原 `CMath` LUT 精确像素混合算法与 `0..=299` 全套转场特效（含缩放/百叶窗/交叉淡入/遮罩等）。
- **像素级字形排版（FreeType 2.13.2）**：内置静态编译 FreeType 2.13.2，完整还原 6 种字符渲染模式、行首/行尾禁则处理、外字映射及 `^` 控制符序列（停顿、速度、颜色、注音/换行等）。
- **音频与视频**：
  - `vorbis.acm`（Format Tag `0x6771`）Ogg Vorbis 封包解码 + 原生 PCM `.wav` 解码；
  - 独立 4 通道混音器（BGM / SE / Voice / Movie 音频），精准还原淡入淡出、循环计数与 `AVAudioEngine` 硬件输出；
  - `pl_mpeg` 原生软件解码 `0001.mpg` / `0002.mpg` MPEG-1 片头与片尾动画及 MP2 立体声音轨，并按原版 MCI 逻辑拉伸至 `800×600`。
- **完整 `.gsc` 虚拟机与双快照回滚**：120 条字节码 Opcode 完整反汇编与执行验证，支持 `save`/`rollback` 双快照机制，覆盖全部 341 个原版与汉化剧本文件。
- **存档兼容与全 9 个 UI 场景**：
  - 字节级兼容 **2007 DL 原生存档（`KhDLsave.dat`）**，并内置 **2002 CD 版存档（`KhDLsave_2002CD.dat`）** 自动识别与迁移导入；
  - 完整复刻标题画面、游戏主界面、Save/Load、Config、Backlog、CG 鉴赏、BGM 鉴赏、Scene 回想等全 **9 个原版 UI 场景**。

### 2. 原版日文 & 中文汉化 V1.0 热切换
- 同时支持 **原版日文（CP932）** 与 **简体中文汉化 V1.0（GBK）**。
- 运行时**三指轻点屏幕**呼出原生快捷菜单，即可一键热切换语言模式，选择结果自动持久化保存，无需重启即可继续游玩或读档。

### 3. 极致原版观感与无遮挡交互
- 锁定横屏 **4:3** 原版逻辑画幅（`800×600`）居中显示，四周纯黑留边（Letterbox/Pillarbox），**零多余虚拟悬浮按键**。
- 完整复刻原版 `compane`（底部控制面板）交互，支持系统快捷功能与鼠标悬停/点击反馈。

### 4. 触控手势与外接键盘操作表

#### 触控手势
| 手势 | 对应原版操作 | 说明 |
|---|---|---|
| **单指轻点 / 拖拽** | 鼠标左键移动与点击 | 推进剧情、点击选项、操作 `compane` 与 UI 面板 |
| **双指轻点** | 鼠标右键 | 隐藏/显示文本框、取消、返回上一级菜单 |
| **双指上下滑动** | 鼠标滚轮 | 向上滑动打开/翻阅履历（Backlog），向下推进/滚回 |
| **单指长按（≥ 0.45s）** | 按住 `Ctrl` | 持续按住触发快进（Skip）模式，松手即停 |
| **三指轻点** | — | 呼出 iOS 原生快捷菜单（语言切换、Save/Load、Config、Auto/Skip、Backlog、Rollback、2002 CD 存档导入、操作说明） |

#### 外接硬件键盘（iPad / 蓝牙键盘）
| 按键 | 虚拟键码 (`VK_*`) | 功能 |
|---|---|---|
| `Enter` / `Space` | `0x0D` / `0x20` | 推进文本 / 确认 |
| `Esc` | `0x1B` | 右键功能（隐框 / 返回） |
| `Tab` | `0x09` | 隐藏 / 显示文本框 |
| `Left/Right Ctrl` | `0x11` | 按住快进（Skip） |
| `Left/Right Shift` | `0x10` | Shift 辅助键 |
| `↑` / `PageUp` | `0x26` | 向上滚动 / 打开履历 |
| `↓` / `PageDown` | `0x28` | 向下滚动 / 推进 |
| `F1` | `0x70` | 原版 F1 快捷操作 |

### 5. 内置本地桌面调试器（Local Web Player）
- 项目内置基于 Rust 引擎编译的本地 Web Player（`./scripts/run-web-player.sh`），可在桌面浏览器直接调试脚本流程、渲染输出与输入事件。

### 6. iOS 14.4 深度兼容方案
- CI 固定使用 **Xcode 16.4** 工具链与 `IPHONEOS_DEPLOYMENT_TARGET=14.4`（`Swift 5.0`），避免新版 iOS SDK 导致旧系统 `dyld` 崩溃；
- 强制开启 `-Wl,-no_fixup_chains` 禁用 `LC_DYLD_CHAINED_FIXUPS`，并保持 **零 `Swift Concurrency`（`libswift_Concurrency`）** 与 **零非系统 `@rpath` dylib** 依赖；
- CI 每次构建自动执行 **6 项 Mach-O & `Info.plist` 门禁检查**：`arm64` 架构、`minos 14.4`、无 `LC_DYLD_CHAINED_FIXUPS`、无 `libswift_Concurrency`、无 `@rpath` 外部动态库、`MinimumOSVersion == 14.4`。

---

## 仓库结构速览

```text
.
├── .github/workflows/
│   └── ci-and-ios-ipa.yml      # Rust Core CI + iOS 14.4 arm64 IPA 自动构建与 6 项门禁校验
├── docker/ & docker-compose.yml # 隔离逆向环境（Ghidra 12.1.4 + REA + JDK 25 + Rust/GCC）
├── docs/
│   ├── formats-and-gfx-spec.md  # Phase 1A：.xfl/.wcg/.lwg/.msk、CMath、转场与 MPEG-1 逆向规格
│   ├── vm-opcodes-spec.md       # Phase 1B：.gsc 格式与全部 120 条 VM Opcode 逆向规格
│   ├── text-audio-save-ui-spec.md # Phase 1C：文字排版、音频混音、存档与 9 大 UI 场景逆向规格
│   ├── ios14.4-porting-plan.md  # iOS 14.4 总体移植架构与门禁规划
│   └── game-assets-manifest.sha256 # 原版游戏资产 SHA-256 完整性基线
├── re/
│   ├── evidence/                # Ghidra/REA 导出的 JSON 结构化逆向证据
│   └── verify_*.py              # 全量资产（图像/剧本/音频/存档）自动化验证脚本
├── scripts/
│   ├── docker-up.sh             # 启动隔离分析容器
│   ├── rea-docker.sh            # 容器内 REA CLI 封装
│   ├── package-game-assets.sh   # 打包离线游戏资产与字体为 tar
│   └── run-web-player.sh        # 启动本地桌面 Web Player 调试器
└── src/
    ├── core/                    # Rust 核心引擎（VFS/解码/合成/FreeType/音频/MPEG/VM/C11 FFI）
    └── ios/                     # Swift + Metal + UIKit + AVAudioEngine iOS 14.4 外壳与 XcodeGen 配置
```

---

## 安装与使用（iOS 14.4+）

### 1. 下载与安装 IPA
1. 前往本仓库的 **[GitHub Releases (`v1.0.0`)](https://github.com/turboegg1145/fujihime-ios/releases/tag/v1.0.0)** 下载 `Fujihime-iOS14.4-arm64.ipa`（已包含运行所需的全部游戏资产与开源日/中字体）。
2. 使用任意常见 iOS 签名/安装工具安装到 `arm64` 设备（系统要求 **iOS 14.4 及以上**）：
   - **TrollStore**（推荐，支持 iOS 14.0–17.0 特定版本永久安装）
   - **AltStore** / **SideStore** / **Sideloadly** / **Esign** 等自签安装工具

> 如果你使用的是通过本仓库 CI（不带资产包的 `push` 构建）生成的空壳 IPA，请在安装后将游戏资产手动放入 App 的共享目录中（见下方「文件管理」）。

### 2. 导入存档与覆盖资源（「文件」App）
本应用已启用 `UIFileSharingEnabled` 与 `LSSupportsOpeningDocumentsInPlace`，可直接在 iOS **「文件」App → 我的 iPhone / iPad → 腐姫** 中管理文件：
- **`Documents/save/`**：存档持久化目录。
  - 原版 2007 DL 存档 `KhDLsave.dat` 可直接放入此目录实现 PC ↔ iOS 双向互通；
  - 若有 **2002 CD 版存档**，将其重命名为 `KhDLsave_2002CD.dat` 放入 `Documents/save/`，再通过三指轻点呼出快捷菜单 → 选择「导入 2002 CD 存档」，即可自动转换为 2007 DL 格式并保留进度。
- **`Documents/override/`**：热覆盖目录。放入其中的 `.xfl`、`grpe/`、`grpo_ex/`、`grps/`、`mov/` 或字体文件将优先于安装包内资源加载。

---

## 从源码构建

### 1. 运行 Rust Core 单元测试与静态检查
```bash
cd src/core
cargo clippy --all-targets -- -D warnings
cargo test --release
gcc -std=c11 -Wall -Wextra -Werror -fsyntax-only include/fujihime_core.h
```

### 2. 启动本地 Web Player（桌面调试）
确保已准备原版游戏目录（默认 `game/腐姫ＤＬ` 或通过环境变量 `FUJIHIME_GAME_DIR` 指定）及运行容器，执行：
```bash
./scripts/run-web-player.sh
```
随后在浏览器访问 `http://127.0.0.1:8080` 即可运行调试器。

### 3. 构建 iOS 14.4 `arm64` IPA
- **GitHub Actions 自动构建**：
  - 每次 `push` 会自动运行 `rust-core-ci` 并构建未签名的 `Fujihime-iOS14.4-arm64.ipa` 空壳包（验证全套 Xcode 16.4 + 6 项 iOS 14.4 门禁）。
  - 手动触发 `workflow_dispatch` 时，可填写可选参数 `assets_release_tag`（指向包含 `fujihime-game-assets-v1.tar` 的私有 Release tag，该压缩包可由 `./scripts/package-game-assets.sh` 生成），CI 将自动注入游戏资产并打包完整 IPA。
- **本地 macOS 构建**：需安装 Xcode 16.4、Rust `aarch64-apple-ios` target 与 `xcodegen`：
  ```bash
  cd src/core
  IPHONEOS_DEPLOYMENT_TARGET=14.4 cargo build --release --target aarch64-apple-ios --lib
  mkdir -p ../ios/Vendor && cp target/aarch64-apple-ios/release/libfujihime_core.a ../ios/Vendor/
  cd ../ios && xcodegen generate
  xcodebuild -project Fujihime.xcodeproj -scheme Fujihime -configuration Release -sdk iphoneos -arch arm64 \
    IPHONEOS_DEPLOYMENT_TARGET=14.4 CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO CODE_SIGN_IDENTITY="" build
  ```

---

## 免责声明与版权说明

- 本仓库中的原创引擎源码、iOS 外壳代码、构建配置、逆向工具脚本与技术文档采用 **[MIT License](LICENSE)** 开源。
- **《腐姫～euthanasia～》（Kusarihime ~euthanasia~）** 的游戏剧本、美术立绘、CG、背景、语音、音乐、影片及相关商标版权均归 **Liar-soft** 所有，**本源码树不包含任何原版受版权保护的游戏资产**。
- 本项目仅用于计算机逆向工程研究、老旧软件跨平台兼容性保存与个人已购正版资产的存档移植体验。请支持正版游戏作品。
