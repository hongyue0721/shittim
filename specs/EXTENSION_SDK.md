# EXTENSION_SDK.md

> 本文件是一套统一 Extension SDK 的唯一事实源。禁止为不同扩展域建立平行生命周期和协议。

## 1. SDK 目标

SDK 必须支持：

- 跨语言；
- 进程隔离；
- 能力组合；
- 局部 Provider；
- 权限声明；
- 按需启动；
- 取消、进度和事件；
- 版本协商；
- 更新、回滚和健康检查；
- 官方与社区实现同等待遇；
- 在 Freedom-first Policy 下，无匹配规则时默认安装/启用/运行；
- 诚实区分 OS-enforced、host-enforced 与 declaration-only 边界。

## 2. Profile

一个 Extension 可以声明一个或多个 Profile。Profile 是能力声明与适配面，不是独立权限系统。

### capability

普通业务能力，如浏览器、邮件、日历、媒体、开发工具。

### computer.scene

窗口、显示器、工作区、焦点、布局和窗口事件。

### computer.semantic

无障碍语义树和元素动作。

### computer.capture

窗口、显示器、区域和持续画面捕获。

### computer.input

键盘、指针、滚动和触控输入。

### computer.event

桌面或应用事件。

### computer.visual_grounding

从画面产生视觉候选，不直接拥有输入权限。

### channel

Telegram、飞书、QQ、Web、语音等入口。

### memory

存储、检索、embedding、图谱、Episode 聚合或画像候选。

### model

模型 API、能力和流式输出适配。

### document

文档格式读取、写入、渲染或转换。

### skill_source

Skill 发现、仓库或打包来源。

### ui

声明式 UI 或隔离 WebView 扩展。

### privilege

平台特权动作集合。此 Profile 只声明/适配固定能力，不能直接调用 Privilege Broker。

Privilege Profile 规则：

- 只能声明固定、可枚举的 privilege action ids 与参数 Schema；
- 不能暴露任意 shell、任意可执行路径或未约束脚本；
- 不能持有或转发 Broker 凭据；
- 唯一合法路径始终是 `Task -> Policy -> agentd -> Broker`；
- Extension 最多返回 CapabilityNeed 或 privilege request 候选，由 Kernel 解释并走 Broker；
- 是否允许安装/启用 privilege Profile 由 Policy、签名与安装规则决定，不是 SDK 隐式特权。

## 3. 扩展包

扩展包至少包含：

- Manifest；
- 可执行入口（Native Sidecar）、可选 WASI Component，或其他宿主支持的运行入口；
- Schema；
- 权限说明；
- 许可证和作者；
- 版本与兼容范围；
- Conformance 声明；
- 可选 UI/文档资源；
- 完整性校验（哈希；签名按来源策略可选或强制）。

WASI Component 是可选运行形态，不是安装或发布的强制前提。不得因“非 WASI”拒绝合法 Native/Remote/MCP 扩展。

## 4. Manifest

Manifest 必须声明：

- extension id（稳定且全局唯一）；
- display name；
- publisher；
- version；
- SDK protocol range；
- core compatibility；
- profiles；
- runtime type；
- entrypoint；
- requested permissions；
- configuration schema；
- data directories；
- optional dependencies；
- update source；
- license；
- trust metadata；
- supported platforms/architectures；
- capability enforcement class hints（见第 15 节）：OS-enforced / host-enforced / declaration-only。

扩展不能在运行时静默新增 Manifest 权限。权限集合变化必须进入更新/安装记录，并由 Policy 决定是否需要 confirm。

## 5. 运行形态

### Native Sidecar

适合系统 API、Channel、模型、本地服务和复杂依赖。

边界事实：

- 未沙箱 Native 代码可绕过 SDK 自检与声明式权限；
- Manifest 权限不等于 OS 隔离；
- 对 Native，SDK 能做的是 host invoke 校验、进程监督、审计、最小句柄授予与崩溃隔离，不能虚构“声明即可限制全部旁路”；
- 实现与文档必须把这一点标为 truthful boundary，而不是安全保证。

### WASI Component

适合纯逻辑、文本转换和低权限功能。宿主只授予显式 Capability。

WASI 是可选加固形态，不是默认安装路径，也不是所有扩展的强制运行时。

### MCP Bridge

用于接入既有 MCP Server。MCP Tool 经过 Bridge 转化为 SDK Capability，仍受 Kernel 权限与任务上下文约束。MCP 不是 Kernel 内部权威协议。

### Remote Provider

只在用户明确配置时允许。必须使用认证、TLS/安全通道、数据发送策略和可撤销凭据。

## 6. 传输

默认：

- Unix Domain Socket；
- Windows Named Pipe；
- 子进程标准流只允许用于受控启动握手，不适合长期大对象流。

控制面采用版本化 JSON-RPC 风格消息。

大对象使用：

- Object Handle；
- 受控临时文件；
- 共享内存；
- 本地流。

Object Handle 必须有：

- id；
- content type；
- size；
- hash；
- owner/task；
- access mode；
- expiration。

## 7. 握手

流程：

1. Kernel 启动扩展并提供一次性连接凭据；
2. 扩展发送 `hello`；
3. 双方协商协议版本和 Profile；
4. Kernel 发送已授予权限和运行上下文；
5. 扩展执行能力探测；
6. Kernel 注册能力档案；
7. 扩展进入 ready。

如果实际能力与 Manifest 不符，按实际能力降级；如果扩展请求未在当前 Task/Policy 下允许的权限，Kernel 不提供相应句柄。

“未批准”指 Policy 输出 deny/confirm 等判定结果，或系统机制拒绝；不是默认首次审批矩阵。

## 8. 统一生命周期

状态：

```text
discovered
installed
disabled
starting
handshaking
ready
degraded
stopping
stopped
crashed
quarantined
incompatible
```

基础操作：

- initialize；
- probe；
- health；
- invoke；
- cancel；
- subscribe/unsubscribe；
- reconfigure；
- shutdown。

默认行为（Freedom-first）：

- 无匹配用户/系统规则时，安装、启用、运行默认允许；
- confirm、deny、quarantine、强制禁用只能来自匹配规则、系统机制、健康失败、兼容失败或明确恢复状态；
- 不因“社区扩展”“AI 自写”“首次安装”自动插入默认审批步骤。

## 9. 调用上下文

每次调用必须携带：

- request id；
- task id；
- action id（副作用时）；
- actor；
- entry point；
- granted capability；
- permission lease refs；
- resource scopes；
- deadline；
- cancellation token；
- idempotency key；
- locale；
- trace id。

扩展不得相信由模型构造的 Actor 或 Permission 字段；这些字段由 Kernel 注入。

### Host Invoke 校验

每次 host invoke 必须校验：

- capability 是否已注册且当前 granted；
- input/output 是否符合 Schema；
- task 是否存在且允许该 Action；
- 匹配规则/Policy 判定结果；
- lease 是否有效且绑定 task/action/resource；
- scope 是否覆盖目标资源。

校验失败时拒绝本次 invoke 并审计。这是 host-enforced 边界：对遵守 SDK 的扩展有效；对未沙箱 Native 旁路不能假装同等强制。

## 10. 能力声明

能力描述必须包含：

- stable capability id；
- summary；
- input/output schema；
- side-effect class；
- required permissions；
- reliability；
- cancellation semantics；
- idempotency semantics；
- progress support；
- verification hints；
- platform constraints；
- cost hints；
- whether user interaction can occur；
- enforcement class（OS-enforced / host-enforced / declaration-only）。

### CapabilityNeed

当当前扩展无法单独完成目标时，可返回结构化 CapabilityNeed，而不是私自串联其他扩展。

CapabilityNeed 至少包含：

- need id；
- capability id 或 capability query；
- required profiles（可选）；
- input summary / partial payload；
- side-effect class；
- resource scopes；
- urgency / deadline hints；
- why needed；
- acceptable alternatives。

规则：

- CapabilityNeed 由 agentd 解析；
- 经 Policy 评估后，由 Kernel 选择 Registry 中的实现并 invoke；
- 不得指定任意扩展 id 以绕过 Registry 选择与权限；
- 可以建议 preferred capability/provider 类别，但最终选择权在 Kernel。

## 11. 结果

统一结果必须区分：

### Machine Data

结构化数据，供 Kernel 与后续任务使用。

### User Presentation

可展示摘要、图片、文档或卡片，但最终表达由 Companion/UI 控制。

### Audit Facts

实际执行动作、资源变化、权限使用和外部确认。

### Artifacts

通过 Object Handle 返回。

结果状态：

- success；
- partial；
- failed；
- cancelled；
- unknown_side_effect；
- waiting_user。

## 12. 错误模型

至少统一：

- unsupported；
- unavailable；
- incompatible；
- permission_denied；
- approval_required；
- invalid_request；
- target_not_found；
- stale_state；
- conflict；
- timeout；
- cancelled；
- provider_crashed；
- external_failure；
- verification_failed；
- partial_side_effect；
- unknown_side_effect；
- rate_limited；
- data_policy_blocked；
- lease_expired；
- scope_violation；
- capability_not_granted。

错误必须说明：

- 是否可重试；
- 是否需要重新观察；
- 是否需要用户操作；
- 已发生哪些副作用；
- 是否存在回滚。

`approval_required` 仅在 Policy 输出 require_confirmation / require_local_confirmation / require_system_authentication 等匹配结果，或系统机制要求时出现；不是默认安装/首次运行错误。

## 13. 取消

扩展必须声明取消语义：

- immediate；
- cooperative；
- after_current_unit；
- not_cancellable。

Kernel 发出取消后，扩展必须回报最终状态。取消不能被当作未发生任何副作用。

## 14. 事件

扩展 Event 必须：

- 有 source extension；
- 有 event type/version；
- 有 timestamp/sequence；
- 有 scope；
- 有可选 task correlation；
- 不携带未授权 Secret。

Kernel 决定哪些事件可触发 Initiative 或 Task，扩展不能自行创建授权任务。

## 15. 权限与边界诚实性

权限由 Domain + Action + Scope 表达，例如：

- filesystem.read `/path`；
- filesystem.write `AI workspace`；
- network.connect `allowed domains`；
- computer.capture `window`；
- computer.input `active session`；
- channel.send `account/chat`；
- memory.query `specific scopes`；
- model.invoke `provider/profile`；
- privilege.request `fixed action ids`。

Extension Manifest 申请的是安装级上限；每次 Task 调用仍需 Kernel 生成任务级权限。

### 三类 enforcement

必须明确区分，不得互相伪装：

| 类别 | 含义 | 示例 |
|---|---|---|
| OS-enforced | 操作系统或沙箱强制 | WASI capability、OS sandbox、portal 授权 |
| host-enforced | agentd/host 在协议与句柄层强制 | invoke 校验、lease、scope、不授予句柄 |
| declaration-only | 仅声明与审计，无法阻止绕过 | 未沙箱 Native 自称无网络 |

规则：

- 文档、UI、审计与 Conformance 必须按实际类别标注；
- Manifest 不等于 OS 隔离；
- 未沙箱 Native 可绕过 SDK；宿主仍应做 host invoke 校验，但不得宣称已限制全部旁路；
- 网络、文件系统、输入等权限在 Native 上可能降级为 declaration-only 或 host-enforced，取决于运行形态与平台机制。

## 16. 跨扩展协作

禁止扩展直接发现、连接或调用另一个扩展。

正确流程：

```text
Extension A returns CapabilityNeed / candidate
-> agentd resolves capability via Registry
-> policy evaluation
-> Extension B invoked in same Task or subtask
```

这样可以防止权限链和隐藏调用。不允许“指定扩展 id 直接跳转”绕过 Registry。

## 17. UI Profile

默认只支持声明式组件：

- text/markdown；
- form；
- table/list；
- status/progress；
- image/artifact；
- action button；
- settings schema；
- diagnostic panel。

需要任意前端代码时，必须运行于隔离 WebView/iframe，使用受限消息桥，不得进入主 Companion DOM 或直接调用本地 API。

## 18. 配置

扩展配置：

- 由 Kernel 保存；
- 使用 Schema 验证；
- Secret 只保存引用；
- 配置变化触发 reconfigure 或重启；
- 扩展不得把不可解释数据偷偷存入全局目录。

## 19. 安装、更新和回滚

### 来源类别

- Native / 官方分发；
- AI 自写；
- 未审核社区扩展。

风险自担原则：

- 未审核社区与 AI 自写扩展的风险由用户/策略承担；
- 系统必须展示来源与信任元数据，不得伪装为官方；
- 技术上仍记录完整性与权限，但不因来源类别默认阻断安装。

### 安装/更新必须记录

无论是否 confirm，安装与更新至少记录：

- 来源（URL/路径/生成任务/仓库）；
- 作者 / publisher；
- 许可证；
- 版本；
- 内容哈希；
- 签名（若有；无签名则明确记录 absent）；
- 声明权限与 enforcement class；
- 健康检查结果；
- 回滚点（上一版本、配置与数据迁移点）。

### 安装前展示

- 作者与来源；
- 许可证；
- 信任等级；
- 权限与 enforcement class；
- 数据和网络访问声明；
- 是否含原生代码；
- 是否支持自动更新；
- 是否 AI 生成 / 未审核。

展示不等于默认审批。无匹配规则时默认继续安装/启用。

### 更新时

- 比较权限差异；
- 验证哈希；按策略验证签名；
- 检查协议兼容；
- 备份配置/数据迁移点；
- 失败回滚旧版本；
- 权限变化是否 confirm 由 Policy 决定，不是“新增权限必须重新批准”的硬编码默认。

### 回滚

- 保留可回滚版本与配置快照；
- 健康失败、兼容失败或 Policy 要求时可回滚；
- 回滚本身是治理动作，需审计，不默认要求用户审批，除非 Policy 命中。

## 20. 信任等级

建议：

- built-in official；
- official extension；
- verified community；
- community unreviewed；
- local development；
- AI-generated candidate。

信任等级可影响匹配规则的默认建议、自动更新策略、最大权限建议和是否允许 privilege Profile 的策略输入，但不代替任务级权限，也不等于默认 deny。

无匹配规则时，信任等级单独不能把安装/启用/运行变成默认拒绝。

## 21. AI 自写扩展

AI-generated Candidate 可以自动迭代安装，只要落在 Policy、Delegation、预算与停止条件之内。

治理（不是默认审批）包括：

- 版本化生成与安装记录；
- 测试 / SDK Conformance / 安全扫描结果；
- 预算（时间、次数、资源、网络费用等，按配置）；
- 停止条件；
- 失败或策略触发时的回滚；
- 可观测性与审计。

明确：

- 不可伪装为官方签名；
- 不强制默认无网络；网络是否允许由声明权限 + Policy/Scope 决定；
- 不强制默认首次审批；
- 权限为任务级与安装级共同约束，不是固定“最小且不可扩展”；
- 权限变化是否 confirm 由 Policy 决定；
- 只能通过公开 SDK 与 Registry 路径安装，不能改 Core。

### Core 不可自改

Agent、Skill、Extension、Provider 不得读取后修改、补丁、热替换或重写：

- agentd Kernel；
- Task/Policy 解释器；
- Audit 完整性机制；
- Privilege Broker；
- Emergency Stop；
- Core Identity；
- 安全/隔离边界；
- 私有 Kernel API。

正式开发者发布升级可更新 Core。AI 自写扩展永远不是 Core。

## 22. Privilege 与 Broker 边界

- Privilege Profile 只声明/适配固定能力；
- Extension 不能直接 Broker；
- 唯一路径：`Task -> Policy -> agentd -> Broker`；
- Broker 只接受固定 Action ID 与严格参数 Schema；
- 普通 Shell 与 Broker 固定特权 Action 是不同通道；禁止用前者规避后者。

## 23. 法律和生态边界

项目可以：

- 清楚展示作者、来源和许可证；
- 标识未审核扩展与 AI 生成扩展；
- 提供删除、禁用、报告和安全隔离；
- 要求发布者接受分发条款；
- 声明 Native/未审核/AI 自写风险自担。

项目不得宣称一条免责声明可以消除所有法律责任。技术设计必须独立满足可验证的 host/OS 边界，并诚实标注 declaration-only。

## 24. Conformance

每个 SDK 实现必须通过：

- 协议握手；
- Schema；
- 权限与 invoke 校验；
- 超时和取消；
- 崩溃恢复；
- 大对象句柄；
- 版本兼容；
- 更新/回滚与安装元数据记录；
- CapabilityNeed 不得绕过 Registry；
- Privilege 不得直连 Broker；
- enforcement class 标注；
- Profile 专用测试；
- Core 自改防护（扩展侧无法获得写 Core 的合法 API）。
