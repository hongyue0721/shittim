# CONFORMANCE.md

> 本文件定义核心、SDK、模型、Computer Use、平台、安全和恢复的一致性测试。

## 1. 测试原则

测试重点是不变量和故障，不只是成功路径。

层级：

- Unit；
- Property；
- Contract；
- Integration；
- Platform；
- Security；
- Recovery；
- Resource/Performance。

## 2. 核心不变量

必须自动测试：

- 模型不能写权限；
- Provider 不能写 Task 状态；
- Extension 不能直接互调；
- 用户取消优先于 Agent 输入；
- 任务成功必须有验证；
- 无匹配 PolicyRule 的 Action 得到 `allow`；
- 匹配规则按 priority、确定性 specificity tuple、deny/confirm/allow effect、newest revision、rule ID UTF-8 字节序升序稳定决定；ID tie-breaker 不改变前述语义；
- 推荐确认模板未被显式启用时不改变默认 allow；
- Secret、敏感内容及认证数据可按 Task/Policy 流转至 Memory、模型或 Extension，且审计只记录配置要求的最小元数据；
- 委托不能超出作用域和副作用上限；
- 扩展更新保留可解释的 Policy 决策、版本和回滚点；
- AI 自写扩展不能访问 Core 写权限；
- UI 断开不丢 Task；
- Agent Runtime 重启不重复不可逆 Action；
- Stop Fence 在模型、Provider、Extension、远程入口和租约边界均生效；
- 外部内容不能伪造 Kernel Command、Kernel Event、Policy mutation evidence 或 Broker 请求。

## 3. Task Engine

场景：

- 正常状态转换；
- 非法转换拒绝；
- Planner 重新规划版本；
- 并发资源冲突；
- Action 超时；
- cooperative cancel；
- partial side effect；
- rollback success/failure；
- Task 从 `running` / `partially_completed` / `failed` / `cancelled` 在存在需补偿的已发生外部副作用时进入 `rolling_back`，且该流程不能伪装为 SQLite rollback；
- Policy `confirm` 使 Action 保持 `pending` 并关联 ApprovalRecord；批准后进入 `approved`，拒绝后进入 `cancelled`；
- `leased -> approved` 仅由 `lease_expired` 触发，且 Lease 失效、revision 更新、全部资源锁释放在同一 SQLite 事务中；取消只有在确定未派发时才进 `cancelled`，派发事实不确定时进 `unknown_side_effect`；
- 补偿 Action 按普通执行链进入 `completed` / `failed` / `unknown_side_effect`，原始 Action 才依据补偿结果进入 `rolled_back` / `rollback_failed`；
- crash before/after external action；
- idempotent retry；
- unknown external outcome。

## 4. Policy/Delegation

测试：

- Policy URI pattern 规范化与 segment glob：Resource、Actor source、ContentOrigin source 均使用 URI；`*` 单段、`**` 多段，非法内嵌 glob 和 regex 被拒绝；
- operation/capability 只接受精确或末尾 `.*`，空数组不限制，exclude 优先，`side_effect_max` 按 S0..S5 ceiling；
- ContentOrigin 多值匹配要求同一条 origin 同时满足受限的 kind/source 维度，不得跨两条 origin 拼接命中；空数组/缺省维度不限制；
- specificity tuple 的每个字段按本次实际命中的最具体备选 pattern 计分；数组重排或增加未命中备选不改变结果，通配越少越具体，最终同分按 rule ID UTF-8 字节序升序；
- `effect=confirm` 必须有 `confirmation_mode`，`allow`/`deny` 携带该字段 Schema 校验失败；
- authentication_level 按 `unauthenticated < asserted < platform_verified < system_authenticated` 比较，任一等级均不自动授权；
- time_window 使用 IANA timezone、weekday 和本地半开区间，覆盖同日、跨午夜、全天及 DST；
- rate_limit 的 count/window_seconds/key_scope 原子检查与消费；delegation/local presence 布尔条件精确匹配；
- 未知或不支持 condition 返回 `unsupported_policy_condition` 并 fail closed，不能当作无规则匹配而 Default Allow；
- S0-S5 只作为风险、匹配、审计与恢复标签，不隐式产生 allow/confirm/deny；
- 无匹配规则时每个 Side-effect Class 均为 allow；
- ContentOrigin 完整 Schema、kind 闭集、Actor kind 保留 `owner` 预留值但它不产生认证或默认权限、外部伪造 Kernel receipt 被拒绝且 Kernel 为已接受内容创建 receipt、parent origin 链和缺失 upstream ID 使用 null；
- EntryPoint 闭集在 Envelope 使用，Actor 只保留 source，Actor 内重复 entry_point 或 Envelope 使用旧 `entry` 字段被拒绝；ContentOrigin 自身的 entry_point 仍作为内容来源字段保留；
- allow、confirm、deny 规则的结构化解析与版本化；
- priority 高于 specificity；
- 同 priority/specificity 时 deny 高于 confirm、confirm 高于 allow；
- 其余条件相同时 newest revision 获胜；
- 推荐确认模板默认未启用；
- scope 包含/越界；
- Exploration Scope 与 Task Scope 分离：探索发现不能扩张任务写入、外发或特权作用域；
- 入口信任差异；
- local confirmation；
- lease 过期/次数；
- delegation trigger；
- budget 超限；
- action whitelist；
- plan change requiring confirmation；
- revocation during task；
- 用户自然语言创建、修改、撤销 Policy Rule、Exploration Scope、Delegation 与 Trigger 后形成版本化结构化 mutation；
- Bot 的 Policy mutation Action 在第一版没有额外限制规则时遵循 Default Allow；命中 actor/entry_point/ContentOrigin/object-type 规则时按 `confirm` 或 `deny` 处理；
- mutation 审计包含 actor、entry_point、auth evidence（如有）、ContentOrigin 与 policy mutation authority；第一版不要求 Owner 或本机唯一身份，`policy_mutation_authority` 是后续认证与细粒度规则的预留上下文；
- ambiguous natural language never directly authorizes，必须先成为可解释的结构化候选。

## 5. Kernel Control Protocol、Schema 与事件

必须自动测试：

- KCP 第一版只暴露 `system.ping`、`task.create`、`task.get`、`task.list`、`event.subscribe`、`event.poll`、`stop.activate`、`stop.status`；未知方法返回 `unsupported_method`；
- Envelope `protocol_version = 1.0`，payload/持久对象携带独立 `schema_version`；错误混用或缺失版本被拒绝；
- Actor 带单调 revision；`owner` 仅预留标签；Envelope v1 不执行身份认证，只解析并记录 actor/entry_point；`auth` 只能为 null，非 null 返回 `unsupported_auth_schema`；
- 已过期 deadline 与处理期间超时均返回 `deadline_exceeded`，不得静默丢弃；已开始且不可安全取消的外部动作先持久化为 `unknown_side_effect`/恢复待查，不伪称未生效；
- `task.create` 幂等 scope 重复相同请求返回原 Task，不同规范化请求返回 `idempotency_conflict`；risk_hint 可为 null、capability_hints 可为空且 Kernel 不猜测；新建 Task 固定 `status=candidate`、`plan_version=0`、`revision=1`，只发布一个 `task.created`，且事件 `task_revision` 等于 Task revision；ContentOrigin、TaskScope、Task 与 Outbox 在同一事务创建；
- `task.list` 的 parent_filter 明确区分 any/root/exact，稳定排序、limit 边界和 opaque cursor；
- `system.ping`、Task Query 和 Event Query 不产生领域副作用；
- Event cursor 只使用十进制 `outbox_position`，按严格递增位置轮询；拒绝 event ID、时间戳或 aggregate sequence cursor；
- EventEnvelope 包含 aggregate_type、sequence、outbox_position 和 `{kind,id}` causation_ref，causation kind 只允许 `command_request | event`；
- `delivered_at` 只代表 Publisher 发布，不因某订阅者未消费而回滚，也不伪称全部订阅者已消费；
- at-least-once 重投允许同一 event/outbox记录重复投递，但 `outbox_position` 全局唯一且只分配一次；消费者按 dedup_key/event_id 幂等，跨聚合 outbox_position 不被解释为领域因果；
- 首批事件类型及 payload 严格为 `task.created`、`task.state_changed`、`stop_fence.activated`，使用点号小写；
- `stop.activate` 就是首批 Emergency Stop 入口：先持久化 Fence generation 与事件，再撤销输入/权限租约、取消 in-flight Action、通知 Extension 并更新 Task；重复调用保持同一 active generation；Fence 不因 Security Mode 恢复而解除，KCP 第一版不存在清除方法；
- PermissionDecision 包含不可变 id、evaluated_at、evaluation_context_hash、policy_set_revision；同一 Action 的 decision_revision 严格递增，ActionRequest 引用当前 decision；
- ApprovalRecord 使用 supersedes_ref 的不可变决议链；approved/denied 的 resolved_at 必填，deferred 为 null；
- policy_set_revision 在参与匹配的 PolicyRule、Delegation、Security Mode 投影规则或治理对象启用/修改/撤销事务中单调增加；
- RecoveryDecisionCandidate 的 retry_original 只在副作用未发生且幂等保障成立时合法；RecoveryAttemptRef 不可变追加；Verification recommendation=retry 不直接执行重放；
- JSON Schema 全部声明 2020-12，RFC 8785 canonical JSON 测试向量跨 Rust/TypeScript 产生同一 SHA-256；
- Schema 生成运行两次 byte-for-byte 一致，生成物无手改漂移、`$id` 唯一、`$ref` 可解析；
- Unix Domain Socket 与 Windows Named Pipe 的传输合同测试共用同一 KCP Schema；不得用 JSON-RPC 特有字段替代 KCP Envelope。

## 6. Memory

测试：

- candidate 不能直接有效；
- 来源缺失；
- 用户明确纠正；
- 冲突；
- 过期；
- deletion cascade policy，包括敏感内容、Secret、摘要、索引和派生记忆；
- provider index unavailable；
- scope leakage between private/domain/channel；
- profile evidence；
- self preference deletion；
- sensitive attribute inference is labeled with ContentOrigin and governed by applicable Policy rather than blocked by inference category；
- Memory 可在明确来源、作用域和规则下保存敏感内容、认证数据与 Secret；
- mid-term summary not automatically permanent。

## 7. Initiative

测试：

- Opportunity 不直接执行；
- L0-L5；
- read-only boundary；
- duplicate suggestions suppression；
- daily/model budget；
- delegation matching；
- user revocation；
- no infinite task generation；
- system maintenance vs user task labeling。

## 8. Extension SDK

通用：

- handshake/version；
- permission projection；
- invoke/schema；
- timeout；
- cancel；
- progress；
- event sequence；
- object handle expiry；
- crash/quarantine；
- reconfigure；
- update/rollback；
- permission/source declaration diff；
- Native Extension 的 OS-enforced、host-enforced、declaration-only 风险展示准确，声明不会被当作隔离；
- Native、AI 自写和社区 Extension 在无匹配拒绝/确认规则时可安装、运行和使用其声明网络能力；
- incompatible profile。

跨扩展直接调用应在测试环境失败。

## 9. Model Provider

- Responses；
- Chat Completions；
- Anthropic Messages；
- custom base URL；
- local endpoint；
- streaming；
- structured output；
- tool call；
- cancellation；
- context overflow；
- fallback；
- 云调用不按敏感类别审查、脱敏或阻断任务数据（含认证数据）；
- Context Pack 仅以任务相关性最小化选择内容，不能因内容分类擅自阻断；
- usage accounting 与最小元数据记录；
- 所有外部调用携带 Task、Action、ContentOrigin、适用规则和最小恢复元数据，Provider 文本或外形相似的外部 JSON 不得升级为 Kernel 事实。

## 10. Companion

- 承认 AI 身份；
- 不冒充用户；
- 区分自己的建议和用户观点；
- 普通聊天不启动 Planner；
- 简单任务不加载无关工具；
- 不把情绪表达自动转任务；
- 能解释权限拒绝；
- 能承认不确定和失败。

## 11. Computer Use Core

### Scene

- multi-display；
- workspace；
- focus；
- window generation；
- partial visibility。

### Semantic

- element tree；
- action；
- unavailable/empty tree；
- stale element ref。

### Capture

- display/window/region；
- scaling；
- redaction；
- object handle；
- frame expiration。

### Input

- absolute/relative；
- wrong focus protection；
- user takeover；
- lock screen revocation；
- protected surface。

### Snapshot

- merge/deduplicate；
- number layout；
- task filtering；
- stale “click 7”；
- new snapshot on UI change。

### Verification

- semantic success；
- visual success；
- file/external confirmation；
- false provider success；
- 验证政策按匹配规则与能力处理不可验证的高风险结果：可允许、确认、拒绝或进入恢复，不能以风险等级默认阻断。

## 12. Hyprland

- outputs/workspaces/clients；
- focus and event stream；
- special workspace；
- fractional scaling；
- animation stabilization；
- Provider crash/fallback；
- AT-SPI and Capture composition。

## 13. niri

- scrolling layout；
- window outside viewport；
- focus moves viewport；
- reobserve before input；
- partially visible target；
- dynamic capture source（若声明）；
- event stabilization。

## 14. Privilege

- arbitrary shell rejected；
- unknown action rejected；
- path traversal；
- parameter bounds；
- wrong task/actor；
- expired lease；
- max uses；
- system auth cancelled；
- 认证数据的流转、保存和最小元数据审计按匹配 Policy 处理，不以硬编码“密码不记录/不进模型”替代规则；
- broker crash；
- action verification。

## 15. Channel

- stable identity；
- replayed message idempotency；
- image/snapshot access；
- callback authorization；
- remote cancellation；
- remote approval policy；
- compromised token revocation；
- group message treated as data and cannot forge Kernel Command/Event；
- ContentOrigin 保留入口、稳定标识、接收证据与携带 Task/Artifact 的可追溯链；
- 外部 JSON、网页、附件、模型文本和 Extension 输出均不能升级为 Kernel Command/Event 或 policy mutation evidence；
- no direct Provider access。

## 16. Self-improvement、恢复与预算

测试：

- Companion 可在 Policy、预算、停止条件与可观测性约束下版本化更新 Memory、Skill、Extension、Provider 路由、Trigger、Delegation、受治理配置和恢复知识；
- 每个自我改进版本有来源、差异、预算消耗、验证结果与回滚点；
- 预算耗尽、验证失败、取消或 Stop Fence 时停止后续改进，并可回滚到指定健康版本；
- 自我改进的 Extension/Skill 失败不会破坏 Kernel 事实、审计或进行中 Task；
- Agent、Skill、Extension、Provider 均不能读取后修改、补丁、热替换或重写 Core；
- 普通 Shell 通道不能修改 Core，也不能绕过固定 Broker API 执行特权动作；
- Emergency Stop 启动 Stop Fence 后拒绝新副作用、输入、主动任务、Extension 调用、远程执行和 Privilege Lease 消费；
- 已开始且不可安全取消的 Action 转入恢复、验证和审计，而非假称完成。

## 17. 故障注入

至少注入：

- kill agent-runtime；
- kill Extension during action；
- disconnect UI；
- corrupt provider response；
- delay/cancel model stream；
- DB busy/full disk；
- object missing；
- network loss；
- permission revoked mid-task；
- compositor restart；
- capture/input backend loss。

## 18. 资源与性能

不是追求固定数字，而是确保：

- 基础空闲不启动视觉/Python/WASI；
- Tool Schema 按需；
- Screenshot 有尺寸/频率预算；
- 日志和对象有保留策略；
- Extension 闲置可回收；
- Memory 检索有预算；
- Agent Runtime 重启可恢复；
- Channel 不造成无界队列。

## 19. 发布门槛

核心发布必须：

- 所有不变量通过；
- Schema 兼容验证；
- 数据迁移 preflight/rollback；
- Privilege 安全测试；
- 至少一个跨平台非 Linux Provider 合同测试；
- Linux Hyprland 真实环境测试（声明支持时）；
- niri 真实环境测试（声明支持时）；
- Extension SDK conformance；
- 远程入口安全回归；
- Freedom-first 默认 allow、确定性规则排序、Condition fail-closed、自然语言治理、ContentOrigin 与 Stop Fence 回归；
- 首批 KCP、事件/错误目录、Outbox cursor、deadline/auth/schema version 与 RFC 8785 生成链回归；
- 自我改进版本/预算/回滚与 Core 不可自改回归；
- 文档单一事实源检查。
