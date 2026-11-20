# 《腐姫～euthanasia～》图像格式、软件合成器、转场与 MPEG-1 视频技术规格书（Phase 1A）

> **权威基准（Ground Truth）**：所有偏移、结构、算术公式与状态机均逆向自原版日文主程序 `game/腐姫ＤＬ/KhimeDL.exe`（SHA-256: `2dfc7b78...`）及原版资源（含 `game/腐姫ＤＬ/补丁/原版备份/`）。汉化版 `KhimeDL_CHS.exe` 与汉化目录资产的差异已明确标注。
> 
> **约定**：除非另有说明，所有多字节整数均为 **Little-Endian (LE)**；像素缓冲区在内存中均为 **Top-Down 32-bit BGRA**（每像素 4 字节：`[B, G, R, A]`，即 `u32` 小端布局 `0xAARRGGBB`）。

---

## 0. 全局色彩约定与查表常量（`KhimeDL.exe`）

### 0.1 倒置 Alpha 约定（Inverted Alpha）

原版引擎采用**倒置 Alpha**：
- `A = 0x00`：**完全不透明**（Opaque，权重 = 1.0）
- `A = 0xFF`：**完全透明**（Transparent，权重 = 0.0）
- 有效不透明度 `opacity = 255 - A`（即 `~A & 0xFF`）。

### 0.2 `CMath` 全局查找表（初始化：`0x43d6c1..0x43d754`）

初始化使用 `0x451d40`（`_ftol`，向零截断）与 `.rdata` 常量 `0x466090 = 256.0`、`0x4660a8 = 1.0 / 256.0 = 0.00390625`：

1. **`CMath::m_ppAddTable[256][256]`**（地址 `0x483820`，大小 `0x10000` 字节）：
   $$\text{AddTable}[i][j] = \left\lfloor \frac{j \cdot (256 - i)}{256} \right\rfloor = \text{u8}\big((j \cdot (256 - i)) \gg 8\big)$$
   - `i = 0` → `j`（恒等 / 全不透明权重）
   - `i = 255` → `(j * 1) >> 8 = 0`（注意：`255` 时所有值均截断为 `0`）

2. **`CMath::m_ppDelTable[256][256]`**（地址 `0x493820`，大小 `0x10000` 字节）：
   $$\text{DelTable}[i][j] = j + \left\lfloor \frac{(256 - j) \cdot i}{256} \right\rfloor = \text{u8}\big(j + ((256 - j) \cdot i) \gg 8\big)$$
   - `CMath::GetWhiteTable(p: u8)`（`0x4015f0`）：返回 `&m_ppDelTable[p][0]`（`0x493820 + p * 0x100`），将通道向白色 `255` 混合。

3. **`CMath::GetEmbosTable(p: u8)`**（`0x401630`）：
   - 若 `p < 128` (`0x80`)：返回 `0x493620 - p * 0x200` = `&m_ppAddTable[254 - 2 * p][0]`（因 `0x493620 = 0x483820 + 0xfe00`；`p=0` → 最暗，`p=127` → `m_ppAddTable[0]` 原值）。
   - 若 `p >= 128` (`0x80`)：返回 `0x483920 + p * 0x200` = `&m_ppDelTable[(p - 128) * 2 + 1][0]`（因 `0x483920 + 0x80 * 0x200 = 0x493920 = 0x493820 + 0x100`；`p=128` → `m_ppDelTable[1]`，`p=255` → `m_ppDelTable[255]`）。

4. **`CMath` 像素辅助函数（`0x401660..0x401984`）**：
   - `0x4016b0(color, alpha16)`：按 `AddTable[alpha16][C]` 缩放 `color` RGB，返回 `0x00RRGGBB`。
   - `0x401700(dst, src, alpha16)`：先对 `src` 缩放 `AddTable[alpha16][C]`，再与 `dst` 进行饱和加（保留 `dst` Alpha）。
   - `0x401780(src, dst, alpha16)`：同上，仅前两个参数顺序相反。
   - `0x401800(src, dst, alpha16)`：按 `AddTable[alpha16][srcC] + AddTable[~alpha16 & 0xff][dstC]` 线性混合 RGB，返回 `0x00RRGGBB`。
   - `0x401890(dst, src)`：倒置 Alpha 空间下的非预乘 Porter-Duff `Over` 合成（详见 §2.3）。

### 0.3 4×4 有序抖动位掩码表（`.data`：`0x46e100` 与 `0x46e120`）

每张表为 `16` 个 `u16`（小端字节序已从 `KhimeDL.exe` 原始节区提取）：

- **模式 `0x12` 表（`0x46e100`, 16×`u16`）**：
  ```rust
  pub const DITHER_TABLE_12: [u16; 16] = [
      0xffdf, 0xefdf, 0x6fdf, 0x6fd7, 0x6bd7, 0x6bd6, 0x6b56, 0x6a56,
      0x2a56, 0x2a52, 0x0a52, 0x0a12, 0x0812, 0x0810, 0x0800, 0x0000,
  ];
  ```
- **模式 `0x13` 表（`0x46e120`, 16×`u16`）**：
  ```rust
  pub const DITHER_TABLE_13: [u16; 16] = [
      0xfffe, 0xfffc, 0xfff8, 0xfff0, 0xff70, 0xf770, 0x7770, 0x3770,
      0x1770, 0x0770, 0x0670, 0x0660, 0x0640, 0x0600, 0x0200, 0x0000,
  ];
  ```
索引规则：`idx = (global_alpha >> 4) & 0xff`（`0..=15`），像素 `(x, y)` 命中的位掩码为 `1u16 << ((x & 3) + ((y & 3) << 2))`。

---

## 1. UNK-01：封包与图像格式精确规范

### 1.1 `.xfl` 资源封包（`0x431e60`）

| 偏移 | 类型 | 字段 | 说明 |
|---|---|---|---|
| `0x00` | `[u8; 4]` | `magic` | 固定 `"LB\x01\x00"`（`0x0001424c`） |
| `0x04` | `u32` | `dir_size` | 目录区总字节数，必须等于 `entry_count * 40` |
| `0x08` | `u32` | `entry_count` | 条目总数 |
| `0x0C` | `[Entry; entry_count]` | `entries` | 每条 `40` 字节：`name: [u8; 32]`（Shift-JIS / CP932，NUL 填充）、`rel_offset: u32`、`size: u32` |
| `12 + dir_size` | `[u8]` | `data_blob` | 条目实际文件偏移 = `12 + dir_size + rel_offset` |

### 1.2 `.wcg` 图像格式（`0x42f3b0`, `0x42f450`, `0x42f520`, `0x42f700`）

#### 1.2.1 文件头（16 字节）

| 偏移 | 类型 | 字段 | 说明 |
|---|---|---|---|
| `0x00` | `[u8; 2]` | `sig` | 固定 `"WG"`（`0x4757`） |
| `0x02` | `u16` | `format_flags` | 原版全量资产固定为 `0x0271`（即 `magic = b"WGq\x02"`）：低 4 位 `(format_flags & 0xF) == 1`；位 `[8:6]` `(format_flags & 0x1C0) == 0x40`（16-bit 交错双流模式） |
| `0x04` | `u32` | `pixel_flags` | 低 8 位 `(pixel_flags & 0xFF) == 0x20`（32 bpp）；实测包含 `0x00000020` 与 `0x40000020` |
| `0x08` | `u32` | `width` | 图像宽度（像素） |
| `0x0C` | `u32` | `height` | 图像高度（像素） |

> **备选非 `0x40` 分支（`0x42f520 -> 0x42f5c0 -> 0x432220`）**：当 `(format_flags & 0x1C0) != 0x40` 时，依次按字节通道 `3`（若 `format_flags & 0x10 != 0` 则再读 `2, 1, 0`）调用 `0x42f5c0` 解码 8-bit 通道流。原版全部 `1,727` 个独立 `.wcg` 及 `154` 个 `.lwg` 内嵌图层均走 `0x40` 分支（`0x42f700`），未使用 `0x42f5c0`。

#### 1.2.2 `0x40` 分支双流交错结构（`0x42f520 -> 0x42f700 -> 0x432ec0`）

文件头之后紧跟 **2 个压缩流**，按顺序分别填充每个 `32-bit BGRA` 像素的高 16 位与低 16 位：
1. **第 1 次调用 `0x42f700(..., phase=1)`**：写入每像素字节 `+2, +3`（`R, A`，即输出 `u16` 数组下标 `1, 3, 5, ...`）。
2. **第 2 次调用 `0x42f700(..., phase=0)`**：写入每像素字节 `+0, +1`（`B, G`，即输出 `u16` 数组下标 `0, 2, 4, ...`）。

每个压缩流头部为 12 字节：
- `uncomp_len: u32` — 必须等于 `width * height * 2`
- `comp_len: u32` — 压缩比特流字节长度
- `dict_field: u32` — 字典条目数 `dict_cnt = (dict_field & 0xFFFF) as usize`（实测高 16 位保留有编码器元信息）
- 紧跟字典：`dict_cnt * 2` 字节，共 `dict_cnt` 个 2 字节像素分量对 `[u8; 2]`
- 紧跟比特流：`comp_len` 字节

#### 1.2.3 位流读取与 LZ/RLE 字典解码（`0x433b40`, `0x433070`, `0x433380`, `0x433510`, `0x4337f0`）

- **比特序（`0x433b40`）**：每字节 **MSB-first**（高位先出），跨字节拼接为大端位流。
- **表分派（`0x432ec0`）**：
  - `dict_cnt < 0x1001` (`4097`)：使用小表解码器 `0x433070` + `0x433510`（`prefix_bits = 3`）
  - `dict_cnt >= 0x1001`：使用大表解码器 `0x433380` + `0x4337f0`（`prefix_bits = 4`）
- **Token 循环**（设输出目标为 `u16` 视图，`idx` 初值为 `phase`，每次写一个 16-bit pair 后 `idx += 2`，直到 `idx >= width * height * 2`）：
  1. 读取 `prefix_bits` 位得 `c`。
  2. **RLE 游程（`c == 0`）**：
     - 再读 `4` 位得 `r_bits`，游程长度 `run = r_bits + 2`（范围 `2..=17`）。
     - 再读 `prefix_bits` 位得 `code`（非 0）。
     - 若 `c != 0`：单次输出 `run = 1`，`code = c`。
  3. **变长字典索引解码**：
     - **小表（`dict_cnt < 0x1001`, `0x433510`）**：
       - `code == 1` → `extra_bits = 1, base = 0`（索引 `0..=1`）
       - `2 <= code <= 6` → `extra_bits = code - 1, base = 1 << extra_bits`
       - `code == 7` → 在 `b = 6..=11` 循环读 `1` 位直到读到 `0`（一元前缀终止），取 `extra_bits = b, base = 1 << b`
     - **大表（`dict_cnt >= 0x1001`, `0x4337f0`）**：
       - `code == 1` → `extra_bits = 1, base = 0`
       - `2 <= code <= 14` → `extra_bits = code - 1, base = 1 << extra_bits`
       - `code == 15` → 在 `b = 14..=15` 读 `1` 位直到读到 `0`，取 `extra_bits = b, base = 1 << b`
     - 读取 `extra_bits` 位无符号整数 `low`，`dict_index = base | low`。
  4. 将 `dict_pairs[dict_index]` 重复写入 `run` 次到 `out_bytes[idx*2 .. idx*2+2]`，每次 `idx += 2`。
- **尾部字节处理实证（关键兼容性）**：
  - `KhimeDL.exe`（`0x4330ed` / `0x4333f0`）在 `idx >= uncomp_bytes` 时立即结束循环，**不校验 `bytes_consumed == comp_len`**，随后直接释放读取缓冲。
  - 原版 `1,727` 个 `.wcg` 与 `154` 个 `.lwg` 图层中全部流的 `bytes_consumed == comp_len`（0 填充字节）；但汉化补丁散落目录 `grpe/`、`grpo_ex/`、`grps/` 中存在 **73 个压缩流**（来自 CHS 重打包工具）在有效位流后多出 **1 个 `0x00` 填充字节**（`comp_len - bytes_consumed == 1`）。Rust 解码器**必须**按引擎行为在写满 `uncomp_bytes` 后即停止，并允许尾部存在无害填充字节（只要不越界读取），切勿把 `bytes_consumed != comp_len` 当作致命错误。

---

### 1.3 `.lwg` 分层图形格式（`0x42b640`, `0x42b6e0`, `0x42bd00`, `0x42e420`）

#### 1.3.1 文件头（24 字节，见 `0x42b6e0`）

| 偏移 | 类型 | 字段 | 说明 |
|---|---|---|---|
| `0x00` | `[u8; 4]` | `magic` | `"LG\x01\x00"`（`0x42b725` 校验 `"LG"`，`0x42b73a` 校验版本 `u16 == 1`） |
| `0x04` | `u32` | `height` | **注意顺序：高在前**（画布高度） |
| `0x08` | `u32` | `width` | **宽在后**（画布宽度） |
| `0x0C` | `u16` | `entry_count` | 图层数量 |
| `0x0E` | `u16` | `flags` | 实测全 15 个原版 `.lwg` 均为 `0x0000` |
| `0x10` | `u32` | `reserved` | 实测全为 `0x00000000` |
| `0x14` | `u32` | `index_bytes` | 紧随其后的变长索引区总字节数 |

#### 1.3.2 图层索引区（`entry_count` 条变长记录）与数据区

每条记录读取 `0x11`（17）字节固定头 + `1` 字节 `name_len` + `name_len` 字节名称（见 `0x42b779..0x42b7c1`）：
- `x: i32` — 图层在画布上的 X 坐标偏移（`0x42d090`，`[layer+0x50]`）
- `y: i32` — 图层在画布上的 Y 坐标偏移（`0x42d0a0`，`[layer+0x54]`）
- `flag: u8` — 图层标志字节（`0x42d0b0`，`[layer+0x58]`）
- `rel_offset: u32` — 相对于**数据区起始（即 `total_data_size` 之后）**的偏移
- `size: u32` — 内嵌 `.wcg` 数据长度
- `name_len: u8` — 图层名字节长度
- `name: [u8; name_len]` — CP932 编码图层名（如 `bg`, `slide_lev`, `back`, `front`, `text`）

索引区末尾紧跟 `total_data_size: u32`，其后即为所有图层内嵌 `.wcg` 数据体（总长 `total_data_size`，可完整由 `decode_wcg` 解码）。

> **`u8 flag` 语义实证（`0x42d0b0`）**：全量 15 个原版 `.lwg`（154 个图层）中观察到的 `flag` 仅取 `{0x08, 0x09, 0x28, 0x29}`（例如 `compane.lwg` 中 `slide_lev=0x29`, `bg=0x28`；`tbox01.lwg` 中 `back=0x08`, `front=0x28`；`wait00.lwg` 中 `body=0x09`, `grow=0x28`）。全 `.text` 交叉引用扫描确认 `0x42d0b0`（`vtbl 0x465cb4 + 0x34`）在 `KhimeDL.exe` 中**无任何调用点**，所有 `.text` 中 `call [reg+0x34]` 均为 `Surface::Blit` 或 COM/DirectSound 方法。`u8 flag` 为原画打包工具保留的静态图层元数据位，运行时渲染与逻辑**不分支依赖该字段**。

---

### 1.4 `.msk` 规则转场遮罩格式（`0x430700`, `0x4307a0`, `0x430900..0x430c10`）

- 文件格式为标准**未压缩 Windows BMP**（`BM`, `biSize == 40`, `biPlanes == 1`, `biCompression == 0`, Bottom-Up 存储，`1 / 8 / 24 / 32 bpp`）。
- 加载后解码为 Top-Down 32-bit 表面，但仅写入每个像素的 **Alpha 字节（`+3`）**（供 `mode=0, sub=2` 规则阈值转场使用）：
  - `1 bpp` (`0x4309c0`)：MSB-first 位掩码，`bit == 1` → `0xFF`，`bit == 0` → `0x00`。
  - `8 bpp` (`0x430a90`)：取调色板第 `idx` 项的 **Blue 分量**（`palette[idx * 4]`）。
  - `24 bpp` (`0x430b50`)：取像素首字节 **Blue 分量**（`src[x * 3]`）。
  - `32 bpp` (`0x430c10`)：取像素首字节 **Blue 分量**（`src[x * 4]`）。
- 行步长按 4 字节对齐：`row_stride = ((w * bpp + 7) / 8 + 3) & !3`，按 `dst_y = h - 1 - row` 翻转为 Top-Down。

---

### 1.5 `.bmp` 格式与 CHS 补丁兼容（`0x430cd0`, `0x430d80`, `0x430f20..0x431400`）

- 支持未压缩 Bottom-Up Windows BMP（`1 / 8 / 24 / 32 bpp`），输出 Top-Down 32-bit BGRA：
  - `32 bpp` (`0x431300`)：逐行上下翻转后直接拷贝 4 字节 BGRA，**保留原始字节 `+3` 作为倒置 Alpha**（实测汉化版 `grpo_tp.xfl` 全部 `36` 张 `.bmp` 均为 `32 bpp`，且 `8000.bmp` 已按 `0x00=opaque, 0xFF=transparent` 预倒置存储）。
  - `1 / 8 / 24 bpp` (`0x430f20..0x4312f4`)：先将 Alpha 清零 (`0x00`)，然后调用 `0x431370` 尝试从同伴流读取 `w * h * 4` 像素覆盖；若无同伴 Alpha，则调用 `0x431400` 执行纯绿透明色键：**若 `(px & 0x00FFFFFF) == 0x00FF00`，改写为 `0xFF00FF00`（透明绿），否则保留 `A = 0x00`（不透明）**。

---

## 2. UNK-03：软件合成器精确规范（`0x435b40`..`0x43b980`）

### 2.1 表面对象状态字段与裁剪（`0x435b40`）

源表面 `src`（`this`）相关成员字段：
- `+0x08`：`pixels: *mut u32`（Top-Down BGRA，步长 `+0x14` 像素）
- `+0x14`：`width: i32`，`+0x18`：`height: i32`
- `+0x1C`：`offset_x: i32`，`+0x20`：`offset_y: i32`
- `+0x28`：`blend_mode: u32`（setter `0x435770`）
- `+0x2C`：`global_alpha: i32`（setter `0x435840`，通常 `0=不透明, 255=全透明`，部分模式用作阈值/步进）
- `+0x30`：`alpha_channel_mode: i32`（setter `0x435910`）
- `+0x34`：`threshold: i32`（setter `0x4359e0`，规则转场进度或二值阈值）
- `+0x38`：`color_or_center: u32`（`0x00RRGGBB` 填充色，或圆/椭圆中心 `(cy << 16) | cx`）

**矩形裁剪（`FUN_00435b40`）**：
给定目标表面 `dst`（宽高 `dw, dh`）、目标裁剪框 `clip_rect = [l, t, r, b]`、请求偏移 `pt = [px, py]`：
- 先将 `clip_rect` 钳制到 `[0, 0, dw, dh]` 得 `[cl, ct, cr, cb]`。
- 源在目标空间的原点 `(sx0, sy0) = (this.offset_x + px, this.offset_y + py)`。
- `dst_rect = [max(cl, sx0), max(ct, sy0), min(cr, sx0 + src.w), min(cb, sy0 + src.h)]`。
- `src_rect = [max(0, cl - sx0), max(0, ct - sy0), min(src.w, cr - sx0), min(src.h, cb - sy0)]`。
- 若 `copy_w = dst_rect.r - dst_rect.l <= 0` 或 `copy_h = dst_rect.b - dst_rect.t <= 0`，直接返回无写入（`< 0` 返回 `0`，`== 0` 不循环返回 `1`）。

### 2.2 `Blit` 主分派总表（`FUN_00436430`）

记：
- `s = src_px`（`[sB, sG, sR, sA]`），`d = dst_px`（`[dB, dG, dR, dA]`）
- 线性混合算子 `LERP_RGB(a, fg, bg)`：每个 RGB 通道独立计算 `AddTable[a][fg_c] + AddTable[~a & 0xFF][bg_c]`，**打包后 Alpha 字节写为 `0x00`**（除非明确标注保留 `dA` 或拷贝 `*src`）。
- 饱和加算子 `SAT_ADD_RGB(base, add_rgb)`（`0xfefefe / 0x10101 / 0x7f7f7f` 位并行技巧）：每个 RGB 通道计算 `min(255, base_c + add_rgb_c)`，**保留 `base` 的 Alpha 字节**。

| `blend_mode` (`+0x28`) | `sub` (`+0x30`) | 函数地址 | 精确像素行为与目标 Alpha 写回规则 |
|---|---|---|---|
| `0` (Normal / Rule) | `0` | `0x437330` | `sA == 0xFF`: 跳过；`sA == 0`: `*dst = *src` (含 `sA`)；否则 `LERP_RGB(sA, s, d)` (**目标 `A` 写 `0`**) |
| `0` | `1` | `0x437500` | 若 `(sA as i32) < threshold`: `*dst = *src` (含 `sA`)；否则跳过 |
| `0` | `2` | `0x4375f0` | 令 `t = threshold as u16`, `u = min(t + 31, 254)`。`sA > u`: 跳过；`sA <= t`: `*dst = *src`；`t < sA <= u`: `a = ((sA - t) * 8) as u8`（因 `(t*0x1fff + sA)*8 mod 65536 == (sA - t)*8`），`LERP_RGB(a, s, d)` (**目标 `A` 写 `0`**) |
| `0` | `3` | `0x4377f0` | 规则遮罩淡至**纯黑**：`sA > u` 跳过；`sA <= t` 写 `0x00000000`；否则 `a = (sA - t) * 8`, `RGB = AddTable[~a][dC]`, **`A = 0`** |
| `0` | `4` | `0x4379d0` | 规则遮罩淡至**纯白**：`sA > u` 跳过；`sA <= t` 写 `0x00FFFFFF`；否则 `a = (sA - t) * 8`, `w = AddTable[a][255]`, `RGB = w + AddTable[~a][dC]`, **`A = 0`** |
| `0` | `5` | `0x437bb0` | 规则遮罩叠亮交叉：`sA > u` 跳过；`sA <= t` `*dst = *src`；否则 `a = (sA - t) * 8`（**注意反汇编 `0x437d07` 以 `u16` 偏移 `* 0x800` 查 `AddTable`：`((sA - t) * 8)` 实际每差 1 步跨 8 行表！即有效索引 `row = ((sA - t) << 3) & 0xFF`**，注：`sub=5` 在反汇编 `0x437bb0` 与 `0x437cde` 实为 `shl edi, 3; and edi, 0xffff; shl edi, 8` = 同样乘以 8 行！Ghidra `* 0x800` 为反编译器折叠显示），先缩放 `s` RGB (`AddTable[a][sC]`) 再 `SAT_ADD_RGB(d, s_scaled)`（保留 `dA`） |
| `0` | `>=6` / 其他 | `0x437250` | `SRCCOPY`：直接 `memcpy` 整行 32-bit 像素（含 `sA`） |
| `2` (Global Alpha) | `0` | `0x437f40` | `effA = max(sA, global_alpha as u8)`；`LERP_RGB(effA, s, d)` (**目标 `A` 写 `0`**) |
| `2` | `1` | `0x438110` | 若 `(sA as i32) < threshold`：`LERP_RGB(global_alpha as u8, s, d)` (**目标 `A` 写 `0`**)；否则跳过 |
| `2` | 其他 (`6` 等) | `0x437d90` | 忽略 `sA`：`LERP_RGB(global_alpha as u8, s, d)` (**目标 `A` 写 `0`**) |
| `3` (Attenuate/Black) | `0` | `0x43a0d0` | 先按 `global_alpha` 压暗源：`s'_C = AddTable[global_alpha][s_C]`，再 `LERP_RGB(sA, s', d)` (**目标 `A` 写 `0`**) |
| `3` | `!=0, !=1` | `0x439f70` | 忽略 `sA`：`RGB = AddTable[global_alpha][s_C]`，**`A = 0`** |
| `4` (Whiten) | `0` | `0x439d60` | 先按 `global_alpha` 漂白源：`s'_C = DelTable[global_alpha][s_C]`，再 `LERP_RGB(sA, s', d)` (**目标 `A` 写 `0`**) |
| `4` | `!=0, !=1` | `0x439c00` | 忽略 `sA`：`RGB = DelTable[global_alpha][s_C]`，**`A = 0`** |
| `5` (Diag Hatch) | `0` / `!=0,!=1` | `0x438d70` / `0x438c60` | 条件 `((lx + ly) & 0xF) > ((global_alpha >> 4) & 0xFFFF)`（`lx, ly` 为裁剪矩形内相对坐标）：`sub=0` 用 `LERP_RGB(sA, s, d)` (`A=0`)；否则直接写 `*src` |
| `6` (Circle Wipe) | 任意 | `0x43adf0` | 圆心 `cx = color_or_center & 0xFFFF, cy = color_or_center >> 16`，半径 `r = global_alpha * 15`；若 `(dx^2 + dy^2) > r^2`（`dx = dst_x - cx, dy = dst_y - cy`）拷贝 `*src` |
| `7` (Ellipse Wipe) | 任意 | `0x43af50` | 令 `rx = global_alpha * 22, ry = global_alpha * 15`，`nx = (|dx|*1000)/rx, ny = (|dy|*1000)/ry`；若 `nx^2 + ny^2 > 1_000_000` 拷贝 `*src` |
| `8` (Half-Blend) | 任意 | `0x4383e0` / `0x4382e0` | 若 `global_alpha < 5` 回退到 `mode 0` (`0x437330`/`0x437250`)；否则 `sub=0` (`0x4383e0`): 若 `sA <= 128` 算 `(s >> 1 & 0x7f7f7f7f) + (d >> 1 & 0x7f7f7f7f)`，否则 `LERP_RGB(sA, s, d)`；`sub!=0,!=1` (`0x4382e0`): 无条件 50/50 半混合（含 Alpha 字节按移位相加） |
| `0xB` / `0xC` | `!=0, !=1` | `0x43a400` / `0x43a2e0` | 若 `(lx + ly) % 10 < global_alpha`: `0xB` 写 `0x00000000`，`0xC` 写 `0x00FFFFFF`；否则拷贝 `*src` |
| `0xD` (Invert) | `0` / `!=0` | `0x43a620` / `0x43a520` | `inv = *src ^ 0x00FFFFFF`；`sub=0` 用 `LERP_RGB(sA, inv, d)` (`A=0`)；否则直接写 `inv` (保留 `sA`) |
| `0xE` (SatAdd Src) | `0` / `!=0,!=1` | `0x438750` / `0x4385c0` | `sub=0`: `a = max(sA, global_alpha)`; `sub!=0`: `a = global_alpha`。缩放 `s'_C = AddTable[a][s_C]`，再 `SAT_ADD_RGB(d, s')` (**保留 `dA`**) |
| `0xF` (SatAdd Dst) | `0` / `!=0,!=1` | `0x438aa0` / `0x438910` | `sub=0`: `a = max(sA, global_alpha)`; `sub!=0`: `a = global_alpha`。缩放 `d'_C = AddTable[a][d_C]`，再 `SAT_ADD_RGB(d', s)` (**注意：由于先构造 `d'` 且高字节为 0，写回后目标 `A` 为 `0`**) |
| `0x12` (Dither A) | `0` / `!=0,!=1` | `0x439670` / `0x439520` | 查 `DITHER_TABLE_12[(global_alpha >> 4) & 0xFF]`，对应 `(lx&3, ly&3)` 位为 1 时：`sub=0` 执行 `LERP_RGB(sA, s, d)`，否则拷贝 `*src` |
| `0x13` (Dither B) | `0` / `!=0,!=1` | `0x4399e0` / `0x439890` | 同上，改查 `DITHER_TABLE_13` |
| `0x14` (Checker/HBlind) | `0` / `!=0,!=1` | `0x439040` / `0x438f50` | `thr = (global_alpha >> 4) & 0xFFFF`。`sub=0`: `(ly & 0xF) > thr && ((lx + ly) & 0xF) > thr` 时 `LERP_RGB(sA, s, d)`；`sub!=0,!=1`: `(ly & 0xF) > thr` 时拷贝 `*src` |
| `0x15` (VBlind) | `0` / `!=0,!=1` | `0x439340` / `0x439230` | `thr = (global_alpha >> 4) & 0xFFFF`。`(lx & 0xF) > thr` 时：`sub=0` 执行 `LERP_RGB(sA, s, d)`，否则拷贝 `*src` |
| `0x16` (Color Fade) | `0` / `!=0` | `0x43a960` / `0x43a7e0` | 设纯色 `C = color_or_center`：`t_C = AddTable[global_alpha][C] + AddTable[~global_alpha][d]`；`sub!=0`: `*dst = t_C` (`A=0`)；`sub=0`: 再执行 `LERP_RGB(sA, t_C, d)` (`A=0`) |
| `0x67` | 任意 | `0x43b0f0` | 若 `(s & 0x00FF0000) == 0`（即 `sR == 0`）：`*dst ^= 0x00FFFFFF`（**保留 `dA`**） |
| `0x68` | 任意 | `0x43b1f0` | 逐通道用 `GetEmbosTable(s_C)` 查表替换 `d_C`（**保留 `dA`**） |
| `0x69` | 任意 | `0x43b380` | 逐通道用 `GetWhiteTable(s_C)` (`DelTable[s_C][d_C]`) 替换 `d_C`（**保留 `dA`**） |
| `0x6A` | 任意 | `0x43b4f0` | 逐通道用 `AddTable[s_C][d_C]` 替换 `d_C`（**保留 `dA`**） |
| `0x6B` | 任意 | `0x43b660` | 发光遮罩：以 `sR` 强度调制 `color_or_center`：`a = (0xFF - ((sR * global_alpha) >> 8)) & 0xFFFF`（实取低位），`glow_C = AddTable[a][color_C]`，再 `SAT_ADD_RGB(d, glow)`（**保留 `dA`**） |
| `0x6C` | 任意 | `0x43b810` | 若 `sR != 0`：对 `d` RGB 逐通道 `AddTable[global_alpha][d_C]`（**保留 `dA`**） |
| `0x6D` | 任意 | `0x43b980` | 若 `sR != 0`：对 `d` RGB 逐通道 `DelTable[global_alpha][d_C]`（**保留 `dA`**） |

> **关键空操作陷阱**：当 `blend_mode ∈ {3, 4, 5, 0xB, 0xC, 0xE, 0xF, 0x12, 0x13, 0x14, 0x15}` 且 `alpha_channel_mode == 1` 时，`0x436430` 直接 `break` 不作任何像素写入（`0xB`/`0xC` 在 `sub == 0` 时同样为空操作）。

### 2.3 独立 Porter-Duff `Over` 合成与离屏表面滤镜

- **独立 Alpha `Over`（`0x4371a0 -> 0x43abc0 -> 0x401890`）**：
  - `sA == 0xFF`：跳过；`sA == 0`：直接拷贝 `*src`。
  - 否则按 `0x401890` 计算真实非预乘 Porter-Duff `Over` 并写回合成后倒置 Alpha：
    - `sa = (~sA) & 0xFF`（标准不透明度 `1..=254`）
    - `da = (~dA) & 0xFF`
    - `rem = ((da * sA) + 0xFF) >> 8`（即 `ceil(da * (255 - sa) / 256)` 近似残余背景不透明度）
    - `out_opacity = sa + rem`
    - `src_w = (sa * 255) / out_opacity`（整数除法），`t_src = (~src_w) & 0xFF`，`t_dst = src_w & 0xFF`
    - `out_C = AddTable[t_src][s_C] + AddTable[t_dst][d_C]`
    - `out_A = (~(out_opacity as u8)) & 0xFF`（**正确写回目标 Alpha 通道**）
- **原地表面滤镜（均保留像素 Alpha 字节，除 `0x435700` 外）**：
  - `0x434c30`（灰度）：`Y = ((R + G + B) * 0x55555556i64 >> 32) as u8`（即 `(R+G+B)/3`），写入 `B=G=R=Y`，保留 `A`。
  - `0x434da0(pct)`（亮度衰减）：`idx = (((pct & 0xFFFF) * 255) / 100) & 0xFF`，逐像素 RGB 查 `AddTable[idx][C]`，保留 `A`。
  - `0x434fc0(color)`（灰度着色）：`Y = (B + G + R) / 3`，用 `GetEmbosTable(Y)` 映射 `color` 的 RGB，保留 `A`。
  - `0x4351b0`（反色）：`*px ^= 0x00FFFFFF`，保留 `A`。
  - `0x435300(color)`（保留 Alpha 填充）：`*px = (*px & 0xFF000000) | (color & 0x00FFFFFF)`。
  - `0x435700(color)`（全表面清色）：直接写入 `color`（同时覆盖 `A` 为 `color >> 24`）。

---

## 3. UNK-04：转场特效全表与帧调度（`0x422060`）

由脚本指令（如 `0x1c -> 0x4238b0`、`0x1d -> 0x4238e0` 等）调用 `FUN_00422060(effect_id: u16, steps: u16, delay_ms: u16)`。
前置守卫：
- 若 `DAT_00471328 != 0`（跳过标志）→ 立即返回。
- 若 `steps == 0` → 设为默认 `steps = 10` (`0xA`)。
- 若 `DAT_00471324 != 0` 或 `DAT_0047138a == 0` → 直接瞬切 `0x4461b0`。
- 转场循环期间仅用 `timeGetTime()` + `Sleep(5)` 等待每帧 `delay_ms`，**不泵主窗口跳过消息**（按键跳过通过外层标志控制）。

### 3.1 完整转场分发表

| `effect_id` | 转场类型 | 实现函数 | 帧数与步进公式 | 底层合成 / 呈现方式 |
|---|---|---|---|---|
| `0` | 瞬切 | `0x42227b -> 0x4461b0` | 1 帧直接拷贝 | `SRCCOPY` |
| `1` | 交叉淡入淡出 (Crossfade) | `0x4483f0` | 共 `steps + 1` 帧（`i = 0..=steps`）：`global_alpha = ((steps - i) * 255) / steps`（`255 -> ... -> 0`），每帧阻塞至 `elapsed > delay_ms` | `mode = 2, sub = 6`（`0x437d90`）；末帧调用 `0x4462c0` 并恢复 `mode=0` |
| `2` | 4×4 有序抖动 (Ordered Dither) | `0x4482c0` | 循环 `steps - 1` 帧（`k = 0..steps-1`）：`global_alpha = ((steps - k) * 255) / steps`（从 `255` 步减至 `510/steps`），循环结束后调用 `0x4462c0` 完成 100% 覆盖 | `mode = 0x12, sub = 6`（`0x439520`，查 `DITHER_TABLE_12`） |
| `3` | 经白色淡入淡出 (Fade Through White) | `0x448c70` (`0x448a30` + `0x448b50`) | **Phase 1 (`0x448a30`)**：旧画面淡白，`steps + 1` 帧，`global_alpha = (i * 255) / steps` (`0 -> 255`)<br>**Phase 2 (`0x448b50`)**：新画面从白淡入，`steps + 1` 帧，`global_alpha = ((steps - i) * 255) / steps` (`255 -> 0`) | 均使用 `mode = 4, sub = 6`（`0x439c00`, `DelTable`）；每帧延迟 `delay_ms` |
| `4` | 经黑色淡入淡出 (Fade Through Black) | `0x448a00` (`0x4487c0` + `0x4488e0`) | **Phase 1 (`0x4487c0`)**：旧画面淡黑，`steps + 1` 帧，`global_alpha = (i * 255) / steps` (`0 -> 255`)<br>**Phase 2 (`0x4488e0`)**：新画面从黑淡入，`steps + 1` 帧，`global_alpha = ((steps - i) * 255) / steps` (`255 -> 0`) | 均使用 `mode = 3, sub = 6`（`0x439f70`, `AddTable`）；每帧延迟 `delay_ms` |
| `5..=99`（实有资产 `14..=19, 21..=26`） | `.msk` 灰度规则遮罩转场 | `0x42223e -> 0x448170` | 格式化路径 `"%s\\ef%02d"` (`0x46dac8`) 加载 `.msk` 至 `0xa0` Alpha。令 `N = steps * 2`；对 `i = 1..=N-1`（共 `2*steps - 1` 帧）：`threshold = (i * 255) / N`；最后调用 `0x4462c0` | `mode = 0, sub = 2`（`0x4375f0`，软阈值 32 级羽化 `[thresh, min(thresh+31, 254)]`） |
| `100..=199` | 焦点放大半混合转场 (Zoom-In 50% Blend) | `0x448540` | 解码十位 `p1 = (id % 100) / 10` 与个位 `p2 = id % 10` 为焦点坐标（`0x421740`）。**忽略 `steps`/`delay_ms`**，固定每次 `Sleep(5)`：<br>**Phase 1**：`z = 0, 20, 40, ..., 600`（31 帧）以 `p1` 放大旧画面；<br>**Phase 2**：`z = 600, 580, ..., 0`（31 帧）以 `p2` 缩回新画面 | `0x446440` (`StretchBlt` 裁剪 `(W*100)/(z+100)` 拉伸至全屏) + `mode = 8, global_alpha = 255, sub = 6` (`0x4382e0` 半混合) |
| `200..=299` | 焦点缩黑再扩开转场 (Zoom-Out to Black) | `0x4486c0` | 同上解码 `p1, p2`，**忽略 `steps`/`delay_ms`**，每次 `Sleep(5)`：<br>**Phase 1**：`z = 0, 50, 100, ..., 2000`（41 帧）以 `p1` 将旧画面缩向黑底；<br>**Phase 2**：`z = 2000, 1950, ..., 0`（41 帧）以 `p2` 从黑底展开新画面 | 每帧先以 `0x435700(0)` 清黑，再 `0x446590`（`StretchBlt` 将全屏缩至 `(W*100)/(z+100)` 居中于焦点） |
| `>= 300` | 无效/空转 | `0x422132 -> 0x422286` | 直接返回 | 无操作 |

**焦点九宫格映射 `FUN_00421740(pt, digit)`**（小键盘方向布局，`W=800, H=600`）：
- `7:(0,0)`、`8:(W/2,0)`、`9:(W,0)`、`4:(0,H/2)`、`5/0:(W/2,H/2)`、`6:(W,H/2)`、`1:(0,H)`、`2:(W/2,H)`、`3:(W,H)`。

> 注：经实盘资源验证，`grps.xfl` 仅包含 `ef14.msk`～`ef19.msk`、`ef21.msk`～`ef26.msk` 共 **12 个**规则遮罩（无 `ef20.msk`）。

---

## 4. UNK-10：MPEG-1 视频实测与 MCI 控制流

### 4.1 `CMpegWindow` 控制流与同步机制（`KhimeDL.exe`）

1. **触发入口**：
   - **标题启动序列（`0x426ce0`）**：收到消息 `0x40e`（且 `DAT_00471354 == 0`）时播放 `.\mov\0002.mpg`（`0x46e02c`）；收到第一次播放完成通知 `0x41e` 时置 `DAT_00471354 = 1` 并播放 `.\mov\0001.mpg`（`0x46e01c`）；收到第二次 `0x41e` 时调用 `0x426d90` 进入标题画面。
   - **脚本播放指令（`0x424dc0`）**：按 `"%s\\%04d.mpg"`（`0x46dafc`）格式化文件名，调用 `0x449060`，随后在脚本线程调用 `FUN_00426f10()` 执行 `WaitForSingleObject(DAT_004712e0, INFINITE)` 阻塞等待。
2. **异步消息投递（`0x449060`）**：
   - `0x4509f0`：拷贝路径至 `[this+0x48]`，向子窗口 `MpegWindowClass`（`0x4505f0` 创建，初始无 `WS_VISIBLE`，覆盖主窗口整个 `800×600` 客户区）投递 `PostMessageA(mpg_hwnd, 0x405, 0, 0)`。
   - `0x4509d0`：紧接着投递 `PostMessageA(mpg_hwnd, 0x402, 0, 0)`。
3. **MCI 打开、拉伸与播放（`0x450900` WndProc）**：
   - `0x405` → `0x450720`：调用 `mciSendCommandA(0, MCI_OPEN [0x803], 0x2200, ...)` 打开 `"mpegvideo"`（`0x46e258`）；若失败直接 `PostMessageA(parent_hwnd, 0x41e, 0, 0)` 避免死锁；成功则用 `MCI_DGV_WINDOW [0x841]` 绑定到 `mpg_hwnd`，再用 `GetClientRect(parent)` 与 `MCI_DGV_PUT_DESTINATION [0x842]` 将视频矩形拉伸至 **`800×600`** 全屏显示。
   - `0x402` → `0x450820`：调用 `mciSendCommandA(dev, MCI_PLAY [0x806], MCI_NOTIFY | MCI_FROM [0x5], ...)` 从头异步播放，将 `[esi+0x14c]` 置 `1`，并取得 `GetWindow(mpg_hwnd, GW_CHILD=5)` 刷新显示（`InvalidateRect`）。
4. **跳过与结束同步（`0x4508a0` & `0x41a2a0`）**：
   - **触发终止条件**（`0x450900`）：`0x202`（`WM_LBUTTONUP` 左键点击）、`0x3b9`（`MM_MCINOTIFY` 且 `wParam & 1` 即 `MCI_NOTIFY_SUCCESSFUL` 自然播完）、`0x403`（内部停止消息）。
   - **按键与右键实证**：`0x450900` **未拦截 `WM_KEYDOWN` / `WM_RBUTTONUP`**，全二进制搜索 `push 0x403` 结果为空（`[]`），说明原版视频子窗口覆盖期间**仅支持左键点击（`WM_LBUTTONUP`）或播完通知跳过/结束**。
   - `0x4508a0` 执行：`ShowWindow(mpg_hwnd, SW_HIDE=0)` → `MCI_STOP [0x808]` (`MCI_WAIT=2`) → `0x4506f0` (`MCI_CLOSE [0x804]`) → `PostMessageA(parent_hwnd, 0x41e, 0, 0)`。
   - 主窗口 `0x41a2a0` 收到 `0x41e` (`WM_USER+0x1E`) 时调用 `SetEvent(DAT_004712e0)`，唤醒阻塞在 `0x426f10()` 的脚本线程。
   - 音量说明：游戏内置 `0x448fd0` 仅控制 DirectSound 槽位音量，未发现对 MCI `mpegvideo` 设置音频卷控制；iOS 移植时视频轨应使用系统/全局主音量或独立静音开关语义对齐。

### 4.2 `pl_mpeg` 全帧视频与全量音频实测验证

在 `fujihime-re` 容器内使用 `re/ghidra/cache/pl_mpeg_test/verify_mpg` 对两部原始 `.mpg` 文件全量解码实测结果（exit code `0`）：

| 文件 | 分辨率 | FPS | 标头时长 (`plm_get_duration`) | 实际解码视频帧数 | 末帧 VPTS | 音频采样率 / 声道 | 解码音频包 / 每声道采样 | 音频时长 | `max_abs` | NaN / 解码错误 | BGRA FNV/滚动校验 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `mov/0001.mpg` | `320×240` | `30.000` | `106.433s` | `3,217` 帧 | `107.2000s` | `44100 Hz` / 立体声 | `4,106` 包 / `4,730,112` | `107.2588s` | `0.8935` | **0** | `0xebfd8dbd74931414` |
| `mov/0002.mpg` | `640×480` | `30.000` | `4.300s` | `131` 帧 | `4.3333s` | `44100 Hz` / 立体声 | `168` 包 / `193,536` | `4.3886s` | `0.0000` (静音流) | **0** | `0x3f068695cd5353bc` |

**工程评估与风险提示**：
1. **`pl_mpeg` 可直接胜任 iOS 14.4 移植**：两部视频共 `3,348` 帧及全部 MP2 立体声音频包均以 **0 错误、0 NaN** 完成软件解码，无需 FFmpeg 体积/专利负担；CPU 渲染后按双线性缩放至逻辑 `800×600` 即可 1:1 还原 MCI `MCI_DGV_PUT_DESTINATION` 行为。
2. **已知边界差异（非故障）**：`plm_get_duration()` 基于文件头估算的时长略短于实际帧时间戳（`106.433s` vs `107.200s`；`4.300s` vs `4.333s`），**严禁使用估算时长截断播放，必须以 `plm_has_ended()` / 实际解码 EOF 为准**；`0002.mpg` 虽含 `44100 Hz` 立体声音轨但实际样本全零（`max_abs=0.0000`），属于原资产正常现象。

---

## 5. 全量资产自动化验证结果

验证脚本：[`re/verify_wcg_lwg.py`](file:///home/eggdog/%E9%A1%B9%E7%9B%AE/%E8%85%90%E5%A7%AC/re/verify_wcg_lwg.py)（在隔离容器 `fujihime-re` 中 4 核实跑，耗时 `160.811s`，退出码 `0`，**0 失败**）：

| 资产集合 | `.wcg` | `.lwg` (内嵌图层数) | `.msk` | `.bmp` | 尾部 `0x00` 填充流字节数 | 结果 |
|---|---|---|---|---|---|---|
| `[ORIG] grpe.xfl` | 329 | 0 | 0 | 0 | 0 | PASS |
| `[ORIG] grpo.xfl` | 669 | 0 | 0 | 0 | 0 | PASS |
| `[ORIG] grpp.xfl` | 198 | 0 | 0 | 0 | 0 | PASS |
| `[ORIG] grps.xfl` | 55 | 15 (154 层) | 12 | 0 | 0 | PASS |
| `[ORIG] grpf.xfl` | 40 | 0 | 0 | 0 | 0 | PASS |
| `[ORIG] grpo_ex.xfl` | 320 | 0 | 0 | 0 | 0 | PASS |
| `[ORIG] 补丁/原版备份/grpo_tp.xfl` | 116 | 0 | 0 | 0 | 0 | PASS |
| **原版资产小计** | **1,727** | **15 (154 层)** | **12** | **0** | **0** | **100% PASS** |
| `[CHS ] grpo_tp.xfl` | 80 | 0 | 0 | 36 (全 32bpp) | 0 | PASS |
| `[CHS ] grpe/` | 10 | 0 | 0 | 0 | 3 | PASS |
| `[CHS ] grpo_ex/` | 210 | 0 | 0 | 0 | 68 | PASS |
| `[CHS ] grps/` | 12 | 2 (76 层) | 0 | 0 | 22 | PASS |
| **汉化补丁资产小计** | **312** | **2 (76 层)** | **0** | **36** | **93** | **100% PASS** |

- 关键样本校验（`sha256` 前缀与倒置 Alpha 实测）：
  - `grpe.xfl/0001.wcg` (`800×600`, `1,920,000B`): `alpha ∈ [0, 0]`，`pixel0 = [13, 28, 25, 0]`，`sha256_prefix = ee7ea087f5ddbb42`
  - `补丁/原版备份/grpo_tp.xfl/8000.wcg` (`496×218`, `432,512B`): `alpha ∈ [0, 255]`，`pixel0 = [0, 0, 0, 255]`，`sha256_prefix = b07de25bb8760648`
  - `grpo_tp.xfl/8000.bmp` (`496×218`, `32bpp`, `432,512B`): `alpha ∈ [0, 255]`，`pixel0 = [0, 0, 0, 255]`，`sha256_prefix = d929d10c24ae7b66`
  - `grps.xfl/compane.lwg` (`39×106`, `18` 层，首层 `slide_lev` `14×60`): `alpha ∈ [0, 0]`，`sha256_prefix = 0a04fad48f3f2ac1`
  - `grps.xfl/ef14.msk` (`800×600`, `480,000B`): `mask ∈ [26, 255]`，`sha256_prefix = c6f9da9de2f4b4db`

---

## 6. UNK 状态汇总

- **UNK-01**（`.xfl` / `.wcg` / `.lwg` / `.msk` / `.bmp`）：**RESOLVED（已 100% 实证闭环）**。注：`KhimeDL.exe` 内保留有 `.wcg` 非 `0x40` 通道分支（`0x42f5c0 -> 0x432220`）和 32bpp TGA 加载类（`0x42f990 -> 0x42f9e0/0x42fa70`），但全量原版及汉化资产均未使用该编码路径；`.lwg` 图层 `u8 flag`（`0x42d0b0`，实测仅 `{0x08, 0x09, 0x28, 0x29}`）在 `KhimeDL.exe` 全二进制无任何调用点。
- **UNK-03**（软件合成器）：**RESOLVED（已 100% 实证闭环）**。全部 `Blit` 模式、查表算术、倒置 Alpha 约定、目标 Alpha 写回/保留规则及裁剪行为已通过汇编级验证。
- **UNK-04**（转场特效全表）：**RESOLVED（已 100% 实证闭环）**。涵盖 `0`, `1`, `2`, `3` (淡白), `4` (淡黑), `5..99` (`.msk` 规则), `100..199` (焦点缩放半混合), `200..299` (焦点缩黑)。
- **UNK-10**（MPEG-1 视频与 MCI）：**RESOLVED（已 100% 实证闭环）**。`pl_mpeg` 全帧 + 全音轨实测 0 错误，MCI 拉伸 `800×600` 与 `WM_LBUTTONUP` / `WM_USER+0x1E` 同步机制已完全证实。

