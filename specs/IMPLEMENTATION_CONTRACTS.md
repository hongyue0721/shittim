# IMPLEMENTATION_CONTRACTS.md

> 本文件定义仓库组织、模块边界、Kernel Control Protocol、参考对象、编码契约和逻辑流程。示例是契约表达，不要求逐字采用代码形式。

## 1. Monorepo

建议：

```text
/
  AGENT.md
  PROJECT_OVERVIEW.md
  specs/
  adr/
  schemas/
  rust/
    agentd/
    crates/
      domain-task/
      domain-policy/
      domain-memory/
      domain-initiative/
      extension-supervisor/
      extension-protocol/
      provider-registry/
      computer-core/
      audit-store/
      object-store/
      platform-common/
  ts/
    agent-runtime/
    desktop-client/
    sdk-typescript/
    mcp-bridge/
  sdk/
    rust/
    typescript/
    python/
    conformance/
  providers/
    linux/
    windows/
    macos/
  workers/
    memorix/
    vision/
  extensions/
    official/
  tests/
```

目录可以调整，但依赖方向必须保持。

## 2. Rust 模块

### domain-task

纯领域：Task、Step、Action、状态机、不变量、Recovery 元数据。

### domain-policy

权限、Side-effect、Approval、Lease、Delegation Match、PolicyRule。

### domain-memory

记忆事实、来源、冲突、候选提交和召回政策。

### domain-initiative

Opportunity、Candidate、预算和调度决定。

### extension-supervisor

进程生命周期、握手、权限装配和健康。

### extension-protocol

Schema、消息、错误、Object Handle。

### provider-registry

Profile、能力档案、选择和降级。

### computer-core

统一桌面模型、Target Resolver、Snapshot 和 Verification。

### audit-store/object-store

审计事实和大对象生命周期。

领域模块不得依赖 UI、Pi 或具体平台 API。

## 3. TypeScript agent-runtime

建议内部模块：

- identity prompt builder；
- model provider adapters；
- role registry；
- context pack builder；
- companion session；
- planner；
- worker runner；
- memory candidate extractor；
- skill/extension author；
- capability discovery client；
- kernel protocol client。

所有现实动作通过 Kernel Client 请求。

## 4. desktop-client

主要视图：

- Conversation；
- Task Center；
- Operation Snapshot；
- Approval Center；
- Memory & Persona；
- Initiative & Delegation；
- Skill/Extension；
- Model Routing；
- Platform Capability/Doctor；
- Audit/Recovery。

UI 不拥有 Task 真相，只订阅并发出 Command。

## 5. Kernel Control Protocol

Kernel Control Protocol（KCP）是 agent-runtime、desktop-client 及其他内部客户端与 agentd 通信的唯一协议。它与 Extension RPC **分开**，后者是 agentd 与 Extension/Provider 进程之间的协议。

### 5.1 设计原则与版本

- Versioned：Envelope 带 `protocol_version`；第一版固定为 `"1.0"`，不支持的版本返回 `unsupported_protocol_version`；
- Schema：Command、Query、Response 与 Event 的 `payload` 都是类型化对象，并带整数 `schema_version`；持久对象同样带 `schema_version`；
- Request/Response：每个 Command 和 Query 有 `request_id`，响应匹配；Command/Query Envelope 自身不带 `schema_version`，只带 `protocol_version`，其 payload 单独带 `schema_version`；
- Actor/Entry：Actor 保留身份来源 `source`，调用入口只出现在 Envelope 的 `entry_point`；Actor 内不得重复 EntryPoint。ContentOrigin 的 `entry_point` 是该内容自身的来源字段，不属于 Actor，也不替代 Envelope 当前调用入口；
- Deadline：每个请求必须带 RFC 3339 UTC `deadline`。agentd 开始处理时若已过期，必须返回 `deadline_exceeded`；处理期间超过 deadline 必须取消可取消工作并返回同一错误。若外部动作已开始且不能安全取消，先按 `CORE_ARCHITECTURE.md` 持久化为 `unknown_side_effect`/恢复待查，再返回 `deadline_exceeded`；不得静默丢弃或伪称没有副作用；
- Idempotency：Command 必须带 `idempotency_key`，其 scope 由具体方法定义；Query 不携带幂等键；
- Expected Revision：修改既有并发对象的 Command 按方法要求携带 `expected_revision`；不匹配返回 `revision_conflict`；
- Auth v1：Envelope 的 `auth` 必须为 JSON `null`。任何非 null 值返回 `unsupported_auth_schema`，不得假装已认证；
- Cursor：首批 Event cursor 是十进制字符串编码的 `outbox_position`；其他列表 cursor 是 Kernel 生成的不透明字符串；
- 未知方法：不在当前 Catalog 的 `command_type` / `query_type` 返回 `unsupported_method`，不得当作 `invalid_request` 或静默忽略。

### 5.2 EntryPoint 与 Actor

`EntryPoint` 第一版是闭集：

```text
local_desktop
local_ipc_client
agent_runtime
personal_remote_channel
group_channel
web_api
extension_originated_event
system_internal
```

Actor Schema：

```text
{
  schema_version: 1,
  revision: <positive integer>,
  id: <non-empty string>,
  kind: owner | known_user | guest | companion | system | extension,
  source: <non-empty stable source string>,
  authentication_level: unauthenticated | asserted | platform_verified | system_authenticated,
  confidence: <number 0..1> | null
}
```

- `source` 描述 actor 记录的来源系统或命名空间，例如平台账户域、Kernel 内部主体注册表或 Extension ID；它不是 EntryPoint；
- `owner` 是为未来 Owner/授权系统预留的 actor 类别；第一版可以保存或透传该标签，但它不是已认证结论，不能从本机入口、source 标签或该枚举值本身推导任何授权；
- 第一版不实现 Owner 身份认证或唯一 Owner 约束；未来认证必须使用独立版本化契约和 authentication evidence；
- `revision` 是 Actor 注册事实的单调版本；Envelope 携带 Actor 快照时必须包含该 revision；
- `authentication_level` 的顺序和无默认授权语义见 `SECURITY_PRIVILEGE.md`。

### 5.3 ContentOrigin Schema

所有进入 Kernel、模型、Memory、Extension 或审计链的内容引用以下对象：

```text
{
  schema_version: 1,
  id: <uuid>,
  kind: user_input | companion_generated | system_generated
      | remote_message | web_content | document_content
      | model_output | extension_output | provider_output | imported_data,
  entry_point: EntryPoint,
  source_uri: <normalized URI> | null,
  upstream_stable_id: <string> | null,
  producer_ref: { kind: actor | model | extension | provider | system, id: <string> },
  received_at: <RFC 3339 UTC timestamp>,
  carrier_ref: { kind: command_request | task | artifact | event, id: <string> },
  parent_origin_refs: [<ContentOrigin id>, ...],
  kernel_receipt: {
    receipt_id: <uuid>,
    content_hash: <lowercase sha256 hex>,
    recorded_at: <RFC 3339 UTC timestamp>
  }
}
```

约束：

- `entry_point: EntryPoint` 是 ContentOrigin 自身的接收来源字段，用于内容来源链和 Policy 匹配；它不属于 Actor。Envelope 同样有自己的 `entry_point`，用于当前 KCP 调用入口；两者语义不同，可以在派生/转发时不同，不要求相等；
- `source_uri` 存在时使用 `SECURITY_PRIVILEGE.md` 的 URI 规范化规则；
- `kernel_receipt.content_hash` 是接收内容的 RFC 8785 JCS UTF-8 字节 SHA-256 lowercase。具体 producer 必须定义被哈希 JSON 对象的精确边界；不得把 Envelope、Kernel ID、时间戳、receipt 自身或后续物化对象混入内容哈希。`task.create` 的精确边界见第 5.5 节；
- 上游没有稳定 ID 时必须为 `null`，不得生成并伪称上游事实；
- `kernel_receipt` 由 Kernel 创建；外部消息 Schema 不接受该字段，若输入出现同名字段必须返回 `invalid_request`，Kernel 随后为已接受内容自行创建 receipt；
- `parent_origin_refs` 表达派生来源，可为空；不得用它替代当前内容自身的 receipt；
- `carrier_ref.kind = command_request` 允许内容先于 Task 创建进入 Kernel，随后可由 Task/Artifact 记录反向引用该 origin。

### 5.4 Envelope

Command：

```text
{
  protocol_version: "1.0",
  message_kind: "command",
  request_id: <uuid>,
  actor: Actor,
  entry_point: EntryPoint,
  auth: null,
  task_id: <uuid> | null,
  context: <object> | null,
  deadline: <RFC 3339 UTC timestamp>,
  idempotency_key: <non-empty string>,
  expected_revision: <non-negative integer> | null,
  command_type: <KCP method name>,
  payload: { schema_version: <positive integer>, ... }
}
```

Query：

```text
{
  protocol_version: "1.0",
  message_kind: "query",
  request_id: <uuid>,
  actor: Actor,
  entry_point: EntryPoint,
  auth: null,
  task_id: <uuid> | null,
  deadline: <RFC 3339 UTC timestamp>,
  query_type: <KCP method name>,
  payload: { schema_version: <positive integer>, ... }
}
```

Response：

```text
{
  protocol_version: "1.0",
  message_kind: "response",
  request_id: <uuid>,
  status: "ok" | "error",
  payload: { schema_version: <positive integer>, ... } | null,
  error: {
    schema_version: 1,
    code: <machine code>,
    message: <human summary>,
    details: <object> | null,
    retryable: <boolean>
  } | null
}
```

`status = ok` 时 `payload` 非 null 且 `error = null`；`status = error` 时相反。未知 Envelope 字段按 Schema 兼容策略处理，未知方法、enum 或 payload schema 不得猜测映射。

### 5.5 首批正式 KCP Catalog

首批目录只包含下列八个方法。未列出的方法不是已承诺 API。

#### `system.ping`

- 属性：Query、只读、无副作用；
- Request payload：`{ schema_version: 1, echo: <string> | null }`；
- Response payload：`{ schema_version: 1, echo: <string> | null, kernel_time: <RFC 3339 UTC>, protocol_version: "1.0" }`；
- 错误：通用 Envelope 错误。

#### `task.create`

- 属性：Command；创建 Kernel Task 事实，不执行 Task 中的外部副作用；
- 接受时间：Kernel 在验证与事务开始前为本次接受固定一个 `accepted_at`，下述所有创建时间均使用这个值，不允许各对象分别取时钟；
- 规范化：只规范化非 null `origin.source_uri`、`task_scope.resource_patterns[]` 与 `task_scope.exclusions[]`，统一使用 `SECURITY_PRIVILEGE.md` 的 Policy URI 语法。数组顺序与重复项原样保留；其他字符串不 trim、不排序、不去重。规范化后的完整 payload 必须再次通过 `TaskCreateRequest` Schema，否则返回对应请求/Pattern 错误；
- 幂等 scope：精确为 `(actor.id, entry_point, command_type, idempotency_key)`；记录与 Task 同生命周期，v1 不清理。全本地创建在一个 SQLite 事务中完成，不引入 `processing` 状态；
- 幂等等价投影是精确 JSON object：`{ actor, entry_point, command_type: "task.create", task_id, context, expected_revision, payload }`。`actor` 是 Envelope 中完整 revision 快照，`task_id` / `context` / `expected_revision` 保留 Envelope 原值（含 null），`payload` 是规范化后的完整 `TaskCreateRequest`。投影排除 `protocol_version`、`message_kind`、`auth`、`request_id`、`deadline`、`idempotency_key`；按 RFC 8785 JCS UTF-8 + SHA-256 lowercase 得到等价哈希；
- 同 scope/key 已有记录且等价哈希相同，返回同一 `task_id` 与该 Task 的当前 revision；哈希不同返回 `idempotency_conflict`。不得把 deadline、request ID 或认证承载差异误判成不同业务创建；
- Request payload：

```text
{
  schema_version: 1,
  proposer: user | companion | system,
  goal: <non-empty string>,
  constraints: [<string>, ...],
  success_criteria: [<non-empty string>, ...],
  risk_hint: <string> | null,
  capability_hints: [<capability id>, ...],
  task_scope: {
    schema_version: 1,
    resource_patterns: [<normalized URI pattern>, ...],
    exclusions: [<normalized URI pattern>, ...],
    allowed_capability_hints: [<capability id>, ...],
    expires_at: <RFC 3339 UTC> | null
  },
  delegation_ref: <uuid> | null,
  parent_task_id: <uuid> | null,
  origin: {
    schema_version: 1,
    kind: <ContentOrigin kind>,
    source_uri: <normalized URI> | null,
    upstream_stable_id: <string> | null,
    producer_ref: { kind: actor | model | extension | provider | system, id: <string> },
    parent_origin_refs: [<ContentOrigin id>, ...]
  }
}
```

Kernel 必须先验证引用，再在同一 SQLite 事务中创建幂等记录、完整 ContentOrigin、TaskScope、Task、固定 `task.creation_recorded` AuditRecord 与唯一 `task.created` Outbox 记录；任何 Schema、引用或 canonical 子事实一致性失败均整体回滚。请求不得提交 Kernel-owned 字段，这样首批目录无需另造 `origin.create` 或 `task_scope.create` API。

引用规则固定为：非 null `parent_task_id` 必须已存在，否则 `parent_task_not_found`；每个 `origin.parent_origin_refs[]` 必须已存在，否则 `parent_origin_not_found`，且持久化时按请求顺序与重复原样保存；当前没有 Delegation authority repository，任何非 null `delegation_ref` 均返回 `delegation_not_found`。该正向路径尚未实现，但 Schema 继续允许非 null，不能用 Schema 禁止值来伪装引用校验。

`ContentOrigin` 物化固定为：Kernel 上层分配 UUID `id` 并交给 repository 校验/写入；`schema_version = 1`；请求的 `kind`、规范化 `source_uri`、`upstream_stable_id`、`producer_ref`、`parent_origin_refs` 原样投影；`entry_point = Envelope.entry_point`；`received_at = accepted_at`；`carrier_ref = { kind: command_request, id: Envelope.request_id }`；Kernel 上层同样分配 UUID `kernel_receipt.receipt_id`，repository 只接收、校验并写入，`recorded_at = accepted_at`。`kernel_receipt.content_hash` 对**规范化后的完整 TaskCreateRequest payload JSON object**执行 RFC 8785 JCS，取 UTF-8 字节的 SHA-256 lowercase；它包含 payload 的全部字段（包括 `schema_version`、`proposer`、`goal`、scope 与 origin），不包含 Envelope、request/deadline/idempotency/auth/entry point/actor、Kernel IDs/时间、receipt 或任何物化对象。复合 hash fixture 见 `schemas/fixtures/kcp/task_create_normalized_hash.v1.json`；它不是 schema-tool 通用 `$schema_id`/`instance` example wrapper。

`TaskScope` 物化固定为：Kernel 上层分配 UUID `id`，repository 只接收、校验并写入；`schema_version = 1`、`revision = 1`、`task_id =` 新 Task ID；规范化后的 `resource_patterns` / `exclusions` 以及请求的 `allowed_capability_hints` 均保持数组顺序和重复；`source_refs` **恰好**为 `[新 ContentOrigin.id]`，不展开 parent origins；`created_by = { actor: Envelope.actor 完整快照, entry_point: Envelope.entry_point }`；`expires_at` 使用请求值；`created_at = updated_at = accepted_at`。

`TaskSpec` 物化固定为：Kernel 上层分配 UUID `id`，repository 只接收、校验并写入；`origin_ref` 与 `task_scope_ref` 分别引用上述新对象；`actor = Envelope.actor` 完整 revision 快照；规范化 payload 中的 `proposer`、`goal`、`constraints`、`success_criteria`、`risk_hint`、`capability_hints`、`delegation_ref`、`parent_task_id` 精确投影；`status = candidate`、`plan_version = 0`、`schema_version = 1`、`revision = 1`、`created_at = updated_at = accepted_at`、`failed_recovery_meta = null`。请求中的 null、空数组、顺序、重复和空白均不得由 Kernel 猜测或整理。

`task.creation_recorded` 是该创建事务的固定 Audit producer：上层 Kernel 显式分配 Audit UUID；`schema_version=1`、`audit_type=task.creation_recorded`、`level=user_activity`、`actor=Envelope.actor`、`entry_point=Envelope.entry_point`、`occurred_at=accepted_at`、`task_id=新 Task.id`；`task_creation_context={ task_revision:1, goal:Task.goal, origin_ref:Task.origin_ref, proposer:Task.proposer }`；`action_id`、`permission_decision_ref`、`approval_record_ref`、`recovery_attempt_ref`、`extension_id`、`provider_id`、`stop_fence_generation`、`policy_context`、`summary` 均为 null；`delegation_ref=Task.delegation_ref`；model/payload manifest/verification/artifact/resource 数组为空；`external_content_status=not_sent`；`content_origin_refs` 恰好为 `[新 ContentOrigin.id]`；`causation_ref={ kind:command_request, id:Envelope.request_id }`；`correlation_id` 必须与同事务 `task.created` Event 完全相同；`rollback_capability=unknown`、`outcome=succeeded`、`reason_codes` 恰好为 `["task_created"]`、`details={}`。Audit 与 Task canonical 子事实不一致时事务失败，不能以 Audit 默认值掩盖不一致。

`task.created` producer 不生成 caller-owned 标识：`event_id`、`correlation_id`、`dedup_key` 必须由 Kernel 上层显式提供；repository 只校验并写入。事件固定 `aggregate_type=task`、`aggregate_id=Task.id`、`sequence=0`、`occurred_at=accepted_at`、`causation_ref={ kind:command_request, id:Envelope.request_id }`；payload 的 `task_id`、`status`、`proposer`、`goal`、`task_revision`、`created_at` 必须与 Task 精确一致。创建事务只允许这一条 Event，不同时发送 `task.state_changed`。

- Response payload：`{ schema_version: 1, task: TaskSpec }`；
- 错误：`invalid_request`、`invalid_scope_pattern`、`delegation_not_found`、`parent_task_not_found`、`parent_origin_not_found`、`idempotency_conflict`、通用错误。

#### `task.get`

- 属性：Query、只读；
- Request payload：`{ schema_version: 1, task_id: <uuid> }`；
- Response payload：`{ schema_version: 1, task: TaskSpec }`；
- 错误：`task_not_found`、通用错误。

#### `task.list`

- 属性：Query、只读；
- Request payload：

```text
{
  schema_version: 1,
  statuses: [TaskStatus, ...],
  parent_filter: {
    mode: any | root | exact,
    task_id: <uuid> | null
  },
  proposer: user | companion | system | null,
  created_after: <RFC 3339 UTC> | null,
  cursor: <opaque string> | null,
  limit: <integer 1..200>
}
```

空 `statuses` 表示不按状态限制。`parent_filter.mode = any` 不限制父级，`root` 只返回 `parent_task_id = null`，`exact` 要求 `task_id` 非 null 且只返回该父 Task 的直接子 Task；其他 mode 下 `task_id` 必须为 null。`proposer = null` 表示不限制；`created_after` 是严格大于。结果按 `(created_at desc, id asc)` 稳定排序。cursor 仍是 Kernel 生成的不透明字符串；其编码与分页键的技术选择不在本批契约中拍板，Task repository 实现前必须在 ADR 或 API 契约中明确。

- Response payload：`{ schema_version: 1, tasks: [TaskSpec, ...], next_cursor: <opaque string> | null }`；
- 错误：`invalid_cursor`、通用错误。

#### `event.subscribe`

- 属性：Query、只读；创建连接级临时订阅句柄，不创建领域副作用；
- Request payload：

```text
{
  schema_version: 1,
  event_types: [<catalog event type>, ...],
  aggregate_types: [<string>, ...],
  after_outbox_position: <decimal string> | null
}
```

空过滤数组表示不限制；`after_outbox_position = null` 表示从订阅建立后新分配的位置开始，不回放历史。

- Response payload：`{ schema_version: 1, subscription_id: <uuid>, next_outbox_position: <decimal string> }`；
- 错误：`invalid_cursor`、`unsupported_event_type`、通用错误。

#### `event.poll`

- 属性：Query、只读；长轮询同样受 Envelope deadline 约束；
- Request payload：`{ schema_version: 1, subscription_id: <uuid>, after_outbox_position: <decimal string>, limit: <integer 1..500> }`；
- Response payload：`{ schema_version: 1, events: [EventEnvelope, ...], next_outbox_position: <decimal string> }`；
- 只返回 `outbox_position > after_outbox_position` 的事件并按位置升序排列；空结果合法；
- 错误：`subscription_not_found`、`invalid_cursor`、通用错误。

#### `stop.activate`

- 属性：Command；Kernel Recovery Invariant，不通过普通 Policy 获得或拒绝授权；不得由外部内容伪造；
- 幂等 scope：全局 Stop Fence generation。Fence 已激活时重复调用返回当前同一 generation，不重复产生状态转换事件；
- Request payload：`{ schema_version: 1, reason: <non-empty string>, origin_ref: <ContentOrigin id> | null }`；
- Response payload：`{ schema_version: 1, active: true, generation: <positive integer>, activated_at: <RFC 3339 UTC>, activated_by: Actor }`；
- 错误：`origin_not_found`、通用错误。

#### `stop.status`

- 属性：Query、只读；
- Request payload：`{ schema_version: 1 }`；
- Response payload：

```text
{
  schema_version: 1,
  active: <boolean>,
  generation: <non-negative integer>,
  activated_at: <RFC 3339 UTC> | null,
  activated_by: Actor | null,
  reason: <string> | null
}
```

- 错误：通用 Envelope 错误。

首批 KCP **不提供**清除 Stop Fence 的方法。解除停止需要未来独立恢复契约、状态转换和测试，不得复用 `stop.activate` 参数或私有开关。

### 5.6 首批正式 Event Catalog

EventEnvelope 字段与 Outbox 语义以 `CORE_ARCHITECTURE.md` 为准。首批 payload：

#### `task.created`

- `aggregate_type = "task"`，`aggregate_id = task_id`；
- payload：`{ schema_version: 1, task_id: <uuid>, status: TaskStatus, proposer: user | companion | system, goal: <string>, task_revision: <positive integer>, created_at: <RFC 3339 UTC> }`；
- `task_revision` 必须等于该事件所描述的 `TaskSpec.revision`；首批 `task.create` 中固定为 `1`；
- 首批 `task.create` 的 `event_id`、`correlation_id`、`dedup_key` 由 Kernel 上层显式提供，repository 不得自行生成；`sequence=0`、`occurred_at=Task.created_at`、causation 为创建 Command request ID，payload 与 Task canonical 子事实不一致时整笔创建事务失败。

#### `task.state_changed`

- `aggregate_type = "task"`，`aggregate_id = task_id`；
- payload：`{ schema_version: 1, task_id: <uuid>, from_status: TaskStatus, to_status: TaskStatus, task_revision: <positive integer>, reason_code: <non-empty string>, changed_at: <RFC 3339 UTC> }`。

#### `stop_fence.activated`

- `aggregate_type = "stop_fence"`，`aggregate_id = "global"`；
- payload：`{ schema_version: 1, generation: <positive integer>, reason: <string>, activated_by_actor_id: <string>, activated_from_entry_point: EntryPoint, activated_at: <RFC 3339 UTC> }`。

事件类型必须精确匹配点号小写名称。新增类型先增加 Schema、Catalog、兼容说明与 Conformance 锚点。

### 5.7 首批错误目录

所有方法均可返回：

- `invalid_request`：Envelope 或 payload 不满足 Schema；
- `unsupported_protocol_version`；
- `unsupported_schema_version`；
- `unsupported_method`；
- `unsupported_auth_schema`；
- `deadline_exceeded`；
- `revision_conflict`；
- `idempotency_conflict`：同 scope/key 对应不同规范化请求；
- `stop_fence_active`：命令将创建普通新副作用而被 Fence 拦截；
- `unsupported_policy_condition`：Policy 含未知或当前实现不支持的 condition，必须 fail closed；
- `internal_error`：未分类 Kernel 错误，不得泄漏 Secret。

方法专属错误见各条目。是否可重试由 `error.retryable` 明示，客户端不得只凭 code 猜测。

### 5.8 本地传输边界

首批 KCP 只要求本机 IPC：Unix 使用 Unix Domain Socket，Windows 使用 Named Pipe。帧边界、连接凭据获取和具体序列化承载由 `adr/0003-kcp本地传输.md` 记录为实施选择。KCP 的领域方法与 Envelope 不等同于 JSON-RPC；在实施 ADR 和 Schema 明确前，不得把 JSON-RPC、HTTP 或 TCP 写成 KCP 已选事实。

### 5.9 Extension RPC（区分）

Extension RPC 是 agentd 与 Extension/Provider 进程之间的协议，不在本节定义（见 `extension-protocol` crate 和 `EXTENSION_SDK.md`）。Extension RPC 有自己的消息格式、错误模型和生命周期管理，不与 KCP 共享 envelope；其现有“JSON-RPC 风格”描述也不构成 KCP 的传输决策。

## 6. 参考逻辑对象

### 6.1 持久与并发对象标记

- 所有持久对象必须有 `schema_version`（整数，用于迁移）；
- 可并发修改的对象必须有 `revision`（单调递增，用于乐观并发控制）；
- `schema_version` 变更规则见第 13 节。

### 6.2 Actor

Actor 的规范 Schema、EntryPoint 分离规则和 authentication_level 枚举见第 5.2 节。持久对象引用 Actor 时保存完整快照或稳定 `actor_ref + actor_revision`，不得在 Actor 内重复 `entry_point`。

### 6.3 TaskSpec

`TaskSpec.origin_ref` 必须引用第 5.3 节的 ContentOrigin；`actor` 使用第 5.2 节 Actor Schema。

```text
id
origin_ref
actor
proposer: user | companion | system
goal
constraints[]
success_criteria[]
risk_hint?             // null 表示调用方没有提供风险提示，Kernel 不推断
capability_hints[]
delegation_ref?
task_scope_ref
parent_task_id?          // Subtask 指向父 Task
status: TaskStatus       // 见 CORE_ARCHITECTURE 第 10 节
plan_version
schema_version
revision
created_at / updated_at
failed_recovery_meta?    // { attempted: bool, last_attempt_at?, failure_reason?, recovery_attempt_refs[] }
```

### 6.4 PlanStep

```text
step_id
task_id
plan_version
seq_index                // 步骤顺序
intent                   // 自然语言描述
capability_refs[]        // 引用的能力 ID
resource_refs[]          // 预期资源
constraints[]            // 步骤级约束
depends_on_steps[]       // 前置步骤 ID
rollback_step_ref?       // 关联的回滚步骤
status: planned | in_progress | completed | skipped | failed
action_ids[]             // 该步骤产生的 Action ID 列表
created_at / updated_at
```

### 6.5 ActionRequest

`ActionRequest.permission_decision_ref` 在 Action 初始 `pending` 且尚未完成首次评估时可以为 `null`；每次评估（包括 confirm/deny）完成后必须立即写入对应 PermissionDecision ID。Action 进入 `approved` 前该引用必须非 null 且指向 `allow` 或已由有效 ApprovalRecord 满足的 confirm decision。

```text
action_id
task_id
step_id?
parent_action_id?        // 补偿 Action 引用原始 Action
capability_id
operation
structured_arguments
resource_refs[]
task_scope_ref
side_effect_class: S0 | S1 | S2 | S3 | S4 | S5
idempotency_key
permission_decision_ref? // 初建 pending 时可空；首次评估后必填，进入 approved 前必须有效
approval_record_ref?    // confirm 时 pending Action 关联的 ApprovalRecord
verification_policy: { strategy, expected_outcome, timeout }
rollback_policy?: { compensatable, compensation_action_ref?, auto_rollback_on }
status: ActionStatus     // 见 CORE_ARCHITECTURE 第 11 节
recovery_meta?: {        // Recovery 元数据
  unknown_side_effect_at?
  recovery_attempted: bool
  recovery_decision_candidate_ids[]
  recovery_attempt_refs[]
  last_recovery_error?
}
lease?: {
  holder
  expires_at
  max_uses
}
schema_version
revision
created_at / updated_at
```

### 6.6 PermissionDecision

```text
id
schema_version
action_id
decision: allow | deny | require_confirmation | require_local_confirmation
         | require_system_authentication | require_plan_revision
reason_codes[]           // 匹配到的规则 ID 或 default_allow
matched_rule_ref?        // 命中的 PolicyRule ID
approval_type?: implicit | user_confirm | local_confirm | system_auth | delegation
granted_scopes[]
binding: {               // 决策绑定，防止权限漂移
  action_id
  plan_version
  resource_refs[]
  key_params_hash        // RFC 8785 canonical JSON 后 SHA-256；参数变化时决策失效
}
decision_revision        // 同一 action_id 内从 1 开始严格单调递增，不得复用或回退
evaluated_at             // RFC 3339 UTC
evaluation_context_hash  // 对规范化完整判定输入做 RFC 8785 + SHA-256
policy_set_revision      // 本次评估使用的有效 Policy 集合全局单调 revision；PolicyRule、Delegation、Security Mode 投影规则或其他参与匹配的治理对象启用/修改/撤销时在同一事务中 +1
expires_at?
lease_ref?
```

`PermissionDecision.id` 是不可变唯一 ID；每次重评估都创建新记录并使同 Action 的 `decision_revision + 1`。PolicyRule 或 Delegation 没有变化也不得复用旧 revision 冒充新评估。`ActionRequest.permission_decision_ref` 指向当前生效记录，绑定内容、上下文哈希或 policy set revision 失效时必须重新评估。

PermissionDecision 适配 Default Allow：无规则命中时 `decision = allow`，`reason_codes = ["default_allow"]`，`matched_rule_ref = null`。Policy condition 不支持属于评估错误，不得生成 Default Allow decision。

### 6.7 PolicyRule

引用 SECURITY_PRIVILEGE.md 为权限判定权威，此处定义 PolicyRule 存储字段：

```text
id
schema_version
revision
name / description
priority                 // 规则优先级，高优先级先匹配
enabled
actor_match: { kind?, source_patterns[]?, entry_point?, auth_level_min? }
content_origin_match: { kinds[]?, source_patterns[]? }
resource_match: { scope_patterns[], exclude_patterns[] }
action_match: { capability_ids[], operation_patterns[], side_effect_max? }
condition: {
  time_window?: { timezone, weekdays[], local_start, local_end }
  rate_limit?: { count, window_seconds, key_scope }
  delegation_required?: boolean
  local_presence_required?: boolean
}
effect: allow | confirm | deny
confirmation_mode?       // effect=confirm 时必填；其他 effect 禁止
expires_at?
created_by / updated_by  // actor + entry_point
created_at / updated_at
source: user_defined | companion_generated | system
```

排序、Pattern、Condition、Specificity 和 Default Allow 语义只由 `SECURITY_PRIVILEGE.md` 定义。`actor_match.source_patterns[]` 使用与 ContentOrigin source 相同的规范化 URI segment-glob 语法；空数组表示不限制。Schema 必须实施 `effect = confirm` 与 `confirmation_mode` 的条件约束。Read-only/Restricted 等命名 Mode 若产生限制，必须投影为可见 PolicyRule；Safe Recovery 与 Stop Fence 只在维护 Kernel 一致性和禁止未知副作用盲目重放的范围内作为不可覆盖 Recovery Invariant，不构成第二套通用权限矩阵。

### 6.8 ExplorationScope

```text
id
schema_version
revision
name
scope_type: read_for_task | background_index | long_term_memory | profile_inference
         | cross_domain_association | prohibited
paths[] | resource_patterns[]
exclusions[]
initiative_level: L0 | L1 | L2 | L3 | L4 | L5
expires_at?
created_at / updated_at
```

### 6.9 TaskScope

TaskScope 是一次 Task 的临时处理边界，不会回写或扩大 ExplorationScope：

```text
id
schema_version
revision
task_id
resource_patterns[]
exclusions[]
allowed_capability_hints[]
source_refs[]            // 用户输入、计划或其他 ContentOrigin
created_by               // actor + entry_point
expires_at?
created_at / updated_at
```

`TaskSpec` 必须引用 `task_scope_ref`；`ActionRequest.resource_refs[]` 必须落在对应 TaskScope 内，超出时作为新的 Policy 输入处理，不能静默修改长期 ExplorationScope。`task.create` 的首版初值固定为 `revision=1`、`source_refs=[新 origin id]`、完整 actor+entry point 的 `created_by`，并令 `created_at=updated_at=accepted_at`；详见第 5.5 节。

### 6.10 ApprovalRecord

```text
id
schema_version
approval_type: user_confirm | local_confirm | system_auth | delegation | implicit
target: { task_id?, action_id?, plan_step_id? }
actor
entry_point
decision: approved | denied | deferred
evidence_refs[]          // 系统认证 token 引用等
supersedes_ref?          // 不可变决议链，指向上一条 ApprovalRecord
expires_at               // 批准有效期
created_at
resolved_at?
```

`ApprovalRecord` 第一版采用不可变记录：`deferred`、`approved`、`denied` 每次决议均创建新 record，新记录通过 `supersedes_ref?` 指向前一记录，Action 原子更新 `approval_record_ref`。因此 ApprovalRecord 不做原地 revision 更新，也不接受 expected_revision；`resolved_at` 对 approved/denied 必填，对 deferred 为 null。

### 6.11 RecoveryDecisionCandidate

RecoveryDecisionCandidate 是未知或失败 Action 的结构化恢复选项，不是授权或执行结果：

```text
id
schema_version
revision
task_id
source_action_id
trigger: unknown_side_effect | failed | cancel_with_committed_effect | compensation_unknown
candidate_kind: verify_external_state | retry_original | compensate | continue_task | stop_task | mark_failed
proposed_action_request?   // 需要现实动作时的完整新 Action 草案
facts: {
  side_effect_confirmed: true | false | null
  original_idempotency_guaranteed: boolean
  external_query_available: boolean
  compensatable: boolean
}
rationale
status: proposed | selected | rejected | expired
permission_decision_ref?  // selected 且需要动作时必须存在
created_at / expires_at?
```

`retry_original` 只可在已确认原副作用未发生、原动作具有可验证幂等保障且不会违反 Stop Fence/Recovery invariant 时成为可选候选；否则 Schema 校验或领域校验必须拒绝。候选被选择后，需要现实动作的路径创建新的 ActionRequest，不得直接改写原 Action 结果。

### 6.12 RecoveryAttemptRef

Task 与 Action 通过不可变引用记录每次恢复尝试：

```text
id
schema_version
task_id
source_action_id
candidate_id
attempt_action_ids[]      // 查询、补偿等新 Action；可为空（如 mark_failed）
started_at
finished_at?
outcome: in_progress | recovered | not_recovered | inconclusive | cancelled
resulting_source_action_status?
verification_result_refs[]
error_code?
```

`RecoveryAttemptRef` 是恢复历史事实，不能覆盖或删除旧尝试；新的尝试使用新 ID。

### 6.13 VerificationResult

```text
id
schema_version
action_id
strategy_used
outcome: verified_ok | verified_failed | inconclusive
verifier_kind
observed_resource_refs[]
before_version?
after_version?
evidence_refs[]
confidence?
verified_at
observations[]: {
  check_type              // return_code | resource_state | snapshot_diff | external_query | user_confirm
  expected
  actual
  passed: bool
  evidence_ref?
}
side_effect_confirmed: bool | null   // null = 无法确认
recommendation: complete | retry | rollback | policy_decision_required
created_at
```

`recommendation = retry` 只表示 Verification 建议产生 `RecoveryDecisionCandidate`，不授权重放。适用条件是：验证结果为 `verified_failed` 或 `inconclusive`，并且恢复事实能证明副作用未发生，或重试由外部系统/Kernel 幂等键保证不会重复副作用。若 `side_effect_confirmed = true`，不得建议重试原 Action；若为 `null`，不可逆 Action 必须先查询外部状态，不能因 recommendation 直接重放。

### 6.14 EventEnvelope

```text
event_id
type                    // 点号小写，如 task.state_changed
schema_version          // EventEnvelope 持久记录 schema
aggregate_type
aggregate_id
sequence                // 首条已提交事件为 0；后续已提交事件严格连续 +1
outbox_position         // 全局单调投递位置，不代表跨聚合因果顺序
occurred_at
causation_ref: { kind: command_request | event, id }
correlation_id
dedup_key
payload                 // 类型化事件体，含自身 schema_version
```

同一聚合中，事务内暂时分配但最终回滚的 `sequence` 不占号；重试必须读取最后已提交序号重新分配。字段语义、cursor 与 `delivered_at` 发布语义见 `CORE_ARCHITECTURE.md`。

### 6.15 AuditRecord

AuditRecord 是 `agentd` 拥有的不可变本地持久事实，不带 `revision`，不接受原地更新。它不是 EventEnvelope，不自动加入首批公开 Event Catalog，也不自动进入 Outbox。

```text
id: <uuid>
schema_version: 1
audit_type: task.creation_recorded | command.accepted | permission.evaluated
          | kernel.invariant_blocked | event.published | recovery.recorded | config.changed
level: user_activity | operational | security | debug
actor: Actor | null
entry_point: EntryPoint
occurred_at: <RFC 3339 UTC>
task_id: <uuid> | null
task_creation_context: {
  task_revision: 1
  goal: <non-empty TaskSpec goal>
  origin_ref: <ContentOrigin uuid>
  proposer: user | companion | system
} | null
action_id: <uuid> | null
permission_decision_ref: <uuid> | null
approval_record_ref: <uuid> | null
recovery_attempt_ref: <uuid> | null
delegation_ref: <non-empty stable ref> | null
model_call_refs: [<non-empty stable ModelCallRecord ref>, ...]
payload_manifest_refs: [<non-empty stable PayloadManifest ref>, ...]
external_content_status: not_sent | sent | unknown
verification_result_refs: [<VerificationResult uuid>, ...]
content_origin_refs: [<ContentOrigin id>, ...]
artifact_refs: [<stable artifact ref>, ...]
resource_refs: [<stable resource ref>, ...]
extension_id: <stable id> | null
provider_id: <stable id> | null
causation_ref: { kind: command_request | event, id } | null
correlation_id: <non-empty string> | null
rollback_capability: compensatable | not_compensatable | unknown
stop_fence_generation: <integer >= 1> | null
policy_context: {
  matched_rule_ref: <non-empty stable ref> | null
  policy_set_revision: <integer >= 0> | null
  decision_ordering_summary: <non-empty string> | null
  policy_mutation_authority: <non-empty string> | null
  authentication_evidence_refs: [<non-empty stable ref>, ...]
} | null
outcome: succeeded | failed | blocked | deferred | observed
reason_codes: [<non-empty code>, ...]
summary: <non-empty string> | null
details: <object>
```

- `audit_type` v1 是闭集，提供可编码分类；列入闭集不等于对应生产路径已经实现；
- 全部 v1 字段均为 required；没有关联事实时使用显式 `null` 或空数组，区别“已知无事实”和“生产者漏字段”；
- `actor` 保存完整 revision 快照。Schema 只约束 null actor 仅可用于 `system_internal`；“确无可归因注册主体”是生产者必须证明的业务事实，不能声称由 Schema 证明；
- `delegation_ref` 与 `model_call_refs` 当前是非空 stable ref：Delegation 与 ModelCallRecord 虽已有规范名，但尚无 source Schema，不得假称 UUID 或已存在持久 Schema；
- `task.creation_recorded` 必须携带非 null UUID `task_id` 与完整 `task_creation_context`，其中 `task_revision=1`，其余字段从同一事务 TaskSpec/Task 复制为不可变创建快照；其他 audit_type 的 context 必须为 null。该快照固定回答“为什么创建任务”，不把 TaskSpec 复制到所有 AuditRecord；`task.create` producer 的其余固定字段、空数组/null 值、reason code 与 Event correlation 见第 5.5 节，任一 canonical 子事实失配均回滚；
- `external_content_status` 固定回答是否外发：`not_sent` 要求空 `payload_manifest_refs`；`sent` 必须至少由 content origin、artifact、resource、model call、payload manifest 或 causation 稳定引用之一支撑；`unknown` 必须有 reason code。PayloadManifest 目前只承诺 stable ref，不新增 source Schema或复制正文；
- `permission_decision_ref` 非空时 `policy_context` 必须非空；未来 repository 必须读取不可变 PermissionDecision，校验 nullable `matched_rule_ref` 与 `policy_set_revision` 完全一致，不一致则 Audit 写入失败并回滚事务。排序摘要、mutation authority 与 auth evidence 是补充审计快照，不替代 PermissionDecision；
- `rollback_capability` 不是独立可编辑结论，而是审计时从 `ActionRequest.rollback_policy`、Verification、Recovery 权威事实投影；明确可/不可补偿必须有权威事实，缺失或不可判定写 `unknown`，可解析事实冲突则写入失败；
- `provider_id` 是本次被审计操作实际使用的 Provider；`model_call_refs` 是提出建议或参与推理的 ModelCallRecord 引用，两者可同时存在且不互相替代。若引用对应本次模型操作，未来持久层校验 provider 一致；
- `policy_context` 是审计时上下文快照，不复制 PolicyRule；Schema 可以校验同一 AuditRecord 内 PermissionDecision ref 非空时 context 非空，但无法跨对象验证字段相等；
- 固定归因字段不得仅放入 `details`。Schema 通过 required 顶层字段要求显式事实并拒绝 `policy_context` 未知字段，但开放 `details` 无法完全禁止重复内容，生产者必须以顶层字段为准；
- `details` 是由日志配置/适用 Policy 控制的结构化扩展正文。默认只记录最小元数据；Secret、Token 与未脱敏正文不默认记录，但 Schema 不硬禁；
- 每当业务契约要求审计时，Kernel 必须先通过 AuditRecord Schema 校验，并在同一个 SQLite 事务中写入业务事实、AuditRecord 以及该业务事实要求的 Outbox 记录。Schema校验、跨对象一致性校验或 AuditRecord 插入失败必须使整个事务回滚；不得降级为只写业务事实或只写日志文本。当前仓库尚未实现 repository，不得声称上述跨对象检查已有代码或测试。

### 6.16 PayloadManifest

与 MODEL_RUNTIME.md 的规范名和字段语义一致。它描述一次模型调用实际候选 Payload 的对象集合，而不是通用对象存储清单：

```text
id
schema_version
task_id
subtask_id? / action_id?
role
intent_router_result_ref?
provider_id / model_id
context_pack_ref / context_pack_revision
objects[]: {
  type
  stable_ref
  revision?
  hash
  size_bytes
  media_type?
}
total_size_bytes
estimated_input_tokens
routing_config_revision
policy_evaluation_ref
created_at
```

Manifest 不保存 Payload 正文，也不承担审批或内容审查。

### 6.17 ModelCallRecord

```text
id
schema_version
manifest_id
task_id
call_type: plan | chat | memory_extraction | initiative | code_gen | verify
provider_request_id?
provider_id / model_id
endpoint_config_revision
started_at / finished_at
latency_ms
input_tokens / output_tokens
cost
result_status
output_object_ref? / output_hash?
stop_reason?
error_code?
retry_of? / fallback_of?
cancel_or_timeout_result?
```

`ModelCallRecord` 是 MODEL_RUNTIME.md 的规范名；不得另建 `ModelCall` 平行类型。

### 6.18 DelegationContract

```text
id
schema_version
revision
domain / scopes
actions                  // 动作白名单
side_effect_ceiling
trigger
approval_policy
budget
notification
verification
rollback
expiration
status: active | revoked | expired | superseded
version
created_by
created_at / updated_at
```

### 6.19 MemoryCandidate

```text
id
schema_version
kind
subject
content/value
scope
evidence_refs[]
confidence
sensitivity
suggested_expiration
reason
status: proposed | committed | rejected | superseded
```

### 6.20 CapabilityDescriptor

```text
id / profile / extension
summary
input / output schema
permissions
side_effect
reliability
cancellation
verification hints
cost / platform
```

### 6.21 OperationSnapshot

```text
snapshot_id
desktop_generation
task_id
frame_handles[]
window / context
candidates[]
transforms[]
created_at / expires_at
```

## 7. 消息路由逻辑

```text
normalize input
-> parse and record actor / entry_point / ContentOrigin（v1 auth 必须为 null，不执行 Envelope 身份认证）
-> load minimal session and relevant memory
-> deterministic command checks (stop, cancel, approvals)
-> Companion/Intent decision
-> chat reply OR Task candidate
-> Kernel validates Task
-> direct execution OR Planner
```

用户新增消息可能：

- 普通对话；
- 修改当前 Task 约束；
- 创建新 Task；
- 回答审批；
- 接管/取消；
- 委托候选确认。

不能全部追加到 Planner 对话末尾。

## 8. 计划逻辑

```text
Task context
-> capability catalog summaries
-> Planner structured plan
-> Kernel normalize and version
-> policy preflight
-> acquire resources per step
-> execute next action
-> verify
-> update/replan
```

Kernel 可以拒绝或拆分 Planner Step。

## 9. Action 执行逻辑

```text
assert task runnable
assert plan version
resolve resources
policy evaluate
approval/lease
acquire lock
persist action intent
invoke extension
persist raw result refs
observe/verify
commit action result
release lock
emit events
```

Extension 返回自然语言"成功"不跳过 verify。

## 10. Delegation Match

匹配时检查：

- active 且 version 最新；
- domain / scope 覆盖；
- actor / entry_point 匹配或包含；
- action 在白名单内；
- side_effect_class 不超过 ceiling；
- trigger 条件满足；
- budget 未耗尽；
- approval_policy 允许；
- expiration 未过期；
- required Skill/Provider version 满足。

### 10.1 越界处理

当 Action 超出 Delegation 白名单或 scope 时：

1. **不自动强制确认或拒绝**；
2. 将 Action 重新提交 Policy Engine 完整评估；
3. Policy Engine 按优先级匹配PolicyRule（包括其他 Delegation、用户全局规则、系统规则）；
4. 无规则命中时按 Default Allow 放行（`decision = allow`）；
5. Delegation 不是唯一授权来源；用户可能有其他匹配规则或默认允许覆盖。

任一超出即**不**做模糊相似授权，也**不**自动降级为"必须确认"，而是走标准 Policy 评估。

## 11. Memory Commit

```text
candidate
-> evidence available?
-> sensitivity/scope check (按 Policy 规则评估)
-> duplicate/conflict?
-> Policy 评估是否需要确认（无规则时 allow，即默认不要求确认）
-> commit memory fact
-> index via provider
```

要点：

- 不再有硬编码的"forbidden sensitivity"黑名单；sensitivity 由用户 Policy Rule 和 ExplorationScope 控制；
- 不再有全局"必须确认"默认；确认需求来自匹配的 Policy Rule（如 `require_confirmation`），无规则时默认 `allow`（自动提交）；
- Provider 索引失败不丢失 Kernel 事实，标记 `index_pending`。

## 12. Provider 选择

Provider Registry 根据：

- Profile；
- platform；
- capability completeness；
- permission availability；
- reliability；
- current health；
- user preference；
- cost；
- task requirements。

允许组合多个 Provider，选择结果写入 Action/Audit。

## 13. Schema、Canonical JSON 与生成物

所有正式 JSON Schema 必须声明 `$schema: "https://json-schema.org/draft/2020-12/schema"` 并使用 JSON Schema 2020-12 语义。Schema 是跨 Rust/TypeScript/SDK 类型与校验器的单一生成源。

### 13.1 版本字段

- KCP Envelope 使用字符串 `protocol_version` 表达协议兼容边界；第一版为 `1.0`；
- KCP payload、Event payload、持久对象和错误对象使用正整数 `schema_version`；
- Envelope 不得用 `schema_version` 替代 `protocol_version`，payload 也不得继承 Envelope 的 protocol version 充当自身 schema version；
- 向后兼容新增可选字段：schema minor 发布记录，但对象内整数 `schema_version` 是否递增由对应 Schema 兼容矩阵明确；
- 删除字段、改变字段含义、收紧既有合法输入或改变 enum 语义属于 breaking schema change，必须新 schema major/version；
- 未知字段是否允许由每个 Schema 的 `additionalProperties` / `unevaluatedProperties` 明示；不得笼统“合理忽略”；
- 未知 enum、Policy condition 或权限语义不可默认映射为 allow；
- 数据迁移有 preflight、backup、verify、rollback；每条持久记录保存 `schema_version`。

### 13.2 Canonical JSON 与哈希

所有契约中的 `*_hash`、幂等请求等价比较、签名输入和生成物稳定性比较，使用 RFC 8785 JSON Canonicalization Scheme（JCS）产生 UTF-8 bytes，再按字段规定计算哈希；未单独指定时哈希为 SHA-256 小写十六进制。不得使用语言默认对象序列化、格式化 JSON 或 map 插入顺序代替 RFC 8785。

### 13.3 Schema 源与生成物规则

- 人工维护的唯一源放在 `schemas/source/`；生成索引与兼容清单放在 `schemas/manifest.json`（首次实现时创建）；
- Rust、TypeScript 与 SDK 类型、validator、API reference 片段只能由 source schema 生成，生成目录必须标注 `GENERATED`，禁止手改；
- 同一生成命令在干净工作树运行两次必须 byte-for-byte 相同；
- CI 必须重新生成并以 `git diff --exit-code` 检查生成物漂移，同时验证所有 `$ref`、唯一 `$id`、2020-12 meta-schema 与示例；
- Schema 尚未创建前，本文是契约事实；文档不得声称已有生成文件或可用 SDK。

### 13.4 并发对象 revision

- 所有可并发修改的持久对象额外携带 `revision`（单调递增整数）；
- 更新操作通过 `expected_revision` 实现乐观锁；
- revision 冲突返回 `revision_conflict` 错误，由调用者重新读取后重试；
- 不可并发修改的对象（如不可变 EventEnvelope 与 AuditRecord）不需要 revision；
- EventEnvelope `sequence` 对每个聚合从首条已提交事件 `0` 开始，后续已提交事件严格连续 `+1`；回滚事务的暂分配不占号；
- AuditRecord v1 使用正式 Schema，是不可变本地事实，不带 revision，不自动公开为 Event 或写入 Outbox；业务要求审计时与业务事实及所需 Outbox 同事务，审计校验/插入失败整体回滚；

## 14. 日志

### 14.1 层级

- User Activity Summary；
- Operational Log；
- Security Audit；
- Debug Trace。

日志使用 Trace/Task/Action/Extension ID 关联。

### 14.2 默认策略

- 默认记录**最小元数据**：AuditRecord 的分类、层级、时间、entry point、适用的 Actor revision 快照，以及 Task/Action、Delegation、模型建议、验证、资源、Policy、回滚/恢复等显式稳定引用与结果原因；
- 不默认记录 Secret、密码、Token、完整未脱敏敏感文档、完整模型 Prompt；
- 用户可通过 Policy Rule 进一步限制或扩大日志内容；
- 日志保留策略由用户配置（默认保留天数、存储上限）。

### 14.3 不硬禁

日志正文使用 AuditRecord 的 `summary` 与 `details`；固定 actor/entry_point/task/action/delegation/model/verification/resource/policy/rollback/recovery/causation/correlation/outcome 等归因必须有顶层显式字段，不能仅藏在 `details`。Schema 无法完全禁止 `details` 重复这些值，生产者必须以顶层字段为准。Secret 和未脱敏正文**不被硬编码禁止**写入日志（某些诊断场景可能需要），但：

- Debug Trace 级别在默认配置中关闭；
- 启用完整日志可由用户或 Bot 配置，并必须记录风险与来源；
- Security Audit 层默认使用脱敏摘要，以减少审计副本；这是展示/复制策略，不是对 Memory、模型 Payload、Extension 数据流或云发送的阻断，配置可选择保留更完整内容。

## 15. ADR

以下变化必须 ADR：

- 新常驻进程；
- 新 SDK 传输；
- 新状态所有者；
- 核心技术栈改变；
- 权限等级改变；
- Core Identity 改变；
- 新特权动作类别；
- 删除平台抽象；
- 允许扩展进程内运行。

ADR 必须说明替代方案和迁移。

## 16. 编码规范

- Rust 核心禁止 `unwrap` 处理可恢复外部错误；
- 所有外部调用有 timeout/cancel；
- TypeScript 开启 strict；
- Schema 生成类型，不手工复制多份；
- 平台特定代码不进入 domain crates；
- Side effect API 名称必须清晰；
- 权限失败不做静默 fallback；
- 错误保留 machine code 与用户可解释摘要。

## 17. AI 编码代理提交模板

每个变更应报告：

- 目标；
- 权威规范；
- 受影响状态所有者；
- 权限/副作用；
- 失败和恢复；
- 新增/变更 Schema；
- 测试；
- ADR（如需要）；
- 未完成或不确定点。

## 18. 禁止模式

- UI 调用 OS API；
- Agent Runtime 执行 Shell；
- Provider 写 Task 状态；
- Extension 写权限数据库；
- Memory Provider 自动拼接 Prompt；
- 角色各自独立服务；
- 九套 SDK；
- Extension Host 无理由独立常驻；
- 通过环境变量把所有 Secret 传给子进程；
- 使用当前焦点作为 Computer Use 唯一目标；
- 在恢复中重放未知外部副作用；
- Kernel Control Protocol 与 Extension RPC 混用同一 envelope；
- Delegation 越界自动降级为确认而不重新评估 Policy；
- `unknown_side_effect` 被当作"可能成功"而乐观继续后续 Action。
