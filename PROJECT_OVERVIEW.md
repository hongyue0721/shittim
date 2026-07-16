# Shittim：独立 AI 个体与可扩展桌面智能体框架

## 1. 项目是什么

Shittim 是项目与产品品牌。Companion 是 Shittim 中长期与用户交流、保持关系连续性并协调任务的 AI 角色概念，而不是产品暂称。

它明确承认自己是 AI，不冒充用户，也不把自己当作用户的数字分身。它可以拥有稳定人格、长期记忆、表达习惯、观点、好奇心和有限主动性，并在用户或自身建立的 Policy 边界内处理真实事务。

系统采用 Freedom-first：没有匹配的 Policy Rule 时，动作默认允许；确认、拒绝和认证只由匹配规则、底层系统机制或明确恢复状态产生。风险等级用于说明、匹配、审计和恢复，不能暗中变成默认禁止矩阵。第一版不建立 Owner 唯一身份认证；每项治理变更记录 actor、entry point、认证证据（如有）、来源与 policy mutation authority，而不预设固定的本机、远程或个人治理主体。

它的目标同时包括：

- 自然交流和情绪价值；
- 长期关系与共同经历的连续性；
- 桌面应用操作；
- 文档处理；
- Skill 学习；
- 在 SDK 范围内为自己增加新能力；
- 通过本地界面以及未来的 Telegram、飞书、QQ/NapCat 等入口接受任务；
- 在用户授权区域内只读探索背景并主动提出有价值的任务。

## 2. 它不是什么

它不是：

- 用户替身或代言人；
- 只依赖截图点击的自动化脚本；
- 一个长期加载所有工具的超级 Prompt；
- 由多个独立人格组成的 Agent 群；
- 一个拥有 root 权限的模型进程；
- 一个只能运行在 CachyOS/Hyprland 上的单平台项目。

## 3. 核心体验

用户始终与同一个 Companion 交流。

底层可以根据任务切换不同模型角色和能力，但这些角色是同一 AI 的不同工作方式：

- Companion 负责交流和关系；
- Planner 只在复杂任务中进行规划；
- Worker 只接收最小子任务上下文；
- Memory Extractor 形成记忆候选；
- Skill/Extension Writer 在公开 SDK 内编写新能力。

用户不会被迫直接面对底层 Coding Agent、Computer Agent 或大量 Tool 调用。

## 4. 精简后的运行时架构

基础安装只需要两个常驻后台运行时：

```text
agentd
  Rust Kernel，负责现实状态、任务、权限、记忆政策和扩展治理

agent-runtime
  TypeScript + Pi，负责人格、理解、规划、模型和自然表达
```

用户打开界面时存在：

```text
desktop-client
  Tauri + React
```

重型模块按需启动：

- Computer Use Provider；
- A_Memorix 或其他 Memory Provider；
- OmniParser/视觉模型；
- Telegram、飞书、QQ Channel；
- 文档和应用专用 Provider；
- AI 自写或第三方 Extension。

需要管理员权限时，独立 Privilege Broker 短暂启动，执行固定动作后退出。

这套设计的原则是：

> 能力完整，但常驻运行时很少；协议开放，但核心很窄。

## 5. 三个产品层

### 5.1 交互层

负责：

- 对话和人格；
- 情绪回应；
- 任务进度；
- Screenshot/Operation Snapshot；
- 权限确认；
- 远程入口。

### 5.2 控制层

由 `agentd` 掌握：

- 任务状态；
- 权限；
- 委托契约；
- 记忆政策；
- 主动性；
- 扩展治理；
- 动作验证；
- 审计与恢复。

### 5.3 能力层

由按需 Provider 和 Extension 提供：

- Computer Use；
- Document；
- Skill；
- Channel；
- Memory；
- Model；
- 未来能力。

三层是职责视图，不代表三个进程。

## 6. 人格、成长和记忆

用户可以在首次配置时键入 Persona Prompt，作为 AI 的人格种子。

Companion 在长期相处中可以形成：

- 自身记忆；
- 表达方式；
- 对任务方式的偏好；
- 对用户边界的理解；
- 共同经历；
- 对错误的反思。

这些内容都必须：

- 有来源，且可追溯其 ContentOrigin；
- 对用户可见；
- 可删除、纠正或重置；
- 不自动扩大已存在规则的作用域；
- 不覆盖用户明确要求；
- 可在 Policy 允许时保存敏感内容、认证材料或 Secret；是否使用可选 Secret Provider 由规则、任务和实现能力决定，不以“一律不得保存”掩盖用户的明确意图。

记忆由 Kernel 的 Memory Domain 管理。A_Memorix 可以作为高级后端，但不能自行把画像注入 Prompt，也不能成为第二套事实系统。删除、纠正和回滚必须按来源关系传播到摘要、索引和派生记忆。

## 7. 探索欲、主动任务与自然语言治理

Companion 可以在 Exploration Scope 中只读探索：

- 项目文档；
- 指定笔记库；
- 授权的日历、邮件或消息范围；
- 自己的任务历史和 Artifact。

Exploration Scope 是发现资料的边界，和 Task Scope 分离：前者不因发现内容而授予修改、外发或特权能力；后者逐个任务规定可操作资源与副作用。探索只能产生发现、记忆候选、任务建议或只读准备结果。修改、发送、删除和系统操作仍必须进入独立 Task Scope、正常 Policy 与审计链路。

用户和 Companion 都可以用自然语言创建、修改、撤销结构化 Policy Rule、Exploration Scope、Delegation 与 Trigger。自然语言先产生可审阅、版本化的候选结构化对象；获得规则允许的 mutation authority 后才生效，并完整记录 actor、entry point、认证证据（如有）、ContentOrigin 和变更理由。第一版不把 mutation authority 硬编码为本机或 Owner 专属。

用户可以把某类重复任务交给 Companion 自动处理。系统不会只保存一句模糊的“以后自动做”，而会建立委托契约，规定：

- 处理什么；
- 在哪里；
- 能做哪些动作；
- 最大副作用；
- 何时触发；
- 是否确认；
- 预算和频率；
- 如何验证；
- 如何撤销。

例如“以后你自己帮我升级”可能最终由一个检测 Skill、定时触发器和受限特权动作组成，但不会变成通用 root 权限。

## 8. Computer Use

Computer Use 融合：

- 无障碍语义树；
- 窗口、显示器和工作区；
- 屏幕或窗口画面；
- 应用专用 API；
- 视觉识别补漏。

动作优先级是：

```text
应用接口
-> 语义控件动作
-> 窗口和快捷键
-> 语义位置 + 输入
-> 视觉定位 + 输入
```

所有副作用都要重新观察和验证。

### Operation Snapshot

需要用户监督时，系统可以发送带编号的截图：

- 只标当前任务相关候选；
- 显示元素语义和风险；
- 用户可以回复“点 7”“往下滚”“停止”；
- 旧快照不能直接复用坐标，执行前必须重新解析并验证目标。

给用户看的标注图与给模型的输入分离，因此不会因为远程可视化而必然产生高 Token 消耗。

## 9. 三平台和 Linux 特殊环境

统一的是桌面概念、动作、可靠性和验证；底层实现由 Provider 完成。

Windows 主要使用 UI Automation 和原生窗口/捕获/输入能力；macOS 使用 Accessibility、ScreenCaptureKit 和系统事件；Linux 使用 AT-SPI、Portal/PipeWire、输入 Provider 与 compositor 适配器。

Hyprland 和 niri 作为示范 Scene Provider：

- Hyprland 处理窗口、工作区、输出和事件；
- niri 处理滚动布局、viewport 可见性和聚焦后的重定位；
- 通用 AT-SPI、截图和输入能力不重复实现。

其他平台维护者可以只实现缺失的 Provider，而不需要重写整个 Computer Use。

## 10. 完整 Extension SDK

项目只有一套 SDK，通过不同 Profile 接入：

- 能力；
- Computer Use 子 Provider；
- Channel；
- Memory；
- Model；
- Document；
- UI；
- Privilege。

SDK 统一提供：

- Manifest；
- 权限；
- 生命周期；
- 能力发现；
- 取消；
- 进度；
- 错误；
- 事件；
- 健康检查；
- 更新和回滚；
- 一致性测试。

常见运行形态是独立进程或可选 WASI；Native Extension 同样可以安装和运行，但其权限展示必须诚实区分 OS-enforced、host-enforced 和 declaration-only，不能将声明伪装为隔离。跨扩展调用必须回到 Kernel。

Extension 可以来自内置、Companion 自写或社区；三者均可按其 Profile 和适用 Policy 安装、运行、更新与回滚。未命中规则时默认允许，不默认要求安装前确认或断开网络。Companion 可以使用公开 SDK 为自己编写或改进 Extension、Skill、记忆、路由、Trigger、Delegation 与受治理配置；这些对象必须版本化、受预算和停止条件约束且可回滚，但不能修改 Kernel 核心、Task/Policy 解释器、Audit 完整性机制、Privilege Broker 或 Emergency Stop。

## 11. 模型与数据

用户数据默认本地保存。模型可以使用：

- OpenAI Responses；
- OpenAI Chat Completions；
- Anthropic Messages；
- OpenAI-compatible 自定义地址；
- 本地兼容端点或本地 Model Provider。

Companion、Planner、Memory Extractor、视觉推理和 Skill Writer 可以使用不同模型。

模型只获得当前任务相关的 Context Pack、工具摘要和记忆，以任务相关性最小化而非按敏感类别审查、脱敏或阻断数据。云调用可按任务需要携带敏感内容、Secret 或认证数据；系统不设置“认证界面绝不上云”之类的绝对禁令，也不以内容过滤替代 Policy。调用记录仅保留完成恢复、计费和审计所需的最小元数据及 ContentOrigin，具体内容是否持久化由 Memory/Policy 决定。

## 12. 安全、Policy 与特权

普通 Agent 永远不以 root/管理员运行。特权动作由极小、无模型的 Broker 执行，并只接受固定、可验证的 Action ID；普通 Shell 是独立通道，不能借此修改 Core 或绕过 Broker 的固定特权 API。

Policy 用 allow、confirm、deny 的结构化规则决定行动；无匹配规则即 allow。系统提供推荐确认模板供用户或 Companion 选择启用，但它不是默认策略。风险、入口、来源、作用域、委托、预算、认证状态和恢复状态都可成为规则匹配条件，并留下可追溯审计。

远程 Channel 可以发起任务、接收截图和监督操作，规则可以决定其是否能够确认敏感行为。第一版记录入口身份及认证事实，但不把权限变更固定给某一入口、设备位置或身份标签。所有外部文本、网页、附件和 Channel 消息都携带 ContentOrigin；它们只能作为数据，不能伪造 Kernel Command、Kernel Event、Policy mutation evidence 或 Broker 请求。

Emergency Stop 在 Kernel 内建立 Stop Fence：一旦停止，新的副作用、租约消费、主动任务、输入、Extension 调用和远程执行均被拦截；停止不依赖模型响应。

## 13. 技术栈

- Rust + Tokio：Kernel 和系统边界；
- TypeScript + Pi：智能运行时；
- Tauri 2 + React：桌面客户端；
- SQLite + FTS5：本地事实、审计和轻量检索；
- JSON Schema 2020-12 + RFC 8785：跨语言契约、生成类型与稳定 canonical hash；
- 本地 KCP（Unix Domain Socket / Windows Named Pipe）：Kernel 客户端控制协议，不预设 JSON-RPC；
- 版本化 JSON-RPC 风格消息 + JSON Schema：Extension RPC 控制面；
- Python Sidecar：可选高级记忆、视觉和 ML；
- 原生 Provider：Windows、macOS、Linux；
- WASI/Wasmtime：可选低权限扩展；
- MCP Bridge：兼容外部 MCP 生态，而不是内部核心协议。

## 14. 关键架构结论

这不是一个 Agent 微服务集群。

它是：

> 两个常驻核心运行时、一个可关闭重连的客户端、多个按需扩展，以及一个只在必要时出现的特权执行器。

它的长期可扩展性来自稳定协议和明确状态所有权，而不是提前启动所有未来组件。
