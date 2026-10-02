[English](README.md) | **简体中文**

# mobile-dev-harness

**让编程 agent 能运行、检查和调试你的 Android App，就像它们在浏览器里检查 Web 应用那样。**

agent 改完一个 Web 应用，可以打开浏览器、点一点、看看控制台，确认改动是否生效。改完一个移动 App，它通常做不到：
只能改完代码，然后祈祷没问题。`mobile-dev-harness`（命令 `mdh`）让 agent 能看到、也能操作真实设备或模拟器，
而且是按 agent 的工作方式设计的：

- **紧凑的界面描述**：当前屏幕被描述成一棵简短的元素树（约 150 token，原始 XML 动辄几千），每个元素都有一个
  像 `e12` 这样的编号，在整个会话里保持不变。
- **操作之后只看变化**：每个动作都会等界面稳定下来，然后只报告发生了什么变化。
- **崩溃第一时间暴露**：新的错误日志、崩溃、native 崩溃和 ANR 会随每次结果一起返回，并附上 App 自己的堆栈和
  导致崩溃的操作步骤。
- **快**：常驻在设备上的 helper 读取界面只要约 10ms（uiautomator 要约 2 秒），还能输入任意语言的文字。
- **一套引擎，两种接口**：给人、脚本和只有命令行的 agent 用的 CLI，以及给 Claude Code 等 agent 用的
  MCP server。

> **状态：早期开发阶段。** 目前只支持 Android，已经可以端到端地驱动真实 App。详见[状态与路线图](#状态与路线图)。

## 效果

```
$ mdh observe
screen dev.mdh.sample/.LoginActivity  1344x2992
[e69] textbox "Email" empty #email
[e70] textbox "Password" empty #password
[e71] checkbox "Remember me" unchecked #remember
[e72] button "SIGN IN" disabled #sign_in

$ mdh type "alice@example.com" --into Email
$ mdh type "correct-horse" --into Password
type •••• into e70 textbox "Password" → ok (783 ms)
screen dev.mdh.sample/.LoginActivity  1344x2992  keyboard
~ [e70] textbox "Password": value empty → ••••
~ [e72] button "SIGN IN": disabled → enabled
```

切换一个开关，连带的副作用也会一起显示：

```
$ mdh tap "Airplane mode"
tap e20 switch "Airplane mode" → ok (923 ms)
screen com.android.settings/.SubSettings  1344x2992
~ [e18] item "Internet": detail "AndroidWifi" → "Airplane mode is on"
~ [e19] item "SIMs": enabled → disabled
~ [e20] switch "Airplane mode": off → on
```

App 崩溃时，agent 立刻就能知道（退出码 5）：

```
$ mdh tap "Crash (Java)"
tap e114 button "CRASH (JAVA)" → ok (979 ms)
screen dev.mdh.sample/.MainActivity  1344x2992  overlay:android
!! CRASH dev.mdh.sample (pid 20272): java.lang.IllegalStateException: Sample crash: could not pay for the cart
     at dev.mdh.sample.Checkout.pay(TroublesActivity.kt:61)
     at dev.mdh.sample.TroublesActivity.onCreate$lambda$0(TroublesActivity.kt:21)
     … 12 more frames
   caused by: java.lang.IllegalArgumentException: cart id must not be blank
   after: key BACK → tap e7 button "TROUBLES" → tap e114 button "CRASH (JAVA)"
[e126] "mdh sample keeps stopping"
[e127] button "App info"
[e128] button "Close app"
```

## 系统架构

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/architecture-dark.svg">
  <img alt="系统架构：编程 agent 通过 mdh mcp 接入，人、CI 和脚本使用 mdh CLI，两者驱动同一套引擎。引擎按五个质量领域组织（控制已完成，校验、UI 一致性、性能和兼容性在计划中），建立在共享的基础层（observe、project、driver、core）之上。在 Android 设备上，mdh helper 保持一个常驻的 UiAutomation 连接，通过 adb forward 与 driver 通信，约 10 毫秒；崩溃、ANR 和错误从 logcat 流入 observe。" src="docs/assets/architecture-light.svg">
</picture>

- **两个入口，一套引擎**：agent 通过 MCP 接入，人、CI 和脚本使用 CLI，拿到的是同样的紧凑文本。
- **五个质量领域，共享一套基础层**：控制已经完成，其余领域都建立在它之上（见[状态与路线图](#状态与路线图)）。
- **设备上的常驻 helper**：`dev.mdh.helper` 保持一个无障碍连接，所以读取屏幕和注入输入都只要几毫秒；logcat 则把
  崩溃和错误信息反馈到每一次的结果里。

这张图是用代码生成的（[`scripts/diagram`](scripts/diagram)），采用 Excalidraw 的风格。

## 为什么用 Rust：验证才是瓶颈

以现在大模型的能力，拖慢 AI 编程任务的往往不是写代码，而是验证。agent 要反复地修改、构建、运行、查看、修复；
一个任务可能要经历几十轮验证，而每一轮都要等 harness 执行完，再等模型读完它返回的结果。所以负责验证的这一环必须足够快，
体现在三个方面：

1. **每次调用花的时间少**：`mdh` 启动只要约 5 毫秒。作为对比，在同一台机器上，一个什么都不加载的空 Python 或 Node
   进程启动就要 30 到 40 毫秒。设备相关的 I/O 也是并发执行的：一次观测会同时读取 UI 树、前台 Activity 和新日志。
   当 agent 通过 CLI 操作时，每一步都是一个新进程，启动开销每次都要付出。
2. **模型读结果花的时间少**：每屏约 150 token，动作之后只返回变化的部分。模型读取的时间和成本都随 token 数增长。
3. **没有额外负担**：单个静态二进制文件（约 6.5MB），设备端 helper 也内置其中。不需要任何运行时，不需要安装依赖，
   在 CI 里和在笔记本上表现一致。

| 在 API 36 模拟器上实测 | 耗时 |
|---|---|
| 用 `uiautomator dump` 读取 UI | 约 2,000 毫秒 |
| 用 `mdh` helper 读取 UI | 约 10 毫秒 |
| 启动 `mdh` 进程 | 约 5 毫秒 |
| `mdh observe` 端到端（UI 树、Activity、日志） | 约 100 毫秒 |
| 执行一个动作并等到界面稳定 | 0.8 到 1.4 秒，主要是 App 自身的动画 |

公平地说：最大的提速来自架构设计，比如用常驻的 helper 代替 `uiautomator`，用 diff 代替整屏内容。Rust 的作用是保证
harness 本身不再额外增加开销，并且在后续计划中的性能、兼容性和视觉检查让每次调用承担更多工作时，依然保持这一点。

## 环境要求

- macOS 或 Linux（Windows 尚未测试）
- [Android SDK platform-tools](https://developer.android.com/tools/releases/platform-tools)（`adb`）；`mdh` 会通过
  `ANDROID_HOME`、`ANDROID_SDK_ROOT` 或 Android Studio 的默认位置找到 SDK
- 一台模拟器，或打开了 USB 调试的真机，Android 8.0（API 26）及以上
- [Rust](https://rustup.rs) 1.85 及以上，用于从源码安装（预编译版本在计划中）

## 安装

```sh
cargo install --git https://github.com/qkmaosjtu/mobile-dev-harness mobile-dev-harness
mdh doctor
```

`mdh doctor` 会检查 SDK、adb、模拟器、JDK 和已连接的设备，并告诉你缺了什么、怎么解决：

```
✓ android-sdk  /Users/you/Library/Android/sdk
✓ adb          Android Debug Bridge version 1.0.41 (/Users/you/Library/Android/sdk/platform-tools/adb)
✓ emulator     AVDs: Pixel_9_Pro_XL
✓ java         openjdk version "17.0.16" 2025-07-15
✓ devices      1 connected
```

第一次使用时，`mdh` 会在设备上安装一个很小的 helper App（`dev.mdh.helper`，约 11KB，内置在 `mdh` 里）。

## 快速上手

```sh
mdh launch com.android.settings      # 启动 App；从此开始关注它的日志和崩溃
mdh observe                          # 看当前屏幕
mdh tap "Network & internet"         # 按标签点击……
mdh tap e20                          # ……或按编号点击
mdh scroll down --until "System"
mdh key back
mdh logs                             # 最近的警告、错误和崩溃报告
```

想试遍所有功能，可以用[示例 App](#示例-app)。

## 在 agent 中使用（MCP）

`mdh mcp` 通过 stdio 以 [MCP](https://modelcontextprotocol.io) 协议提供同一套能力。

**Claude Code：**

```sh
claude mcp add mdh -- mdh mcp
```

**其他 MCP 客户端：**

```json
{ "mcpServers": { "mdh": { "command": "mdh", "args": ["mcp"] } } }
```

| 工具 | 作用 |
|---|---|
| `mdh_observe` | 当前屏幕；可选只看变化，或附带截图 |
| `mdh_act` | 一个或多个动作（`tap`、`long_press`、`type`、`swipe`、`scroll`、`key`），每个都报告发生了什么变化 |
| `mdh_wait` | 等待某个元素出现或消失 |
| `mdh_logs` | 最近的日志和崩溃报告 |
| `mdh_app` | 启动、停止或安装 App |
| `mdh_status` | 设备和会话状态；切换设备或重置会话 |

然后就可以对 agent 说：*"打开示例 App，用 alice@example.com 登录，检查消息列表能不能正常显示。"* 返回结果和
CLI 打印的紧凑文本一样；App 崩溃时会以错误的形式返回，并附上崩溃报告。

只能使用命令行的 agent 可以直接调用 CLI，所有命令都支持 `--json`。

## 命令

| 命令 | 说明 |
|---|---|
| `mdh doctor` | 检查工具链和设备 |
| `mdh devices` | 列出已连接的设备和模拟器 |
| `mdh observe [--diff]` | 当前屏幕；`--diff` 只显示自上次查看以来的变化 |
| `mdh screenshot [-o FILE] [--max-edge 1024]` | 保存一张缩小后的 JPEG 截图 |
| `mdh tap TARGET` | 点击元素 |
| `mdh long-press TARGET [--duration-ms 800]` | 长按元素 |
| `mdh type TEXT [--into TARGET] [--append] [--enter]` | 设置当前输入框的文字（任何语言） |
| `mdh swipe X1 Y1 X2 Y2 [--duration-ms 300]` | 在两个坐标之间滑动 |
| `mdh scroll up\|down\|left\|right [--in TARGET] [--until TARGET]` | 滚动，可以一直滚到某个元素出现为止 |
| `mdh key NAME` | 按键：`back`、`home`、`enter` 等 |
| `mdh wait TARGET [--gone] [--timeout 10]` | 等待元素出现（或消失） |
| `mdh logs [--level warn] [--lines 50]` | App 最近的日志和崩溃报告 |
| `mdh launch APP` · `mdh stop PACKAGE` · `mdh install APK [-g]` | 启动、停止、安装 App |
| `mdh session show` · `mdh session reset` | 查看或重置会话 |
| `mdh mcp` | 以 MCP 协议提供工具 |

全局参数：`--device SERIAL`（连接了多台设备时使用）和 `--json`。

### 目标的写法

凡是需要 `TARGET` 的地方，都可以这样写：

| 写法 | 示例 | 说明 |
|---|---|---|
| 编号 | `e12` | 来自最近一次观测；在整个会话里保持不变 |
| 标签 | `"Sign in"` | 先找完全一致的，再忽略大小写，最后找包含它的 |
| 选择器 | `id=login`、`text=Sign in`、`text~=sign`、`role=switch` | 用 `;` 组合：`role=switch;text=Wi-Fi` |
| 坐标 | `540,1200` | 最后的手段 |

找不到目标时，错误信息会列出最相近的几个元素；如果某个编号已经不在屏幕上，会告诉你它原来是什么元素、是在哪个页面看到的。

### 输出格式和退出码

文本输出是给 agent 和人看的。加上 `--json` 后，所有命令都输出同一种格式：

```json
{ "schema": "mdh/v1", "ok": false, "data": null,
  "error": { "code": "ELEMENT_NOT_FOUND", "message": "…", "hint": "closest matches: …" },
  "warnings": [], "timing_ms": { "total": 75 } }
```

| 退出码 | 含义 |
|---|---|
| 0 | 成功 |
| 1 | App 的状态不符合预期：找不到元素、匹配到多个、元素被遮挡，或等待超时 |
| 2 | 参数或目标写法有误 |
| 3 | 环境问题：没有 SDK、没有设备、helper 无法使用 |
| 5 | App 崩溃或失去响应（ANR） |
| 10 | 内部错误 |

## 工作原理

- **会话**：连续的命令共享同一个会话（保存在当前目录的 `.mdh/session.json`；MCP server 为每个连接维护一个会话）。
  这正是编号能保持不变、结果能只显示变化的原因。用 `mdh session reset` 可以重新开始。
- **设备端 helper**：`dev.mdh.helper` 会保持一个无障碍连接，所以读取屏幕只要几毫秒而不是几秒；它还能输入
  `adb shell input` 做不到的 Unicode 文字。
- **等待**：每次操作后，`mdh` 会一直等到界面不再变化，包括加载圈和加载中的状态；App 失去响应时也能察觉。
- **日志**：`mdh` 增量读取 logcat，并按进程把日志归到你的 App 上，所以你只会看到新出现的问题。

需要注意：

- **每台设备同时只能有一个无障碍客户端。** 使用 `mdh` 期间，同一台设备上其他依赖 UiAutomation 的工具
  （uiautomator、Appium、Maestro、mobile-mcp）无法工作，反之亦然。运行 `mdh session reset` 可以停止 helper。
- **没有任何遥测。** 所有数据都留在你的电脑上。

## 示例 App

[`examples/android-sample`](examples/android-sample) 是一个专门用来体验所有功能的小 App：View 和 Compose 页面、
登录表单、100 条的长列表、联动的开关、WebView、深链、运行时权限、一个故意留下的 edge-to-edge 布局 bug，
还有会崩溃、会 native 崩溃、会卡死（ANR）、加载很慢和会打印错误日志的按钮。

```sh
cd examples/android-sample && ./gradlew assembleDebug
mdh install build/outputs/apk/debug/mdh-sample-debug.apk
mdh launch dev.mdh.sample
```

测试账号：`alice@example.com` / `correct-horse`。

## 状态与路线图

目前已经可用（Android）：观测屏幕、操作界面、等待、日志和崩溃报告、CLI 以及 MCP server。后续计划围绕五个质量领域展开：

| | 领域 | 计划内容 |
|---|---|---|
| ✅ | **控制** | 可靠地驱动 App（已完成） |
| ⏳ | **构建** | 从源码构建，编译错误清晰易读；一条命令完成构建、安装和启动 |
| ⏳ | **校验** | 断言、带证据的验证结论、录制的操作流程可作为回归测试回放、Claude Code 插件 |
| ⏳ | **UI 一致性** | 与基线对比、与设计稿对比、跨配置的布局检查、无障碍规则 |
| ⏳ | **性能** | 对照基线检查启动耗时、卡顿、内存和 CPU |
| ⏳ | **兼容性** | 在不同 Android 版本、屏幕尺寸、系统配置和厂商设备上运行同样的检查 |
| ⏳ | **更多平台** | React Native、Expo、Flutter，然后是 iOS |

详细设计（英文）：[设计总览](docs/DESIGN.md)、[功能设计](docs/design/01-functional.md)、
[技术架构](docs/design/02-architecture.md)、[架构决策记录](docs/adr/)。

## 参与贡献

非常欢迎提 issue 和参与讨论，尤其是 `mdh` 描述得不好的界面：附上 `mdh observe` 的输出和一张截图会非常有帮助。
详见 [CONTRIBUTING.md](CONTRIBUTING.md)；编程 agent 还应该阅读 [AGENTS.md](AGENTS.md)。

## 许可证

本项目采用 [Apache License 2.0](LICENSE-APACHE) 或 [MIT license](LICENSE-MIT) 双许可，你可以任选其一。

除非你另有明确声明，你有意提交并被纳入本项目的任何贡献（按 Apache-2.0 许可证的定义）都将按上述方式双重许可，
不附加任何其他条款或条件。
