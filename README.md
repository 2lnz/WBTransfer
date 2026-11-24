<p align="center">
  <img src="assets/logo.png" width="112" alt="WBTransfer logo">
</p>

<h1 align="center">WBTransfer</h1>

<p align="center">
  把单账号 JSON 整理成 <code>wb-switch-accounts</code> 备份格式的 Windows 桌面工具
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: Apache-2.0" src="assets/badge-license.svg"></a>
  <img alt="Platform: Windows 10/11" src="assets/badge-platform.svg">
  <img alt="Rust: 1.75+" src="assets/badge-rust.svg">
  <img alt="Dependencies: 3" src="assets/badge-dependencies.svg">
</p>

---

## 这是什么

某些工具在切换账号时，使用一种「数组 + 双层 `auth_raw`」的备份格式保存登录凭据；而导出单个账号时得到的却是扁平对象。两者字段名、层级、时间戳精度都不一样，手工改容易出错。

**WBTransfer** 就是一个只做这件事的小工具：读入扁平的单账号 JSON，输出标准的 `wb-switch-accounts` 备份数组，字段顺序、嵌套层级、时间戳单位都与官方格式对齐。

## 功能

- **格式转换** —— 单对象 → 数组包装 + 双层 `auth_raw` 嵌套
- **时间戳换算** —— 从 JWT payload 的 `exp` 解码出毫秒级 `expiresAt` / `refreshExpiresAt`
- **拖放导入** —— 点「导入」弹出虚线框窗口，可直接把 `.json` 拖进去
- **同名不覆盖** —— 目标文件已存在时自动追加 `(1)`、`(2)`，绝不覆盖已有数据
- **深色界面** —— 原生 Win32 自绘，无第三方 UI 框架，无 WebView
- **单文件运行** —— 约 460 KB，静态链接，不依赖 .NET / VC 运行库
- **零后台行为** —— 纯本地文件读写，不联网、不写注册表、不留驻

## 界面

```
┌──────────────────────────────────────────────────────┐
│ WBTransfer                                      ─ □ ✕ │
│ WBTransfer                                            │
│ 单账号 JSON                                           │
│                                                       │
│ 源文件                                                │
│ [C:\path\to\account.json            ] [浏览…] [导入]   │
│ 输出到                                                │
│ [C:\path\to\wb-switch-accounts.json ] [另存为…]       │
│                                                       │
│ [ 开始转换 ]                                          │
│ 就绪                                                  │
│ 日志                                                  │
│ ┌───────────────────────────────────────────────┐     │
│ │ → 转换日志、exp 换算过程                       │     │
│ └───────────────────────────────────────────────┘     │
└──────────────────────────────────────────────────────┘
```

## 使用方法

1. 双击 `wbtransfer.exe`
2. 点「浏览…」选择文件，或点「导入」把 JSON **拖进弹出的虚线框**
3. 确认输出路径（默认是源文件同目录的 `wb-switch-accounts-<今天日期>.json`）
4. 点「开始转换」，状态栏变绿表示成功，日志区会打印 `exp` 换算过程

也可以直接把 `.json` 拖到主窗口任意位置。

## 转换规则

输入（单账号扁平对象）：

```json
{
  "id": "...", "uid": "...", "nickname": "...", "email": "...",
  "access_token": "...", "refresh_token": "...", "token_type": "Bearer",
  "expires_at": 1797773427, "status": "normal",
  "auth_raw": { "accessToken": "...", "expiresAt": 1797773427205, ... },
  "profile_raw": { ... }, "created_at": 1795351596, "last_used": 1795354312
}
```

输出（数组，单元素）：

| 输出字段 | 取值 |
|---|---|
| `id` / `uid` / `nickname` | 输入同名字段 |
| `email` | 固定 `null` |
| `variant` | 固定 `"cn"` |
| `enterpriseName` | 固定 `null` |
| `enterpriseId` | 固定 `""` |
| `access_token` / `refresh_token` / `token_type` | 输入同名字段 |
| `domain` | 固定 `"www.codebuddy.cn"` |
| `expiresAt` | **`access_token` 的 JWT `exp` × 1000** |
| `refreshExpiresAt` | **`refresh_token` 的 JWT `exp` × 1000** |
| `auth_raw.account` | `{ email, uid, nickname }` |
| `auth_raw.auth` | 输入的账号对象整体原样嵌套 |
| `profile_raw` | `{ email, uid, nickname }` |
| `createdAt` | 转换时刻的毫秒时间戳 |

> ⚠️ `expiresAt` 不是输入 `expires_at × 1000`。两者通常差十几秒，必须从 JWT payload 解码才能对齐。

## 从源码构建

**环境要求**：Windows 10/11，Rust 1.75 或更高版本。

```bash
cargo build --release
```

产物位于 `target/release/wbtransfer.exe`（MSVC 工具链）或 `target/x86_64-pc-windows-gnu/release/wbtransfer.exe`（GNU 工具链）。

也可以直接双击 `build.bat`。

**依赖**：只有三个 crate —— `serde_json`、`base64`、`windows-sys`，全部来自 crates.io。

## 项目结构

```
.
├─ src/main.rs        全部逻辑（转换 + Win32 界面）
├─ Cargo.toml
├─ build.bat          一键构建
├─ app.ico            窗口 / 任务栏图标（运行时从程序目录加载）
├─ embed_icon.py      辅助脚本：把任意 exe 的图标注入到产物，让资源管理器也显示图标
├─ assets/logo.png    Logo（即程序图标，取自 app.ico 的 256px 版本）
└─ LICENSE            Apache License 2.0
```

### 关于图标

程序启动时会读取**自身目录下的 `app.ico`** 作为窗口和任务栏图标；文件不存在则用系统默认图标。想换图标，直接替换 `app.ico` 即可。

若希望 **exe 文件本身** 在资源管理器里也显示这个图标（而不只是运行时的窗口图标），用附带脚本把它写进 PE 资源节：

```bash
python embed_icon.py app.ico target\release\wbtransfer.exe
```

`build.bat` 在编译后会自动执行这一步；机器上没有 Python 时会自动跳过，不影响程序功能。脚本也接受任意 exe 作为图标来源：

```bash
python embed_icon.py <带图标的exe> <目标exe>
```

## 技术说明

界面是纯 Win32 API 手写，没有使用 egui / Tauri / Electron：

- `BS_OWNERDRAW` + `WM_DRAWITEM` 自绘圆角按钮
- `WM_CTLCOLORSTATIC` / `WM_CTLCOLOREDIT` 统一深色配色
- `SetWindowTheme(h, "DarkMode_Explorer", NULL)` 让滚动条跟随暗色主题
- `DwmSetWindowAttribute(20, 1)` 让标题栏走深色（`dwmapi.dll` 运行时动态加载）
- 通过 `GetDeviceCaps(LOGPIXELSX)/96` 做 DPI 缩放，125% / 150% 下不糊不缩
- 拖放窗口是独立窗口类，`DragAcceptFiles` + `WM_DROPFILES` 收文件，路径通过自定义消息回传主窗口

## 常见问题

**Q：转换后 `createdAt` 和原备份文件不一样？**
A：`createdAt` 记录的是「这次转换发生的时刻」，本来就该变化。其余字段与源数据严格对应。

**Q：能批量转换多个账号吗？**
A：当前版本一次处理一个文件。多个账号请分别转换后手动合并数组，或用脚本循环调用。

**Q：支持 macOS / Linux 吗？**
A：不支持。界面完全基于 Win32 API。

## 许可证

[Apache License 2.0](LICENSE)

```
Copyright 2026 The WBTransfer Authors

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0
```
