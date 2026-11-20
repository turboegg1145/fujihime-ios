# 《腐姫～euthanasia～》`.gsc` 脚本格式与虚拟机指令全规格书（Phase 1B）

> **权威基准（Ground Truth）**：所有偏移、位域掩码、数据结构与语义均逆向自原版日文主程序 `game/腐姫ＤＬ/KhimeDL.exe`（SHA-256: `2dfc7b78d32867716ab0406ccbf176b55a76faea1cb9c40feac25c90775222fe`）及原版脚本封包 `game/腐姫ＤＬ/补丁/原版备份/scr.xfl`（341 个 `.gsc`）。汉化版 `scr.xfl`（341 个 `.gsc`）的实测差异与兼容要求已完整标注。
>
> **交叉索引**：本文涉及的图像解码（`.wcg`/`.lwg`/`.msk`）、图层合成（`0x436430`）、转场特效（`0x422060`）与 MPEG-1 视频控制（`0x449060`/`0x450720`）底层像素与状态机规格，详见 Phase 1A 规格书 [`docs/formats-and-gfx-spec.md`](formats-and-gfx-spec.md)。
>
> **约定**：所有多字节整数均为 **Little-Endian (LE)**；脚本字节码指针 `pc` 为相对字节码区起始（文件偏移 `0x24`）的字节偏移。

---

## 1. `.gsc` 二进制文件格式（`0x417c70` / `0x4179d0` / `0x417ab0`）

脚本加载器由 `FUN_00417c40` 封装：`FUN_00417c70` 从虚拟文件系统读取 `%s\%04d.gsc`（失败时 `FUN_00417fe0` 回退尝试 `RsComp.dll` 编译 `.tsc`，原版及汉化发布版均只使用 `.gsc`）。

### 1.1 36 字节文件头（9 × `u32`）

`FUN_00417c70` 连续读取 9 个 32 位小端整数到脚本对象 `this`：

| 文件偏移 | `this` 字段偏移 | 字段名 | 类型 | 语义与实证 |
|---:|---:|---|---|---|
| `0x00` | *(局部变量)* | `file_size` | `u32` | `.gsc` 逻辑总字节数（原版全部 341 文件 `file_size == len(data)`） |
| `0x04` | *(局部变量)* | `header_size` | `u32` | 固定为 `36`（`0x24`） |
| `0x08` | `+0x28` | `code_size` | `u32` | 字节码区长度（字节） |
| `0x0C` | `+0x2C` | `str_off_size` | `u32` | 字符串偏移表字节数；加载后除以 4（`>> 2`）得条目数 `str_count` |
| `0x10` | `+0x30` | `str_data_size` | `u32` | 字符串数据区总字节数 |
| `0x14` | `+0x34` | `arr_off_size` | `u32` | 整数数组偏移表字节数；加载后除以 4（`>> 2`）得条目数 `arr_count` |
| `0x18` | `+0x38` | `arr_data_u16_cnt` | `u32` | 整数数组数据区 **`u16` 元素个数**（实际字节数 = `arr_data_u16_cnt * 2`） |
| `0x1C` | `+0x60` | `lbl_table_size` | `u32` | 标签偏移表字节数；加载后除以 4（`>> 2`）得标签条目数 `lbl_count`（文件内连续存放**两张**同长表） |
| `0x20` | `+0x5C` | `lbl_names_size` | `u32` | 标签名字符串区总字节数 |

### 1.2 七节区顺序与精确文件长度

紧接 36 字节头部之后，依次连续存放以下节区：

| 节区序号 | `this` 指针偏移 | 字节长度 | 内部结构与用途 |
|---:|---:|---|---|
| 1 | `+0x00` | `code_size` | **字节码指令流**：起始为 `pc = 0`（`this+0x64`） |
| 2 | `+0x04` | `str_off_size` | **字符串偏移表**：`u32[str_count]`，相对节区 3 起始；索引 `0` 固定为 `0`（空串） |
| 3 | `+0x08` | `str_data_size` | **字符串数据区**：NUL 结尾字符串序列（偏移 `0` 为 `0x00`；原版 CP932 / 汉化 GBK） |
| 4 | `+0x0C` | `arr_off_size` | **数组索引表**：`u32[arr_count]`，以 `u16` 为单位索引节区 5；索引 `0` 固定为 `0` |
| 5 | `+0x10` | `arr_data_u16_cnt * 2` | **数组数据区**：`u16` 数组；每条记录格式为 `[len: u16, elem_0: u16, ..., elem_{len-1}: u16]`（见 `0x417ab0`，供 opcode `0x12` 使用） |
| 6a | `+0x4C` | `lbl_table_size` | **标签名偏移表**：`u32[lbl_count]`，相对节区 7 起始；索引 `0` 保留为 `0` |
| 6b | `+0x54` | `lbl_table_size` | **标签目标字节码偏移表**：`u32[lbl_count]`，供 `0x4179d0` 按标签名返回目标 `pc` |
| 7 | `+0x44` | `lbl_names_size` | **标签名数据区**：NUL 结尾 ASCII 标签名（如 `"omake"`, `"8011"`..`"8031"`） |

$$\text{calc\_size} = 36 + \text{code\_size} + \text{str\_off\_size} + \text{str\_data\_size} + \text{arr\_off\_size} + 2 \cdot \text{arr\_data\_u16\_cnt} + 2 \cdot \text{lbl\_table\_size} + \text{lbl\_names\_size}$$

> **字符串读取与动态插值（`0x417a90` → `0x421310` = `0x421200` + `0x421130`）**：
> `0x417a90(idx)` 返回 `&str_data[str_off[idx]]`。送往文本显示或标签比对前经过两遍展开（均使用 `IsDBCSLeadByteEx` 跳过双字节汉字第二字节）：
> 1. `0x421200`（整数变量展开）：识别 `@` 前缀计数 `depth`（通过 `0x44b620` 解析可选 `-` 与十进制索引 `idx`），对 `vars[idx]` 执行 `depth` 次解引用后以 `%d`（半角 ASCII 数字）替换；
> 2. `0x421130`（字符串变量展开）：识别 `$0`..`$9` 并替换为 `str_vars[0..9]`（`DAT_00476a88 + digit * 0xC9`）。

---

## 2. 虚拟机状态模型与寻址机制

### 2.1 核心存储区（均已对照 `0x41de30` / `0x41e160` / `0x41f420` 实证）

| 存储区 | 原版内存地址 / 指针 | 类型与容量 | 说明 |
|---|---|---|---|
| **临时寄存器组 `regs`** | `DAT_00483270` | `i16[]` | 表达式指令族 `0x1000..0xF000` 的结果目标；`regs[0]` 为条件寄存器（被 `0x03 jz` / `0x04 jnz` 测试） |
| **脚本变量数组 `vars`** | `DAT_00471368` | `i16[7000]`（快照复制 `0xdac` dwords = `14,000B`；`0xca` 上限钳制 `9999`） | `vars[0]` 存放 `0x10 ret` 返回值及 `0x19/0x6f/0x71/0x72/0x99` 结果；`vars[10..19]` 存放 `0x0f`/`0xc8` 传入的 10 个参数；`vars[9000..9999]` 在 `0x14` (`bg_load`) 中写入已看 CG 标志 |
| **字符串变量 `str_vars`** | `DAT_00476a88` | `[u8; 201][10]`（步长 `0xC9`） | `$0..$9` 局部字符串变量，由 `0x96..0x9b, 0x9e..0x9f` 读写 |
| **全局字符串槽** | `DAT_004713b0 + 2` | `[u8; 201][...]`（步长 `0xC9`） | 由 `0x9e`/`0x9f` 与 `str_vars` 互拷，随全局进度保存 |
| **图层 Z 序与分组表** | `DAT_00476570` / `DAT_00476638` | `i16[100]` / `u16[100]` | 记录图层 `0..99` 的 Z 深度与批量操作分组号（`0x28`/`0x29`） |
| **坐标缩放因子** | `DAT_00476908` / `0x47690a` | `i16, i16` | 由 `0x2f coord_scale(sx, sy)` 设置，对 `0x1e` / `0x20` / `0x21` / `0x22` 的坐标乘以 `(sx, sy)`，并对 `0x6e` 返回值做整除 |
| **脚本调用栈** | `DAT_00471bdc` (`depth`), `0x471b9e`, `0x471bb4` | 最大深度 `9`（`0..=8`） | `0x0f` / `0xc8` 压栈返回 `(script_id, return_pc)`，`0x10` 弹栈恢复；超限弹出 `'スクリプトネストオーバーフロー'` / `'スクリプトネストアンダーフロー'` |
| **全局标志与已读位图** | `DAT_004712bc` / `DAT_004712b8` | 位图树 | `0x72`/`0x14` 读写 `DAT_004712bc`（CG/全局标志）；`0x51`/`0x52` 查询并置位 `DAT_004712b8[script_id][str_idx]` 已读位 |
| **回滚与存档快照** | `0x471b8c` → `0x477750` → `0x47d314` | `0x16f1` dwords (`0x5bc4` B) + `14,000B` `vars` | `0x37` 快照至 `0x477750`，`0x32` 保存回滚检查点至 `0x47d314`，`0x33` 恢复回滚并重绘 |

### 2.2 `expr` 4 字节操作数编码（`0x421350` x86 汇编实证）

`FUN_00421350` 每次固定读取 **4 字节**（`u32 raw` via `0x417a70`）：
- `val: i16 = (raw & 0xFFFF) as i16`
- `depth: i16 = ((raw >> 16) & 0xFFFF) as i16`
- 若 `depth <= 0`（标准立即数为 `0`）：直接返回 **`val`**（16 位有符号立即数或输出变量索引）。
- 若 `depth > 0`（`1` = 直接变量 `vars[val]`，`2` = 二级间接 `*vars[val]`，以此类推）：循环执行 `depth` 次 `val = vars[val]` 后返回。

### 2.3 字节码读取器 EOF 守卫（`0x417a30..0x417af0`）

- `0x417a30` (`u16`) / `0x417a50` (`i16`) / `0x417a70` (`u32`) 在 `pc >= code_size` 时返回 `0`（不推进 `pc`）。
- `0x417af0(target)` 仅当 `target < code_size` 时更新 `pc = target` 并返回 `1`；若 `target >= code_size` 则返回 `0` 且不改写 `pc`。
- **实测边界现象**：全库 341 个脚本中共有 **20 处** `0x03`/`0x05`/`0x0e` 跳转的目标 `dst == code_size`（编译器在文件末尾 `if/else` 或 `select` 块结尾生成向块尾的跳转）：
  - 在 `0001.gsc` 等脚本中为前条 `goto_script` 之后不可达的跳转；
  - 在 `0399.gsc`（`jz 3788` 后接 `ret 0`）、`3456.gsc`（`select` 分支 3 跳 `1624`）、`5109.gsc`（`jz 2888` 后接 `ret 0`）中，即使进入 `code_size`，语义均为**当前脚本逻辑结束**。Rust VM 实现 `jump(target)` 时应将 `target == code_size` 视为合法结束位置（若调用栈非空则等同隐式返回，否则退出脚本执行），切勿在解码器报错。

---

## 3. 高位表达式与赋值指令族（`0x1000..0xF000`）

当 `op = read_u16()` 满足 `(op & 0xF000) != 0` 时，进入运算指令族（`0x41f560..0x41f900`）。

### 3.1 16 位操作码位域（对照 `0x41f420` / `0x41f450` / `0x41f4b0` / `0x41f500` 汇编）

```text
 15..12 | 11..10  |  9..8   |  7..4   |  3..0
--------+---------+---------+---------+--------
  op_hi | lhs_mod | rhs_mod | lhs_dep | rhs_dep
```

- **`lhs_mod` (`(op >> 10) & 3`)** 与 **`lhs_dep` (`(op >> 4) & 0xF`)**（读取由 `0x41f450`，写入由 `0x41f500`）：
  - `0`：立即数 `lhs_raw`（仅读；写入无操作）
  - `1`：临时寄存器 `regs[lhs_raw]`
  - `2`：变量 `vars[deref(lhs_dep, lhs_raw)]`，其中 `deref(d, v)` 循环 `d` 次 `v = vars[v]`（`0x41f420`）
- **`rhs_mod` (`(op >> 8) & 3`)** 与 **`rhs_dep` (`op & 0xF`)**（读取由 `0x41f4b0`）：
  - `0`：立即数 `rhs_raw`
  - `1`：临时寄存器 `regs[rhs_raw]`
  - `2`：变量 `vars[deref(rhs_dep, rhs_raw)]`

### 3.2 全部 15 条运算指令规格

| 高 4 位 | 总字节 | 操作数序列 | 处理函数 | 精确语义（全部为 `i16` 运算，写入 `regs[reg_dst]`） |
|---|---:|---|---|---|
| `0x1000` | **8** | `reg_dst: u16, lhs_raw: i16, rhs_raw: i16` | `0x41f590` → `0x41f500` | 赋值：`val = read_rhs(rhs_raw)`; 写入 `lhs`（`regs` 或 `vars`），且写入 `regs[reg_dst] = val` |
| `0x2000` | **8** | `reg_dst: u16, lhs_raw: i16, rhs_raw: i16` | `0x41f5c0` | 逻辑或：`regs[reg_dst] = (lhs != 0 \|\| rhs != 0) ? 1 : 0`（短路求值） |
| `0x3000` | **8** | `reg_dst: u16, lhs_raw: i16, rhs_raw: i16` | `0x41f620` | 逻辑与：`regs[reg_dst] = (lhs != 0 && rhs != 0) ? 1 : 0`（短路求值） |
| `0x4000` | **8** | `reg_dst: u16, lhs_raw: i16, rhs_raw: i16` | `0x41f680` | 相等：`regs[reg_dst] = (lhs == rhs) ? 1 : 0` |
| `0x5000` | **8** | `reg_dst: u16, lhs_raw: i16, rhs_raw: i16` | `0x41f6c0` | 大于等于：`regs[reg_dst] = (lhs >= rhs) ? 1 : 0` |
| `0x6000` | **8** | `reg_dst: u16, lhs_raw: i16, rhs_raw: i16` | `0x41f700` | 大于：`regs[reg_dst] = (lhs > rhs) ? 1 : 0` |
| `0x7000` | **8** | `reg_dst: u16, lhs_raw: i16, rhs_raw: i16` | `0x41f740` | 小于等于：`regs[reg_dst] = (lhs <= rhs) ? 1 : 0` |
| `0x8000` | **8** | `reg_dst: u16, lhs_raw: i16, rhs_raw: i16` | `0x41f780` | 小于：`regs[reg_dst] = (lhs < rhs) ? 1 : 0` |
| `0x9000` | **8** | `reg_dst: u16, lhs_raw: i16, rhs_raw: i16` | `0x41f7c0` | 不等：`regs[reg_dst] = (lhs != rhs) ? 1 : 0` |
| `0xA000` | **8** | `reg_dst: u16, lhs_raw: i16, rhs_raw: i16` | `0x41f800` | 加法：`regs[reg_dst] = lhs.wrapping_add(rhs)` |
| `0xB000` | **8** | `reg_dst: u16, lhs_raw: i16, rhs_raw: i16` | `0x41f840` | 减法：`regs[reg_dst] = lhs.wrapping_sub(rhs)` |
| `0xC000` | **8** | `reg_dst: u16, lhs_raw: i16, rhs_raw: i16` | `0x41f880` | 乘法：`regs[reg_dst] = lhs.wrapping_mul(rhs)` |
| `0xD000` | **8** | `reg_dst: u16, lhs_raw: i16, rhs_raw: i16` | `0x41f8c0` | 除法：`regs[reg_dst] = lhs / rhs`（有符号 16 位截断除） |
| `0xE000` | **8** | `reg_dst: u16, lhs_raw: i16, rhs_raw: i16` | `0x41f900` | 取模：`regs[reg_dst] = lhs % rhs` |
| `0xF000` | **6** | `reg_dst: u16, rhs_raw: i16` | `0x41f560` | 加载寄存器：`regs[reg_dst] = read_rhs(rhs_raw)` |

---

## 4. 全部 120 条命令 Opcode 完整规格（`0x03..0xD5`）

> **操作数类型简写**：`u16` = `0x417a30`（2B）；`u32` = `0x417a70`（4B）；`expr` = `0x421350`（4B，带变量/间接寻址解码）。每条命令字节总长 = `2 + sum(操作数字节)`。`出现次数` 统计自原版（与汉化版完全一致）全部 341 个 `.gsc`。

### 4.1 控制流、脚本调用与子程序（13 条）

| Opcode | 总字节 | 助记名 | 操作数签名 | 处理函数 | 341脚本频次 | 精确语义与副作用 |
|---|---:|---|---|---|---:|---|
| `0x03` | 6 | `jz` | `target: u32` | `0x41f980` | 886 | 若 `regs[0] == 0`，跳转至 `target` |
| `0x04` | 6 | `jnz` | `target: u32` | `0x41f960` | 0 | 若 `regs[0] != 0`，跳转至 `target` |
| `0x05` | 6 | `jmp` | `target: u32` | `0x41f9a0` | 297 | 无条件跳转至 `target` |
| `0x08` | 2 | `exit` | *(无)* | `0x41f9f0` | 0 | 退出 VM 分发循环 |
| `0x09` | 4 | `rand_reg` | `reg_dst: u16` | `0x41f940` | 3 | `regs[reg_dst] = rand() as i16` |
| `0x0A` | 2 | `wait_click` | *(无)* | `0x424140` | 4 | 等待玩家点击（不自动清屏；快进/Auto 时休眠 `DAT_00471394`） |
| `0x0B` | 2 | `wait_click_clear` | *(无)* | `0x424230` | 0 | 等待玩家点击后清空文本槽 `0` |
| `0x0C` | 10 | `goto_script` | `script_id: expr, label_str: u32` | `0x4214c0` | 146 | 卸载当前脚本并加载 `%04d.gsc`；`label_str != 0` 时经 `0x4179d0` 定位标签偏移，否则 `pc = 0`（失败返回 0 退出 VM） |
| `0x0D` | 6 | `sleep_100ms` | `ticks: expr` | `0x4240e0` | 152 | 若未按 Ctrl/FastSkip，休眠 `ticks * 100` ms |
| `0x0F` | 50 | `call_script` | `script_id: expr, label_str: u32, args: [expr; 10]` | `0x421540` | 384 | 压入当前 `(script_id, next_pc)` 至调用栈（最大深度 9），跳转至新脚本（及可选标签），并把 `args[0..10]` 写入 `vars[10..20]` |
| `0x10` | 6 | `ret` | `ret_val: expr` | `0x4216b0` | 428 | 弹出调用栈，重载原脚本与返回 `pc`，写入 `vars[0] = ret_val` |
| `0x13` | 6 | `sub_vm_call` | `script_id: expr` | `0x424ef0` | 0 | 在栈上备份完整全局游戏状态与文本框状态并嵌套运行 `script_id` 的 VM 循环，返回后恢复现场（全作未使用） |
| `0xC8` | 46 | `gosub_local` | `target_pc: u32, args: [expr; 10]` | `0x421620` | 26 | 压入当前 `(script_id, next_pc)` 至调用栈，跳转至本脚本内 `target_pc`，并将 `args[0..10]` 写入 `vars[10..20]` |

### 4.2 选项分支、系统菜单与回滚存档（10 条）

| Opcode | 总字节 | 助记名 | 操作数签名 | 处理函数 | 341脚本频次 | 精确语义与副作用 |
|---|---:|---|---|---|---:|---|
| `0x0E` | 60 | `select_6` | `count: u16, prompt_str: u32, targets: [u32; 5], choice_strs: [u32; 5], opt0: expr, opt1: expr, opt2: expr` | `0x424340` | 30 | 弹出最多 5 项分支菜单（实测 `count ∈ {1,2,3}`, `opt = (0,1,0)`）；若在快进则自动中止快进并恢复画面；等待玩家选定第 `k` 项后调用 `0x41f9a0(targets[k])`（同时把 `prompt_str` 写入 `DAT_00482ee4`） |
| `0x11` | 2 | `menu_pause` | *(无)* | `0x424e10` | 0 | 打开存档/菜单界面（`0x41ad70`）并等待退出 |
| `0x19` | 10 | `call_subscene_dll` | `str_idx: u32, arg0: expr` | `0x424790` | 0 | 写入路径到 `DAT_0048307c`、`vars[0] = arg0`，关闭音频并通过消息 `0x40f` 退出当前场景（类似切换外挂/子场景 DLL） |
| `0x32` | 2 | `rollback_save` | *(无)* | `0x41e160` | 0 | 若 `DAT_0047135e == 0`，将当前快照缓冲区 `0x477750` 备份到回滚缓冲区 `0x47d314` |
| `0x33` | 2 | `rollback_restore` | *(无)* | `0x424060` | 0 | 从回滚缓冲区 `0x47d314` 恢复状态并重载脚本与画面 |
| `0x34` | 2 | `skip_stop` | *(无)* | `0x424110` | 0 | 强制停止 Ctrl/Scene Skip 状态并重绘画面 |
| `0x35` | 6 | `rollback_flag` | `enable: expr` | `0x424020` | 0 | 设置 `DAT_00471bf4 = enable`（控制等待后是否自动保存回滚检查点） |
| `0x37` | 2 | `save_snapshot` | *(无)* | `0x41de30` | 0 | 将当前运行时状态 `0x471b8c` 与 `vars` 快照到 `0x477750`（供存档使用） |
| `0x3A` | 2 | `abort_if_flag` | *(无)* | `0x41f9f0` | 0 | 若 `DAT_0047135c != 0` 直接退出 VM 分发循环 |
| `0xD5` | 14 | `map_select` | `mode: expr, param2: expr, unused: expr` | `0x424550` | 0 | 使用 `DAT_00471070` 打开地图/特殊选择器，结果写入 `vars[0]`（全作未使用） |

### 4.3 文本显示、文本框样式与历史记录（27 条）

| Opcode | 总字节 | 助记名 | 操作数签名 | 处理函数 | 341脚本频次 | 精确语义与副作用 |
|---|---:|---|---|---|---:|---|
| `0x2A` | 10 | `tbox_show` | `slot: expr, show: expr` | `0x422c50` | 737 | 显示 (`show!=0`) 或隐藏 (`show==0`) 文本窗口并触发 `0x421860` 刷新 |
| `0x30` | 10 | `portrait_face` | `face_id: expr, mode: expr` | `0x422df0` | 21 | 设置对话框立绘头像 `face_id`（`0x421fb0` → `0x415770`） |
| `0x38` | 22 | `tbox_style` | `p1..p5: [expr; 5]` | `0x423fd0` | 1 | 配置文本框属性 (`p1, p3, p4` → `0x415160`; `DAT_00471bec = p2`; `DAT_00471bf0 = p5` 自动存档快照开关) |
| `0x39` | 2 | `tbox_reset` | *(无)* | `0x424040` | 0 | 重置文本框状态（`0x414830`） |
| `0x50` | 6 | `text_clear` | `slot: expr` | `0x4228d0` | 0 | 清除文本槽 `slot` 并立刻刷屏 |
| `0x51` | 30 | `text_voice` | `slot: expr, voice_id: expr, loop: expr, delay_50ms: expr, unused_str: u32, text_str: u32, clear: expr` | `0x422a70` | 14686 | **对话主指令**：检查未读与跳过标志；标记 `(script_id, text_str)` 已读；覆盖 `DAT_00482ee4` backlog；渲染文本（`0x415380`）；若 `voice_id != 0` 播语音（延时 `delay_50ms*50`ms）；调用 `0x422830(slot, clear)` 等待点击并按需清框 |
| `0x52` | 26 | `text_say` | `slot: expr, p2: expr, p3: expr, p4: expr, text_str: u32, clear: expr` | `0x422900` | 0 | 无语音追加文本（`lstrcatA` 到 `DAT_00482ee4`，其余同 `0x51`；全作均使用 `0x51`） |
| `0x53` | 6 | `text_clear_wait` | `slot: expr` | `0x422710` | 3 | 清除文本槽 `slot` 后触发 `0x421860`（非快进时等待一次点击） |
| `0x5A` | 14 | `tbox_text_pos` | `slot: expr, x: expr, y: expr` | `0x422640` | 6 | 设置文本区原点 `(x, y)` |
| `0x5B` | 22 | `tbox_rect` | `slot: expr, x1: expr, y1: expr, x2: expr, y2: expr` | `0x4225f0` | 0 | 设置文本区矩形边界 |
| `0x5C` | 10 | `tbox_font_size` | `slot: expr, size: expr` | `0x422540` | 12 | 设置字体字号 |
| `0x5D` | 10 | `tbox_wrap_cfg` | `slot: expr, val: expr` | `0x4226d0` | 12 | 设置换行/页缓冲高度参数（`0x415700`） |
| `0x5E` | 6 | `tbox_mode` | `mode: expr` | `0x422670` | 12 | 设置排版模式（如竖排/横排切换，`0x415750`） |
| `0x5F` | 10 | `tbox_color_preset` | `slot: expr, preset: expr` | `0x4223b0` | 12 | 预设文字色表：`1=0xFFFFFF, 2=0x79F1F2, 3=0xB73333, 4=0xFFDE00, 5=0xF8B1EF, 6=0x7FDFA5, 7=0xC187F6, 8=0xFAA25A, default=0x000000` |
| `0x60` | 10 | `tbox_char_spacing` | `slot: expr, px: expr` | `0x4224c0` | 12 | 设置字符间距 |
| `0x61` | 10 | `tbox_line_spacing` | `slot: expr, px: expr` | `0x4224e0` | 12 | 设置行间距 |
| `0x62` | 10 | `tbox_shadow` | `slot: expr, mode: expr` | `0x422500` | 12 | 设置文字阴影/描边模式 |
| `0x63` | 14 | `tbox_speed` | `slot: expr, speed: expr, p3: expr` | `0x422520` | 12 | 设置打字机输出速度 |
| `0x64` | 14 | `tbox_face_pos` | `slot: expr, x: expr, y: expr` | `0x422560` | 0 | 设置立绘头像坐标 `(x, y)` |
| `0x65` | 10 | `tbox_icon_pos` | `x: expr, y: expr` | `0x422690` | 4 | 设置点击等待光标动画坐标 `(x, y)` |
| `0x66` | 6 | `text_voice_wait_mode` | `mode: expr` | `0x424030` | 0 | 设置 `DAT_00471bfc`（决定文本显示前/后清屏时机） |
| `0x67` | 10 | `tbox_font_type` | `slot: expr, font_id: expr` | `0x422590` | 12 | 切换字体类型（`ＭＳ ゴシック` / `ＭＳ 明朝`） |
| `0x68` | 18 | `tbox_bg_color` | `slot: expr, r: expr, g: expr, b: expr` | `0x4225b0` | 12 | 设置对话框半透明底色 `(R << 16 \| G << 8 \| B)` |
| `0x69` | 6 | `tbox_bg_alpha` | `alpha: expr` | `0x4226b0` | 12 | 设置对话框背景透明度 |
| `0xD2` | 10 | `backlog_set_title` | `str_idx: u32, reset: expr` | `0x4247e0` | 0 | 记录场景回想/章节标题（`DAT_0047728c..`；全作未使用） |
| `0xD3` | 18 | `backlog_add_entry` | `str_idx: u32, p2: expr, p3: expr, p4: expr` | `0x424820` | 0 | 追加条目到 `DAT_00477298`（最多 100 条；全作未使用） |
| `0xD4` | 6 | `backlog_set_extra` | `val: u32` | `0x4248d0` | 0 | 写入 `DAT_0047774c`（全作未使用） |

### 4.4 图形、背景、CG、立绘图层、动画与特效（37 条）

> 所有图层下标 `layer ∈ 0..=99`（`>= 100` 弹出 `'レイヤー指定違反'`，部分多图层指令支持 `layer == 0` 表示全图层或 `layer > 100` 按组号 `layer % 100` 批量匹配 `DAT_0047663a`）。转场/合成细节参见 [`docs/formats-and-gfx-spec.md`](formats-and-gfx-spec.md)。

| Opcode | 总字节 | 助记名 | 操作数签名 | 处理函数 | 341脚本频次 | 精确语义与副作用 |
|---|---:|---|---|---|---:|---|
| `0x14` | 10 | `bg_load` | `cg_id: expr, alpha_or_mode: expr` | `0x422c80` | 538 | 若 `cg_id < 1000` 置位 `vars[9000 + cg_id] = 1` 并调用 `0x41e230(cg_id)` 解锁 CG 鉴赏；在图层 0 加载 CG（`0` 清空）；非批量模式触发转场 `0` |
| `0x15` | 6 | `bg_clear` | `white_flag: expr` | `0x422d00` | 0 | 将图层 0 清为纯白 (`white_flag==1`) 或纯黑 (`0`)；非批量模式触发转场 `1` |
| `0x16` | 18 | `spr_load_layer0` | `cg_id: expr, x: expr, y: expr, alpha: expr` | `0x422d40` | 0 | 向图层 0 加载图像并刷屏 |
| `0x17` | 18 | `shake` | `count: expr, dir: expr, u3: expr, u4: expr` | `0x424aa0` | 19 | 画面震动：`dir=1` 纵向 (`0x447da0`)、`dir=2` 环向 (`0x447eb0`)、其他横向 (`0x447c90`) |
| `0x18` | 10 | `flash` | `count: expr, mode: expr` | `0x424b10` | 21 | 画面闪烁：`1` 多彩闪 (`0x448080`)、`2` 纯蓝 (`0xFF`)、`3` 青色 (`0x80FFFF`)、其他纯白 (`0xFFFFFF`) via `0x448100` |
| `0x1A` | 2 | `batch_begin` | *(无)* | `0x423880` | 917 | 置 `DAT_004712f0 = 1`：开启批量绘图缓冲，后续图层修改暂不上屏直至 `0x1B` 或转场指令 |
| `0x1B` | 2 | `batch_end` | *(无)* | `0x423890` | 64 | 清除 `DAT_004712f0 = 0` 并调用 `0x421860()` 刷新画面 |
| `0x1C` | 14 | `transition` | `effect_id: expr, steps: expr, delay_ms: expr` | `0x4238b0` | 853 | 清 `DAT_004712f0 = 0` 并调用 `FUN_00422060(effect_id, steps, delay_ms)` 执行转场（`0, 1, 2, 3, 4, 5..99, 100..299`） |
| `0x1D` | 10 | `zoom_transition` | `out_flag: expr, focus_digit: expr` | `0x4238e0` | 0 | 清 `DAT_004712f0 = 0` 并调用 `FUN_00422060((out_flag!=0 ? 100 : 200) + focus_digit, 0, 0)` |
| `0x1E` | 26 | `spr_load` | `layer: expr, img_id: expr, x: expr, y: expr, z_mode: expr, alpha: expr` | `0x422d70` | 14 | 向 `layer`（非 0）加载编号图像 `img_id`，坐标 `(x*sx, y*sy)` |
| `0x20` | 26 | `spr_load_str` | `layer: expr, x: expr, y: expr, z_mode: expr, alpha: expr, str_idx: u32` | `0x422e20` | 30 | 向 `layer` 加载/渲染字符串 `str_idx`（如 `1160.gsc` 日期标题） |
| `0x21` | 22 | `spr_load_grp` | `layer: expr, x: expr, y: expr, img_id: expr, alpha: expr` | `0x422e70` | 0 | 批量加载图像（支持 `0`/单层/`>100` 组匹配） |
| `0x22` | 22 | `spr_load_rel` | `layer: expr, dx: expr, dy: expr, img_id: expr, alpha: expr` | `0x422f30` | 0 | 按当前坐标增量 `(dx*sx, dy*sy)` 加载图像 |
| `0x23` | 10 | `spr_show_mode` | `layer: expr, mode: expr` | `0x422ff0` | 0 | 调用 `0x421d20` 更新图层透明/显示子状态 |
| `0x24` | 10 | `spr_clear` | `layer: expr, mode: expr` | `0x423080` | 739 | 清除/卸载图层图像（`0x421b30` → `0x406790`），支持 `0` 全清或按组清除 |
| `0x25` | 10 | `spr_visible` | `layer: expr, vis: expr` | `0x423110` | 64 | 设置图层可见性 `vis`（`0x406420`） |
| `0x26` | 14 | `spr_pos` | `layer: expr, x: expr, y: expr` | `0x423230` | 6 | 设置图层坐标基准 `(x, y)`（`0x406300`） |
| `0x27` | 14 | `spr_blend` | `layer: expr, mode: expr, param: expr` | `0x423190` | 132 | 设置图层混合模式与参数（`0x406460` → `0x4097a0`） |
| `0x28` | 10 | `spr_z` | `layer: expr, z: expr` | `0x4232d0` | 0 | 设置图层 Z 序 `DAT_00476570[layer] = z` 并重排合成器顺序 |
| `0x29` | 10 | `spr_group` | `layer: expr, group_id: expr` | `0x423350` | 0 | 设置图层分组号 `DAT_00476638[layer] = group_id` |
| `0x2B` | 10 | `eyes_overlay` | `es_id: expr, blend_flag: expr` | `0x4235a0` | 30 | 加载 `grps\es%03d` 到特殊遮罩层 `DAT_0047106c`，`blend_flag==1` 使用模式 `0x67`，否则 `0x68` |
| `0x2C` | 6 | `eyes_z` | `z: expr` | `0x423710` | 0 | 设置 `DAT_0047106c` 图层 Z 序 |
| `0x2D` | 10 | `filter_overlay` | `alpha_pct: expr, mode_flag: expr` | `0x423760` | 11 | 全屏纯白覆盖层 `DAT_0047121c`，不透明度 `(alpha_pct * 255) / 100`，`mode_flag==1` 用模式 `0x6D`（提亮），否则 `0x6C`（压暗） |
| `0x2E` | 6 | `filter_z` | `z: expr` | `0x423830` | 1 | 设置 `DAT_0047121c` 图层 Z 序 |
| `0x2F` | 10 | `coord_scale` | `sx: expr, sy: expr` | `0x4226f0` | 4 | 设置脚本坐标缩放基准 `(DAT_00476908, DAT_0047690a)` |
| `0x31` | 10 | `spr_type` | `layer: expr, type_id: expr` | `0x423370` | 4 | 设置图层对象类型：`0` 静态图像，非 `0` 为动画精灵（`0x40a370`） |
| `0x3B` | 18 | `spr_load_centered` | `layer: expr, img_id: expr, mode: expr, alpha: expr` | `0x422dc0` | 0 | 加载图像并按锚点自动居中对齐（`0x421970` → `0x4065c0`） |
| `0x46` | 18 | `spr_anim_x` | `layer: expr, p2: expr, p3: expr, p4: expr` | `0x423910` | 26 | 设置图层横向/类型 0 移动动画（`0x406860`） |
| `0x47` | 18 | `spr_anim_y` | `layer: expr, p2: expr, p3: expr, p4: expr` | `0x423940` | 4 | 设置图层纵向/类型 1 移动动画（`0x4068d0`） |
| `0x48` | 6 | `spr_anim_stop` | `layer: expr` | `0x4239a0` | 6 | 停止图层动画（`0x406a90`） |
| `0x49` | 14 | `timer_wait_loop` | `var_idx: expr, mode: expr, p3: expr` | `0x4239f0` | 1 | 计时/动画等待循环，剩余时间写回 `vars[var_idx]` |
| `0x4A` | 6 | `auto_clear_anim` | `enable: expr` | `0x423bb0` | 1 | 设置 `DAT_00471bf8`（等待循环结束时是否自动停止全图层动画） |
| `0x4B` | 22 | `spr_anim_frame` | `layer: expr, img_id: expr, x: expr, y: expr, slot: expr` | `0x4239c0` | 26 | 设置图层叠加子帧图像 `slot ∈ {0, 1}`（`0x4069b0`） |
| `0x4D` | 18 | `spr_anim_alpha` | `layer: expr, p2: expr, p3: expr, p4: expr` | `0x423970` | 0 | 设置图层类型 2 动画（`0x406940`） |
| `0x78` | 10 | `spr_anchor` | `layer: expr, val: expr` | `0x423470` | 141 | 设置图层扩展参数 `[0x28c]+0x50`（`0x406340`） |
| `0x79` | 10 | `spr_dir` | `layer: expr, str_idx: u32` | `0x4234f0` | 177 | 设置图层资源目录前缀 `".\\%s"`（如 `.\grpo`, `.\grpp`） |
| `0xC9` | 22 | `spr_clip_rect` | `layer: expr, l: expr, t: expr, r: expr, b: expr` | `0x4233f0` | 0 | 设置图层裁剪矩形 `[0x28c]+0x1a..0x20`（`0x4064a0`） |

### 4.5 音频与视频控制（10 条）

| Opcode | 总字节 | 助记名 | 操作数签名 | 处理函数 | 341脚本频次 | 精确语义与副作用 |
|---|---:|---|---|---|---:|---|
| `0x3C` | 14 | `bgm_play` | `bgm_id: expr, loop_flag: expr, fade_ms: expr` | `0x424d30` | 91 | 记录 `DAT_00471b8c = bgm_id`，调用 `0x448d90` 交叉淡入播放 BGM |
| `0x3D` | 10 | `bgm_stop` | `unused: expr, fade_ms: expr` | `0x424d80` | 30 | 清零 `DAT_00471b8c = 0`，调用 `0x448f40` 淡出停止 BGM |
| `0x3E` | 6 | `se_play_once` | `se_id: expr` | `0x424b90` | 12 | 记录 `DAT_00471b90 = se_id`，停止旧 SE 并单次播放 `wav\%04d.wav` |
| `0x3F` | 14 | `se_play` | `se_id: expr, loop_cnt: expr, vol_50: expr` | `0x424c00` | 120 | 播放 SE 槽 0：`999` 无限循环 (`-1`)、`0` 单次、其他 `n-1` 次；音量/参数 `vol_50 * 50` |
| `0x40` | 6 | `se_stop` | `fade_ms: expr` | `0x424c90` | 32 | 淡出停止 SE 槽 0 |
| `0x41` | 6 | `mpg_play` | `mpg_id: expr` | `0x424dc0` | 1 | 格式化 `.\mov\%04d.mpg`，调用 `0x449060` 拉伸 `800×600` 播放并阻塞至 `WM_USER+0x1E` |
| `0x42` | 18 | `voice_play` | `voice_id: expr, loop: expr, p3: expr, delay_50ms: expr` | `0x424cc0` | 10 | 独立在槽 1 播放语音 `voice\[sub\]%04d.wav`（`id / 10000` 子目录，延时 `delay_50ms * 50`ms） |
| `0x43` | 6 | `voice_stop` | `fade_ms: expr` | `0x424d10` | 2 | 停止槽 1 语音 |
| `0x44` | 2 | `se_wait` | *(无)* | `0x424e70` | 1 | 阻塞等待 SE 槽 0 播放结束 |
| `0x45` | 2 | `voice_wait` | *(无)* | `0x424eb0` | 1 | 阻塞等待语音槽 1 播放结束 |

### 4.6 数值、数组、全局标志、UI 计量条与字符串变量（23 条）

| Opcode | 总字节 | 助记名 | 操作数签名 | 处理函数 | 341脚本频次 | 精确语义与副作用 |
|---|---:|---|---|---|---:|---|
| `0x12` | 10 | `load_array` | `var_idx: expr, arr_idx: u32` | `0x424900` | 20 | 调用 `0x417ab0` 将 `.gsc` 数组节区第 `arr_idx` 项复制到 `vars[var_idx..]` |
| `0x6E` | 14 | `get_spr_pos` | `layer: expr, out_x_var: expr, out_y_var: expr` | `0x424980` | 0 | 读取图层坐标除以缩放因子写入 `vars[out_x_var]` 与 `vars[out_y_var]` |
| `0x6F` | 14 | `muldiv` | `a: expr, b: expr, c: expr` | `0x4249e0` | 0 | `vars[0] = ((a as i32 * b as i32) / c as i32) as i16` |
| `0x70` | 6 | `ftol_noop` | `val: expr` | `0x424a00` | 0 | 调用 `__ftol()` 截断写入 `vars[0]`（未传入浮点寄存器，属编译器废指令，全作 0 次） |
| `0x71` | 10 | `pow_i16` | `base: expr, exp: expr` | `0x424a30` | 0 | `vars[0] = base^exp`（`exp <= 1` 时直接返回 `base`） |
| `0x72` | 10 | `global_flag` | `flag_id: expr, is_set: expr` | `0x424a60` | 506 | 若 `is_set != 0` 置位全局标志 `flag_id` (`0x41e230`)；否则读取至 `vars[0]` (`0` 或 `1`) |
| `0x82` | 18 | `gauge_set_parts` | `idx: expr, num_id: expr, bar_id: expr, fix_id: expr` | `0x423bc0` | 5 | 配置 `DAT_00471040[idx]` UI 控件数字/进度条/底图部件（`0x40f300`） |
| `0x83` | 22 | `gauge_set_range` | `idx: expr, min: expr, max: expr, p4: expr, p5: expr` | `0x423c20` | 2 | 设置计量条范围（`0x40f630`） |
| `0x84` | 10 | `gauge_visible` | `idx: expr, vis: expr` | `0x423c70` | 11 | 设置计量条可见性（`0x40f480`） |
| `0x86` | 14 | `gauge_pos` | `idx: expr, x: expr, y: expr` | `0x423cb0` | 2 | 设置计量条整体坐标 `(x, y)`（`0x40f4c0`） |
| `0x87` | 22 | `gauge_part_pos` | `idx: expr, nx: expr, ny: expr, bx: expr, by: expr` | `0x423cf0` | 2 | 设置数字与条子部件坐标（`0x40f510`） |
| `0x88` | 14 | `gauge_value` | `idx: expr, val: expr, anim: expr` | `0x423d40` | 4 | 设置计量条当前值并更新（`0x40f5b0`） |
| `0x96` | 10 | `str_assign_lit` | `str_var: expr, str_idx: u32` | `0x423da0` | 0 | `str_vars[str_var] = str_table[str_idx]` |
| `0x97` | 10 | `str_concat_lit` | `str_var: expr, str_idx: u32` | `0x423dd0` | 0 | `str_vars[str_var] += str_table[str_idx]` |
| `0x98` | 10 | `str_from_int_fw` | `str_var: expr, int_var: expr` | `0x423e00` | 0 | 将 `vars[int_var]` 转为全角 Shift_JIS 数字串（`0x44b680`）存入 `str_vars[str_var]` |
| `0x99` | 10 | `str_to_int` | `out_var: expr, str_var: expr` | `0x423e40` | 0 | 解析 `str_vars[str_var]` 为整数（`0x44b5d0`）存入 `vars[out_var]` |
| `0x9A` | 10 | `str_copy` | `dst: expr, src: expr` | `0x423e80` | 0 | `str_vars[dst] = str_vars[src]` |
| `0x9B` | 10 | `str_concat` | `dst: expr, src: expr` | `0x423ec0` | 0 | `str_vars[dst] += str_vars[src]` |
| `0x9C` | 14 | `richedit_open` | `mode: expr, u2: expr, u3: expr` | `0x423f80` | **0** | 投递 `WM_USER+0x32 (0x432)` 打开 RichEdit 输入界面并阻塞等待（**全作 0 次**） |
| `0x9D` | 22 | `richedit_cfg` | `idx: expr, val: expr, u3..u5: [expr; 3]` | `0x423fb0` | **0** | 写 `DAT_00477262[idx * 4] = val`（**全作 0 次**） |
| `0x9E` | 10 | `str_save_global` | `str_var: expr, slot: expr` | `0x423f00` | 0 | 复制 `str_vars[str_var]` → 全局槽 `slot` |
| `0x9F` | 10 | `str_load_global` | `str_var: expr, slot: expr` | `0x423f40` | 0 | 复制全局槽 `slot` → `str_vars[str_var]` |
| `0xCA` | 14 | `memset_vars` | `start: expr, end: expr, val: expr` | `0x424930` | 2 | 批量赋值 `vars[start ..= min(end, 9999)] = val` |

> 上表共列出全部 **120** 条命令 opcode（4.1 节 13 条 + 4.2 节 10 条 + 4.3 节 27 条 + 4.4 节 37 条 + 4.5 节 10 条 + 4.6 节 23 条 = **120** 条，其中实测脚本共使用 **72** 种、未使用 **48** 种）。

---

## 5. 原版 vs 汉化脚本差异与 `RICHED32` 实证结论（UNK-06）

### 5.1 原版 vs 汉化 V1.0 全量 341 `.gsc` 差异盘点

通过 [`re/verify_gsc.py`](../re/verify_gsc.py) 对 `补丁/原版备份/scr.xfl`（ORIG）与根目录 `scr.xfl`（CHS）逐字节比对：

1. **字节码与跳转偏移 100% 完全一致（`0` 差异）**：全部 341 个 `.gsc` 的 `code_size`（总计 `1,125,986` 字节）、`68,819` 条指令、跳转目标与数组数据区在原版与汉化版中完全相同。
2. **文件级差异分类（`85 + 243 + 9 + 4 = 341`）**：
   - **`85` 个完全字节一致**（系统逻辑/无文本脚本）。
   - **`243` 个仅字符串表变化**（CP932 → GBK/CP936）。
   - **`9` 个场景回想脚本被汉化工具清零标签表**（`1125.gsc`, `2058.gsc`, `2059.gsc`, `2245.gsc`, `3177.gsc`, `3179.gsc`, `3262.gsc`, `4203.gsc`, `5110.gsc`）：原版中这些脚本带有供 `0399.gsc`（场景回想模式）`0x0f call_script` 跳转的标签名 `"8011"`..`"8031"`（总计 10 个标签，加上 `0101.gsc` 的 `"omake"` 共 11 个标签）。汉化重打包工具保留了 `hdr[7]/hdr[8]` 大小却把这 9 个文件尾部标签数据全清为 `0x00`（`0101.gsc` 的 `"omake"` 未被清零）。Rust VM 在汉化模式下需注意：**若按字面加载汉化尾部，在场景回想跳转这 9 个脚本会因找不到标签落回 `pc = 0`（同 PC 汉化版行为）；由于字节码 100% 一致，也可视配置决定忠实复现原版或修复回想标签映射**。
   - **`4` 个汉化工具重打包尾部字节异常**（均不涉及字节码与字符串正文）：
     - `1122.gsc`（+2 字节 `0x00` 尾随填充）、`2233.gsc`（+6 字节 `0x00`）、`3202.gsc`（+1 字节 `0x00`）：XFL 条目长度比 `file_size` 多出 NUL 填充字节，`0x417c70` 只按头部的 `file_size`/各节区长度读取，天然忽略多余尾字节。
     - `1160.gsc`（文件长 `188B`，`calc_size = 192B`）：汉化重打包将该文件字符串 `1`（`"^cw８月１４日・木曜日"`）置为空串后，尾部无用占位区仅写了 `11` 个 `0x00`（原需 `15` 个 `0x00`），且该文件无任何 `0x12` 数组或标签访问。Rust `.gsc` 解析器在解析 `arr/lbl` 尾部时**须允许剩余字节不足且全零时宽容截断填充为 0**。

### 5.2 `RICHED32` / `WM_USER+0x32` 交叉引用实证

通过 Ghidra 交叉引用与 `.text` 字节搜索实证完整调用链如下：
- 字符串 `"RICHED32.DLL"`（`0x46d154`）仅被引用 1 处：`0x403236`（位于 `FUN_00403210`）。
- 字符串 `"RichEdit"`（`0x46d120`）仅被引用 1 处：`0x40363c`（位于 `FUN_00403210`）。
- `FUN_00403210` 仅被调用 1 处：`0x41b025`（位于 `FUN_0041b010`）。
- `FUN_0041b010` 仅由主窗口过程 `FUN_0041a2a0` 在消息 `param_2 == 0x432`（`WM_USER + 0x32`）分支调用。
- 在 `.text`（`0x401000..0x462fff`，401,408 字节）全段搜索 `32 04 00 00`（`0x432`），**唯一命中点为 `0x423f93`**，即 `FUN_00423f80`（**opcode `0x9c`**）内 `PostMessageA(hwnd, 0x432, wParam, 0)`；无任何 UI 菜单、快捷键或场景初始化路径触发 `0x432`。
- 全库 341 个原版与 341 个汉化 `.gsc` 反汇编实测：**`0x9c` 出现 `0` 次，`0x9d` 出现 `0` 次**（其余字符串变量操作 `0x96..0x9b, 0x9e, 0x9f` 同样全部为 `0` 次）。
- **结论**：`RICHED32` 文本输入子系统为 RScript 通用引擎遗留代码，在《腐姫》全作中完全不可达，**可 100% 安全排除于 iOS 14.4 移植范围之外**（Rust VM 对 `0x9c`/`0x9d` 保留反汇编识别即可，运行时无需挂载任何 RichEdit UI）。

---

## 6. 全量自动化验证结果（341 + 341 `.gsc`）

验证脚本：[`re/verify_gsc.py`](../re/verify_gsc.py)（容器 `fujihime-re` 内执行，退出码 `0`），结构化报告：[`re/evidence/phase1b-vm-opcodes.json`](../re/evidence/phase1b-vm-opcodes.json)。

| 语料库 | 文件数 | 通过 / 失败 | 字节码总字节 | 总指令数 | 跳转总数（含 EOF） | 字符串总数 / 引用数 | 编码校验 | 未知 Opcode / 截断 / 越界 |
|---|---:|---:|---:|---:|---:|---:|---|---|
| **原版日文** (`补丁/原版备份/scr.xfl`) | 341 | **341 / 0** | 1,125,986 | 68,819 | 1,607 (20 EOF) | 19,161 / 37,878 | CP932 100% PASS | **0** |
| **汉化 V1.0** (`scr.xfl`) | 341 | **341 / 0** | 1,125,986 | 68,819 | 1,607 (20 EOF) | 19,161 / 37,878 | GBK 100% PASS | **0** |

---

## 7. Phase 1B 未知项状态汇总

- **UNK-02**（`.gsc` 格式与 120 条 VM Opcode）：**RESOLVED（已 100% 实证闭环）**。
- **UNK-06**（341+341 全量反汇编验收与 `RICHED32` 核查）：**RESOLVED（已 100% 实证闭环）**。
