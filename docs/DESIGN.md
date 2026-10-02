# Mobile Coding Harness — 设计文档（草案 v0.1）

> 目标：让 coding agent 在移动端开发中拥有和 Web 端同等水平的「改代码 → 跑起来 → 验证」闭环。
> 项目名：`mobile-dev-harness`，CLI 命令 `mdh`。License：MIT OR Apache-2.0。

## 1. 定位

- **不是**又一个设备遥控 MCP（mobile-mcp 已经做了）。
- **是**一套面向 agent 的「验证 harness」：构建、状态准备、观测、断言、回放、成本控制，全链路打通。
- 核心与 agent 无关（CLI + MCP），Claude Code plugin 是第一个也是体验最好的集成。

## 2. 首版范围决策

| 项 | 决策 | 理由 |
|---|---|---|
| 平台 | Android 先行 | adb 能力完整，Linux CI 可跑，实现最快 |
| 项目类型 | 原生 Gradle | 最底层，RN/Flutter 适配后续建在其上 |
| Driver | 自研，封装 adb CLI | 掌控 UI 树压缩、截图策略等核心体验 |
| 语言 | Rust | 单二进制分发、启动快、并发好；MCP 用官方 `rmcp` SDK |

## 3. 要解决的 7 个问题 → 模块映射

| # | 问题 | 模块 | 关键手段 |
|---|---|---|---|
| 1 | 构建/安装 | `mdh-build` | 识别 Gradle 工程、application 模块、variant；增量构建；**压缩 Gradle 报错**为 agent 可读摘要 |
| 2 | 验证流程 | `mdh-verify` + plugin skill | 结构化 verdict（pass/fail + 证据），skill 规定「没有 pass 的 verdict 不算完成」 |
| 3 | 状态准备 | `mdh-state` | `pm grant` / `install -g`、`pm clear`、深链 `am start -d`、关闭动画、测试数据注入 |
| 4 | 日志/崩溃 | `mdh-observe` | `logcat --pid` 按进程过滤、`-b crash`、ANR/tombstone 检测，崩溃自动附堆栈 |
| 5 | 可重复回归 | `mdh-verify` | 交互过程自动录制为 YAML flow，可回放、可进 CI |
| 6 | 成本/速度 | `mdh-ui` | 默认返回**压缩 UI 树**（只保留可交互/有文本节点，稳定引用 `e12`）；截图按需、降采样、差异检测 |
| 7 | 框架差异 | `mdh-build` 适配器 | 首版 Native Gradle；预留 RN / Flutter / Expo / iOS 适配点 |

## 4. 架构

```
crates/
  mdh-core      公共类型：Device, App, UiNode, Observation, Verdict；配置；错误
  mdh-driver    trait Driver + android(adb) 实现；后续 ios(simctl+AXe)
  mdh-ui        uiautomator dump → 压缩树；截图/降采样/diff
  mdh-observe   logcat 过滤、崩溃/ANR 检测
  mdh-build     Gradle 适配：探测、构建、安装、报错摘要
  mdh-state     权限、深链、动画、清数据、fixtures
  mdh-verify    断言、flow 录制/回放、verdict
  mdh-mcp       MCP server（rmcp），暴露上述能力为 tools
  mdh-cli       `mdh` 二进制（clap）；`mdh mcp` 启动 MCP server
integrations/
  claude-code/ plugin：.mcp.json、skills/verify、hooks
examples/
  android-sample/  示例 App，同时作为集成测试与 benchmark 目标
```

依赖方向：`cli/mcp → verify/build/state → ui/observe → driver → core`。

### 4.1 Driver trait（草图）

```rust
#[async_trait]
pub trait Driver {
    async fn devices(&self) -> Result<Vec<Device>>;
    async fn install(&self, dev: &Device, artifact: &Path, grant_all: bool) -> Result<()>;
    async fn launch(&self, dev: &Device, app: &AppId, deeplink: Option<&str>) -> Result<()>;
    async fn ui_tree(&self, dev: &Device) -> Result<RawUiTree>;
    async fn screenshot(&self, dev: &Device) -> Result<Image>;
    async fn input(&self, dev: &Device, action: InputAction) -> Result<()>; // tap/type/swipe/key
    async fn logs(&self, dev: &Device, filter: LogFilter) -> Result<LogStream>;
}
```

首版用 `tokio::process` 调 adb CLI，简单可靠；性能瓶颈出现后再考虑 adb wire protocol 或设备端 helper。

### 4.2 已知技术风险

- **`uiautomator dump` 慢（1–3s）且在动画/非 idle 时失败**。首版：关闭动画 + 重试；中期：设备端 helper APK（instrumentation server，类似 Appium UiAutomator2 / Maestro 的做法）。
- **Compose / WebView / Canvas** 的无障碍信息可能不全 → 需要截图兜底，Budget 策略要能检测「树信息不足」。
- **Gradle 构建慢** → 优先 `installDebug` 增量；daemon 保活；配置缓存提示。

## 5. 项目配置 `mdh.yaml`

由 `mdh init` 自动探测生成，agent 与人都可编辑：

```yaml
android:
  project: .
  module: app
  variant: debug
  applicationId: com.example.app
  device:
    avd: Pixel_9_Pro_XL      # 或 serial: emulator-5554
state:
  disableAnimations: true
  permissions: [android.permission.POST_NOTIFICATIONS]
  deeplinks:
    settings: example://settings
flows: .mdh/flows/
```

## 6. Agent 侧体验（Claude Code）

- `mdh run`：一键 构建 → 安装 → 启动 → 返回首屏观测（压缩树 + 日志摘要）。
- `mdh verify <flow|断言>`：返回结构化 verdict，失败时附截图路径与相关日志。
- plugin skill `/verify`：规定验证流程；hook：会话结束前若有未验证改动则提醒。

## 7. 路线图

| 里程碑 | 内容 | 完成标志 |
|---|---|---|
| M0 脚手架 ✅ | workspace、CI（fmt/clippy/test）、License、CONTRIBUTING、`mdh doctor`、`mdh devices` | CI 绿 |
| M1 Driver + 观测 | 设备列表/启动模拟器、安装启动、压缩 UI 树、截图、输入、logcat、崩溃检测；CLI + MCP | Android 上可替代 mobile-mcp 基本操作 |
| M2 构建 | Gradle 探测、增量构建安装、报错摘要、`mdh run` | 示例 App 一条命令跑起来 |
| M3 状态 + 配置 | `mdh init`、权限、深链、动画、清数据 | 进入指定页面无需手动点击 |
| M4 验证 | 断言、flow 录制/回放、verdict；Claude Code plugin | agent 能产出带证据的 pass/fail |
| M5 成本/速度 | 截图策略、diff、设备端 helper；benchmark | 公布 token/耗时/成功率数据 |
| 之后 | iOS（simctl + AXe）、RN、Flutter、Expo | — |

## 8. 测试策略

- **单元测试**：用录制好的 uiautomator XML、logcat、Gradle 输出作为 fixture，不依赖设备。
- **集成测试**：`MDH_E2E=1` 时针对模拟器运行；CI 用 Linux + Android emulator。
- **Benchmark**：`examples/android-sample` 上的一组标准任务，衡量成功率、token、耗时，作为开源项目的可信度来源。

## 9. 待定

- flow 格式：自定义 YAML 还是兼容 Maestro 格式
