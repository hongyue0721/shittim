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
- `task.create` 只对非 null origin.source_uri 与 TaskScope include/exclude URI pattern 做 Policy URI 规范化；顺序/重复保留，其他字符串不 trim/排序，规范化 payload 重新通过 TaskCreateRequest Schema；
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
- TaskScope resource containment 纯函数：include 空=不限制、exclude 优先、每个 resource 均须满足、resources 空在完整验证 patterns 后为 true；stored pattern 必须已规范化否则 `InvalidScopePattern`；非法 concrete URI 为 `InvalidResourceUri`；先完整验证全部输入，前面越界不得掩盖后面非法 URI；`Ok(true/false)` 只表示边界包含，不授权、不改 Scope；顺序/重复不影响结果且不修改数组；`*`/`**`/query/fragment 复用 Policy URI 语义；
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
- `task.create` 幂等 scope 精确为 `(actor.id, entry_point, command_type, idempotency_key)`；等价投影只含完整 actor revision 快照、entry_point、固定 command_type、Envelope task_id/context/expected_revision 与规范化完整 payload，明确排除 protocol/message/auth/request/deadline/key；RFC 8785 + SHA-256 同 hash 返回原 task_id 和当前 revision，不同 hash 返回 `idempotency_conflict`；记录与 Task 同生命周期、v1 不清理且无 processing 状态；
- `task.create` receipt content hash 精确覆盖规范化后的完整 TaskCreateRequest payload，不含 Envelope/Kernel IDs/时间/receipt/物化对象；共享复合 hash fixture `schemas/fixtures/kcp/task_create_normalized_hash.v1.json`（不是 schema-tool 通用 `$schema_id`/`instance` example wrapper）必须自动验证：`schema-tool validate` 校验其中 `normalized_payload`，实际 CLI `canonicalize --hash` 分别处理 `normalized_payload` 与 `idempotency_projection`，并与 Rust `sha256_canonical` 得到相同 lowercase hash；
- `task.create` 固定一个 accepted_at，并令 ContentOrigin received/receipt、TaskScope created/updated、Task created/updated、Audit occurred 与 Event occurred 全部等于该值；分别取时钟的实现不合格；
- `task.create` TaskScope 初值固定 UUID/schema1/revision1/task ID、请求数组原序保留、`source_refs=[新 origin id]` 不展开 parent、完整 actor+entry point created_by、请求 expires、双时间 accepted_at；ContentOrigin 的 UUID/entry/carrier/receipt/parent 投影同样逐字段校验；
- `task.create` 固定 `task.creation_recorded` producer：上层 Audit ID、严格 null/空数组、`reason_codes=["task_created"]`、`details={}`、唯一 origin ref、Task delegation、command causation、与 `task.created` 相同 correlation；与 Task canonical 子事实不一致必须使事务失败；
- `task.created` event ID/correlation/dedup 由 Kernel 上层显式提供，repository 不生成；sequence=0、唯一事件、command causation、payload 与 Task 精确一致；
- 非 null parent task 与每个 parent origin 必须存在；当前任何非 null delegation_ref 返回 `delegation_not_found`，Schema 仍允许非 null，正向 Delegation authority 路径明确未实现；
- `task.create` 幂等 scope 重复相同请求返回原 Task，不同规范化请求返回 `idempotency_conflict`；risk_hint 可为 null、capability_hints 可为空且 Kernel 不猜测；新建 Task 固定 `status=candidate`、`plan_version=0`、`revision=1`，只发布一个 `task.created`；ContentOrigin、TaskScope、Task、Audit、幂等记录与 Outbox 在同一事务创建；
- `task.list` 的 parent_filter 明确区分 any/root/exact，稳定排序、limit 边界和 opaque cursor；cursor 编码技术选择留待 repository 实现前的 ADR/API 拍板，本批不把任一编码写成事实；
- Value preflight 输入只接受已由调用方解析的 `serde_json::Value`；bytes、UTF-8、JSON parse、frame、最大尺寸、clock/backend/ID 不进入该层；优先加入现有 `kernel-kcp` 并复用 handler/ports/response 门，禁止复制 Catalog/response/handler abstraction；公开调用必须分成 `preflight_value -> narrow_to_registered -> TypedDispatcher.dispatch`，不得用一站式 `process_value` 暗示全 Catalog 可执行；
- preflight 严格短路优先级为 request_id response eligibility > message_kind/family > protocol > auth > family-specific method > 根 payload.schema_version > 完整 Envelope/方法 Schema + generated typed decode；测试必须构造多个同时错误的输入证明高优先级结果稳定胜出；
- 非 object，或顶层 request_id 缺失/非 string/非法 UUID，固定得到本地 `PreflightLocalRejection::UncorrelatableRequest { kind, message }` 且不产生 wire response；合法 UUID string 在所有 error response 中逐字原样保留；本地 ContractFailure 也只有固定 safe kind/message，不暴露内部 schema ID/detail；
- request_id 可关联后，message_kind 缺失/非 string/response/未知均为 `invalid_request`；family 确定后只解释对应 discriminator，另一 family discriminator 留给最终 Schema 作为未知字段；protocol 缺失/非 string为 `invalid_request`、string 非 `1.0` 为 `unsupported_protocol_version`；auth 缺失为 `invalid_request`、任意非 null为 `unsupported_auth_schema`；
- command 只按 generated command Catalog 检查 command_type，query 只按 generated query Catalog 检查 query_type；字段缺失/非 string为 `invalid_request`，不属于所选 family 为 `unsupported_method`，包括 query `task.create` 与 command `task.get` 等跨 family 错配；测试必须证明实现没有手写第二份八方法目录；
- payload missing/non-object，或根 schema_version missing/JSON number 非 i64/u64 integer（包括 `1.0` 与超出 i64/u64 可表示范围的数字）/<=0 为 `invalid_request`；根正 integer !=1 为 `unsupported_schema_version`；嵌套 schema_version 与其它业务字段/enum/format/unknown field失败只为 `invalid_request`；
- 完整 family Envelope Schema、方法 payload Schema 与 generated typed decode 对八方法逐项有 Accepted 用例；五个已知未实现方法也必须先完整 decode，畸形 payload 不得提前变成 KnownCatalogMethodNotImplemented；
- `kernel-contracts` 必须提供并测试结构化 `ContractFailureStage` 等价分类：`CallerSchemaValidation` 唯一映射 wire `invalid_request`；`WireDecodeAfterSchema`、`PayloadDecodeAfterSchema`、`GeneratedDiscriminatorMapping`、`SchemaCatalog` 均本地 ContractFailure；`UnknownSchema`/Catalog 与 post-Schema serde 失败逐项有定向测试，禁止匹配 ContractError/Schema/serde message；
- 五个已知但未注册方法 narrow 后得到不可序列化的本地 `KnownCatalogMethodNotImplemented`，不能成为 `KcpError`、`unsupported_method`、`method_unavailable` 或 `internal_error`；三个 registered variant 分别只路由 ping/create/get；
- dispatcher 构造只复用现有 `KernelClock`、`KernelIdGenerator`、`TaskApplicationBackend`，按 ping/create/get variant 传递所需端口，不创造平行接口；它不重复 Schema/deadline、不改写 `HandlerResult`；fake port 矩阵证明 response、ContractFailure 与 post-commit notification intents 均无损透传，family/discriminator/payload variant 错配不能进入 RegisteredRequest；
- 五类 preflight wire error 逐项断言固定 code/message、`schema_version=1`、`details=null`、`retryable=false`、protocol/message/status/payload/error互斥；最终 error envelope 必须通过不可替换 generated response Schema；crate-private fault seam证明最终门失败转本地 ContractFailure且不发送 wire response；
- public API/trait bounds 测试或 compile-fail 锚点证明 preflight response validator 不可注入、`PreflightLocalRejection` 与 `KnownCatalogMethodNotImplemented` 不实现 Serialize，且没有公开一站式全 Catalog 执行入口；
- 未实现五方法 handler、bytes/frame/transport/server 的情况下组合根必须拒绝启动 server；不得以“运行时返回 known-unimplemented”替代启动阶段完整性检查；
- typed application handler 输入必须已经通过对应 Envelope Schema、方法 payload Schema 与 typed decode；正常 dispatcher 路径还必须先完成三方法 registration narrow。现有公共 `handle_*` 的错配输入继续产生本地 InputMethodMismatch，但 handler 不得重复 protocol/auth/method/schema 分类，也不得把内部错误重新归为 `invalid_request`；
- `system.ping`、`task.create`、`task.get` handler 使用 fake backend/fake clock/deterministic fake ID generator 做库级矩阵；Value preflight/registration/dispatcher 同样只做不可连接库级测试，本阶段不得启动 Socket/Named Pipe 或构造可连接 server；
- 三方法第一次可观察操作是 clock 读取，并在任何 ID/backend 前按 `now >= deadline` 检查；入口已过期不访问 backend、不分配 ID；
- `system.ping` 复用第一次时间为 `kernel_time`，完成时第二次读 clock；`task.get` backend 后第二次读 clock；两者完成时到期均返回 `deadline_exceeded`；backend 错误与 deadline 同时出现时，成功读取到的到期结果优先，完成 clock 自身失败则 `internal_error`；
- `task.create` 第一次 clock 同时固定唯一 `accepted_at`；deterministic fake generator 必须证明恰好分配 Task/Scope/Origin/receipt/Audit/Event 六个合法、两两不同的 UUID 及独立非空 correlation/dedup，生产 UUID 版本不固定且不得从 caller 字段派生；handler 在 backend 前验证 UUID 格式与本次互异，失败为 `internal_error`；
- deadline 必须把 Envelope RFC 3339 文本与 clock UTC instant 按时间点比较，禁止字符串字典序；deadline 解析失败在任何 ID/backend 前映射 `internal_error`；
- `task.create` adapter 只通过 backend 高阶端口调用现有 repository；测试 spy 必须证明 handler 不复制 normalize/hash/Audit/Event，SQLite adapter 只在 `SqliteStore::with_write_transaction` 内调用 `create_task`，不暴露 transaction/SQL；typed Envelope 到 `TaskCreateEnvelopeFacts` / request / allocation 的字段映射逐项断言；
- backend error 使用 §5.10.2 的闭集分类；SQLite adapter 对当前每个 `StoreErrorCode` 穷举转换，同名公开分类或 `Internal`，新增 Store code 导致编译/测试更新，不允许 wildcard 或 message 匹配；
- Created 与 Replayed 都返回 backend 给出的当前 Task；Created 必须同时返回与本次 operation Event UUID 相等的 `committed_event_id`，adapter 不能证明绑定时返回 Internal；仅 Created 携带一个 `TaskCreatedCommitted {task_id,event_id}` post-commit notification intent，Replayed 无 intent；Created 后即使 response contract 失败成为本地 HandlerContractFailure 也必须保留 intent；notifier 在事务外，失败不改变 response、不回滚事实、不声称 delivered；
- `task.create` 事务内不做第二次 clock 读取、不轮询/取消；backend 的 Created/Replayed/错误返回后才第二次读 clock。完成到期优先返回 `deadline_exceeded`；完成 clock 失败优先 `internal_error`；未到期才映射 backend 结果。Created post-commit 到期/clock failure 均保留事实与 intent；同一幂等键重放返回当前 Task；
- `task.get` 的 `None` 精确映射 `task_not_found`；Created/Replayed/get-found/not-found 均有独立用例；
- 对三方法逐项测试 stable error mapping，不匹配 message：`invalid_scope_pattern`、`idempotency_conflict`、`delegation_not_found`、`parent_task_not_found`、`parent_origin_not_found`、`sqlite_busy`、`sqlite_full`、`sqlite_corrupt`、`stored_data_invalid`，以及 constraint/contract/serialization/not-found/internal/open/config/migration 等折叠 `internal_error`；断言固定 safe message、`details=null` 与 retryable。deadline 和 sqlite_busy 为 true，其余本矩阵为 false；
- 每个成功 payload 在装 Envelope 前用原方法 response Schema 验证；每个最终成功/错误 Response Envelope 再用通用 Schema 验证，并断言 request_id 原样、protocol `1.0`、message `response`、success/error 互斥；Response 无 method discriminator，测试按原方法选择 payload Schema；
- 构造成功 payload 的 response Schema 失败必须安全转换为 `internal_error`；最终 error Envelope 若也无法验证则产生本地 HandlerContractFailure，不发送未验证响应；Created 路径必须证明该 failure 仍向组合根返回 post-commit intent；
- Event cursor 只使用十进制 `outbox_position`，按严格递增位置轮询；拒绝 event ID、时间戳或 aggregate sequence cursor；
- EventEnvelope 包含 aggregate_type、sequence、outbox_position 和 `{kind,id}` causation_ref，causation kind 只允许 `command_request | event`；同一聚合首条已提交事件 `sequence = 0`，后续已提交事件严格连续 `+1`，事务回滚的暂分配不占号；
- `delivered_at` 只代表 Publisher 发布，不因某订阅者未消费而回滚，也不伪称全部订阅者已消费；
- `mark_delivered` 是 public 业务写 convenience，必须委托统一 `BEGIN IMMEDIATE` / `with_write_transaction`；只有 `COMMIT` 成功后才允许返回 `Marked | AlreadyMarked | NotFound`。transaction-bound crate-private helper 在外层主动 `Err` 或 panic 时必须 rollback，公共重试可再次 `Marked`；unhealthy store 上 public mark fail closed 且另一健康 store 确认未改变；writer contention 映射 `sqlite_busy`，释放后 retry 得到 `Marked`；两个独立 store/connection 争同一 position、不同 timestamp 时，成功路径恰好一个 `Marked` 与一个 `AlreadyMarked`，DB 时间等于 winner 传入值且 loser 不覆盖；
- at-least-once 重投允许同一 event/outbox记录重复投递，但 `outbox_position` 全局唯一且只分配一次；消费者按 dedup_key/event_id 幂等，跨聚合 outbox_position 不被解释为领域因果；
- AuditRecord v1 是 `agentd` 拥有的不可变本地事实，不带 revision；全部字段 required，无关联事实使用显式 null/空数组；未知字段、未知 audit_type、`policy_context` 未知字段和非 `system_internal` 的 null actor 被 Schema 拒绝；`task.creation_recorded` 必须有 UUID task_id 与严格创建快照（revision=1、goal、origin、proposer），其他 audit_type 的该快照必须为 null；Actor 非空时必须保存完整 revision 快照；
- AuditRecord 的稳定引用闭包必须结构化回答任务创建原因、Delegation、模型建议/推理、VerificationResult、修改资源、是否外发、回滚能力、Stop Fence/恢复影响，以及匹配规则、排序依据、policy mutation authority 与 auth evidence；`not_sent` 拒绝非空 manifest refs，`sent` 的 producer 至少提供 content origin/artifact/resource/model call/payload manifest/causation 支撑，`unknown` 必须有 reason code；ModelCallRecord/PayloadManifest/Delegation 当前只使用非空 stable ref，不声称已有 source Schema 或 UUID；
- `permission_decision_ref` 非空时 `policy_context` 必须非空；未来 Audit repository 必须将 nullable matched_rule_ref、policy_set_revision 与不可变 PermissionDecision 比对，失配使 Audit/业务/Outbox 同事务回滚；该跨对象一致性当前只有 Conformance 契约，没有 SQLite 实现或自动化测试；
- `rollback_capability` 必须由 ActionRequest.rollback_policy、Verification、Recovery 权威事实投影且不可独立编辑；事实缺失/不可判定用 unknown，可解析事实冲突使事务失败；`provider_id` 表示实际操作 Provider，`model_call_refs` 表示建议/推理参与者，二者可并存；同一模型操作的 provider 一致性属于未来 repository 检查；
- AuditRecord 不自动成为首批公开 Event、不自动进入 Outbox；业务契约要求审计时，业务事实、AuditRecord 与该业务事实要求的 Outbox 在同一事务提交，Schema/跨对象一致性/插入失败整体回滚；固定归因必须有顶层 required 字段，不能仅藏进 details，但 Schema 无法完全禁止开放 details 重复这些值；正文默认最小记录但不硬禁 Secret；
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
