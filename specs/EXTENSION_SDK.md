# EXTENSION_SDK.md

> 本文件是 Extension SDK Base 的唯一事实源：定义 Manifest、Profile/version negotiation、能力发现、调用、取消、进度、健康、事件、Object Handle、安装治理与 Core Conformance。各 Optional Profile 的领域对象与动作语义由独立 Profile 规范定义，不在 Base 复制。

## 1. SDK 目标

SDK 必须支持：

- 跨语言；
- 进程隔离；
- 能力组合；
- 可选 Profile 与局部 Provider；
- Profile/version negotiation 与 capability discovery/probe；
- 权限声明；
- 按需启动；
- invoke、cancel、progress 与 health；
- 未来可扩展的 Extension Event 职责（当前未实现，见第 14 节）；
- Object Handle 和大对象传输；
- 更新、回滚和健康检查；
- 官方与社区实现同等待遇；
- 在 Freedom-first Policy 下，无匹配规则时默认安装/启用/运行；
- 诚实区分 OS-enforced、host-enforced 与 declaration-only 边界；
- 可选 Profile 缺失时 Core 仍可完整启动，能力查询按 `unsupported` / `unavailable` / `incompatible` / `capability_not_granted` 的固定语义返回。

### 1.1 两张不同的方向图

**源码 / Schema 依赖**约束 import、生成和公开类型所有权：

```text
Optional Profile package
  -> Extension SDK public contracts
    -> Core public contracts
```

Shittim Core 与 Extension SDK Base 不得反向引用、生成或固定任何 Profile 领域类型。

**运行时宿主控制 / 调用**描述 negotiation、grant、invoke、cancel 与监督：

```text
Core host
  -> SDK host boundary
    -> negotiated Optional Profile implementation
```

这不是源码依赖反转。两张图方向相反但语义不同，禁止合并为一句“依赖只能单向”。Profile 可以组合 SDK 原语，但不得要求 Shittim Core 或 Extension SDK Base 理解其对象模型。禁止为 Computer Use、Memory、Channel 等域建立平行生命周期、传输、安装器、权限租约或 Extension 间调用协议。

### 1.2 缺失是合法状态

- 没有安装任何实现某 Optional Profile 的 Extension 时，`agentd` 必须正常启动；
- Manifest 未声明、握手未协商、probe 未确认或健康状态丢失的 capability 都不在可调用集合中；
- capability query 必须按固定语义返回：当前安装/发行/协商集合不提供为 `unsupported`；已支持但当前暂不可用为 `unavailable`；版本无兼容交集为 `incompatible`；能力已存在但当前调用未获 grant 为 `capability_not_granted`。不能返回空成功、占位 Object Handle、伪造 Provider 或“稍后会有”的假能力；
- Planner、UI 与 Profile adapter 都只能消费 Registry 的实际档案，不能把 capability hint 当成已存在事实。

## 2. Profile 模型

Profile 是对一组能力语义、版本和 Conformance 的命名合同。Extension 可以声明零个或多个 Profile；Profile 是能力声明与适配面，不是进程类型、Kernel 模块或独立权限系统。

每项 Profile 声明至少表达：

- stable profile id / family；
- supported version range；
- 可选 facets；
- capability namespaces；
- 对应 capability descriptor 集合；
- Profile Conformance claim（若有）；
- platform/runtime constraints。

通用 SDK 只解释这些协商元数据，不解释 Profile payload 的领域含义。详细语义必须由 Profile 规范和其版本化 Schema 提供；SDK 文档不得复制完整 Profile 对象、状态机或坐标等细节。

### 2.1 通用 Profile families

#### capability

普通业务能力，如浏览器、邮件、日历、媒体、开发工具。

#### channel

Telegram、飞书、QQ、Web、语音等入口。

#### memory

存储、检索、embedding、图谱、Episode 聚合或画像候选。

#### model

模型 API、能力和流式输出适配。

#### document

文档格式读取、写入、渲染或转换。

#### skill_source

Skill 发现、仓库或打包来源。

#### ui

声明式 UI 或隔离 WebView 扩展。

#### privilege

平台特权动作集合。此 Profile 只声明/适配固定能力，不能直接调用 Privilege Broker。

### 2.2 Computer Use Optional Profile family

Computer Use 是**可选、条件生效、独立验收**的 Profile family，不是 Shittim Core 能力或基础产品完成项。具体 facet Catalog 的唯一事实源是 [`COMPUTER_USE.md`](COMPUTER_USE.md) 第 4 节；SDK 不复制该闭集。非穷举例子包括 `computer.scene`、`computer.capture`、`computer.application`，但新增、删除、职责和完整发布集合都只能在该 Catalog 定义。

Extension 不必实现全部 facets，Kernel 也不得从 family 名推断任何 facet 已存在。具体 Display、Window、Screenshot、Coordinate、desktop generation、OperationSnapshot、输入控制或应用适配语义属于 Computer Use Profile 规范，不进入 Extension SDK Base 合同。

### 2.3 Privilege Profile 规则

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
- profiles（每项含 stable id/family、version range、facets 与 capability namespaces）；
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
2. 扩展发送 `hello`，声明 SDK protocol range 与 Manifest identity；
3. 双方先协商 SDK protocol，再对每个 Profile 独立协商 compatible version/facets；未协商项不得进入 Registry；
4. Kernel 发送已授予权限和运行上下文；
5. 扩展对已协商项执行 capability discovery/probe，返回实际 descriptor、限制和 enforcement class；
6. Kernel 只将 Manifest 声明、negotiation 结果、正式且兼容的 versioned operation payload/result Schema（声明事件能力时再加 event Schema）与 probe 事实的交集标记为 **discovery-visible**；
7. 扩展进入 ready、degraded 或 incompatible。具体调用还需独立检查 grant、scope、Task/Action（副作用时）、lease 与 Stop Fence，只有通过后才是 **invocable**。无副作用的只读 observation 不需要虚构副作用 Action grant，但仍必须使用已协商 Schema 并通过调用身份、scope 与数据策略。

协商和探测的边界：

- Manifest 是安装级声明上限，不是运行时可用性证明；
- Profile version 不兼容时只影响对应 Profile claim；除非 SDK protocol 本身不兼容，不得让一个可选 Profile 拖垮整个 Core；
- 实际能力弱于 Manifest 时按实际结果降级；实际报告超出 Manifest 时拒绝越界能力并审计；
- 未声明、未协商、未 probe，或缺少兼容 operation payload/result Schema（事件能力还缺 event Schema）的能力不得 discovery-visible 或 invoke，也不得在 Registry 中以“unknown but available”存在；
- discovery-visible 不等于 invocable：前者是可发现的协商事实，后者还需当前调用的 grant 与边界检查；
- 如果扩展请求未在当前 Task/Policy 下允许的权限，Kernel 不提供相应句柄。

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

当前没有正式 Extension Event Schema、transport、persistence 或 subscription 合同。未来若建立 Extension Event control operation，`subscribe/unsubscribe` 才进入该合同；在此之前不得把下面的生命周期操作列表解读为事件订阅已实现。

基础操作：

- initialize；
- negotiate profiles/versions；
- discover/probe；
- health；
- invoke；
- cancel；
- progress stream/report；
- reconfigure；
- shutdown。

默认行为（Freedom-first）：

- 无匹配用户/系统规则时，安装、启用、运行默认允许；
- confirm、deny、quarantine、强制禁用只能来自匹配规则、系统机制、健康失败、兼容失败或明确恢复状态；
- 不因“社区扩展”“AI 自写”“首次安装”自动插入默认审批步骤。

## 9. 调用上下文

每次调用必须携带：

- request id；
- task id（属于 Task 时；纯 discovery/diagnostic 可为 null）；
- action id（副作用时）；
- Core注入的PermissionDecision material/observation fingerprint与可消费Approval resolution ref（适用时）；Profile不得计算material等价、签发/失效Approval或自行决定复用；
- actor；
- entry point；
- granted capability（副作用或其他需要 grant 的调用；无副作用 discovery/observation 可为空，但不能绕过 Schema/scope）；
- permission lease refs；
- resource scopes；
- deadline；
- cancellation token；
- idempotency key；
- locale；
- trace id。

扩展不得相信由模型构造的 Actor 或 Permission 字段；这些字段由 Kernel 注入。

### Host Invoke 校验

每次 host invoke 必须按调用类别校验：

- capability 是否 discovery-visible；需要 grant 的调用还必须校验当前 granted；
- input/output 是否符合已协商 operation payload/result Schema；
- 副作用调用的 task/action 是否存在且允许执行；纯 discovery/只读 observation 不虚构副作用 Action；
- 适用的匹配规则/Policy 判定结果；
- 副作用调用的 lease 是否有效且绑定 task/action/resource；
- scope 是否覆盖目标资源。

校验失败时拒绝本次 invoke 并审计。这是 host-enforced 边界：对遵守 SDK 的扩展有效；对未沙箱 Native 旁路不能假装同等强制。

## 10. 能力声明

能力描述必须包含：

- stable capability id 与所属 negotiated profile/version/facet；
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

`unsupported`、`unavailable`、`incompatible`、`capability_not_granted` 不得任选：

- `unsupported`：当前安装、发行物或协商后的 Profile/facet/operation 集合不提供该能力；
- `unavailable`：能力已支持且版本兼容，但因 disabled、quarantined、health/backend/session 等当前状态暂不可用；
- `incompatible`：SDK、Profile 或 operation payload/result Schema（事件能力包括 event Schema）版本没有兼容交集；
- `capability_not_granted`：能力已 discovery-visible 且版本兼容，但当前调用没有所需 grant。

错误必须说明：

- 是否可重试；
- 是否需要重新观察；
- 是否需要用户操作；
- 已发生哪些副作用；
- 是否存在回滚。

`approval_required`仅在Policy输出require_confirmation / require_local_confirmation / require_system_authentication / require_remote_signature / require_plan_revision等匹配结果，或系统机制要求时出现；不是默认安装/首次运行错误。

## 13. 取消

扩展必须声明取消语义：

- immediate；
- cooperative；
- after_current_unit；
- not_cancellable。

Kernel 发出取消后，扩展必须回报最终状态。取消不能被当作未发生任何副作用。

## 14. Extension Event（未来协议职责，当前未实现）

当前仓库没有正式 Extension Event Schema、transport、persistence 或 subscription 合同。以下条目只定义未来协议建立后必须承担的职责，不声称 Kernel 现在可接收、保存、索引、订阅或稳定转发 Extension Event：

未来 Extension Event 必须：

- 有 source extension instance；
- 有 Profile namespace（若为 Profile 私有事件）、event type/version；
- 有 timestamp/extension-local sequence；
- 有 scope；
- 有可选 task/action correlation；
- payload 符合 Extension 自己声明的版本化 Schema；
- 不携带未授权 Secret。

未来协议中仍由 Kernel 决定哪些事件可触发 Initiative 或 Task，扩展不能自行创建授权任务。Profile 私有 Event 即使未来进入 SDK 传输层，也不等于进入 Core Event Catalog。除非另立正式 persistence/subscription 合同，否则不得进入 Core EventEnvelope、SQLite Outbox，或声称可持久保存、索引、重放、订阅和稳定转发。

Computer Use 的 `snapshot.created` / `snapshot.expired` / `user_takeover.started` / `user_takeover.ended` 当前只是未来 `computer.event` 合同候选，并非已实现事件。未来即使先成为 Profile 私有 Extension Event，晋升 Core 公共事件仍必须另有正式 Kernel payload Schema、Catalog、兼容性与 Conformance。

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

禁止扩展直接发现、连接或调用另一个扩展。Extension/Profile也不得直接物化Child Task；唯一新写入口是Core拥有的`kernel.task/task.child.create` Action，Extension只能返回CapabilityNeed或结构化proposal，由Kernel完成Policy与原子materialization。

正确流程：

```text
Extension A returns CapabilityNeed / candidate
-> agentd resolves capability via Registry
-> policy evaluation
-> Extension B invoked in same Task or Child Task
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

## 24. Conformance 与完成声明

### 24.1 Extension SDK Base Conformance

每个 SDK 实现无条件必须通过：

- 协议握手与 SDK protocol version negotiation；
- Manifest/Profile 声明校验；
- Profile/version negotiation 的交集与拒绝语义；
- capability discovery/probe，且未声明/未协商/未 probe 的能力不可调用；
- Schema；
- 权限与 invoke 校验；
- 超时、取消和进度；
- 健康与崩溃恢复；
- 大对象句柄；
- 更新/回滚与安装元数据记录；
- CapabilityNeed 不得绕过 Registry；
- Privilege 不得直连 Broker；
- enforcement class 标注；
- Extension Event 仍为 future contract：缺少正式 Schema/transport/persistence/subscription 时，不得进入 Core EventEnvelope/Outbox 或宣称已可保存、索引、订阅、稳定转发；
- 可选 Profile 全部缺失时 Core 仍可启动，并按固定语义返回 `unsupported`；已支持但当前暂不可用返回 `unavailable`；版本不兼容返回 `incompatible`；已发现但未获 grant 返回 `capability_not_granted`；
- Registry 只有在正式 versioned operation payload/result Schema 存在、兼容并完成协商后才可把能力标为 discovery-visible；事件能力还必须有正式 event Schema。负向测试覆盖 Schema 缺失、版本无交集、仅 Manifest、仅 probe 与 discovery-visible 但不可 invocable；
- Core 自改防护（扩展侧无法获得写 Core 的合法 API）。

### 24.2 Profile Conformance

Profile 专用测试是 claim-conditional：只有统一 `ProfileClaim` 集合中存在对应精确 claim id 时才运行；`distribution_asserted=true` 时还成为该发行物的对外发布门。Profile 套件可以测试该领域的对象、动作、私有事件、平台限制与验证语义，但不得修改 Extension SDK Base 的通用成功定义。

Computer Use 不属于 Core Conformance 或基础产品完成项。只有明确声明 Computer Use Profile 支持时，才按其独立 Profile/version/facet 进行验收；缺失该声明是合法状态，不得用空壳 capability、占位 Schema 或 declaration-only 实现冒充通过。
