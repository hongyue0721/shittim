# REFERENCES.md

> 参考项目只用于借鉴或作为可替换 Provider。禁止复制其宿主假设成为 Companion Core 的不可替换依赖。

## 1. 分类总览

本文件中的外部项目按角色分类，避免把“能用的依赖”和“只看不抄的参考”混为一谈。

### direct dependency

可在实现中直接依赖或嵌入，但仍须通过 Companion 边界接入：

- Pi Agent（agent-runtime / 模型抽象）；
- Tauri（桌面客户端壳）；
- 平台语言运行时与本仓库选定的基础库（Rust/Tokio、SQLite+FTS5、TypeScript/Node 等，细节见实现契约，不在此展开版本钉死）。

### optional provider

可作为可替换 Provider / Adapter，默认不强制安装；其中 Computer Use 相关项目仅能作为未来 Optional Profile Provider/参考，不属于 Core 能力：

- A_Memorix（Memory Provider 候选）；
- OmniParser（未来 Computer Use Profile 的 Visual Grounding Provider）；
- computer-use-linux（未来 Linux Computer Use Profile Provider）；
- pi-computer-use（未来 Windows/macOS Computer Use Profile Provider 或架构参考实现）；
- Wasmtime/WASI（可选低权限扩展运行时）；
- Set-of-Mark 方法（未来 Computer Use Profile 的标注方法参考，通常是方法而非独立服务）。

### reference only

只借鉴思想、评测或产品拆分，不成为 Core 运行时：

- MaiBot；
- Agent-S；
- UFO。

### bridge

兼容外部生态的桥，不是 Kernel 内部权威协议：

- MCP。

### platform mechanism

操作系统提供的机制，不是第三方产品依赖：

- Linux：AT-SPI、XDG Desktop Portal、PipeWire、compositor IPC、polkit、systemd user/system 服务；
- Windows：UI Automation、Win32/DWM、Windows capture、UAC/service/elevated helper；
- macOS：Accessibility、ScreenCaptureKit、Authorization/Privileged Helper。

## 2. Pi Agent

分类：direct dependency（agent-runtime 侧）。

借鉴/使用：

- TypeScript Agent Runtime；
- 模型适配；
- Agent loop；
- Tool/Extension；
- 临时 Worker；
- SDK 嵌入。

不交给 Pi：

- 权限事实；
- Task 状态权威；
- Privilege；
- Extension 信任；
- Memory 真相。

Pi 是可替换的 runtime/模型编排依赖，不得把 Pi 的宿主假设上升为 agentd Kernel 不变量。

## 3. MaiBot

分类：reference only。

借鉴：

- Planner 与最终回复分离；
- **Turn/Action Gate（MaiBot 历史术语）**；
- Deferred Tool；
- 学习旁路；
- 中期摘要与启发式召回；
- Worker 隔离。

### 术语对齐（避免冲突）

- MaiBot 文档中的 **Turn/Action Gate** 仅作为历史术语保留在此参考说明中；
- Companion 规范中的对应概念名称是 **Intent Router**（见 `MODEL_RUNTIME.md`）；
- 实现、Schema、日志、新文档应使用 **Intent Router**，不要并行引入 Turn Gate / Action Gate 作为规范名；
- 阅读 MaiBot 或迁移笔记时，见到 Turn/Action Gate 应映射理解到 Intent Router / 入口路由，而不是 Computer Use 的 Execute Gate。

不照搬：

- 群聊专属复杂度；
- 每轮 Planner；
- 多个学习 Agent 服务；
- 与产品目标无关的社群行为模块。

## 4. A_Memorix

分类：optional provider。

可作为 Memory Provider 或代码/模型参考：

- Evidence/Relation/Episode/Profile；
- BM25 + vector + graph；
- 强化、衰减和维护。

必须隔离：

- MaiBot 生命周期；
- 直接 Prompt 注入；
- 记忆事实所有权；
- 将 Python 依赖泄漏为 Kernel 硬依赖。

注意许可证和发行策略。Memory 事实所有权仍在 Companion Memory Domain。

## 5. computer-use-linux

分类：optional Profile Provider / reference。

可作为 Linux Computer Use Extension Profile 的 Provider/参考：

- AT-SPI；
- Window backend；
- Portal/screenshots；
- Input fallback；
- Hyprland；
- doctor/capability report。

不能替代：

- Computer Use Profile composition contract；
- Snapshot Composer；
- Profile 专用的 Execute Gate / Input Session Lease；
- 权限；
- Task 和验证。

Execute Gate 与 Input Session Lease 是该 Profile 的专用术语，不是 Core 通用执行边界；通用 Core 仍以 Task、Policy、Broker、验证和审计链为边界。

## 6. pi-computer-use

分类：optional Profile Provider / architecture reference。

借鉴：

- observe/search/inspect/act/wait；
- 状态作用域；
- successor state；
- 动作后验证。

定位：Windows/macOS 早期 Provider 或架构参考。不得绑定 Core 数据模型。

## 7. OmniParser

分类：optional Profile Provider / reference。

定位：Visual Grounding Provider，产生候选，不直接执行输入。

候选必须进入未来 Computer Use Profile 的 Target Resolver，与语义/应用候选融合，并接受该 Profile 专用的 Execute Gate 约束。

## 8. Set-of-Mark

分类：optional Profile method / annotation reference。

定位：Operation Snapshot 的编号标注方法。候选来源仍由语义、应用和视觉融合决定。

## 9. Agent-S / UFO

分类：reference only。

用于参考：

- GUI grounding；
- Agent 评测；
- Windows 多来源 UI 自动化。

不直接成为核心 Agent Runtime，避免与 Pi/Kernel 重叠。

## 10. OS 原生机制

分类：platform mechanism。

### Linux

- AT-SPI；
- XDG Desktop Portal；
- PipeWire；
- compositor IPC（含 Hyprland、niri 等专用增强，但是否完整支持由能力探测报告）；
- polkit；
- systemd user/system 服务。

### Windows

- UI Automation；
- Win32/DWM；
- Windows capture；
- UAC/service/elevated helper。

### macOS

- Accessibility；
- ScreenCaptureKit；
- Authorization/Privileged Helper。

这些机制是 Provider 的平台事实来源，不是 Companion 可替换“第三方库品牌”。

## 11. Tauri

分类：direct dependency（desktop-client）。

用于轻量跨平台桌面客户端和系统 WebView。Kernel 必须独立于 Tauri 窗口；UI 退出不等于 agentd 退出。

## 12. MCP

分类：bridge。

用于外部工具兼容桥。内部 SDK 除工具调用外还需要生命周期、权限、事件、升级和恢复，因此不以 MCP 作为唯一内部协议。

MCP Tool 进入系统时必须经 Bridge 转化为 SDK Capability，并走 Task/Policy。

## 13. Wasmtime/WASI

分类：optional provider / runtime。

用于可选的低权限、可移植逻辑扩展。不是所有安装的强制依赖，也不是 Extension 的默认形态。

Native Sidecar、MCP Bridge、Remote Provider 均可在无 WASI 时合法存在。

## 14. 与规范名对照

| 外部/历史说法 | Companion 规范名 | 所在事实源 |
|---|---|---|
| MaiBot Turn/Action Gate | Intent Router | `MODEL_RUNTIME.md` |
| 截图点击工具链 | Computer Use Extension Profile composition contract | `COMPUTER_USE.md` |
| MCP tools | Extension Capability via MCP Bridge | `EXTENSION_SDK.md` |
| 任意 root helper | Privilege Broker 固定 Action | `SECURITY_PRIVILEGE.md` |

避免把未来 Computer Use Profile 的 Execute Gate 与 Core 通用执行边界或 MaiBot 历史 Turn/Action Gate 混称。

## 15. 引用原则

在实现中参考外部项目时：

- 记录版本/commit；
- 保留许可证；
- 不宣称未经验证的平台能力；
- 不复制私有接口假设；
- 通过 Companion SDK Adapter 隔离；
- 外部组件升级不得绕过 Conformance；
- direct dependency 升级不得反向修改 Kernel 状态所有权；
- optional provider 缺失时必须降级报告，不得假装能力存在；
- reference only 项目不得被实现为硬依赖。
