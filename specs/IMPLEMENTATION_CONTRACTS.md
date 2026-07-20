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
      kernel-task-creation/      # 已实现的纯task creation编排crate；尚未接repository/handler
      domain-memory/
      domain-initiative/
      extension-supervisor/
      extension-protocol/
      provider-registry/
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
  providers/                # optional package / 独立 workspace / distribution 示例；不是 Core build 依赖
    linux/
    windows/
    macos/
  workers/
    memorix/                # optional worker 示例
    vision/                 # optional Computer Use/ML package 示例；不是 Core build 依赖
  extensions/
    official/
  tests/
```

目录可以调整，但必须分别保持两类方向：源码/Schema 依赖为 `Optional Profile package → Extension SDK public contracts → Core public contracts`，Shittim Core 与 Extension SDK Base 不得反向引用 Profile 领域类型；运行时控制/调用为 `Core host → SDK host boundary → negotiated Profile implementation`，描述 negotiation、grant、invoke、cancel 与监督，不是源码依赖。Computer Use 平台 Provider 必须归 optional package、独立 workspace 或独立 distribution，可放在 `extensions/`、独立 `providers/` workspace 或仓库外分发包；上图 `providers/`、`workers/vision` 都只是 optional package 示例，**不**进入 Core 默认 workspace/build dependency。本阶段也不为其创建空目录、占位 crate 或实现声明。通用 `extension-protocol`、`provider-registry`、`object-store` 与 capability infrastructure 继续属于基础设施。

## 2. Rust 模块

### domain-task

纯领域：Task、Step、Action、状态机、不变量、Recovery 元数据。

### domain-policy

权限、Side-effect、Approval、Lease、Delegation Match、PolicyRule。

### kernel-task-creation

正式生产 ownership 已落地为纯 crate `kernel-task-creation`。当前已实现职责为：root v2 / child proposal 共用字段规范化；root receipt projection、root idempotency projection及其JCS/SHA-256；child normalized proposal / receipt hash；root/child allocation领域验证。它依赖`kernel-contracts`并调用`domain-policy`提供的权威URI parser/normalizer；不依赖 SQLite/KCP，不分配 ID，不读取 Delegation、parent、Action、PermissionDecision、Approval、Credential、Challenge或任何repository事实，不开启事务也不写存储。全部业务事实必须由caller以typed input显式注入。

`ChildTaskDeltaProjectionV1`、`MaterialAuthorizationProjectionV1`、`ObservationEvidenceProjectionV1`不属于本crate本阶段职责，也不得为了task creation方便塞入其中；它们未来由专门的authorization projection owner或独立切片实现。依赖方向只固定为`kernel-contracts + domain-policy -> kernel-task-creation -> kernel-sqlite（未来调用方）`，不据此声称`kernel-contracts`与`domain-policy`彼此依赖。组合根或KCP handler可调用纯API，但不得复制同义normalization/hash/allocation validator。`domain-task`继续只拥有Task/Action状态机与不变量，不增加policy依赖。

URI解析与规范化的当前唯一权威实现仍是`domain-policy`；`kernel-task-creation`只做编排和错误映射，不得复制、分叉或包装出第二套语义。长期如需消除领域命名耦合，可另立ADR把该实现抽到中立URI crate，但在该ADR与抽取完成前不得先制造并行实现。当前pure library已实现，但repository/handler/materializer尚未接入；不得把library完成误记为runtime composition或`V2InitialBuildActive`完成。

### domain-memory

记忆事实、来源、冲突、候选提交和召回政策。

### domain-initiative

Opportunity、Candidate、预算和调度决定。

### extension-supervisor

进程生命周期、握手、权限装配和健康。

### extension-protocol

Schema、消息、错误、Object Handle。

### provider-registry

Profile 协商元数据、通用能力档案、Provider 选择和降级；不得解释某一 Profile 的领域对象。

### platform-common

只允许跨领域、领域无关的宿主机制，例如 OS process/IPC primitives、文件权限抽象、clock/handle plumbing 与通用 platform detection。不得容纳 Display、Window、Workspace、Screenshot、Coordinate、desktop generation、Input Session Lease、compositor 事件等桌面领域模型，也不得成为 Computer Use 平台 Provider 的藏身 Core crate。

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
- Approval Center；
- Memory & Persona；
- Initiative & Delegation；
- Skill/Extension；
- Model Routing；
- Platform Capability/Doctor；
- Audit/Recovery。

当且仅当对应 optional Profile 已安装、启用、协商并由 probe 确认为可用时，desktop-client 才可挂载该 Profile 的专属视图；例如 Computer Use 的 Operation Snapshot 视图属于这种条件性 UI。`desktop-client` 本身不等于 Computer Use，也不得在 Profile 缺失时显示占位能力或暗示桌面操控可用。

UI 不拥有 Task 真相，只订阅并发出 Command。

## 5. Kernel Control Protocol

Kernel Control Protocol（KCP）是 agent-runtime、desktop-client 及其他内部客户端与 agentd 通信的唯一协议。它与 Extension RPC **分开**，后者是 agentd 与 Extension/Provider 进程之间的协议。

### 5.1 设计原则与版本

- Versioned：Envelope 带 `protocol_version`；第一版固定为 `"1.0"`，不支持的版本返回 `unsupported_protocol_version`。payload version **按方法 Catalog 判定**，不得全局假定所有方法只接受同一 schema version；
- Schema：Command、Query、Response 与 Event 的 `payload` 都是类型化对象，并带整数 `schema_version`；持久对象同样带 `schema_version`；
- Request/Response：每个 Command 和 Query 有 `request_id`，响应匹配；Command/Query Envelope 自身不带 `schema_version`，只带 `protocol_version`，其 payload 单独带 `schema_version`；
- Actor/Entry：Actor 保留身份来源 `source`，调用入口只出现在 Envelope 的 `entry_point`；Actor 内不得重复 EntryPoint。ContentOrigin 的 `entry_point` 是该内容自身的来源字段，不属于 Actor，也不替代 Envelope 当前调用入口；
- Deadline：每个请求必须带 RFC 3339 UTC `deadline`。agentd 开始处理时若已过期，必须返回 `deadline_exceeded`；处理期间超过 deadline 必须取消可取消工作并返回同一错误。若外部动作已开始且不能安全取消，先按 `CORE_ARCHITECTURE.md` 持久化为 `unknown_side_effect`/恢复待查，再返回 `deadline_exceeded`；不得静默丢弃或伪称没有副作用。`task.create` 的本地 SQLite 事务不是外部动作，也不得中途取消；其 commit 后超时语义与恢复规则由第 5.10 节单独定义；
- Idempotency：Command 必须带 `idempotency_key`，其 scope 由具体方法定义；Query 不携带幂等键；
- Expected Revision：修改既有并发对象的 Command 按方法要求携带 `expected_revision`；不匹配返回 `revision_conflict`；
- Auth v1：Envelope的`auth`必须为JSON `null`。Approval v2的identity challenge/evidence是独立对象/流程，不改变该Envelope v1事实；未来若升级KCP auth必须另立版本合同；
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

### 5.3 ContentOrigin 与输入 Scope Schema

`InputContentOriginV1`是 **common component** 的正式独立封闭对象，只含 caller 可声明来源事实；`ContentOriginV2`是stored对象，增加Kernel-owned id/received_at/carrier/receipt。二者不是同一对象的“可选字段模式”，禁止把stored字段加入input Schema。`InputContentOriginV1`的全部字段required、`additionalProperties=false`：

```text
InputContentOriginV1 {
  schema_version: 1,
  kind: user_input | companion_generated | system_generated
      | remote_message | web_content | document_content
      | model_output | extension_output | provider_output | imported_data,
  source_uri: URI|null,
  upstream_stable_id: non-empty string|null,
  producer_ref: {kind: actor | model | extension | provider | system, id: non-empty string},
  parent_origin_refs: UUID[]
}
```

`source_uri`只在非null时规范化；`parent_origin_refs`可空，元素出现时必须是合法UUID string，保序保重复且不设`uniqueItems`。该input对象没有`id`、`received_at`、`carrier_ref`、`kernel_receipt`、stored revision或任何Kernel时间。

`InputTaskScopeV1`是 **task component** 的正式独立封闭对象，不是stored `TaskScope`的裁剪视图。全部字段required、`additionalProperties=false`：

```text
InputTaskScopeV1 {
  schema_version: 1,
  resource_patterns: non-empty string[],
  exclusions: non-empty string[],
  allowed_capability_hints: non-empty string[],
  expires_at: RFC 3339 date-time|null
}
```

三个数组都可为空；元素出现时必须non-empty，保序保重复且不设`uniqueItems`。`expires_at`是required-nullable string，现行source Schema使用Draft 2020-12可编码的`pattern`与已启用assertion的`format: "date-time"`双门，后续修改必须持续保持：

```json
{
  "type": ["string", "null"],
  "format": "date-time",
  "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-5][0-9](?:\\.0+)?(?:Z|[+-][0-9]{2}:[0-9]{2})$"
}
```

`pattern`只固定本合同的canonical raw接受范围：完整文本、只接受大写`T`/`Z`、时区为`Z`或`±HH:MM`、秒为`00..59`、fraction缺失或全部为`0`；它故意不尝试证明月份、日期、小时和offset取值合法。`format`负责完整RFC 3339 date-time解析及日期/时间/offset合法性；即使底层format实现接受小写`t`/`z`，也会被pattern拒绝。两门必须同时通过，不使用custom format，也不得只依赖任一门。任何fraction含非零数字都是caller-invalid，禁止截断、四舍五入或把它伪装成规范化成功；合法值必须先解析为instant，再输出UTC秒精度`YYYY-MM-DDTHH:MM:SSZ`。该对象没有stored `id/task_id/revision/source_refs/created_by/created_at/updated_at`或其它source/time字段；缺失事实不得通过默认值伪造成stored Scope。

`ContentOriginV2`增加Kernel-owned id/received_at/carrier/receipt。v1仅保留Schema/fixture历史验证资产，无production store read/write/migration API；active producer使用v2。v2 carrier联合不含transition anchor：

- `schema_version = 2`；
- `carrier_ref.kind = command_request | task | artifact | event | action`；这是ContentOrigin carrier联合，不含`action_transition`（transition anchor只属于Event/Audit causation）；
- 当内容由`task.child.create` proposal物化时，`carrier_ref={kind:action,id:<创建Action>}`，不得伪造command或event carrier；
- receipt、parent origin、entry/source/producer语义不变。

`ContentOriginV2`完整stored wire为：

```text
{
  schema_version: 2,
  id: <uuid>,
  kind: user_input | companion_generated | system_generated
      | remote_message | web_content | document_content
      | model_output | extension_output | provider_output | imported_data,
  entry_point: EntryPoint,
  source_uri: <normalized URI> | null,
  upstream_stable_id: <string> | null,
  producer_ref: { kind: actor | model | extension | provider | system, id: <string> },
  received_at: <RFC 3339 UTC timestamp>,
  carrier_ref: { kind: command_request | task | artifact | event | action, id: <string> },
  parent_origin_refs: [<ContentOrigin id>, ...],
  kernel_receipt: {
    receipt_id: <uuid>,
    content_hash: <lowercase sha256 hex>,
    recorded_at: <RFC 3339 UTC timestamp>
  }
}
```

v1历史形状为：

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

### 5.3.1 可编码投影、JCS preimage 与规范化

以下对象都是现行source Schema中的**封闭可编码对象**；`additionalProperties=false`，所有列出的字段required。`T | null`表示required-nullable，缺字段与`null`不同。本批新Schema中，凡items为JSON string的数组都可为空；元素出现时必须non-empty（UUID/URI等更强格式约束继续适用），保留输入顺序与重复，且不得设置`uniqueItems`。只有明确标为set projection的派生字段才允许按对象合同排序/去重；不能因JCS会排序object key就整理数组。

`TaskCreateRequestV2`、`ChildTaskProposalV1`、`NormalizedRootTaskCreatePayloadV2`与`NormalizedChildTaskProposalV1`除各自root `schema_version` const和独立Schema身份外，caller-owned字段集合及字段约束完全同构。共享约束的唯一source site固定为task component的`NormalizedRootTaskCreatePayloadV2`文档内`$defs`；它是**canonical task-create proposal field contract**的中立宿主，不表示root业务依赖child，也不把child语义提升为root语义。该既有root的`$defs`精确包含`proposer`、`goal`、`constraints`、`success_criteria`、`risk_hint`、`capability_hints`、`task_scope`、`delegation_ref`、`origin`九项；禁止新增可独立注册、生成或计数的第13个Schema，也禁止复制九套约束。

四个root分别保留自己的封闭object、required列表与`schema_version` const：`TaskCreateRequestV2=2`、`NormalizedRootTaskCreatePayloadV2=2`、`ChildTaskProposalV1=1`、`NormalizedChildTaskProposalV1=1`。宿主root的properties必须使用同document local fragment，例如`#/$defs/goal`；其它三个document的对应properties必须使用absolute fragment `$ref`，例如`https://schemas.shittim.local/task/normalized_root_task_create_payload/v2#/$defs/goal`，不得使用relative跨document ref。`task_scope`与`origin`两个definition再分别whole-schema absolute `$ref` `https://schemas.shittim.local/task/input_task_scope/v1`与`https://schemas.shittim.local/common/input_content_origin/v1`。现有SchemaGraph以canonical `$id`+真实JSON Pointer标识fragment，且renderer已经支持local/absolute fragment与use-site lineage，因此此self-host不会制造独立root、循环identity或第13个Schema；若未来实现证明该既有能力回归，必须修复graph/renderer根因，不能改回child业务宿主或复制约束绕过。

本批12 Schema中，所有普通string值、nullable string的非null值和string array元素在出现时都必须`minLength: 1`；因此`risk_hint`非null时、`upstream_stable_id`非null时也必须non-empty。普通字符串不trim、不作Unicode重写；`" "`是否满足仅由`minLength`判断，不能偷偷引入trim语义。UUID、URI、date-time或enum继续使用更强约束。

通用hash算法固定为：先构造并再次Schema验证对象 → RFC8785 JCS → UTF-8 bytes（无BOM、无尾换行）→ SHA-256 → lowercase hex。签名preimage另见§6.10.3。URI规范化必须调用`domain-policy`当前唯一权威的`SECURITY_PRIVILEGE.md` §2.1 parser/normalizer：scheme/host小写、默认端口移除、dot segment解析、percent hex大写且不解码保留字符；query/fragment保留并按该规则规范化。`kernel-task-creation`只编排调用与错误映射，不复制URI parser；未来抽取中立URI crate必须另立ADR并迁移唯一实现，不能先并存第二套。普通字符串不trim、不Unicode重写；UUID按Schema lowercase canonical text。`InputTaskScopeV1.expires_at` raw由上述`pattern + format`双门约束：大写`T/Z`、`Z|±HH:MM`、秒`00..59`、无fraction或fraction全零；任何非零fraction都是caller-invalid且禁止截断/四舍五入；合法值解析为instant后统一输出UTC秒精度`YYYY-MM-DDTHH:MM:SSZ`。

#### `NormalizedRootTaskCreatePayloadV2`

这是 active `TaskCreateRequestV2` 的唯一规范化 payload：字段表与该 request 的 caller-owned 字段**一一对应**，**没有**`parent_task_id`、`context`或任何 Kernel-owned ID。`context`仍只属于Command Envelope，且只在下文的idempotency projection出现一次；不能在wire payload提交同名副本。

| 字段 | 类型/空值 | 规范化与来源规则 |
|---|---|---|
| `schema_version` | const `2` | required；来自 payload |
| `proposer` | `user\|companion\|system` | required；原值 |
| `goal` | non-empty string | required；不trim、不作Unicode改写 |
| `constraints` | non-empty string[] | required；保序保重复，可空数组 |
| `success_criteria` | non-empty string[] | required；保序保重复，可空数组 |
| `risk_hint` | non-empty string\|null | required-nullable；非null时`minLength: 1`；不推断、不trim |
| `capability_hints` | non-empty string[] | required；保序保重复，可空数组 |
| `task_scope` | `InputTaskScopeV1` | required；字段约束通过`https://schemas.shittim.local/task/normalized_root_task_create_payload/v2#/$defs/task_scope` absolute fragment进入canonical宿主；宿主`$defs/task_scope`再whole-schema引用`https://schemas.shittim.local/task/input_task_scope/v1`；`schema_version=1`；`resource_patterns[]`与`exclusions[]`逐项URI-pattern规范化、保序保重复；`allowed_capability_hints[]`保序保重复；`expires_at`required-nullable并UTC秒精度规范化 |
| `delegation_ref` | UUID\|null | required-nullable；UUID为lowercase canonical text；非null authority验证不改变该值 |
| `origin` | `InputContentOriginV1` | required；字段约束通过`https://schemas.shittim.local/task/normalized_root_task_create_payload/v2#/$defs/origin` absolute fragment进入canonical宿主；宿主`$defs/origin`再whole-schema引用`https://schemas.shittim.local/common/input_content_origin/v1`；`schema_version=1,kind,source_uri|null,upstream_stable_id|null,producer_ref{kind,id},parent_origin_refs[]`全量出现；仅`source_uri` URI规范化，parent refs保序保重复 |

数组规则没有未列出的隐式set语义；尤其`capability_hints`、scope hints及parent origin refs不得排序或去重。raw Schema/preflight之后，`origin.source_uri`、`task_scope.resource_patterns[]`与`task_scope.exclusions[]`的任何normalization失败统一公开映射现有闭集code `invalid_scope_pattern`：details精确为`{input_kind:"origin_source_uri",index:null}`、`{input_kind:"resource_pattern",index:<zero-based integer>}`或`{input_kind:"exclusion",index:<zero-based integer>}`。历史code/message名称不扩大成“只有pattern”；`InputTaskScopeV1.expires_at`非零亚秒则是caller-invalid并映射`invalid_request`，不借用`invalid_scope_pattern`。规范化后必须再次执行normalized Schema验证与typed decode；该阶段失败是本地internal contract failure，最终安全wire映射为`internal_error`，绝不能回退成caller错误。

receipt preimage就是`NormalizedRootTaskCreatePayloadV2`的JCS UTF-8 bytes。v2 idempotency preimage为封闭的`RootTaskCreateIdempotencyProjectionV1 {schema_version:1,actor,entry_point,command_type:"task.create",task_id:null,context,expected_revision:null,payload:NormalizedRootTaskCreatePayloadV2}`；projection自身的`schema_version=1`是required const，参与JCS与idempotency hash，不能在hash前剥离。`context`作为required-nullable Envelope JSON值原样出现一次，JCS只排序object key，不改变其字符串或数组。两者都使用本节通用JCS/SHA-256规则。root official fixture中的`normalized_payload`本身就是receipt projection，禁止再复制一个同义对象字段；不得借用legacy v1或child fixture。

#### `NormalizedChildTaskProposalV1`

| 字段 | 类型/空值 | 投影规则 |
|---|---|---|
| `schema_version` | const `1` | required |
| `proposer` | enum | 原值 |
| `goal` | non-empty string | 不trim |
| `constraints` | non-empty string[] | 保序保重复，可空 |
| `success_criteria` | non-empty string[] | 保序保重复，可空 |
| `risk_hint` | non-empty string\|null | required-nullable；非null时`minLength: 1`，不推断、不trim |
| `capability_hints` | non-empty string[] | 保序保重复，可空 |
| `task_scope` | `InputTaskScopeV1` | 字段约束通过`https://schemas.shittim.local/task/normalized_root_task_create_payload/v2#/$defs/task_scope` absolute fragment进入canonical宿主；宿主`$defs/task_scope`再whole-schema引用`https://schemas.shittim.local/task/input_task_scope/v1`；`schema_version=1`；`resource_patterns`、`exclusions`逐项URI-pattern规范化并保序保重复；`allowed_capability_hints`保序保重复；`expires_at`required-nullable并UTC规范化 |
| `delegation_ref` | UUID\|null | required-nullable |
| `origin` | `InputContentOriginV1` | 字段约束通过`https://schemas.shittim.local/task/normalized_root_task_create_payload/v2#/$defs/origin` absolute fragment进入canonical宿主；宿主`$defs/origin`再whole-schema引用`https://schemas.shittim.local/common/input_content_origin/v1`；`schema_version=1`、`kind`、规范化`source_uri|null`、`upstream_stable_id|null`、`producer_ref{kind,id}`、`parent_origin_refs[]`保序保重复；与stored ContentOriginV2严格分名 |

明确排除所有Kernel-owned ID、parent/status/plan/revision/time、receipt/carrier、Action completion和Envelope字段。该对象的JCS hash是child proposal hash，也是ContentOrigin receipt内容边界。

#### Task creation official fixture外层合同

以下三个路径是权威测试制品路径，wrapper字段名也是权威合同，但wrapper本身**不是business JSON Schema**、不得进入`schemas/manifest.json`、不得生成Rust/TS业务类型：

- root：`schemas/fixtures/kcp/task_create_normalized_hash.v2.json`；
- child：`schemas/fixtures/task/child_task_proposal_normalized_hash.v1.json`；
- allocations：`schemas/fixtures/task/task_creation_allocations.v1.json`。

root fixture外层固定为：

```text
{
  fixture_version: 1,
  raw_envelope: <KcpCommandEnvelopeV2>,
  normalized_payload: <NormalizedRootTaskCreatePayloadV2>,
  receipt_preimage: { jcs_utf8_hex: <lowercase hex>, sha256: <lowercase 64-hex> },
  idempotency_projection: <RootTaskCreateIdempotencyProjectionV1>,
  idempotency_preimage: { jcs_utf8_hex: <lowercase hex>, sha256: <lowercase 64-hex> },
  tamper_cases: [{
    case_id: <non-empty unique string>,
    operation: add | replace,
    pointer: <strict RFC6901 pointer into raw_envelope>,
    value: <JSON value>,
    expected: {
      result: raw_schema_rejected | normalization_rejected | hashes_computed,
      public_error: { code: <Error Catalog code>, details: <exact details|null> } | null,
      hash_relations: {
        receipt: same | different | not_computed,
        idempotency: same | different | not_computed
      }
    }
  }]
}
```

official root fixture harness的执行边界固定为：先执行`KcpCommandEnvelopeV2` raw Schema，再执行`TaskCreateRequestV2` payload Schema，最后调用`kernel-task-creation`完成normalization/projection/hash。该harness不执行Value preflight、`preflight_value`、`narrow_to_registered`或dispatcher；因此不能把未执行层的错误码借入本层。`public_error`只是fixture harness对已执行Schema/normalization层的合同化投影，不是已发送的KCP wire response。特别地，`auth`改为非null时，固定为`raw_schema_rejected`、`public_error={code:"invalid_request",details:null}`，receipt与idempotency两hash均为`not_computed`；不得写成`unsupported_auth_schema`。独立preflight对同一非null`auth`仍按其自身合同返回`unsupported_auth_schema`，两者不互相覆盖。

child fixture外层固定为：

```text
{
  fixture_version: 1,
  raw_proposal: <ChildTaskProposalV1>,
  normalized_proposal: <NormalizedChildTaskProposalV1>,
  proposal_preimage: { jcs_utf8_hex: <lowercase hex>, sha256: <lowercase 64-hex> },
  tamper_cases: [{
    case_id: <non-empty unique string>,
    operation: add | replace,
    pointer: <strict RFC6901 pointer into raw_proposal>,
    value: <JSON value>,
    expected: {
      result: raw_schema_rejected | normalization_rejected | hash_computed,
      public_error: { code: <Error Catalog code>, details: <exact details|null> } | null,
      hash_relation: same | different | not_computed
    }
  }]
}
```

fixture只保存JCS UTF-8 bytes的lowercase `jcs_utf8_hex`与hash字段`sha256`，不得重复保存canonical JSON string、base64或带前缀digest。tamper `pointer`按raw基准对象解释；`add`要求目标member不存在或array插入位置合法，`replace`要求目标已存在，解析失败必须使fixture harness失败而不是跳过case。

root tamper最小权威矩阵：`auth`改为非null时，只在本fixture实际执行的`KcpCommandEnvelopeV2` raw Schema阶段拒绝，不进入payload Schema、normalization或hash；固定期望为`result=raw_schema_rejected`、`public_error={code:"invalid_request",details:null}`、receipt/idempotency两者均为`not_computed`。该断言不执行也不借用preflight的`unsupported_auth_schema`；独立preflight合同仍单独验证后者。合法改变`deadline`、`request_id`或`idempotency_key`时两hash均`same`；合法改变`context`时receipt `same`、idempotency `different`；改变任一纳入payload字段时两hash均`different`；增加`payload.parent_task_id`必须raw Schema拒绝；URI lexical equivalent经normalization后两hash均`same`；任一纳入数组重排或改变重复次数时两hash均`different`。child fixture相应覆盖URI lexical equivalent、数组顺序/重复、origin/scope normalization错误与非零`expires_at`亚秒caller-invalid。allocation fixture精确外层见§6.10.6。

#### `ChildTaskDeltaProjectionV1`

> ownership冻结：本投影不属于`kernel-task-creation`本阶段。以下仅定义未来authorization projection合同；未来必须由专门authorization projection owner或独立切片实现，并把所需权威事实作为输入，不能借task creation helper读取repository或顺手生成。

| 字段 | 类型/空值 | 投影规则 |
|---|---|---|
| `schema_version` | const `1` | required |
| `parent_task_id` / `parent_task_revision` / `parent_task_scope_ref` | UUID/positive integer/UUID | current authoritative facts |
| `parent_resource_patterns` / `parent_exclusions` | normalized pattern[] | 按stored order保留重复 |
| `child_resource_patterns` / `child_exclusions` | normalized pattern[] | 来自normalized proposal，保序重复 |
| `parent_allowed_capability_hints` / `child_allowed_capability_hints` | string[] | **set projection**：按UTF-8 bytes升序、去重；分别只取父/子权威`TaskScope.allowed_capability_hints`，不是Policy授权、Delegation actions或Capability Registry可用集合 |
| `added_resource_patterns` / `removed_resource_patterns` / `added_exclusions` / `removed_exclusions` | string[] | multiset差：按值UTF-8升序，每个重复出现一次；不能只做集合差 |
| `added_capabilities` / `removed_capabilities` | string[] | set差，UTF-8升序唯一 |
| `parent_delegation_ref` / `child_delegation_ref` | UUID\|null | required-nullable |
| `delegation_change` | `unchanged|added|removed|replaced` | 由前两字段确定 |
| `delegation_authority_ref` | stable ref\|null | child non-null时必须非null |
| `delegation_revision` | positive integer\|null | 与authority同nullability |
| `delegation_scope_hash` | sha256\|null | authority canonical scope projection hash |
| `authority_status` | `not_applicable|verified` | 不允许unknown/default |

#### `MaterialAuthorizationProjectionV1`

| 字段组 | required字段 |
|---|---|
| identity | `schema_version=1`、完整`actor` revision快照、`entry_point` |
| task/action/plan | `task_id`、`task_revision`、`task_plan_version`、`action_id`、`action_revision` |
| operation | `capability_id`、`operation`、`side_effect_class`、`normalized_key_params`、`key_params_hash` |
| scope/resource | `task_scope_ref`、`resource_refs`（normalized URI set：UTF-8升序唯一）、`resource_refs_hash`、`child_task_delta_hash|null` |
| authority | `delegation_ref|null`、`delegation_authority_ref|null`、`delegation_revision|null`、`policy_set_revision` |
| target semantics | `target_kind`、`target_stable_ref|null`、`destination|null`、`protected_surface_labels` |
| content/plan material | `content_origin_refs`（lowercase canonical UUID text set）、`task_proposal_hash|null`、`proposed_plan_version|null`、`proposed_plan_hash|null` |

`MaterialAuthorizationProjectionV1`**禁止**纳入PermissionDecision的`id`、`decision_revision`、`evaluated_at`、`observation_evidence_fingerprint`、`approval_chain_id`、`reusable_resolution_ref`或任何由本投影hash反向派生的字段；否则会形成self-reference，并使只更新observation的新PermissionDecision无法与旧resolution材料等价。Action revision仍属于执行材料；PermissionDecision revision只属于Approval subject freshness，不属于material hash。`content_origin_refs`逐项先经UUID parser并输出lowercase canonical text，再按该UTF-8文本升序、完全重复去重；非法或非canonical存储值fail closed，不按原输入大小写排序。

`normalized_key_params`是Action operation Schema定义的完整material参数对象；不得用摘要字段替代，`key_params_hash`必须等于该对象JCS hash。`destination`是`{kind,stable_ref,account_ref|null,channel_ref|null}`或null。`protected_surface_labels`为`[{label,classification,source_ref}]`，按`label,classification,source_ref` UTF-8 tuple升序、完全重复去重；坐标、snapshot、confidence、observed_at不进入material。字段没有事实时只在表中允许null的地方写null，不以空字符串代替。

材料等价算法固定为：从两次评估各自读取同版本权威事实 → 独立构造并Schema验证`MaterialAuthorizationProjectionV1` → JCS → SHA-256 lowercase → 比较32-byte digest（不得只信stored hash）→ 同时确认旧approved resolution仍是current head、未过期/失效且subject中除PD freshness字段外的材料绑定一致。只有digest相等才允许新PD的`reusable_resolution_ref`指向旧resolution；PD ID/revision不同本身不破坏材料等价，任何纳入字段不同都必须失效。

#### `ObservationEvidenceProjectionV1`

该投影本身是以`observation_kind`判别的tagged union，不能为“无Profile观察”伪造provider/snapshot：

| branch | required字段与规则 |
|---|---|
| `not_applicable` | `schema_version=1`、`observation_kind="not_applicable"`；无其它字段，表示本次授权不依赖瞬时Profile观察 |
| `observed` | `schema_version=1`、`observation_kind="observed"`、`provider_ref`、`provider_revision`、`snapshot_ref|null`、`snapshot_generation|null`、`target_observation_ref|null`、`coordinate_transform_hash|null`、`observed_at`、`valid_until`、`evidence_refs`、`protected_surface_observations`、`destination_observation_ref|null` |

`observed`支中snapshot ref/generation成对null或非null；`valid_until>observed_at`；`evidence_refs`按stable ref UTF-8 bytes升序唯一；protected-surface observations按输入证据顺序保留并保留重复。provider必须是真实协商并产出该观察的Provider，禁止用`core`、`none`、`system`等伪provider填空。该对象只描述瞬时证据；actor、operation、capability、resource scope、destination语义、material protected labels、Task/plan/action/PermissionDecision revision不得进入。

对应hash分别命名`material_authorization_fingerprint`与`observation_evidence_fingerprint`，不能再使用含混的`evaluation_context_hash`。`not_applicable`与`observed`各自必须有official fixture；observed fixture还覆盖snapshot成对空值、证据排序及伪provider负例。

每个对象至少有一个official fixture，包含raw input、normalized object、JCS UTF-8 bytes、SHA-256与tamper cases；root TaskCreate v2另有`normalized_payload`/receipt/idempotency复合fixture，child proposal/delta/material/observation各有独立fixture，禁止复用v1向量冒充。

### 5.4 Envelope

本批新增且不得改名的两个root Schema是：

- `KcpCommandEnvelopeV2`：`https://schemas.shittim.local/kcp/command_envelope/v2`；
- `KcpQueryEnvelopeV2`：`https://schemas.shittim.local/kcp/query_envelope/v2`。

二者的顶层`protocol_version`仍固定`"1.0"`；Schema版本名V2描述breaking validation architecture，不把wire protocol升级成2.0。新Envelope只验证公共结构、family method闭集与根`payload.schema_version`为**u32 method-version编码域**（`1..=4294967295`）；该上界与manifest/generated `MethodVersionBinding`公开`u32`类型一致，不是任意策略。新Envelope**不得**含任何方法payload conditional `$ref`：每个新Envelope正好0个method payload conditional `$ref`。因此只禁止生成`TypedKcpCommandEnvelopeV2`与`TypedKcpQueryEnvelopeV2`这两个generic envelope typed decode，以及任何仅由这两个Envelope的0 conditional refs同义派生的wrapper；不得据此笼统禁止其它有正式判别映射的`Typed*V2`类型。payload业务Schema由generated MethodVersionBinding按`family+method+request version`第二阶段选择。`KcpQueryEnvelopeV2`即使当前query payload都仍为v1，也因验证架构从method-only conditional binding改为family+method+version两阶段选择而属于breaking replacement，必须保留独立V2身份。retained v1 Command/Query Envelope source、ID和条件refs不得修改。

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
  payload: { schema_version: <u32 integer 1..=4294967295>, ... }
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
  payload: { schema_version: <u32 integer 1..=4294967295>, ... }
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

#### `task.create`（active root-only v2）

- 属性：Command；只创建**根 Task** Kernel事实，不执行Task目标中的外部副作用；
- active payload版本：仅`schema_version=2`。`TaskCreateRequestV1`永久冻结为legacy validation/fixture/历史验证资产，不进入active registration/dispatcher，production请求携带v1时返回`unsupported_schema_version`；
- Envelope约束：`task_id=null`、`expected_revision=null`；`context`保留为业务关联上下文并进入幂等等价投影；
- 规范化：只对非null `origin.source_uri`、`task_scope.resource_patterns[]`、`task_scope.exclusions[]`及`task_scope.expires_at`执行本节明确的规范化；前三类URI normalization失败统一`invalid_scope_pattern`并使用§5.3.1精确details，非零亚秒`expires_at`为caller-invalid `invalid_request`；post-normalize Schema/typed合同失败为本地internal contract failure；
- 幂等scope：`(actor.id, entry_point, "task.create", idempotency_key)`，记录与Task同生命周期；
- `normalized_payload`必须精确为§5.3.1的`NormalizedRootTaskCreatePayloadV2`（与`NormalizedChildTaskProposalV1`共享字段规则但不是同一Schema，且不含parent）；receipt preimage就是其JCS UTF-8 bytes；
- v2幂等等价投影必须精确构造为封闭对象`RootTaskCreateIdempotencyProjectionV1 {schema_version:1,actor,entry_point,command_type:"task.create",task_id:null,context,expected_revision:null,payload:NormalizedRootTaskCreatePayloadV2}`；`schema_version=1` required const参与JCS/hash，`context`只在该外层projection出现一次，原样作为JSON值，JCS仅规范object key，不调整其数组/字符串；
- official root fixture必须位于`schemas/fixtures/kcp/task_create_normalized_hash.v2.json`，并按§5.3.1固定wrapper保存raw envelope、normalized payload、idempotency projection、两份`jcs_utf8_hex`/`sha256`及tamper；`normalized_payload`本身就是receipt preimage对象，不得复制同义receipt projection或保存canonical string；
- v2幂等等价投影排除protocol/message/auth/request/deadline/idempotency key；receipt排除全部Envelope、Kernel IDs、时间、receipt与物化对象；
- `task.create` Request payload必须解码为`TaskCreateRequestV2`；其九项caller-owned字段均通过absolute fragment引用`NormalizedRootTaskCreatePayloadV2#/$defs`这一canonical宿主，不能直接whole-schema绕过宿主引用`InputTaskScopeV1`或`InputContentOriginV1`。宿主自身properties使用local fragment；宿主`$defs/task_scope`和`$defs/origin`再分别whole-schema引用`https://schemas.shittim.local/task/input_task_scope/v1`与`https://schemas.shittim.local/common/input_content_origin/v1`；其它root同样逐字段使用宿主absolute fragment。不得在Catalog段再维护一份内联平行字段表。

v2**不存在**`parent_task_id`字段；unknown-field策略必须拒绝caller提交该字段。每个parent origin必须存在，否则`parent_origin_not_found`。非null Delegation必须由authority repository验证，否则`delegation_not_found`/`delegation_inactive`/`delegation_not_authorized`；不能继续使用“所有非null都固定not found”的legacy行为。

单事务创建：idempotency record、ContentOrigin v2、TaskScope、`TaskSpec.parent_task_id=null`的Task、`TaskCreationProvenance(kind=root_command_v2)`、AuditRecord v2与唯一`task.created` EventEnvelope v2。所有Kernel-owned UUID/correlation/dedup由上层分配；所有创建时间使用唯一`accepted_at`；canonical readback与引用闭包失败整体回滚。

`TaskCreateResponseV2`的完整payload为`{schema_version:2,task,creation_provenance_ref}`。其中`task`必须直接`$ref`当前active retained `https://schemas.shittim.local/v1/task/task_spec.json`；这表示TaskCreate v2返回当前TaskSpec合同，不把TaskSpec标成legacy，也不得为满足名称对称虚构TaskSpec v2。`creation_provenance_ref`是UUID。

`task.created`固定`causation_ref={kind:command_request,id:Envelope.request_id}`。Response payload为上述`TaskCreateResponseV2`。方法错误：`invalid_request`（raw/Schema）、`unsupported_schema_version`（包括legacy v1 production请求）、`invalid_scope_pattern`、`delegation_not_found`、`delegation_inactive`、`delegation_not_authorized`、`parent_origin_not_found`、`idempotency_conflict`、storage与通用错误。

#### `TaskCreateRequestV1` legacy冻结

v1包含`parent_task_id`，现有未发布Rust typed handler/SQLite repository仍实现了root与direct-child创建。自ADR-0006与ADR-0009起：

- source Schema/生成类型可保留为fixture与历史验证资产（仍被部分v2合同交叉引用或作为legacy validation schema）；
- 不得进入active KCP dispatcher、server registration或production write；
- 实现切片必须**删除** v1 task.create handler/adapter/repository write API与legacy direct-child写路径，而不是保留“可读旧库”分支；
- 不执行、也不支持v1业务数据迁移；旧开发数据库必须按§13.7拒绝启动；
- v1精确hash、旧producer与当前实现细节保留在§5.10 legacy实现说明和既有fixture中，只描述代码事实，不构成active API。

#### `task.child.create`（Kernel-local Action operation）

这不是KCP方法。唯一入口是父Task拥有的Action：

```text
capability_id = "kernel.task"
operation = "task.child.create"
side_effect_class = S1
external_side_effect = none
```

`structured_arguments`必须解码为`ChildTaskProposalV1`：

```text
{
  schema_version: 1,
  proposer: user | companion | system,
  goal: <non-empty string>,
  constraints: [<non-empty string>, ...],
  success_criteria: [<non-empty string>, ...],
  risk_hint: <non-empty string> | null,
  capability_hints: [<capability id>, ...],
  task_scope: InputTaskScopeV1,
  delegation_ref: <uuid> | null,
  origin: InputContentOriginV1
}
```

`origin`字段的正式输入类型名是`InputContentOriginV1`，它与stored `ContentOriginV2`分离：前者只包含caller可声明的`schema_version=1,kind,source_uri|null,upstream_stable_id|null,producer_ref,parent_origin_refs`，禁止id、received_at、carrier_ref与kernel_receipt；后者由Kernel分配这些事实。proposal禁止任何child ID、`parent_task_id`、status、plan version、revision、created/updated time、Action completion字段及其shadow同义字段。父Task ID只取`ActionRequest.task_id`。

Kernel计算`ChildTaskDeltaProjectionV1`并取hash；child proposal hash固定为`NormalizedChildTaskProposalV1`的JCS hash。两者一并进入`MaterialAuthorizationProjectionV1`，不得只存自然语言“delta摘要”。Scope不继承、不求交；扩大按Freedom-first裁决。Delegation ref必须显式出现，非null必须验证current/active/applicable。

执行前置：Action current revision CAS、status为approved/leased执行边界要求的状态、current PermissionDecision与必要Approval可消费、Action Lease/generation/holder有效、Stop Fence未激活、proposal/引用/delta/Delegation authority有效。失败使用`revision_conflict`、`action_not_executable`、`approval_required`/`approval_invalidated`、`lease_expired`、`stop_fence_active`及上述scope/delegation错误。

单一`BEGIN IMMEDIATE`物化bundle：

1. 以`action_id`查materialization唯一键并校验执行幂等映射：同一Action且proposal/material hashes相同、bundle完整则canonical readback返回原child；同一Action但proposal hash或material hash不同返回`child_materialization_conflict`；同一`(execution_generation,idempotency_key)`已绑定不同Action返回`idempotency_conflict`；mapping存在但bundle或关系不完整返回`stored_data_invalid`；
2. 一次分配child Origin/Scope/Task/provenance/Audit/Event/Verification及Action completion所需IDs；
3. ContentOrigin v2 carrier为Action；Task `parent_task_id=Action.task_id`；provenance=`child_action_v2`；
4. 写VerificationResult、AuditRecord v2、child `task.created`与`action.state_changed` Outbox；前者direct causation为该Action，后者causation为预持久化的`action_transition` anchor，禁止Action self-causation或伪造`action.requested` Event；
5. 原子更新Action result、verification ref、status=`completed`、revision与materialized child ref；
6. 在commit前从同一事务canonical readback并验证完整bundle。

任一步失败全回滚。同一Action全生命周期（跨generation）最多一个child；generation/idempotency只证明同一次执行，不形成多child命名空间。commit后响应丢失/timeout按同一Action/generation/idempotency重放。发现mapping存在但bundle或Action completion不完整返回`stored_data_invalid`并进入Safe Recovery，不做局部补写或第二次创建。

#### `task.get`

- 属性：Query、只读；
- Request payload：`{ schema_version: 1, task_id: <uuid> }`；
- Response payload：`{ schema_version: 1, task: TaskSpec }`；
- 错误：`task_not_found`、`sqlite_busy`、`sqlite_full`、`sqlite_corrupt`、`stored_data_invalid`、通用错误；typed handler 的稳定映射见第 5.10 节。

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
- Response payload：`{ schema_version: 1, events: [EventEnvelope v1, ...], next_outbox_position: <decimal string> }`；retained `EventPollResponse v1`的`events.items` exact引用retained `EventEnvelope v1`，因此**绝不能返回EventEnvelope v2**；
- 只返回 `outbox_position > after_outbox_position` 的事件并按位置升序排列；空结果合法。post-0003 mixed Outbox只是存储/Publisher能力，不使当前KCP `event.poll`自动具备mixed wire能力：若下一条可见记录是v2，当前v1 handler不得跳过、降解、重写成v1或推进cursor，且在未来KCP response升级切片完成前不得注册/启动该handler；未来mixed poll必须新增或升级versioned response payload、MethodVersionBinding、typed handler与Conformance后才可返回v2；
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

> **实现状态**：本节的八个Event v2 source Schema、manifest/compiler、generated catalog/typed types与conformance已经实现；SQLite migration 0003、mixed Outbox API、严格stored decoder与savepoint transaction poison也已实现。active business producer、Publisher与runtime仍未实现。版本化Schema与统一Outbox实施决策见ADR-0008。

EventEnvelope字段与Outbox语义以`CORE_ARCHITECTURE.md`为准。v1 Envelope仅保留Schema/fixture历史验证资产，无production store read/write/migration API；active producers使用EventEnvelope v2与CausationRef v2（`command_request | event | action | action_transition`）。现有payload可维持各自业务payload版本；首批新增`ActionStateChangedPayload v1`与`ApprovalStateChangedPayload v1`。Envelope版本与payload版本正交：EventEnvelope v2正式复用retained `TaskCreatedPayload v1`、`TaskStateChangedPayload v1`与`StopFenceActivatedPayload v1`。

active Event Catalog的唯一Schema权威是manifest中exact `EventEnvelopeV2` claimant与其root mapping的共同闭包。claimant检查必须先按以下**保留身份或结构形状**收集候选，再要求候选集合恰好等于一个exact claimant；不能只搜索exact entry而漏掉伪装者：

- **保留身份候选**：任一entry的`id`等于`https://schemas.shittim.local/event/event_envelope/v2`，或`title`等于`EventEnvelopeV2`，或`source`等于`schemas/source/event/event_envelope.v2.json`，即成为候选；这三项任一被占用但其余metadata不exact都属于partial claimant并fail closed。
- **结构候选（宽松门）**：`component=event`、`kind=envelope`，root `properties.schema_version.const=2`、`properties.type`为非空string enum，并存在至少一个mapping-like `allOf`分支：`if.properties.type.const`为string，且`then.properties`同时出现`aggregate_type`与`payload`。候选门**不要求**root `type=object`、`additionalProperties=false`、完整`required`、whole-root `$ref`可解析、全branch同形或enum/mapping双射；它不得调用strict parser，也不得产生任何mapping fact。缺失/true `additionalProperties`、错误root type、缺required、partial branch或其它近似伪装都必须先成为candidate，再由exact门fail closed。
- **非候选**：普通event object/payload、没有上述完整discriminator+conditional mapping形状的其它envelope，不因名字含`event`或拥有开放`payload`就被误抓。

exact claimant metadata固定为`component=event,kind=envelope,title=EventEnvelopeV2,id=https://schemas.shittim.local/event/event_envelope/v2,source=schemas/source/event/event_envelope.v2.json,version=2,compatibility=breaking-replacement,schema_version_field=schema_version`。exact root还必须明确`type=object`、`additionalProperties=false`并required至少包含`type/schema_version/aggregate_type/payload`。随后才读取registry load时为每个envelope**恰好解析一次并按schema id缓存**的strict `EnvelopeConditionalBinding`：`type` enum精确为五个active type且无重复；`allOf`每个成员都必须是一个且仅一个支持profile的mapping branch，`if`只能以required `type`的单一const判别，`then.properties`必须给出单一`aggregate_type.const`和单一payload **whole-schema `$ref`**，仅`stop_fence.activated`还必须给出`aggregate_id.const="global"`；inline payload、fragment `$ref`、缺失/重复type branch、同type多branch、enum无branch、branch无enum、一个branch混合多个type、额外未知mapping branch均拒绝。whole-schema `$ref`指fragment为空、解析后identity为目标manifest root；不得接受inline等价Schema。Catalog authority、target payload closure与typed `EnvelopeWireBinding`都只能投影同一个缓存IR实例，不得再次解析raw Schema。恰好一个exact claimant且上述五branch双射成立后，才可派生type、aggregate与payload binding；不得按ID suffix、source文件名或手写平行表派生。

生成Rust目录的public事实固定为：

```rust
pub struct EventTypeBinding {
    pub event_type: &'static str,
    pub aggregate_type: &'static str,
    pub payload_schema_id: &'static str,
    pub payload_schema_version: u64,
}

pub const EVENT_ACTIVE_BINDINGS: &[EventTypeBinding] = &EVENT_ACTIVE_BINDING_ARRAY;
pub const EVENT_LEGACY_V1_BINDINGS: &[EventTypeBinding] = &EVENT_LEGACY_V1_BINDING_ARRAY;
pub const EVENT_ACTIVE_TYPES: &[&str] = &EVENT_ACTIVE_TYPE_ARRAY;
pub const EVENT_LEGACY_V1_TYPES: &[&str] = &EVENT_LEGACY_V1_TYPE_ARRAY;
```

两个private fixed-size binding arrays分别按EventEnvelopeV2与retained EventEnvelope root `type` enum的Schema声明顺序生成。renderer必须以binding初始化作为event type/aggregate/payload ID/version的唯一数据源，再用通用`const fn project_event_types<const N: usize>(&[EventTypeBinding; N]) -> [&'static str; N]`生成private type arrays并暴露上述slices；禁止在renderer或生成物中维护第二份字符串/顺序表，且必须有真实Rust编译测试。`payload_schema_id`为manifest exact whole-schema root ID，`payload_schema_version`取payload root自身版本。删除模糊`EVENT_V1_TYPES`且不保留alias。

首批 payload：

#### `task.created`

- `aggregate_type = "task"`，`aggregate_id = task_id`；
- payload：`{ schema_version: 1, task_id: <uuid>, status: TaskStatus, proposer: user | companion | system, goal: <string>, task_revision: <positive integer>, created_at: <RFC 3339 UTC> }`；
- `task_revision` 必须等于该事件所描述的 `TaskSpec.revision`；首批 `task.create` 中固定为 `1`；
- `task.created`可以由root command或child-create Action产生；root因果为`command_request`，child因果必须为`action`；
- 首批root create与child materialization的`event_id`、`correlation_id`、`dedup_key`由Kernel上层显式提供，repository不得自行生成；`sequence=0`、`occurred_at=Task.created_at`、payload与Task canonical子事实不一致时整笔事务失败。

#### `task.state_changed`

- `aggregate_type = "task"`，`aggregate_id = task_id`；
- payload：`{ schema_version: 1, task_id: <uuid>, from_status: TaskStatus, to_status: TaskStatus, task_revision: <positive integer>, reason_code: <non-empty string>, changed_at: <RFC 3339 UTC> }`。

#### `action.state_changed`

`action.completed`不作为首批类型：它只描述一个终态，无法统一表达approved→leased→in_flight→completed等转换，且把Action自身作为自身causation会形成自环。首批active Catalog正式加入通用`action.state_changed`：

- `aggregate_type="action"`，`aggregate_id=action_id`；同Action首条已提交事件sequence=0，此后严格+1；
- payload `ActionStateChangedPayload v1`全部required：`{schema_version:1,action_id,task_id,from_status,to_status,action_revision,execution_generation,permission_decision_ref,approval_resolution_ref,materialized_child_task_ref,verification_result_refs,reason_code,changed_at}`；三个单值ref字段required-nullable，verification数组required可空、成员为UUID且拒绝重复；`action_revision>=1`、`execution_generation>=0`、`reason_code`非空；
- Schema必须表达的单记录条件：`to_status=completed|failed`时`verification_result_refs`非空；`materialized_child_task_ref!=null`时`to_status=completed`且verification非空；`approval_resolution_ref!=null`时`permission_decision_ref!=null`。Action合法边、`from_status != to_status`、revision恰好递增1、generation/current Lease、PD/Approval可消费、child operation身份及与commit后Action逐字段相等属于repository跨对象验证，不能伪装成Schema能力；
- causation禁止指向同一Action。`CausationRef v2`为该事件增加`action_transition` branch：`{kind:"action_transition", action_id, transition_id}`。`transition_id`是Kernel在创建/推进Action的command、scheduler dispatch、lease-expiry recovery或materialization调用开始时预分配的UUID因果锚点，持久化在Action transition intent中；它不是Event，也不是Action ID；
- child materialization事务内，child `task.created` causation仍为`{kind:"action",id:action_id}`；同事务`action.state_changed` causation为对应materialization transition anchor。两事件共享correlation id，但各自event id/dedup key独立；
- payload与commit后的Action revision/status/result、PD/Approval/Verification refs逐字段相等，否则事务回滚。

因此CausationRef v2闭集最终为`command_request | event | action | action_transition`。`action`用于“某Action直接产生另一个聚合事实”，`action_transition`只用于Action自身状态事件，消除self-causation。

#### `approval.state_changed`

Approval不可变链的每次current-head变化都必须可观察，正式Catalog只使用一个通用事件而非requested/resolved/invalidation三套平行事件：

- `aggregate_type="approval_chain"`、`aggregate_id=approval_chain_id`；初始request sequence=0，此后每次已提交head变化严格+1；
- payload `ApprovalStateChangedPayload v1`全部required：`{schema_version:1,change_kind,approval_chain_id,from_head_ref,to_head_ref,from_record_kind,to_record_kind,subject_kind,confirmation_mode,request_ref,resolution_ref,invalidation_ref,replacement_request_ref,permission_decision_ref,action_id,reason_code,changed_at}`；`change_kind`是显式判别字段，闭集精确为`initial_request|resolution|invalidation_without_replacement|replacement_request`，用于消除initial与replacement都可能出现`to_record_kind=request`的结构歧义；所有不适用ref以及`from_record_kind`均required-nullable；`to_head_ref`、`to_record_kind`、subject/mode/reason/time非null；record/subject/mode必须分别以whole-schema `$ref`直接引用`ApprovalRecordKindV2`、`ApprovalSubjectKindV2`、`ConfirmationModeV1`，不得inline、fragment-ref或复制enum；
- Schema必须以`change_kind`的四值enum与四个完整`allOf if/then` branch双射表达下列真值表；每个branch都固定from/to kind，并把六个变化相关ref设为UUID non-null或`type:null`。所有行之外组合拒绝：

| change_kind | from_record_kind | to_record_kind | from_head_ref | request_ref | resolution_ref | invalidation_ref | replacement_request_ref | to_head_ref |
|---|---|---|---|---|---|---|---|---|
| `initial_request` | null | `request` | null | non-null | null | null | null | non-null |
| `resolution` | `request` | `resolution` | non-null | non-null | non-null | null | null | non-null |
| `invalidation_without_replacement` | `request|resolution` | `invalidation` | non-null | non-null | `from_record_kind=resolution`时non-null，否则null | non-null | null | non-null |
| `replacement_request` | `request|resolution` | `request` | non-null | non-null | `from_record_kind=resolution`时non-null，否则null | non-null | non-null | non-null |

`request_ref`始终指该链中为本次subject/mode建立基线的request：initial时是新request；resolution时是直接前驱request；invalidation/replacement时是被失效request本身，或被失效resolution的`request_ref`。Schema能表达上述null/non-null、from/to kind与resolution旧head分支，但Draft 2020-12不能表达两个任意实例字符串相等，因此repository必须执行exact equality：initial `to_head_ref=request_ref`；resolution `from_head_ref=request_ref`且`to_head_ref=resolution_ref`；无replacement invalidation `to_head_ref=invalidation_ref`；replacement `to_head_ref=replacement_request_ref`；所有invalidation/replacement的`invalidation_ref`等于新invalidation record ID，若旧head是resolution则`resolution_ref=from_head_ref`，若旧head是request则`request_ref=from_head_ref`且`resolution_ref=null`；replacement request record的`predecessor_ref=invalidation_ref`。repository还必须验证整链canonical subject一致、mode从对应request投影、operation subject的PD/Action绑定及old/new current-head真实关系；初始request的`permission_decision_ref/action_id`按subject真实绑定，可为null但不能猜默认；
- 固定转换：初始request `null→request`；resolve `request→resolution`；invalidate without replacement `request|resolution→invalidation`；invalidate with replacement在同一事务形成一个逻辑head转换`request|resolution→request`，payload同时携带invalidation与replacement request，不得发出可被消费者误认作稳定head的中间invalidation事件；
- producer固定为Approval repository的`append_request`（仅新链）、`resolve`、`invalidate_and_optionally_replace`。每次成功调用恰好写一条事件；CAS loser、Schema/evidence失败和replay不写事件；
- causation必须来自业务调用的真实来源：KCP/内部command用`command_request`，已有Event驱动用`event`，Action直接触发Approval变化用`action`；Approval event不得以自己或同chain作因果。所有三种写方法必须接收上层分配的event id、correlation、dedup、causation和`changed_at`，并与Approval/Audit/Action/PD更新同事务；
- payload的head、record kind、subject、mode、PD/Action与reason必须从同事务canonical records/current relation投影，consumer readback失配为`stored_data_invalid`。

#### `stop_fence.activated`

- `aggregate_type = "stop_fence"`，`aggregate_id = "global"`；
- payload：`{ schema_version: 1, generation: <positive integer>, reason: <string>, activated_by_actor_id: <string>, activated_from_entry_point: EntryPoint, activated_at: <RFC 3339 UTC> }`。

事件类型必须精确匹配点号小写名称。新增类型先增加 Schema、Catalog、兼容说明与 Conformance 锚点。

### 5.7 v2权威 Error Catalog

本节是全量v2 wire error的唯一事实源；`docs/api/error-catalog.md`只能镜像/索引，CI必须检测漂移。每行固定`code/message/details/retryable/适用场景`，details未列出的键禁止，敏感正文禁止。所有错误对象`schema_version=1`。

| code | 固定message | details | retryable | 适用场景 |
|---|---|---|---:|---|
| `invalid_request` | `request is invalid` | null | false | 已执行的preflight中可关联但Envelope/payload结构无效；official root fixture harness的raw Envelope/payload Schema拒绝也固定投影为此code |
| `unsupported_protocol_version` | `protocol version is not supported` | null | false | protocol string不支持；仅适用于执行protocol/preflight判定的层 |
| `unsupported_schema_version` | `payload schema version is not supported` | null | false | method active version不含请求version；仅适用于执行method-aware preflight判定的层 |
| `unsupported_method` | `method is not supported` | null | false | family catalog无method；仅适用于执行method Catalog/preflight判定的层 |
| `unsupported_auth_schema` | `authentication schema is not supported` | null | false | **仅**KCP Value preflight执行auth判定时v1 auth非null；official root fixture harness不执行preflight，不得借用此code |
| `deadline_exceeded` | `request deadline exceeded` | null | true | 入口/完成deadline |
| `internal_error` | `internal kernel error` | null | false | 未分类内部合同/配置/序列化失败 |
| `revision_conflict` | `object revision changed` | `{object_kind,object_id,expected_revision,actual_revision}` | true | expected revision CAS失败 |
| `idempotency_conflict` | `idempotency key was used for different facts` | `{operation_kind,stable_operation_id}` | false | 同execution idempotency key绑定不同Action或同scope key不同事实 |
| `sqlite_busy` | `kernel storage is busy` | null | true | 锁超时 |
| `sqlite_full` | `kernel storage is full` | null | false | 空间不足 |
| `sqlite_corrupt` | `kernel storage is corrupt or invalid` | null | false | SQLite损坏 |
| `stored_data_invalid` | `stored kernel data failed integrity validation` | `{object_kind,object_id,integrity_check}` | false | canonical/readback/relation/mapping不完整 |
| `task_not_found` | `task was not found` | `{task_id}` | false | active Task读取/child父Task缺失 |
| `parent_task_not_found` | `parent task was not found` | `{task_id}` | false | 仅legacy TaskCreate v1 |
| `parent_origin_not_found` | `parent content origin was not found` | `{origin_id}` | false | origin ref缺失 |
| `origin_not_found` | `content origin was not found` | `{origin_id}` | false | stop.activate或通用origin引用缺失 |
| `invalid_scope_pattern` | `task scope contains an invalid URI pattern` | 精确三形：`{input_kind:"origin_source_uri",index:null}`、`{input_kind:"resource_pattern",index:<zero-based integer>}`、`{input_kind:"exclusion",index:<zero-based integer>}` | false | task-create origin URI或scope pattern normalization失败；历史code/message保留 |
| `invalid_cursor` | `cursor is invalid` | `{cursor_kind}` | false | task.list/event cursor结构或范围无效 |
| `unsupported_event_type` | `event type is not supported` | `{event_type}` | false | subscribe filter含非Catalog type |
| `subscription_not_found` | `event subscription was not found` | `{subscription_id}` | false | 临时subscription不存在/过期 |
| `stop_fence_active` | `kernel stop fence is active` | `{generation}` | false | 新副作用被Fence阻断 |
| `unsupported_policy_condition` | `policy condition is not supported` | `{condition_kind}` | false | 未知condition fail closed |
| `delegation_not_found` | `delegation was not found` | `{delegation_ref}` | false | delegation缺失 |
| `delegation_inactive` | `delegation is not active` | `{delegation_ref,status}` | false | 非active/current |
| `delegation_not_authorized` | `delegation does not authorize these facts` | `{delegation_ref,failed_dimension}` | false | authority不覆盖 |
| `action_not_found` | `action was not found` | `{action_id}` | false | Action缺失 |
| `action_not_executable` | `action is not executable` | `{action_id,status,reason_code}` | false | 状态/operation/lease前置失败 |
| `permission_decision_not_found` | `permission decision was not found` | `{permission_decision_ref}` | false | PD缺失 |
| `permission_decision_stale` | `permission decision is stale` | `{permission_decision_ref,stale_dimension}` | true | material/observation/current revision失效 |
| `approval_required` | `a current approval resolution is required` | `{approval_chain_id,confirmation_mode}` | false | 需current approved resolution |
| `lease_not_found` | `action lease was not found` | `{action_id,generation}` | true | Lease缺失 |
| `lease_expired` | `action lease is expired` | `{action_id,generation}` | true | Lease过期 |
| `lease_holder_mismatch` | `action lease holder does not match` | `{action_id,generation}` | false | holder错配 |
| `fence_generation_mismatch` | `stop fence generation changed` | `{expected_generation,actual_generation}` | true | 执行事务二次读取变化 |
| `child_materialization_conflict` | `child task materialization facts conflict` | `{action_id,conflicting_dimension}` | false | 同Action proposal/material hashes不同 |
| `verification_failed` | `action verification did not succeed` | `{action_id,verification_result_ref,outcome}` | false | Verification非成功 |
| `material_fingerprint_mismatch` | `authorization material does not match` | `{expected_fingerprint,actual_fingerprint}` | false | material不等 |
| `observation_evidence_stale` | `observation evidence is stale` | `{permission_decision_ref}` | true | observation过期/变化 |
| `approval_not_found` | `approval record was not found` | `{approval_record_ref,active_approval_chain_id}` | false | record缺失；查询active关系时链键固定`active_approval_chain_id`，不是持久对象近义字段 |
| `approval_head_conflict` | `approval chain head changed` | `{approval_chain_id,expected_head_ref,actual_head_ref}` | true | current-head CAS loser |
| `approval_invalidated` | `approval resolution is invalidated` | `{approval_chain_id,resolution_ref,invalidation_ref}` | false | resolution已失效 |
| `approval_expired` | `approval resolution is expired` | `{approval_chain_id,resolution_ref,expires_at}` | false | resolution过期 |
| `approval_subject_mismatch` | `approval subject does not match` | `{approval_chain_id,subject_kind}` | false | subject/hash/current facts错配 |
| `approval_evidence_mismatch` | `approval evidence does not match` | `{approval_chain_id,evidence_kind}` | false | mode evidence矩阵错配 |
| `challenge_not_found` | `approval challenge was not found` | `{challenge_id}` | false | challenge缺失 |
| `challenge_expired` | `approval challenge is expired` | `{challenge_id,expires_at}` | false | 过期 |
| `challenge_consumed` | `approval challenge was already consumed` | `{challenge_id,consumed_at}` | false | nonce replay |
| `challenge_revoked` | `approval challenge is revoked` | `{challenge_id,revoked_at}` | false | 已撤销 |
| `signature_invalid` | `approval signature is invalid` | `{challenge_id,credential_ref,algorithm_kind}` | false | 已知算法签名无效 |
| `audience_mismatch` | `approval audience does not match` | `{challenge_id}` | false | audience错配 |
| `credential_not_found` | `approval credential was not found` | `{credential_ref}` | false | credential revision缺失 |
| `credential_inactive` | `approval credential is not active` | `{credential_ref,status}` | false | revoked/expired/replaced |
| `credential_revision_mismatch` | `approval credential revision does not match` | `{credential_ref,expected_revision,actual_revision}` | false | revision错配 |
| `authentication_evidence_mismatch` | `authentication evidence does not match` | `{evidence_ref,evidence_kind}` | false | local/system evidence绑定失败 |
| `local_presence_required` | `verified local presence is required` | `{approval_chain_id}` | false | local mode无有效evidence |
| `system_authentication_required` | `system authentication is required` | `{approval_chain_id}` | false | system mode未完成认证 |

完整同事实是replay成功；不同Action material facts是`child_materialization_conflict`，execution key跨Action是`idempotency_conflict`，不完整是`stored_data_invalid`。旧的“already materialized”错误不进入v2 Catalog。preflight response eligibility/优先级仍见§5.11；handler和repository必须从本表generated error catalog枚举映射，不能维护第二份message/details。

### 5.8 本地传输边界

首批 KCP 只要求本机 IPC：Unix 使用 Unix Domain Socket，Windows 使用 Named Pipe。帧边界、连接凭据获取和具体序列化承载由 `adr/0003-kcp本地传输.md` 记录为实施选择。KCP 的领域方法与 Envelope 不等同于 JSON-RPC；在实施 ADR 和 Schema 明确前，不得把 JSON-RPC、HTTP 或 TCP 写成 KCP 已选事实。

### 5.9 Extension RPC（区分）

Extension RPC 是 agentd 与 Extension/Provider 进程之间的协议，不在本节定义（见 `extension-protocol` crate 和 `EXTENSION_SDK.md`）。Extension RPC 有自己的消息格式、错误模型和生命周期管理，不与 KCP 共享 envelope；其现有“JSON-RPC 风格”描述也不构成 KCP 的传输决策。

### 5.10 Legacy v1 `system.ping` / `task.create` / `task.get` typed application handler

本节记录**现有未发布Rust代码事实**：三个handler仍绑定v1 generated envelope/payload，其中`task.create v1`允许`parent_task_id`。自ADR-0006与ADR-0009起，它不是active production合同；后续实现必须以active v2 handler替换dispatcher中的v1 create registration，并**删除** v1 create handler/adapter/repository write API，不得以migration/test工具名义保留production可触达写路径。

除task.create世代状态外，本节的response门、clock/ID/backend分层仍是v2实现应复用的实现原则，不能复制平行栈。公共`handle_*`函数当前接受generated typed Envelope，并在family/discriminator/payload variant错配时本地`InputMethodMismatch`；handler不得接受raw JSON或transport frame，也不得把内部错误伪装成`invalid_request`。

#### 5.10.1 输入、输出与最终 Schema 门

handler 输入分别是：

- `system.ping`：discriminator已确认为`system.ping`的typed query与v1 request；
- `task.create`：**当前legacy实现**为discriminator已确认为`task.create`的v1 typed command/request；active实现必须替换为v2 typed request，v1不得由dispatcher传入；
- `task.get`：discriminator已确认为`task.get`的typed query与v1 request。

每次调用输出一个库级 `HandlerResult` 概念值：

```text
HandlerResult =
  | Response(HandledResponse {
      response: KcpResponseEnvelope,
      post_commit_notification_intents: [PostCommitNotificationIntent, ...]
    })
  | ContractFailure {
      failure: HandlerContractFailure,
      post_commit_notification_intents: [PostCommitNotificationIntent, ...]
    }
```

正常 KCP 失败放在 `response.status = error` 中，不作为 transport 异常。构造成功响应时必须先将 typed payload 序列化，并用原方法对应的 `system_ping_response` / `task_create_response` / `task_get_response` Schema 校验；随后将 payload 放入通用 Response Envelope，并再次用 `response_envelope` Schema 校验。成功 payload 校验失败必须改为安全的 `internal_error` 响应，不能发送该 payload；如果连最终 error Response Envelope 也无法通过通用 Schema，handler 必须返回仅供组合根记录/终止该次调用的本地 `ContractFailure`，不得发送未验证响应。

所有响应固定：

- `protocol_version = "1.0"`；
- `message_kind = "response"`；
- `request_id` 与输入 Envelope 字节值原样相同；
- `status = ok` 时 `payload` 非 null 且 `error = null`；`status = error` 时 `payload = null` 且 `error` 非 null；
- Response Envelope 没有 method discriminator。调用方必须依已配对的原请求方法选择成功 payload Schema，不能从响应猜方法。

#### 5.10.2 可注入端口与 backend 边界

实现使用下列高阶端口语义；名称可按 Rust 社区惯例微调，但参数所有权、结果边界和错误分类不得改变：

```text
KernelClock.now_utc() -> RFC3339 UTC instant | ClockError

KernelIdGenerator.next_uuid(purpose: Task | TaskScope | ContentOrigin
  | KernelReceipt | AuditRecord | Event) -> UUID | IdGenerationError
KernelIdGenerator.next_opaque_id(purpose: Correlation | EventDedup)
  -> non-empty string | IdGenerationError

TaskApplicationBackend.create_task(operation: TaskCreateOperation)
  -> Created { current_task: TaskSpec, committed_event_id: UUID }
   | Replayed { current_task: TaskSpec }
   | BackendError
TaskApplicationBackend.get_task(task_id: UUID)
  -> Some(current TaskSpec) | None | BackendError

BackendError =
  | InvalidScopePattern
  | IdempotencyConflict
  | DelegationNotFound
  | ParentTaskNotFound
  | ParentOriginNotFound
  | SqliteBusy
  | SqliteFull
  | SqliteCorrupt
  | StoredDataInvalid
  | Internal
```

`RootTaskCreateAllocationV2`（active root v2）是task component、`kind=object`的正式封闭对象，全部字段required，且自身`schema_version: const 2`。其七个UUID为`task_id,task_scope_id,content_origin_id,kernel_receipt_id,creation_provenance_id,audit_record_id,task_created_event_id`，另有独立`correlation_id,task_created_dedup_key`。`schema_version`不计入七UUID数量。

`BackendError` 是 handler 唯一接受的 backend 失败分类，不携带 SQL 或 payload。SQLite adapter 必须按 `StoreError.code` 穷举转换：同名业务/storage code 转为对应同名变体；`ConstraintViolation`、`ContractInvalid`、`SerializationFailed`、`NotFound`、`InternalStoreError`、`InvalidDatabasePath`、`SqliteOpenFailed`、`SqliteConfigurationFailed`、`MigrationFailed`、`MigrationDrift`、`DatabaseSchemaTooNew`、`InvalidCursor` 及未来未在本合同增加公开映射的 Store code 均转为 `Internal`。handler 只匹配 `BackendError`，不得直接依赖 `kernel-sqlite` 或匹配 `StoreError.message`。

SQLite adapter 的 `create_task` 必须在 adapter/agentd 组合边界调用：

```text
SqliteStore::with_write_transaction(|transaction|
  transaction.create_task(task_create_command))
```

只有 `with_write_transaction` 已成功 commit 后，backend 才能返回 `Created` 或 `Replayed`。`Created.committed_event_id` 必须等于本次 `TaskCreateOperation` 中传入并由 repository 写入 Outbox 的 Event UUID；adapter 若不能证明该绑定必须返回 `BackendError::Internal`，不得产生 Created。handler 构造 notification intent 时使用 `current_task.id` 和 `committed_event_id`，不从响应 payload、查询或第二次分配猜测 Event ID。handler 不复制 URI normalize、receipt/idempotency hash、引用校验、Audit producer 或 Event producer。两种结果都使用 backend 返回的**当前** `TaskSpec` 构造响应，不使用 handler 预先拼装的 Task。

backend error 必须是稳定枚举/分类；SQLite adapter 按 `StoreErrorCode` 穷举映射，严禁匹配 `StoreError.message`。clock/ID 生成失败进入 `internal_error`，且在 backend 调用前失败时不得访问 backend。

#### 5.10.3 时钟、ID 与 deadline 读取点

到达 deadline 的判定固定为两个 RFC 3339 值解析成 UTC instant 后执行 `now >= deadline`，不得按字符串字典序比较。typed decode 已保证 Envelope deadline 符合 Schema；handler 仍必须将该字符串解析成可比较 instant。解析失败视为本地 contract/internal failure，返回 `internal_error`，并且在任何 ID 生成或 backend 访问前失败；clock port 返回的值必须本身是已解析 UTC instant，序列化到响应或 repository 时使用合法 RFC 3339 UTC 文本。第 5.11 节 Value preflight 不读取 clock、不做 deadline 比较；本节每个 typed handler 的**第一个可观察操作**必须是 `KernelClock.now_utc()`，在任何 ID 生成或 backend 访问前完成入口检查。

- `system.ping`：第一次读取为 `started_at`；若到期，立即返回 `deadline_exceeded`。成功候选 payload 的 `kernel_time = started_at`。构造候选后第二次读取 `completed_at`；若读取失败返回 `internal_error`，若 `completed_at >= deadline` 则丢弃成功候选并返回 `deadline_exceeded`。不得访问 Task backend。
- `task.get`：第一次读取为 `started_at` 并入口检查；随后只调用一次 `get_task`；backend 返回成功、`None` 或错误后都进行第二次 `completed_at` 读取。若完成时到期，返回 `deadline_exceeded`，不返回或映射 backend 结果；未到期时，`None` 映射 `task_not_found`，错误按第 5.10.5 节映射。
- `task.create`：第一次也是本次唯一的 `accepted_at` 读取；它同时用于入口检查和全部创建事实时间。**legacy v1 handler**在调用backend前按purpose分配六个UUID与两个opaque ID；active root v2 handler改为按`RootTaskCreateAllocationV2`分配七个UUID与两个opaque ID。SQLite `BEGIN IMMEDIATE` 到 commit/rollback 结束之间不得读取clock、轮询deadline或中途取消。backend 返回 Created、Replayed 或错误后都进行第二次 `completed_at` 读取；若 `completed_at >= deadline`，必须优先返回 `deadline_exceeded`，不再暴露该次 backend 结果。Created/Replayed 的既有事实保留，backend 错误则没有本次新提交事实；均不得声称事务被 deadline 取消。

UUID版本不由任何合同固定：generator必须返回满足对应Schema `format: uuid`的全局唯一UUID。legacy v1本次六UUID必须两两不同；active root v2 allocation的七UUID必须两两不同，并在backend前验证格式/互异性，违反为`internal_error`，不得依赖数据库碰撞间接发现。测试使用deterministic fake，不得把固定测试UUID写成生产派生规则。`correlation_id`与`dedup_key`不是caller-owned，也不从`request_id`、`idempotency_key`或Task ID推导：generator分别生成独立、非空opaque值；本次Audit/Event共用同一个correlation，dedup值对该Event全局唯一。repository继续只校验并写入。

完成检查的优先级固定：backend 已返回后，第二次 clock 失败映射 `internal_error`；第二次 clock 成功且到期映射 `deadline_exceeded`；只有未到期时才返回/映射 backend 结果。ID 生成在 backend 前失败时直接 `internal_error`，不做无意义的完成检查，也不访问 backend。

`task.create` commit 后才发现 deadline 到期时，客户端恢复规则固定为：以同一 `(actor.id, entry_point, "task.create", idempotency_key)` 和同一业务投影重放；重放会返回当前 Task；已知 task ID 时也可调用 `task.get`。客户端不得因 `deadline_exceeded` 自动换新 idempotency key。三个方法的 `deadline_exceeded.retryable` 都是 `true`：两个 Query 可安全重试，`task.create` 只能按上述相同幂等键重试。post-commit clock 失败同样不能回滚事实，返回 `internal_error` 并保留下述 notification intent。

#### 5.10.4 post-commit notification intent

`Created { current_task, committed_event_id }` 结果产生且只产生一个 `TaskCreatedCommitted { task_id: current_task.id, event_id: committed_event_id }` post-commit notification intent；`committed_event_id` 的可信绑定由第 5.10.2 节 backend 合同保证。`Replayed`、`system.ping`、`task.get` 和所有 commit 前失败均不产生 intent。该 intent 只是通知组合根“持久 Outbox 已有一条已提交的 `task.created`，可唤醒 Publisher”的 best-effort 提示，不是 EventEnvelope，不重复写 Outbox，也不表示 Publisher 已发布或任何订阅者已消费。

一旦 backend 返回 `Created`，无论后续得到成功、完成时钟 `deadline_exceeded`、时钟/response contract `internal_error` 或本地 `HandlerContractFailure`，handler 的返回路径都必须保留该 intent，使组合根可以尝试通知；响应无法发送也不能吞掉已提交通知事实。组合根在数据库事务外消费 intent；notifier/publisher wake-up 失败只能记录并依赖 durable Outbox 后续扫描恢复，不能回滚 Task、不能把已经构造的成功响应改成错误，也不能把 deadline/internal 响应改写成别的 code。

#### 5.10.5 Legacy三handler映射说明

本表只记录现有legacy三handler的实现回归；active v2全量wire Error Catalog唯一事实源是§5.7。实现升级时必须从generated §5.7 catalog取message/details/retryable，不能让本legacy子集覆盖权威表。

本批所有 error 的 `schema_version = 1`、`details = null`。message 是固定安全字符串，不拼接 `StoreError.message`、SQL、路径、payload、内部 ID 或堆栈：

| handler 条件 / backend 分类 | KCP `code` | 固定 `message` | `retryable` |
|---|---|---|---:|
| 三方法入口或完成检查到期 | `deadline_exceeded` | `request deadline exceeded` | `true` |
| `task.get` 返回 `None` | `task_not_found` | `task was not found` | `false` |
| `InvalidScopePattern` | `invalid_scope_pattern` | `task scope contains an invalid URI pattern` | `false` |
| `IdempotencyConflict` | `idempotency_conflict` | `idempotency key was used for different task facts` | `false` |
| `DelegationNotFound` | `delegation_not_found` | `delegation was not found` | `false` |
| `ParentTaskNotFound` | `parent_task_not_found` | `parent task was not found` | `false`（legacy TaskCreate v1 only） |
| `ParentOriginNotFound` | `parent_origin_not_found` | `parent content origin was not found` | `false` |
| `SqliteBusy` | `sqlite_busy` | `kernel storage is busy` | `true` |
| `SqliteFull` | `sqlite_full` | `kernel storage is full` | `false` |
| `SqliteCorrupt` | `sqlite_corrupt` | `kernel storage is corrupt or invalid` | `false` |
| `StoredDataInvalid` | `stored_data_invalid` | `stored task data failed integrity validation` | `false` |
| `ConstraintViolation`、`ContractInvalid`、`SerializationFailed`、`NotFound`、`InternalStoreError`，以及 open/config/migration/schema-too-new/drift、clock、ID、response contract 等其余失败 | `internal_error` | `internal kernel error` | `false` |

方法适用性固定：`system.ping` 只产生 deadline 或 internal；`task.get` 还产生 task-not-found 与读取 backend 的 storage/internal 映射；`task.create` 还产生五个业务错误与写 backend 的 storage/internal 映射。`invalid_request`、版本/auth/method/schema preflight 错误不属于本表；本批也不新增 `method_unavailable`。

#### 5.10.6 阶段门

三个 typed handler 与第 5.11 节的 Value preflight/注册式 dispatcher 都只能作为不可连接的库级边界。UTF-8、JSON parse、bytes/frame、最大尺寸、transport path/pipe name、连接 ACL/credentials 和 server 生命周期未关闭前，不得提供可连接 Socket/Named Pipe server。五个已知但未注册方法没有正式 handler 时，不得把它们描述为可用，也不得用 `method_unavailable`、`unsupported_method` 或任一 wire error 掩盖实现缺口。

### 5.11 `serde_json::Value` preflight 与三方法注册式 dispatcher

本节闭合不可连接的库级 raw-value 边界：唯一输入是调用方已经解析得到的 `serde_json::Value`。bytes、UTF-8、JSON parse、4-byte frame、最大尺寸、transport、clock、backend 与 Kernel ID 均不在本层。实现必须公开分步 API `preflight_value -> narrow_to_registered -> TypedDispatcher.dispatch`；不得提供名为 `process_value` 或语义等价的一站式 API，使调用方误以为首批八方法都已可执行。建议将该边界直接加入现有 `kernel-kcp` crate，复用其 handler/ports/response 门；如因依赖方向拆分 module/crate，也不得复制 response 构造、Catalog 或 typed handler abstraction。

#### 5.11.1 公共概念值与分步 API

公共概念值固定为：

```text
preflight_value(value: serde_json::Value) -> PreflightResult

PreflightResult =
  | Accepted(TypedCatalogRequest)
  | Response(KcpResponseEnvelope)
  | LocalRejection(PreflightLocalRejection)

TypedCatalogRequest =
  | Command(TypedKcpCommandEnvelope)
  | Query(TypedKcpQueryEnvelope)

narrow_to_registered(request: TypedCatalogRequest) -> RegistrationResult

RegistrationResult =
  | Registered(RegisteredRequest)
  | KnownCatalogMethodNotImplemented(KnownCatalogMethodNotImplemented)

PreflightLocalRejection =
  | UncorrelatableRequest {
      kind: UncorrelatableRequest,
      message: "request cannot be correlated"
    }
  | ContractFailure {
      kind: ContractFailure,
      message: "preflight contract failure"
    }

KnownCatalogMethodNotImplemented =
  | TaskList
  | EventSubscribe
  | EventPoll
  | StopActivate
  | StopStatus

RegisteredRequest =
  | SystemPing(TypedKcpQueryEnvelope)
  | TaskCreate(TypedKcpCommandEnvelope)
  | TaskGet(TypedKcpQueryEnvelope)

TypedDispatcher::new(clock, ids, task_backend)
TypedDispatcher.dispatch(request: RegisteredRequest) -> HandlerResult
```

名称可按 Rust 社区惯例微调，但三步边界、结果分类和所有权不得合并。`TypedCatalogRequest` 必须携带 generated typed Envelope，不能退回 `Value`、手写 payload 或只保留 method string。`RegisteredRequest`的目标active集合仍为`system.ping`、`task.create`、`task.get`，但`task.create` variant必须是v2。当前代码中的v1 variant仅legacy实现，active narrow不得构造。

`KnownCatalogMethodNotImplemented` 只用于已通过完整 Schema 与 generated typed decode 的 `task.list`、`event.subscribe`、`event.poll`、`stop.activate`、`stop.status`。它是本地库结果，不是 `KcpError`，不得实现 `Serialize`，不得进入 Response Envelope，也不得转换为 `unsupported_method`、`method_unavailable` 或 `internal_error`。组合根在这五个方法拥有正式 handler 前必须 fail closed 且禁止启动 KCP server；不能接收请求后再用本地缺口冒充协议错误。

`TypedDispatcher` 是显式三方法注册表，不是按字符串开放反射的 router。它在构造时接收第 5.10 节已有的 `KernelClock`、`KernelIdGenerator` 与 `TaskApplicationBackend` 引用/所有权；dispatch 时按 variant 只把所需端口传给对应 handler：ping 使用 clock，create 使用 clock/ids/backend，get 使用 clock/backend。它只把三个 `RegisteredRequest` variant 路由到第 5.10 节对应 handler；不得重复 Schema、protocol/auth/method、payload version 或 deadline 检查，不得改写 handler 的 `HandlerResult`，并必须无损保留 `post_commit_notification_intents`。不得为 dispatcher 创造平行 clock/backend/ID 接口。新增注册方法必须同时增加正式 handler、注册 variant、dispatcher 路由、Conformance 与 server 阶段门更新。

#### 5.11.2 固定判定优先级

`preflight_value` 必须严格按下列顺序短路；较低优先级错误不得覆盖较高优先级结果：

1. `request_id` response eligibility；
2. `message_kind` 与 command/query family；
3. `protocol_version`；
4. `auth`；
5. family-specific method discriminator；
6. 根 `payload.schema_version` 的JSON形状与method-aware active version；
7. 完整 Command/Query Envelope Schema、所选方法+版本payload Schema与generated typed decode。

第一步要求输入是 JSON object，且顶层 `request_id` 是可由 UUID parser 接受的 string。合法文本必须原样复制到任何 error response，不得重新格式化、大小写规范化或生成替代 ID。`Value` 非 object，或 `request_id` 缺失、非 string、不是合法 UUID时，返回 `PreflightLocalRejection::UncorrelatableRequest`，不得发送 wire response。

有合法可关联 `request_id` 后：

- `message_kind` 缺失、非 string、值为 `response` 或任一未知值，返回 `invalid_request`；只有 `command` / `query` 选择对应 family；在 family 确定后只解释该 family 的 discriminator，另一 family 的 discriminator 若存在则留给最终 Envelope Schema 按未知字段归为 `invalid_request`；
- `protocol_version` 缺失或非 string，返回 `invalid_request`；string 但不等于 `"1.0"`，返回 `unsupported_protocol_version`；
- `auth` 缺失返回 `invalid_request`；任意非 null 值返回 `unsupported_auth_schema`；
- command family 只读取 `command_type`，query family 只读取 `query_type`。对应字段缺失或非 string返回 `invalid_request`；string 不在该 family 的完整 generated Catalog 返回 `unsupported_method`。因此 query `task.create`、command `task.get` 等 family 错配固定为 `unsupported_method`，即使该名称存在于另一 family；
- method判断必须使用generated Catalog或由同一生成源导出的等价闭集，禁止在preflight手写第二份目录；
- `payload`缺失或非object，或其根`schema_version`缺失、不是`serde_json::Number`的`is_i64() || is_u64()`整数、`<=0`，返回`invalid_request`；这拒绝`1.0`等浮点表示及不可保留的超范围数值；
- 根version是正整数后，按已选family+method查询generated method-version catalog。不存在该version返回`unsupported_schema_version`。active `task.create`只接受2；version 1虽是known legacy schema也必须返回该错误，不能进入typed Accepted或registration。其他方法在各自升级前可继续只接受1；
- 只有根`payload.schema_version`参与版本优先分类。嵌套对象version、普通业务字段、enum、format、unknown field或方法payload其它约束失败由完整Schema门归为`invalid_request`。

前述结构检查完成后，必须选择该方法+active version对应的generated Envelope/payload Schema验证，再调用对应generated typed decode。普通caller输入不满足完整合同返回`invalid_request`；成功时只返回active-version `Accepted(TypedCatalogRequest)`。v1 `task.create`在active preflight只得到`unsupported_schema_version`，不得借active preflight进入dispatcher；独立fixture/historical Schema validation不得伪装为production write入口。

#### 5.11.3 caller invalid 与本地合同失败

`kernel-contracts` 在实现本节前必须新增结构化的 preflight decode 阶段分类，而不是让上层从现有 `ContractError` 文本猜测。最小合同固定为：

```text
ContractFailureStage =
  | CallerSchemaValidation
  | WireDecodeAfterSchema
  | PayloadDecodeAfterSchema
  | GeneratedDiscriminatorMapping
  | SchemaCatalog

ClassifiedContractFailure {
  stage: ContractFailureStage,
  schema_id: <string> | null,
  safe_kind: CallerInvalid | InternalContractFailure
}
```

`kernel-contracts` 可以通过扩展 `ContractError` 变体，或提供接收明确调用阶段的结构化分类 API 实现该形状；但 preflight 不得自行检查错误 message。映射必须穷举且固定：

| 发生点 / 结构化 stage | preflight 结果 |
|---|---|
| 已知目标 Schema 对 caller `Value` 的 validation violation（`CallerSchemaValidation`） | wire `invalid_request` |
| Envelope Schema 已通过后，generated raw wire struct 反序列化失败（`WireDecodeAfterSchema`） | 本地 `ContractFailure` |
| 方法 payload Schema 已由 Envelope 条件映射通过后，generated payload type 反序列化失败（`PayloadDecodeAfterSchema`） | 本地 `ContractFailure` |
| 已通过步骤 5 的 generated Catalog method 无对应 generated match、variant/discriminator 映射冲突或缺失（`GeneratedDiscriminatorMapping`） | 本地 `ContractFailure` |
| embedded catalog 解析/编译失败、目标 Schema ID 缺失或 `$ref`/catalog 装载失败（`SchemaCatalog`） | 本地 `ContractFailure` |

现有 `ContractError::SchemaValidation` 不能同时表示第一行和 post-Schema decode 三行；实现时必须拆分或由调用 API 显式标记 stage。`UnknownSchema` / `Catalog` 固定归入 `SchemaCatalog`；typed decode 在步骤 5 已确认 method 后若仍出现“unsupported command/query discriminator”，固定归入 `GeneratedDiscriminatorMapping`，不能再次变成 caller 的 `unsupported_method`。`Canonicalize` / `InvalidJson` 不应出现在已接收 `Value` 的正常 preflight；如因内部 response 或 catalog 路径出现，归入 `InternalContractFailure`。

preflight 调用顺序必须保持：先单独执行完整 generated Envelope Schema validation；该调用只有明确的 caller validation violation 才映射 `invalid_request`。验证成功后再执行 generated typed decode；decode 后阶段的任何失败均按上表本地 fail closed。generated `deserialize_wire` / `deserialize_payload` 必须产生可区分的结构化 stage，`schema-tool` 生成模板和 generated drift 测试必须共同锚定该事实。

preflight 不得按 `ContractError` 的显示文本、JSON Schema 错误字符串或 serde message 猜测失败来源。本地 `ContractFailure` 只暴露上文固定的 safe kind/message，不包含 schema ID、method、payload、Secret、Schema error 全文、路径或堆栈；结构化 `schema_id` 只能留在内部诊断对象，不进入该公共本地拒绝值。

#### 5.11.4 固定 preflight error response

可关联错误全部构造通用 `KcpResponseEnvelope`，固定 `protocol_version="1.0"`、`message_kind="response"`、原样 `request_id`、`status="error"`、`payload=null`，并使用下表。所有 error 固定 `schema_version=1`、`details=null`、`retryable=false`：

| 条件 | `code` | 固定 `message` |
|---|---|---|
| caller value/Envelope/payload 不合法 | `invalid_request` | `request is invalid` |
| protocol string 不是 `1.0` | `unsupported_protocol_version` | `protocol version is not supported` |
| 根 payload schema positive integer 不在所选 family+method 的 active request version 集合 | `unsupported_schema_version` | `payload schema version is not supported` |
| method 不属于所选 family Catalog | `unsupported_method` | `method is not supported` |
| auth 非 null | `unsupported_auth_schema` | `authentication schema is not supported` |

最终 error response 必须复用 `kernel-kcp` 现有通用 KCP error/Response 构造与 crate-private generated response Schema finalizer；不得复制第二套 response 栈或公开 validator 注入点。生产 public API 不接受 validator、Schema catalog 或 bypass flag；fault seam 只能是 crate-private unit-test seam，不能成为 feature、SDK port 或依赖注入接口。如果最终 error response 无法序列化或未通过通用 Response Schema，返回本地 `PreflightLocalRejection::ContractFailure`，不得发送未验证 response，也不得降级为另一 wire error。

#### 5.11.5 阶段门与非目标

本节不改变 ADR-0003 的 transport 决策，也不实现 bytes/UTF-8/JSON parse/frame、连接身份、server、clock、deadline、backend、ID 或五个缺失 handler。preflight 只建立“可关联并完整 typed”的请求事实；deadline 仍由三个 registered typed handler 按第 5.10 节检查。首批八方法的 Schema/Catalog 承诺与“当前只有三方法可执行”必须同时真实表达，禁止以技术兜底制造看似可用但 dispatcher/handler 关系不一致的 server。

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
parent_task_id?          // Child Task 指向父 Task
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

`ActionRequest.permission_decision_ref`在Action初始`pending`且尚未完成首次评估时可以为`null`；每次评估完成后必须立即指向current PermissionDecision。Action进入`approved`前，该decision必须为allow，或其confirmation要求已由可消费的Approval v2 current resolution满足。

```text
action_id
task_id
step_id?
parent_action_id?
capability_id
operation
structured_arguments
resource_refs[]
task_scope_ref
side_effect_class: S0 | S1 | S2 | S3 | S4 | S5
idempotency_key
execution_generation       // 每次Lease/执行尝试单调增加；不扩大Action业务唯一性
permission_decision_ref?
approval_chain_id?       // required-nullable；确需Approval时为稳定chain id，implicit/delegation为null
verification_policy: { strategy, expected_outcome, timeout }
rollback_policy?: { compensatable, compensation_action_ref?, auto_rollback_on }
result?: {
  materialized_child_task_ref? // 仅kernel.task/task.child.create completed时存在
  verification_result_refs[]
}
status: ActionStatus
recovery_meta?: {...}
lease?: { holder, generation, expires_at, max_uses }
schema_version
revision
created_at / updated_at
```

#### 6.5.1 `kernel.task/task.child.create`

该Action的固定语义：

- `task_id`是parent Task权威；`capability_id=kernel.task`、`operation=task.child.create`、`side_effect_class=S1`；
- `structured_arguments`必须是§5.5的`ChildTaskProposalV1`；禁止child id/parent/status/revision/time；
- `resource_refs`至少包含parent Task stable ref及proposal声明的material scope refs摘要；不得用空refs隐藏父子关系；
- PermissionDecision material fingerprint覆盖proposal hash、父子scope/capability/delegation delta；
- 一个`action_id`全生命周期最多一个`materialized_child_task_ref`，execution generation或Lease重取不能产生第二个child；
- completed必须引用Kernel-local materialization VerificationResult；外部副作用为none，但业务事实创建仍按S1审计/恢复；
- Action completion与child facts必须同事务；正式Outbox为child `task.created`与Action `action.state_changed`，后者使用transition anchor。

### 6.6 PermissionDecision v2

PermissionDecision v1仅保留Schema/fixture历史验证资产，无production read/write/migration。active evaluation写v2不可变记录：

```text
id
schema_version: 2
action_id
decision: allow | deny | require_confirmation | require_local_confirmation
         | require_system_authentication | require_remote_signature
         | require_plan_revision
reason_codes[]
matched_rule_ref?
decision_revision
evaluated_at
policy_set_revision
material_authorization_fingerprint
observation_evidence_fingerprint
binding: {
  action_id
  action_revision
  task_id
  plan_version
  capability_id
  operation
  side_effect_class
  resource_refs[]
  key_params_hash
  delegation_authority_ref?
}
approval_requirement?: {
  confirmation_mode
  approval_chain_id?
  reusable_resolution_ref?
}
expires_at?
lease_ref?
```

- material fingerprint对规范化授权实质事实做RFC8785+SHA-256；observation fingerprint独立覆盖snapshot/generation/provider refs/coordinate transform/observation evidence等瞬时事实；
- 任一observation变化都使旧decision/Lease不可消费并要求新观察/新decision；
- 若且仅若Core证明新material fingerprint与旧approved resolution绑定值相等，且旧resolution未失效/过期，可由新decision引用`reusable_resolution_ref`；Profile无权判断等价；
- material变化必须使旧Approval head invalidation并重评；仍需确认时创建新request；
- Default Allow生成allow decision但不创建Approval；Delegation authority写入binding/审计，不创建Approval；
- 同一Action的decision_revision严格递增，每次重评均新ID；Action指向current decision。

### 6.7 PolicyRule v2

引用 SECURITY_PRIVILEGE.md 为权限判定权威，此处定义 production PolicyRule v2存储字段；v1仅保留Schema/fixture历史验证资产，无production read/write/migration：

```text
id
schema_version: 2
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
confirmation_mode?       // effect=confirm 时必填：generic|local|system_authentication|remote_signature|plan_revision；其他 effect 禁止
expires_at?
created_by / updated_by  // actor + entry_point
created_at / updated_at
source: user_defined | companion_generated | system
```

排序、Pattern、Condition、Specificity 和 Default Allow 语义只由 `SECURITY_PRIVILEGE.md` 定义。`actor_match.source_patterns[]` 使用与 ContentOrigin source 相同的规范化 URI segment-glob 语法；空数组表示不限制。Schema 必须实施 `effect = confirm` 与 `confirmation_mode` 的条件约束。Read-only/Restricted 等命名 Mode 若产生限制，必须投影为可见 PolicyRule；Safe Recovery 与 Stop Fence 只在维护 Kernel 一致性和禁止未知副作用盲目重放的范围内作为不可覆盖 Recovery Invariant，不构成第二套通用权限矩阵。

实现约束（`domain-policy` matcher 内部）：普通未匹配与真实评估错误必须用 typed outcome 分离（Matched / NotMatched / `PolicyError`）。禁止用 `invalid_policy_rule` 或任意 magic message 充当“未匹配”哨兵；真实 `PolicyError` 一律 fail closed，不得落入 Default Allow。该约束不改变公开 API、错误码、排序、求值顺序或 winner-only rate-limit 语义。

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

#### 6.9.1 Resource containment（纯边界判断）

`domain-policy` 提供纯函数 `resource_refs_within_task_scope(resource_patterns, exclusions, resource_refs) -> Result<bool, ResourceContainmentError>`，只回答“给定 concrete resource URI 列表是否全部落在该 TaskScope 的 include/exclude 边界内”。它：

- **不是授权**：`Ok(true)` / `Ok(false)` 不产生 Policy 决策、PermissionDecision、Approval，也不修改 Scope；
- **不复用 PolicyRule applicability**：不得调用 Policy matcher 的 `match_resources`（后者是“规则是否适用于任一 resource”的规则匹配语义）；只复用底层 Policy URI parse/normalize/segment-glob match；
- **include 空数组 = 不限制**；**exclude 优先于 include**；**每个 resource 都必须满足**；`resource_refs` 为空时，在完整验证 patterns 后返回 `Ok(true)`；
- **先完整验证全部输入，再返回 containment 布尔值**：前面的越界 resource 不得提前 `Ok(false)` 而掩盖后面的非法 URI/pattern；
- **stored TaskScope pattern 必须合法且已规范化**（`normalize_uri_pattern(pattern) == pattern`），否则 `InvalidScopePattern`（含 input kind + index）；concrete resource URI 可先规范化再匹配，非法则 `InvalidResourceUri`（含 index）；
- 数组**顺序/重复不影响布尔结果**，且**不得修改输入数组**；query/fragment 与 `*` / `**` 完全复用 `SECURITY_PRIVILEGE.md` §2.1 现有语义。

错误结构必须可机读：公开 `ResourceContainmentErrorCode::{InvalidScopePattern, InvalidResourceUri}`、`ResourceContainmentInputKind::{ResourcePattern, Exclusion, ResourceRef}`、`index`，并保留底层 `PolicyError` 作为 `source`（调用方可经 `source()` / 结构化访问取得，不得仅靠 message 文本匹配）。

### 6.10 ApprovalRecord v2

ApprovalRecord v1仅可作为Schema/fixture历史验证资产，production read/write/migration均禁止（ADR-0009）。v2必须由SchemaGraph表达为两层真正tagged union，禁止任何全部optional的平面替代：外层`record_kind`，内层`subject.subject_kind`。公共顶层字段表：

| 字段 | 类型 | required/null规则 |
|---|---|---|
| `id` | UUID | required |
| `schema_version` | const `2` | required |
| `approval_chain_id` | UUID | required；整条链稳定不变 |
| `predecessor_ref` | UUID\|null | **required-nullable**；初始request为null，其余必须非null且等于append前current head |
| `record_kind` | `request|resolution|invalidation` | required discriminator |
| `subject` | SubjectV2 | required；每条链所有record逐字同一canonical subject |
| `created_at` | UTC timestamp | required |
| `expires_at` | UTC timestamp\|null | required-nullable；request/resolution可非null，invalidation固定null |
| `record` | kind-specific object | required；由`record_kind`选择 |

`SubjectV2`公共字段只有`subject_kind`，每支`additionalProperties=false`：

| subject_kind | required字段 |
|---|---|
| `operation` | `task_id`、`task_revision`、`task_plan_version`、`action_id`、`action_revision`、`permission_decision_ref`、`permission_decision_revision`、`policy_set_revision`、`material_authorization_fingerprint`、`capability_id`、`operation`、`side_effect_class`、`resource_refs_hash`、`key_params_hash` |
| `task_proposal` | `candidate_task_id`、`candidate_revision`、`proposal_hash`、`proposer_actor_ref`、`task_scope_hash`、`delegation_ref|null`、`policy_set_revision` |
| `plan_revision` | `task_id`、`task_revision`、`base_plan_version`、`proposed_plan_version`、`proposed_plan_hash`、`policy_set_revision` |

operation支绑定执行时的Action与PD revision，`task_plan_version`不可省略；任何Action/PD/policy/plan/material/capability/operation/side-effect/resource/key参数变化都使resolution不可消费。`resource_refs_hash`来自Material projection的normalized URI set，`key_params_hash`必须等于同projection内完整normalized key params的hash。

`record`字段表：

| record_kind | required字段与null规则 |
|---|---|
| `request` | `request_id`(等于顶层id)、`confirmation_mode=generic|local|system_authentication|remote_signature|plan_revision`、`requested_by_actor`、`requested_from_entry_point`、`reason_codes[]`、`challenge_ref`、`request_expires_at`；`challenge_ref`仅`remote_signature|system_authentication`必须非null，其余固定null；`request_expires_at`必须等于顶层`expires_at`且非null |
| `resolution` | `request_ref`、`decision=approved|denied`、`resolved_by_actor`、`resolved_from_entry_point`、`resolved_at`、`evidence_refs[]`、`remote_response_ref|null`、`local_presence_evidence_ref|null`、`system_auth_evidence_ref|null`；evidence branch必须与原request confirmation mode精确匹配；顶层`expires_at`是approved有效期，denied固定null |
| `invalidation` | `invalidated_record_ref`、`reason_code=material_changed|subject_revision_changed|policy_requirement_changed|delegation_inactive|credential_revoked|evidence_expired|approval_expired|stop_fence_activated|manual_revocation|migration_quarantine`、`invalidated_at`、`invalidated_by_actor`(Actor\|null)、`invalidated_from_entry_point`、`replacement_request_ref`(UUID\|null)；顶层`expires_at=null` |

request、resolution、invalidation都不另设顶层actor/evidence可选拼装；身份字段只在对应record支中出现。数组`reason_codes`/`evidence_refs`按producer顺序保留，重复拒绝（它们是stable ref/code set）；空`reason_codes`非法。mode-specific resolution矩阵是wire硬约束：

| mode / decision | 专属ref | `evidence_refs`精确集合 |
|---|---|---|
| `generic`, approved或denied | 三个专属ref均null | `[]`；resolver actor/entry是唯一决议证据 |
| `local`, approved或denied | `local_presence_evidence_ref`非null，其余null | 精确`[local_presence_evidence_ref]` |
| `system_authentication`, approved | `system_auth_evidence_ref`非null，其余null | 精确`[system_auth_evidence_ref]`；denied只能表示真实完成的拒绝且同样需要该evidence，取消/认证失败不创建resolution |
| `remote_signature`, approved或denied | `remote_response_ref`非null，其余null | 精确`[remote_response_ref]` |
| `plan_revision`, approved或denied | 三个专属ref均null | `[]`；subject必须为`plan_revision` |

`evidence_refs`不能null、不能包含额外审计ref、不能重排出第二种合法表示。resolution `request_ref`必须指向直接前驱request；invalidation `invalidated_record_ref`必须等于被替换/终止的旧head。

#### 6.10.1 canonical confirmation词表、SubjectProjection与链CAS

Policy、PermissionDecision、Approval request统一使用算法无关闭集：

```text
ConfirmationModeV1 = generic | local | system_authentication | remote_signature | plan_revision
```

PolicyRule v2 production write的`confirmation_mode`只允许该闭集；PolicyRule v1按逐对象lifecycle保留Schema/fixture历史验证资产，无production read/write/migration；不因v2出现而把其它v1对象整体判legacy。PermissionDecision v2映射固定为：`generic→require_confirmation`、`local→require_local_confirmation`、`system_authentication→require_system_authentication`、`remote_signature→require_remote_signature`、`plan_revision→require_plan_revision`。Approval request wire直接复用相同canonical值，不保存算法名或旧别名；UI可显示文案但不得持久化别名。`remote_signature`是正式Policy入口且不绑定算法；算法只由远程response的algorithm tagged union决定。

`SubjectProjectionV1`是`subject_hash`唯一preimage，全部branch封闭且字段与Approval `SubjectV2`一一对应：`schema_version=1`加对应`subject_kind`及§6.10 Subject表的全部字段；字段名、nullability与枚举完全相同，不包含Approval record/chain/head/request/resolution、时间、confirmation mode或evidence。规范化固定为：UUID lowercase canonical text；resource/key/material/proposal/scope/hash为已验证lowercase 64-hex；URI字段若未来branch增加必须按§5.3.1 URI规则；时间若未来增加必须UTC秒精度；普通字符串不trim/Unicode改写；数组仅按Subject branch显式规则处理，当前operation/task_proposal/plan_revision均无可变数组。构造→Schema验证→RFC8785 JCS→UTF-8无BOM/换行→SHA-256 lowercase，结果即`subject_hash`。

Approval `subject` wire保存完整`SubjectV2`，不得只保存hash；repository从canonical subject重建`SubjectProjectionV1`并校验hash。Remote challenge/response/preimage与SystemAuthenticationEvidence统一绑定该`subject_hash`；LocalPresenceEvidence不复制hash，但resolution时必须经request_ref间接绑定同一subject。Official fixture必须包含三branch各一份subject、projection、JCS bytes与hash，operation fixture还必须被remote/system evidence vector共同复用；任一subject字段tamper都改变hash。

Action持久字段统一为`approval_chain_id: UUID|null`；PermissionDecision的`approval_requirement.approval_chain_id`、ApprovalRecord顶层`approval_chain_id`与Action字段必须完全相等，不得另造近义chain字段。Repository暴露闭集方法：

- `append_request(approval_chain_id, expected_head_ref=null, request_record, event_allocation)`；**只创建新链**，链已存在、expected非null或request predecessor非null均拒绝；不得用于replacement；
- `resolve(approval_chain_id, expected_head_ref, resolution_record, authorization_evidence, event_allocation)`；expected/current必须为request；
- `invalidate_and_optionally_replace(approval_chain_id, expected_head_ref, invalidation_record, replacement_request|null, event_allocation)`；current必须为request或approved resolution，replacement若非null必须在同事务成为新head，且invalidation的`replacement_request_ref`等于新request id、replacement的`predecessor_ref`等于invalidation id。

repository current-head查询/映射字段统一命名`current_head_ref`；它不是ApprovalRecord重复字段。

每个方法在单一`BEGIN IMMEDIATE`中验证canonical subject equality、expected head CAS、record Schema、subject current revisions/hash、evidence/expiry；append immutable record(s)，更新唯一current-head映射，更新Action/Task current approval关联，撤销受影响Permission/Privilege/Action Lease，写Audit与规定Outbox。任一步失败全回滚。不得提供update/delete；不得按created_at/max(id)猜head；unique keys至少保证record id唯一、每approval_chain_id一个current_head_ref、`(approval_chain_id, predecessor_ref)`在非null predecessor下最多一个committed successor。CAS loser返回`approval_head_conflict`。

`validate_usable_resolution`必须同时读取current head、subject当前事实、Action current PD、material hash、credential/evidence/challenge状态、resolution expiry与Stop Fence；任何不满足返回稳定错误，绝不自动fallback到更老approved record。

#### 6.10.2 Identity repositories、Challenge终态与证据对象

Identity事实必须由独立repository拥有，不允许Approval repository用opaque JSON临时保存：

| repository | 闭集方法与CAS |
|---|---|
| Credential | `register`、`rotate(expected_revision,new_credential)`、`revoke(expected_revision,reason,revoked_at)`、`get(credential_id,revision|null)`；每id revision从1连续，至多一个current active，rotate原子标记旧revision replaced/revoked并写新revision |
| Challenge | `issue`、只读`get`、`revoke(expected_state=issued)`、`consume(expected_state=issued,consumed_at)`、`expire_challenge_with_expected_state(expected_state=issued,expired_at,audit_allocation)`；状态只允许issued→consumed\|expired\|revoked，nonce/`(challenge_id,request_ref)`唯一 |
| Local evidence | `insert_local_presence`、`get_local_presence`；immutable canonical，只允许受信本地producer |
| System evidence | `insert_system_authentication`、`get_system_authentication`；immutable canonical，只允许注册OS adapter producer |

Challenge的`get`只返回当前已持久化事实，**绝不**因现在已过`expires_at`而写状态。任何会消费或解析challenge的事务（remote `resolve`、system `resolve`及直接`consume`）必须在同一`BEGIN IMMEDIATE`内遵守以下唯一语义：读取canonical challenge后，若`state=issued && now>=expires_at`，先调用`expire_challenge_with_expected_state(expected_state=issued, expired_at=now, audit_allocation)`作CAS；CAS winner在该事务写入`state=expired`、identity/security Audit，提交后返回`challenge_expired{challenge_id,expires_at}`；Challenge不是Approval chain，**不得**写`approval.state_changed`。CAS loser必须重读current challenge：读到`expired`仍返回同一`challenge_expired`，读到`consumed`/`revoked`则按其真实终态码返回。若初读已是`expired`，直接返回同一`challenge_expired`。因此并发过期resolve/consume至多一条失效Audit，且所有观察到过期的调用得到同码；后台sweeper可以调用相同helper清理issued记录，但绝不是正确性依赖。

`expire_challenge_with_expected_state`必须只接受当前`issued`为expected state，校验exact-version Schema、JCS bytes和`expires_at<=expired_at`，原子CAS写终态与identity/security Audit；不得由get、UI读取或sweeper之外的旁路UPDATE改写状态。其Audit allocation由调用上层显式提供正式`AuditAllocationV2`（§6.16.0a：`audit_record_id,correlation_id,occurred_at,causation_ref`），必须有独立UUID、opaque correlation与真实causation；它与Approval Event allocation不是同一对象，且不写Approval event；Audit `audit_type`固定`identity.challenge_expired`（§6.16.2）。

所有get执行exact-version Schema、canonical JCS byte equality、关系镜像与authority校验；损坏返回`stored_data_invalid`。Credential/Challenge/Evidence wire的字段authority分别是credential registrar/issuer、Kernel challenge issuer、受信transport/desktop attestor、注册OS authentication adapter；caller不能覆盖status、revision、nonce、时间、verifier或hash。remote/system resolution必须通过Approval repository持有的**transaction-bound helper**调用challenge/credential/evidence读取、验证、expire/consume；helper只能从同一个`BEGIN IMMEDIATE` transaction取得，禁止在事务外读取后再append resolution，禁止nested connection造成TOCTOU。

`LocalPresenceEvidenceV1`字段全部required：`schema_version=1,id,session_ref,transport_kind=unix_peer|windows_pipe_peer|desktop_session,peer_principal_ref|null,observed_actor,entry_point,challenge_ref|null,presence_kind=interactive_session|explicit_local_confirmation,observed_at,valid_until,verifier_kind=kernel_transport|desktop_client_attestation,evidence_hash`。只由Kernel/受信本地边界producer写；`valid_until>observed_at`；resolution的actor/entry/challenge必须匹配。

`SystemAuthenticationEvidenceV1`字段全部required：`schema_version=1,id,mechanism=polkit|windows_uac|macos_authorization_services|other_versioned,mechanism_version,platform_subject_ref|null,challenge_ref,subject_hash,material_authorization_fingerprint,result=verified,verified_at,valid_until,verifier_ref,evidence_blob_hash`。取消/失败不创建该对象；`other_versioned`必须有非空机制version且未来Schema允许的adapter，不接受自由“success”字符串。

#### 6.10.3 Remote/System challenge与resolution纪律

`RemoteApprovalChallengeV1`全部required：`schema_version=1,challenge_id,approval_chain_id,request_ref,audience,nonce,nonce_encoding:"base64url_no_pad",task_id,subject_hash,material_authorization_fingerprint,allowed_decisions:["approved","denied"],credential_ref,issued_at,expires_at,state:issued|consumed|expired|revoked,consumed_at:null|timestamp,revoked_at:null|timestamp,revocation_reason:null|string`。`allowed_decisions`必须精确为该顺序且无重复；nonce由Kernel CSPRNG生成，解码后至少32 bytes；`audience`是部署配置的canonical URI且进入签名；TTL固定上限5分钟、`expires_at>issued_at`。状态转换仅`issued→consumed|expired|revoked`，终态不可逆。

`SystemAuthenticationChallengeV1`是Kernel为`system_authentication` request签发的独立challenge，不得借用Remote对象或把OS认证塞入Envelope auth。全部字段required：`schema_version=1,challenge_id,approval_chain_id,request_ref,nonce,nonce_encoding:"base64url_no_pad",task_id,subject_hash,material_authorization_fingerprint,allowed_decisions:["approved"],issued_at,expires_at,state:issued|consumed|expired|revoked,consumed_at:null|timestamp,revoked_at:null|timestamp,revocation_reason:null|string`。nonce由Kernel CSPRNG生成、解码后至少32 bytes；它与challenge id共同构成不可预测的单次binding，OS authority必须接收并回显/绑定该nonce，虽不要求签名。TTL上限5分钟且`expires_at>issued_at`。OS authority只接收该challenge的受信绑定并完成真实机制，不自行决定Approval。

system处理固定为两阶段：Kernel在request时issue `SystemAuthenticationChallengeV1`；注册OS authority成功完成后才写`SystemAuthenticationEvidenceV1(result=verified)`，其中challenge、subject hash、material fingerprint、机制/平台主体（可得时）、验证时间和expiry必须绑定。随后`resolve`在**同一事务**读取current challenge与current Approval head，先执行上述过期CAS纪律，再验证`state=issued`、request/chain/task/subject/material binding、evidence的challenge/binding/`result=verified`/validity、request仍为current head及全部subject当前事实；只有全部成立才CAS consume并append approved resolution、更新head/Action/PD关系、写Audit与唯一`approval.state_changed`后commit。它不要求签名、Credential或remote audience，但nonce/state/expiry/current-head纪律与remote相同。OS取消、失败、缺失evidence、过期或绑定/head不匹配都不得写resolution或consumechallenge：分别返回`system_authentication_required`、`authentication_evidence_mismatch`、`challenge_expired`、`challenge_consumed`、`challenge_revoked`、`approval_head_conflict`或相应current-fact错误；CAS loser重读终态，过期仍统一为`challenge_expired`。

remote处理固定为一个repository事务：读取issued challenge与credential → 先按本节expire helper处理`issued && now>=expires_at` → 检查其余state/TTL/audience/request/current head/actor/task/subject/material/decision/time（`issued_at<=signed_at<=expires_at`，允许的clock skew仅部署配置且不能延长challenge expiry）→ 重建preimage并验签 → CAS将nonce/challenge标为consumed → append resolution → 更新current head/Action关联 → 写Audit/Outbox → commit。验签、expire/consume、append、head CAS不可拆事务；并发response恰好一个成功。challenge终态错误与签名/受众/credential/evidence错配使用Error Catalog固定码。

#### 6.10.4 Remote signature algorithm v1

远程签名算法使用可扩展tagged union `RemoteSignatureAlgorithmV1`。首版唯一branch为`{algorithm_kind:"ed25519",public_key_encoding:"base64url_no_pad",public_key}`；branch discriminator required const、unknown field拒绝。通用confirmation mode始终是`remote_signature`，不能携带具体算法名。未来新增算法必须增加新branch或对象版本、crypto vector和兼容裁决，不能给ed25519 branch塞可选算法参数。

`CredentialRefV1`：`{schema_version:1,credential_id,credential_revision,actor_ref,issuer_ref,signature_algorithm:RemoteSignatureAlgorithmV1,not_before,expires_at,status:active|revoked|expired,replaced_by_ref:null|UUID}`。Ed25519 public key解码后必须恰好32 bytes；验证时必须读取同id+revision且active/current，时间有效。

`RemoteApprovalChallengeV1`的精确wire、issued→expired的CAS语义与remote/system resolve transaction顺序以§6.10.3为准。`RemoteApprovalResponseV1`全部required：`schema_version=1,challenge_id,approval_chain_id,request_ref,audience,nonce,credential_ref,actor,task_id,subject_hash,material_authorization_fingerprint,decision:approved|denied,signed_at,algorithm_kind:"ed25519",signature_encoding:"base64url_no_pad",signature`。algorithm_kind必须与credential的`signature_algorithm` branch一致；signature解码后恰好64 bytes。response不得自带expires_at/public key或算法参数覆盖challenge/credential事实。

`RemoteApprovalSignaturePreimageV1`全部required且字段顺序不影响JCS：`schema_version=1,purpose:"shittim.remote-approval.v1",algorithm_kind:"ed25519",challenge_id,approval_chain_id,request_ref,audience,nonce,credential_id,credential_revision,actor_ref,task_id,subject_hash,material_authorization_fingerprint,decision,signed_at,challenge_issued_at,challenge_expires_at`。签名bytes固定为该对象RFC8785 JCS UTF-8 bytes本身，不先hash、不加换行、不加外层JSON/string；ed25519 branch按RFC8032 pure mode验证。

remote处理顺序、CAS/过期并发语义与错误映射以§6.10.3为准；本节只定义crypto preimage与vectors。

每个crypto对象有Schema fixture与official vector：公开RFC8032 Ed25519 test vector用于primitive，项目vector固定credential/challenge/response/preimage JCS bytes/signature；tamper覆盖每个绑定字段、signature bit、重放、过期/撤销、错误key revision和双并发resolution。

#### 6.10.5 有效性与plan revision

implicit/default/explicit allow与Delegation authority不生成Approval。material变化、subject revision/hash变化、Policy requirement变化、Delegation/credential/evidence过期撤销、Stop Fence或resolution expiry必须通过`invalidate_and_optionally_replace`闭合；纯observation变化只使PD/Lease失效，Core重算相同material hash后新PD可引用current approved resolution。

`plan_revision` approved后只允许以Task expected revision CAS接受subject中精确plan hash/version；随后所有相关Action新建PermissionDecision。plan resolution不自动成为operation resolution。

### 6.10.6 Approval Event allocation、Repository闭集、唯一键与恢复硬门

`ApprovalEventAllocationV1`是Approval repository三个CAS方法的**正式required对象**：`event_id:UUID,correlation_id:opaque,dedup_key:opaque,changed_at:UTC-second,causation_ref:CausationRefV2`。`event_id`格式合法，`correlation_id`/`dedup_key`非空且独立，`changed_at`为本次业务head变化的唯一时间；`causation_ref`必须为v2正式branch并指向真实外部command/event/action，禁止指向Approval event、同chain或future对象。event ID、chain ID、record IDs、Action/PD IDs与causation所含ID必须按各自语义互异；只有CausationRef明确指向的既有外部事实可以重复引用，任何ID不得由request、chain、Task或idempotency key派生。所有值由上层分配；replay从已提交head mutation/Event读取原allocation，禁止重新分配或用“当前时间”重建。

`append_request`、`resolve`与`invalidate_and_optionally_replace`三种CAS方法均**必须**接收该对象，且在同一`BEGIN IMMEDIATE`内消费其`event_id/correlation_id/dedup_key/changed_at/causation_ref`：成功head变化以`changed_at`投影record、Audit与Event的业务时间，写恰好一个`approval.state_changed`；CAS loser、validation/evidence失败和同事实replay不消费新allocation、不写新Event。Challenge失效Audit不使用本对象，因为Challenge不是Approval chain。

`RootTaskCreateAllocationV2`（active root v2）是task component、`kind=object`的正式封闭对象，全部字段required：`schema_version=2,task_id,task_scope_id,content_origin_id,kernel_receipt_id,creation_provenance_id,audit_record_id,task_created_event_id,correlation_id,task_created_dedup_key`。七个UUID格式合法且本对象内两两不同；`schema_version`不计入七UUID。两个opaque值只由Schema强制non-empty，不发明hex、base64或长度格式，也不从request/idempotency/task推导。重放从idempotency record读取原allocation并验证bundle，不重新分配。

`ChildTaskMaterializationAllocationV1`是task component、`kind=object`的正式封闭对象，全部字段required：`schema_version=1,child_task_id,task_scope_id,content_origin_id,kernel_receipt_id,creation_provenance_id,verification_result_id,audit_record_id,task_created_event_id,action_state_changed_event_id,action_transition_id,correlation_id,task_created_dedup_key,action_state_changed_dedup_key`。十个UUID本对象内两两不同；`schema_version`不计入十UUID。三个opaque值只由Schema强制non-empty，不发明hex、base64或长度格式。重放从action materialization mapping与transition intent读取，禁止重新分配。

JSON Schema只验证各字段自身格式/shape，不伪装能表达跨字段UUID互异、allocation UUID不得等于parent/action/PD/Approval/credential/challenge ID，或correlation/dedup彼此独立；这些必须由producer/repository validation与Conformance fake-generator/negative测试闭合。

生产allocation validator由`kernel-task-creation`唯一拥有。其Rust API不得接收自由`Vec<UUID>`/bag，而必须接收版本化、字段闭合的typed input snapshot；这些类型是纯Rust API input，不是source JSON Schema，不进入`schemas/manifest.json`，也不生成业务wire类型：

```text
RootTaskCreateExternalUuidRefsV1 {
  command_request_id: UUID,
  delegation_ref: Option<UUID>,
  parent_origin_refs: Vec<UUID>
}

ChildTaskMaterializationExternalUuidRefsV1 {
  parent_task_id: UUID,
  action_id: UUID,
  permission_decision_id: UUID,
  approval_resolution_ref: Option<UUID>,
  credential_refs: Vec<UUID>,
  challenge_refs: Vec<UUID>,
  delegation_ref: Option<UUID>,
  parent_origin_refs: Vec<UUID>
}
```

root新Task没有parent；actor与upstream stable ID不是UUID关系槽，不进入该类型。child allocation自身创建`verification_result_id`，`action_transition_id`也是内部allocation字段，二者都不是external ref；Lease/generation不是§6.10.6此处要求比较的UUID槽，不得猜入。Credential/Challenge在真实路径可能没有或有多个，因此使用required `Vec`字段而不是猜成单个optional；`Option`/`Vec`本身明确表达“本次无该事实”。两个input对象的每个字段在Rust构造时都required，helper只通过字段穷举flatten该闭集；caller遗漏字段必须编译失败，不能运行时默默漏入自由集合。

helper先再次按对应allocation Schema验证，再验证：七/十个内部UUID分别两两不同；任一内部UUID不等于typed snapshot flatten后的任一external UUID；所有opaque non-empty；同一allocation内opaque彼此不同。新增外部关系槽时必须修改类型并强制所有caller编译期更新；若需要保持旧API兼容则新增显式V2类型，禁止向自由bag追加约定。helper不得查询repository补事实，也不得从allocation/字符串猜关系；调用方必须先读取并验证权威事实，再完整注入snapshot。验证失败在进入repository前fail closed，不依赖数据库constraint碰撞。

allocation official fixture唯一路径为`schemas/fixtures/task/task_creation_allocations.v1.json`，外层固定：

```text
{
  fixture_version: 1,
  root: {
    schema_id: "https://schemas.shittim.local/task/root_task_create_allocation/v2",
    external_uuid_refs: {
      command_request_id: <uuid>,
      delegation_ref: <uuid> | null,
      parent_origin_refs: [<uuid>, ...]
    },
    valid_allocation: <RootTaskCreateAllocationV2>,
    tamper_cases: [{
      case_id: <non-empty unique string>,
      operation: add | replace,
      pointer: <strict RFC6901 pointer into valid_allocation>,
      value: <JSON value>,
      expected: {
        schema_valid: <boolean>,
        domain_result: accepted | duplicate_internal_uuid | external_uuid_collision | duplicate_opaque | not_evaluated
      }
    }]
  },
  child: {
    schema_id: "https://schemas.shittim.local/task/child_task_materialization_allocation/v1",
    external_uuid_refs: {
      parent_task_id: <uuid>,
      action_id: <uuid>,
      permission_decision_id: <uuid>,
      approval_resolution_ref: <uuid> | null,
      credential_refs: [<uuid>, ...],
      challenge_refs: [<uuid>, ...],
      delegation_ref: <uuid> | null,
      parent_origin_refs: [<uuid>, ...]
    },
    valid_allocation: <ChildTaskMaterializationAllocationV1>,
    tamper_cases: [<同形状>]
  }
}
```

allocation fixture harness的顺序固定为：对应allocation Schema validation → 仅在`schema_valid=true`时typed decode → 仅在typed decode成功后调用allocation domain validator。Schema invalid时必须短路，禁止typed decode与domain validator；因此空opaque向量固定为`schema_valid=false`、`domain_result=not_evaluated`，不得把Schema约束失败伪装成domain结果。wrapper的`domain_result`闭集只允许`accepted`、`duplicate_internal_uuid`、`external_uuid_collision`、`duplicate_opaque`和`not_evaluated`。

该fixture不保存JCS bytes、canonical string或hash，只证明Schema shape与allocation domain validator；`external_uuid_refs`对象字段必须精确对应上述Rust typed snapshot，fixture harness不得把它降格成自由UUID数组。每个root/child都必须至少包含Schema可通过但domain分别以`duplicate_internal_uuid`、`external_uuid_collision`、`duplicate_opaque`拒绝的向量，以及空opaque的Schema拒绝向量。它不证明生产generator随机、全局唯一或“不从caller派生”；generator非派生性必须由独立fake/spy与实现审查证明。

ID generator purpose闭集至少包含Task、TaskScope、ContentOrigin、KernelReceipt、CreationProvenance、VerificationResult、AuditRecord、Event、ActionTransition、Correlation、EventDedup；generator只分配，不拥有业务关系。repository写前验证格式/互异，commit后canonical readback逐项验证allocation。

不得以“通用JSON store”代替领域repository。实现必须暴露并穷举以下方法，新增写路径需修订合同：

| repository | 允许方法 |
|---|---|
| Action | `insert_pending`、`get`、`compare_and_set_policy_binding`、`acquire_lease`、`transition_with_expected_revision`、`complete_child_materialization`、`release_or_expire_lease`、`list_recovery_candidates` |
| PermissionDecision | `append`、`get`、`get_current_for_action`、`validate_current_for_execution`；不可update/delete |
| Approval | §6.10.1三种append复合方法、`get_record`、`get_current_head`、`list_history`、`validate_usable_resolution`；不可update/delete；不提供legacy v1 production read/migration API |
| ActionTransition | §6.14的`insert_intent`、`get_intent`、`get_for_action_revision`、`mark_committed_with_event`、`reconcile_intent` |
| ChildMaterialization | `materialize`、`get_by_action`、`get_by_child_task`、`reconcile`；不可部分append/update/delete |
| Credential/Challenge/Local/System Evidence | §6.10.2闭集方法 |
| RootTaskCreate | `create_root_v2`、`get_idempotency_record`、`reconcile_root_create`；v1 write API必须删除，不在production接口 |

必要唯一业务键（不要求具体SQLite列名）：Action ID唯一；每Action的`decision_revision`唯一且连续；Action current `permission_decision_ref`必须等于PD repository current mapping；Action `approval_chain_id`必须与current PD requirement相等，current approved `approval_resolution_ref`只能来自该chain且通过usable validation；每Approval record ID唯一；每chain恰好一个current-head映射；非null predecessor最多一个committed successor；每Action全生命周期最多一个child materialization；每Task creation provenance恰好一个；transition intent/event一一对应；Event ID/dedup key/outbox position按既有合同唯一。

所有Action transition使用expected revision CAS；Lease消费还CAS holder+generation+expiry；Stop Fence generation在执行事务再次读取。child materialization事务边界固定为§5.5 bundle，Approval remote事务固定为§6.10.3。调用者不能取得connection/transaction/raw SQL绕过验证。

消费者读取伪代码：

```text
read canonical record bytes
-> validate exact object-version Schema
-> verify JCS byte equality
-> typed decode tagged union
-> verify repository relation mirrors and unique-key closure
-> resolve referenced current revisions/heads
-> verify hashes from canonical projection, never trust stored duplicate hash alone
-> return typed fact; on any mismatch stored_data_invalid and do not mutate
```

reconciliation只返回`committed|absent|corrupt`：committed要求bundle全部存在且canonical/关系/Outbox/Action completion一致；absent要求无mapping且无孤立bundle对象；其余corrupt进入Safe Recovery。禁止consumer自动补行、选择时间最新Approval、重算后覆盖stored hash或第二次物化child。

不提供v1业务数据migration lifecycle。v2从零构建（ADR-0009）：fresh baseline之外的旧开发库在open/start时拒绝并返回稳定`reinitialize-required`诊断；禁止自动清库、隐式升级、读后补写或quarantine半迁移。v1 Approval/PD/TaskCreate Schema可保留为历史验证资产，但不得有production store write/migration API，也不得从v1 optional target猜造v2 subject。

稳定StoreError/RepositoryError到wire error映射以Error Catalog为准，必须按枚举穷举；constraint若能确定为revision/head/unique material冲突映射对应稳定码，其余内部约束不向caller泄漏并转`internal_error`，数据已提交但读取不一致转`stored_data_invalid`。

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

### 6.14 Action transition authority

`ActionTransitionRefV1`与`ActionTransitionIntentV1`的唯一权威定义在本节，不能由Audit合同或Event页面另起wire定义。**本次Event v2八Schema切片只交付`ActionTransitionRefV1`的ref/wire；`ActionTransitionIntentV1`不属于该八项，必须在后续Action-transition authority Schema批次与repository一起正式落地。任何producer在该批次前不得手写临时intent struct、自由JSON或私有近义类型，也不得因为ref已生成就声称Action producer闭环。**

`ActionTransitionRefV1`是正式封闭wire对象：

```text
{
  kind: "action_transition",
  action_id: <uuid>,
  transition_id: <uuid>
}
```

`CausationRef v2`的`action_transition` branch必须直接`$ref`该对象；branch不得复制字段或另起近义名。`ActionTransitionIntentV1`是先于Action状态事件持久化的不可变事实，全部字段required：`schema_version=1,transition_id,action_id,expected_action_revision,execution_generation,from_status,to_status,reason_code,correlation_id,created_at`。revision/generation为非负整数，状态必须是领域合法边。

`ActionTransitionIntentV1`后续Schema身份固定为task component、`kind=domain_object`、`compatibility=new-contract`、exact ID `https://schemas.shittim.local/task/action_transition_intent/v1`、exact source `schemas/source/task/action_transition_intent.v1.json`、`schema_version_field=schema_version`，direct refs只有whole-schema retained `ActionStatus`。其Schema与active Action持久对象Schema可先于repository批次落地（见§13.6.4），但transition repository/migration及producer fixture必须与active Action repository同批实现；不得塞入本次八Schema而破坏“恰好八项”，也不得先由producer创建临时类型。

唯一键固定为`transition_id`全局唯一，以及`(action_id, expected_action_revision, execution_generation, from_status, to_status, reason_code)`唯一；同一个intent重放必须canonical readback返回原事实，不分配新transition。Repository闭集为`insert_intent`、`get_intent`、`get_for_action_revision`、`mark_committed_with_event`、`reconcile_intent`；没有update/delete。`mark_committed_with_event`只可在同一事务CAS Action从expected revision/`from_status`到`to_status`并append唯一`action.state_changed`，event causation精确等于该intent的`ActionTransitionRefV1`。读取必须校验intent、Action revision/generation、event payload/causation/correlation和唯一关系；缺失或冲突为`stored_data_invalid`。reconcile只返回`prepared|committed|corrupt`，不得补造event或另换transition id。

### 6.15 CausationRef 与 EventEnvelope版本

> **实现状态**：下述CausationRef v2、ActionTransitionRef v1与EventEnvelope v2 source Schema/generated Rust，以及SQLite migration 0003/mixed Outbox API已实现；active producer/runtime仍未实现。exact清单与component DAG见§13.6.2及ADR-0008。

```text
CausationRef v2 = tagged union:
- `{kind:"command_request",id:<uuid>}`
- `{kind:"event",id:<uuid>}`
- `{kind:"action",id:<uuid>}`
- `{kind:"action_transition",action_id:<uuid>,transition_id:<uuid>}`
```

- v1只允许`command_request | event`，仅保留Schema/fixture历史验证资产，无production store read/write/migration API；
- `command_request`、`event`、`action`的`id`均为UUID；source最小语义骨架固定为union层required `kind`四值enum与closed `oneOf`：前三支为`additionalProperties:false`的inline `{kind const,id UUID}`，`action_transition` branch必须整支直接whole-schema root `$ref ActionTransitionRefV1`，union以`unevaluatedProperties:false`闭合，不复制字段、不用fragment ref；
- `ActionTransitionRefV1`不携带schema_version、revision或generation；Schema版本由ID表达，revision/generation由`transition_id`解析到的不可变intent权威校验，避免引用对象与intent双源；
- `action`必须指向真实Action，不接受capability/operation字符串或不存在的占位ID；用于该Action直接产生的其他聚合事实；
- `action_transition`只用于Action自身状态事件，transition_id必须来自已持久化transition intent且不得等于event/action id；
- child `task.created`直接因果必须是`task.child.create` Action；对应`action.state_changed`使用action_transition；禁止绕行伪造`action.requested` Event或Action self-causation。

```text
EventEnvelope v2 {
  event_id
  type
  schema_version: 2
  aggregate_type
  aggregate_id
  sequence
  outbox_position
  occurred_at
  causation_ref: CausationRef v2
  correlation_id
  dedup_key
  payload
}
```

同一聚合中，事务内暂时分配但最终回滚的`sequence`不占号；cursor/delivered语义不变。`outbox_position`为正ASCII十进制字符串；本批为兼容v1继续接受前导零，repository从INTEGER position重建时输出无前导零普通十进制，未来收紧必须版本化。`occurred_at`是唯一Envelope业务时间；不新增`emitted_at`，`delivered_at`只属于Outbox metadata。`correlation_id`与`dedup_key`为non-empty opaque string。EventEnvelope v1仅保留Schema/fixture历史验证资产。EventEnvelope v2必须以五个conditional mapping闭合type→aggregate→payload；Schema可固定type/aggregate、stop fence `aggregate_id=global`与payload shape，但aggregate/payload ID相等、revision/head/transition等跨对象关系由repository验证。

### 6.16 AuditRecord v2 exact wire

Action transition的权威wire、intent、CAS、唯一键与reconciliation见§6.14；Audit只引用其结果，不重复定义。AuditRecord v1仅保留Schema/fixture历史验证资产；现有`task.create v1`实现属ADR-0009待删除的v1写路径。active producers使用下列完整v2对象；全部顶层字段required，无关联事实显式null/空数组：

```text
{
  id, schema_version: 2,
  audit_type, level, actor, entry_point, occurred_at,
  task_id, task_creation_context,
  action_id, permission_decision_ref, approval_resolution_ref,
  recovery_attempt_ref, delegation_ref,
  model_call_refs, payload_manifest_refs, external_content_status,
  verification_result_refs, content_origin_refs, artifact_refs, resource_refs,
  extension_id, provider_id,
  causation_ref, correlation_id,
  rollback_capability, stop_fence_generation,
  policy_context,
  outcome, reason_codes, summary, details
}
```

v2中的`approval_resolution_ref`只能引用确实消费的approved current resolution；implicit/delegation为null。`task_creation_context`为required-nullable完整对象`{task_revision,goal,origin_ref,proposer,creation_provenance_ref,creation_kind,accepted_at,materialized_at}`。`policy_context`为required-nullable完整对象`{matched_rule_ref|null,policy_set_revision,permission_decision_revision,material_authorization_fingerprint,observation_evidence_fingerprint,reused_approval_resolution_ref|null,child_task_delta_hash|null,authentication_evidence_refs[]}`。`level`/`outcome`/`external_content_status`/`rollback_capability`闭集沿用v1同名字段；`causation_ref`升级为CausationRef v2（`command_request|event|action|action_transition`）。不得以“v1加几个字段”的增量实现替代此完整shape。固定归因字段（actor/entry_point/task/action/PD/approval/recovery/delegation/model/verification/origin/artifact/resource/extension/provider/causation/correlation/policy/outcome/reason_codes等）必须写在顶层required字段；**禁止**把同一业务事实仅塞进`details`伪装完成。

#### 6.16.0 AuditRecord v2 `audit_type` 闭集

`audit_type` v2 是可编码闭集。列入闭集不等于对应生产路径已经实现；未知值由Schema拒绝。v2闭集= v1语义升级保留 + 新业务类型：

```text
audit_type (v2 closed set):
  task.creation_recorded
  | command.accepted
  | permission.evaluated
  | kernel.invariant_blocked
  | event.published
  | recovery.recorded
  | config.changed
  | approval.requested
  | approval.resolved
  | approval.invalidated
  | identity.challenge_expired
  | identity.credential_registered
  | identity.credential_rotated
  | identity.credential_revoked
  | identity.local_presence_recorded
  | identity.system_authentication_recorded
```

- v1七类在v2继续使用同名code，但必须落在完整v2 wire上（含`approval_resolution_ref`、v2 `task_creation_context`/`policy_context`/CausationRef v2），不得用v1 legacy shape冒充active。
- Approval业务使用独立`approval.requested|resolved|invalidated`，**不得**用`command.accepted`/`config.changed`/`details`伪装；Event Catalog仍只发`approval.state_changed`，Audit类型与Event类型正交。
- Challenge expiry使用`identity.challenge_expired`，**不得**写成`approval.*`或产生`approval.state_changed`。
- Credential/local/system identity facts使用对应`identity.*`类型；即使本批producer未实现，Schema与合同也必须先定义闭集成员。
- 仅`task.creation_recorded`允许非null `task_creation_context`；其余v2类型该字段必须为null。

#### 6.16.0a `AuditAllocationV2`

`AuditAllocationV2`是独立Audit写入路径（尤其无伴随Event allocation的路径）的**正式required对象**，全部字段required：

```text
AuditAllocationV2 {
  audit_record_id: <uuid>
  correlation_id: <non-empty opaque>
  occurred_at: <RFC 3339 UTC instant, second precision where producer requires>
  causation_ref: CausationRefV2   // 真实外部 command_request|event|action|action_transition
}
```

规则：

- `audit_record_id`格式合法；`correlation_id`非空opaque，不从request/idempotency/task/challenge/approval id派生；`causation_ref`必须指向真实既有外部事实，禁止self-causation、指向同次Audit、Approval event或future对象。
- 仅写Audit、不写业务Event的路径（例如challenge expiry）**必须**由上层显式提供`AuditAllocationV2`；CAS loser/replay不得重新分配或“用当前时间”重建。
- 与业务Event同事务的producer可从更大的bundle allocation投影Audit字段，但投影结果必须满足同一四元组语义：
  - root `task.create v2`：从`RootTaskCreateAllocationV2`取`audit_record_id`+共享`correlation_id`；`occurred_at=accepted_at`；`causation_ref=command_request`。
  - child materialization：从`ChildTaskMaterializationAllocationV1`取`audit_record_id`+共享`correlation_id`；`occurred_at=materialized_at`；`causation_ref=action`。
  - approval initial/resolution/invalidation：从`ApprovalEventAllocationV1`投影`correlation_id`、`occurred_at=changed_at`、`causation_ref`；`audit_record_id`由上层与`event_id`一并分配并在写前与event/record IDs互异校验；Audit与Event共用correlation与causation，但Audit id ≠ event id。
- Challenge expiry**禁止**消费`ApprovalEventAllocationV1`；必须独立`AuditAllocationV2`。
- generator purpose闭集须覆盖`AuditRecord`与`Correlation`；repository写前验证格式/互异，commit后canonical readback。

#### 6.16.1 TaskCreationProvenance与producer

`TaskCreationProvenance`生产闭集只保留两支（ADR-0009取消legacy direct-child/import migration provenance）：

| kind | branch required字段/null边界 |
|---|---|
| `root_command_v2` | `command_request_id,entry_point,actor,receipt_ref`; `parent_task_id=null,action_id=null` |
| `child_action_v2` | `parent_task_id,action_id,permission_decision_ref,approval_resolution_ref|null,verification_result_ref,proposal_hash,child_task_delta_hash`; command refs null |

ContentOrigin v2 child producer固定`carrier_ref=action`；root固定command request。receipt preimage：root为normalized TaskCreate v2 payload；child为`NormalizedChildTaskProposalV1`；两者parent origin refs保序保重复，receipt hash不含carrier、IDs、时间或provenance。

固定producer表（对象/时间/Outbox总览）：

| producer | 对象版本/IDs | 时间 | causation/correlation | Outbox |
|---|---|---|---|---|
| root `task.create v2` | Origin v2、Scope v1、Task当前active version、Provenance v1、Audit v2、EventEnvelope v2；使用`RootTaskCreateAllocationV2`，七UUID逐purpose分配且互异，correlation/dedup opaque独立 | 唯一`accepted_at`投影到origin received/receipt、scope/task、provenance、audit、event；`materialized_at=null` | command request；Audit/Event同correlation | 恰好一个`task.created` |
| child materialization | Origin v2、Scope v1、Task active、Provenance v1、Verification active、Audit v2、EventEnvelope v2、Action active、ActionTransitionIntent v1；使用`ChildTaskMaterializationAllocationV1`，十UUID逐purpose分配且互异 | 唯一`materialized_at`投影到所有新事实与Action changed time；`accepted_at`为proposal进入Kernel的既有时间且不得重写 | child Task/Audit causation=Action；Action event=`ActionTransitionRefV1`；bundle共享correlation | `task.created`+`action.state_changed` |
| approval initial request | ApprovalRecord v2 request、Audit v2、ApprovalStateChangedPayload v1/EventEnvelope v2；消费`ApprovalEventAllocationV1`（Audit字段按§6.16.0a投影） | 单一`changed_at=record.created_at=audit.occurred_at=event.occurred_at` | caller真实command/event/action；同correlation | 恰好一个`approval.state_changed` |
| approval resolution | ApprovalRecord v2 resolution及mode evidence引用、Audit v2、Action/PD关系更新、Approval event；消费`ApprovalEventAllocationV1` | `changed_at=resolved_at=record.created_at=audit/event time` | caller真实来源；remote还消费challenge | 恰好一个`approval.state_changed` |
| approval invalidation/replacement | invalidation与可选replacement request、Audit v2、Lease/关系更新、Approval event；消费`ApprovalEventAllocationV1` | 两record同一业务`changed_at`，各自created_at等于该值 | invalidating command/event/action | 恰好一个逻辑head `approval.state_changed`，无中间事件 |
| challenge expiry | Challenge终态与`identity.challenge_expired` Audit v2；不属于Approval chain；**必须**消费独立`AuditAllocationV2` | helper的`expired_at`=`audit.occurred_at` | 调用resolve/consume/sweeper的真实causation；独立correlation | **无**`approval.state_changed` |
| credential register/rotate/revoke | Credential权威事实 + 对应`identity.credential_*` Audit v2；Audit allocation由上层`AuditAllocationV2`或同事务bundle提供 | 业务`registered_at`/`rotated_at`/`revoked_at`投影`occurred_at` | 真实command/system authority causation | 默认无Approval/Task event |
| local presence / system authentication evidence | Evidence权威事实 + `identity.local_presence_recorded` / `identity.system_authentication_recorded` Audit v2 | evidence时间投影`occurred_at` | 受信transport/OS adapter权威causation | 默认无Approval event；resolution另走approval producer |

#### 6.16.2 v2 producer固定字段矩阵

下列矩阵是active v2 producer的逐项合同；测试必须逐格断言。无关联事实写显式null/空数组；`summary`默认null；`details`默认`{}`且不得承载本表已规定的顶层归因。`level`取值闭集：`user_activity|operational|security|debug`。

| producer | audit_type | level | actor / entry_point authority | task / action / PD / approval refs | external_content_status | rollback_capability | outcome | reason_codes（精确） | summary / details | null/empty 强制 | causation_ref / correlation_id |
|---|---|---|---|---|---|---|---|---|---|---|---|
| root `task.create v2` | `task.creation_recorded` | `user_activity` | 同事务command Envelope的actor完整快照与entry_point | `task_id`=新root Task；完整`task_creation_context`（revision=1,goal,origin_ref,proposer,creation_provenance_ref,creation_kind=`root_command_v2`,accepted_at,materialized_at=null）；action/PD/approval/recovery/delegation/extension/provider=null | `not_sent` | `unknown` | `succeeded` | `["task_created_root_v2"]` | `summary=null`,`details={}` | verification/model/manifest/artifact/resource/authentication数组空；policy_context=null | command_request；与`task.created`同correlation；allocation见`RootTaskCreateAllocationV2` |
| child materialization | `task.creation_recorded` | `user_activity` | 同事务Action/proposal权威actor与entry_point | `task_id`=child；完整context（creation_kind=`child_action_v2`,accepted_at=既有proposal时间,materialized_at=本次）；`action_id`/PD/Verification必有；`approval_resolution_ref`仅在真实消费approved resolution时非null否则null | `not_sent`（无外发）或按真实外发事实 | 从ActionRequest.rollback_policy/Verification/Recovery投影，缺省`unknown` | `succeeded` | `["task_created_by_action_v2"]` | `summary=null`,`details={}` | 无关联model/manifest/artifact/resource/extension/provider/recovery按实为null/空；policy_context在PD非空时必填并与PD一致 | action；bundle共享correlation |
| approval initial request | `approval.requested` | `security` | request record的请求actor完整快照与`requested_from_entry_point` | task/action/PD按subject真实绑定；`approval_resolution_ref=null`；`task_creation_context=null` | `not_sent` | `unknown` | `deferred` | `["approval_requested"]` | null / `{}` | recovery/delegation/model/manifest/verification/origin/artifact/resource/extension/provider按无关联null/空；policy_context按PD可空规则 | 真实command/event/action；与`approval.state_changed`同correlation；`ApprovalEventAllocationV1`投影 |
| approval resolution | `approval.resolved` | `security` | resolution的actor完整快照与作出决议的entry_point | task/action/PD按subject；`approval_resolution_ref`=本resolution（approved时供后续消费；denied亦写本ref作为决议事实）；context=null | `not_sent` | `unknown` | approved→`succeeded`；denied→`blocked` | approved:`["approval_resolved_approved"]`；denied:`["approval_resolved_denied"]` | null / `{}` | 无关联数组空；mode evidence进权威ApprovalRecord与必要policy/auth evidence refs，不塞details | 真实caller；remote另绑定已consume challenge；同correlation |
| approval invalidation（含optional replacement） | `approval.invalidated` | `security` | invalidation的`invalidated_by_actor`（可null仅当system_internal且可证明）与`invalidated_from_entry_point` | task/action/PD按被失效subject；若`invalidated_record_ref`指向approved resolution，则`approval_resolution_ref`=该resolution；若指向尚未resolution的request，则`approval_resolution_ref=null`；不得为request失效伪造resolution ref；context=null | `not_sent` | `unknown` | `observed` | 与invalidation `reason_code`一致的单元素数组（如`["material_changed"]`） | null / `{}`；replacement request id若存在只能出现在权威invalidation record与事件payload，不藏入details | 无关联数组空；Lease撤销、current-head更新与replacement均由同事务关系事实证明 | invalidating command/event/action；与唯一`approval.state_changed`同correlation；`ApprovalEventAllocationV1` + 同事务Audit allocation |
| challenge expiry | `identity.challenge_expired` | `security` | actor可为null仅当`entry_point=system_internal`且为sweeper/kernel路径；resolve/consume路径使用调用方actor/entry | task_id=challenge.task_id；action/PD/approval_resolution_ref全部**null**（Challenge不是Approval chain）；context=null | `not_sent` | `unknown` | `observed` | `["challenge_expired"]` | null / `{}`；challenge_id/expires_at等诊断不得替代顶层type，可在details仅作补充且不能改变type语义 | 全部业务approval/PD/verification/model等ref null/空 | 真实resolve/consume/sweeper causation；**独立**`AuditAllocationV2`；**无**Approval event / 不消费`ApprovalEventAllocationV1` |
| credential registered | `identity.credential_registered` | `security` | registrar authority actor/entry | task/action/PD/approval全null；context=null | `not_sent` | `unknown` | `succeeded` | `["credential_registered"]` | null / `{}` | 无关联空/null | 真实注册command/authority；`AuditAllocationV2`或同事务bundle |
| credential rotated | `identity.credential_rotated` | `security` | registrar authority actor/entry | 同上全null | `not_sent` | `unknown` | `succeeded` | `["credential_rotated"]` | null / `{}` | 同上 | 同上 |
| credential revoked | `identity.credential_revoked` | `security` | registrar authority actor/entry | 同上全null | `not_sent` | `unknown` | `succeeded` | `["credential_revoked"]` | null / `{}` | 同上 | 同上 |
| local presence recorded | `identity.local_presence_recorded` | `security` | evidence `observed_actor`与`entry_point` | task/action/PD/approval按evidence绑定可null；context=null | `not_sent` | `unknown` | `observed` | `["local_presence_recorded"]` | null / `{}` | 无关联空/null | 受信transport/desktop attestor causation；独立或bundle Audit allocation |
| system authentication recorded | `identity.system_authentication_recorded` | `security` | 注册OS adapter/system_internal权威 | task按challenge绑定；action/PD/approval_resolution_ref在evidence阶段为null（决议另有`approval.resolved`） | `not_sent` | `unknown` | evidence `verified`→`succeeded` | `["system_authentication_recorded"]` | null / `{}` | 无关联空/null | OS adapter/kernel causation；与后续approval resolution audit不得合并为一条 |
| command.accepted / permission.evaluated / kernel.invariant_blocked / event.published / recovery.recorded / config.changed | 同名v2 type | 按业务：user命令偏`user_activity`/`operational`；安全阻断`security`；诊断可`debug` | 真实actor/entry；null actor仅system_internal可证明 | 按真实关联填task/action/PD/approval/recovery；**禁止**用这些type表达approval head变化或challenge expiry；context仅creation type非null | 按真实外发 | 按权威投影或`unknown` | 按结果闭集 | producer自定义非空code数组，但不得复用本表已占用的精确reason | 顶层优先；details不承载固定归因 | 无关联显式null/空 | 真实causation；correlation按是否伴随Event决定共享或独立 |

通用强制：

- 业务契约要求审计时，业务事实、AuditRecord v2与规定Outbox同事务Schema校验+canonical readback；失败整体回滚。
- active v2与legacy v1 producer的顶层固定值必须由同一producer测试逐项断言；root/child固定值以本矩阵为准。v1 legacy值只用于历史fixture，不得复制到v2。
- 不得将approval initial/resolution/invalidation或challenge expiry压缩进同一条Audit的`details`；每种业务一条对应`audit_type`。

以下是v1 legacy字段：

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
causation_ref: { kind: command_request | event, id } | null // v1 legacy；v2允许action
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

- `audit_type` v1 是闭集（仅上表七类），提供可编码分类；列入闭集不等于对应生产路径已经实现；active v2闭集与producer矩阵见§6.16.0/§6.16.2，不得把v1七类误当作v2完整闭集；
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
- 每当业务契约要求审计时，Kernel 必须先通过 AuditRecord Schema 校验，并在同一个 SQLite 事务中写入业务事实、AuditRecord 以及该业务事实要求的 Outbox 记录。Schema 校验、适用的跨对象一致性校验或 AuditRecord 插入失败必须使整个事务回滚；不得降级为只写业务事实或只写日志文本；
- 当前 `kernel-sqlite` 已实现 Task create/get repository 的固定 `task.creation_recorded` producer：Task/TaskSpec/ContentOrigin/Audit/唯一 `task.created` Event 的 canonical 子事实、共同 `accepted_at` 与同事务失败回滚均有实现和测试；通用 Audit Store 也已实现 `sent` 单记录至少一个支撑引用非空检查；
- 仍未实现 PermissionDecision / policy context 跨对象字段相等、rollback 权威投影、Provider / ModelCall 跨对象一致性与其它业务 producer。

### 6.17 PayloadManifest

与 MODEL_RUNTIME.md 的规范名和字段语义一致。它描述一次模型调用实际候选 Payload 的对象集合，而不是通用对象存储清单：

```text
PayloadManifestV2 {
  id
  schema_version: 2
  task_id
  child_task_id? / action_id?
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
}
```

`PayloadManifestV1`历史wire仍含旧child字段名，仅保留Schema/fixture历史验证资产，无production read/write/migration；active v2只生成`child_task_id`，禁止同时接受两个字段或自动fallback。Manifest不保存Payload正文，也不承担审批或内容审查。

### 6.18 ModelCallRecord

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

### 6.19 DelegationContract

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

### 6.20 MemoryCandidate

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

### 6.21 CapabilityDescriptor

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

`CapabilityDescriptor` 与 Provider Registry 的选择输入保持通用，只描述协商、Schema 引用、权限、副作用、可靠性、健康、成本与平台等跨 Profile 元数据。Profile 专属结构只能出现在已协商的 versioned operation payload/schema（包括 typed Extension result；未来若有正式 Extension Event 合同则包括其 event payload）或 Object Handle 所引用对象中；不得把窗口、坐标、截图、desktop generation 等字段加入通用 descriptor、KCP 或其他 Core 顶层对象。

### 6.22 Optional Profile 参考对象

Computer Use 的 Operation Snapshot 归属 optional Profile，完整语义只见 [`COMPUTER_USE.md`](COMPUTER_USE.md)。当前仓库没有其正式 Profile Schema，因此不得把它当作 Core object、KCP API、已生成 SDK 类型或已实现能力。

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

Provider Registry 根据通用元数据选择：

- Profile 与协商版本；
- platform；
- capability completeness；
- permission availability；
- reliability；
- current health；
- user preference；
- cost；
- task requirements。

允许组合多个 Provider，选择结果写入 Action/Audit。Registry 与 Core 只处理 descriptor、grant、health、stable resource ref 和选择事实，不解释 Profile 私有对象；具体领域参数与结果必须留在已协商的 versioned operation payload/schema（包括 typed Extension result；未来正式 Extension Event 合同另行定义 event payload）或 Object Handle 中。

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
- 无 v1→v2 业务数据迁移：旧库一律 reinitialize-required 拒绝；fresh baseline 的 schema ledger/DDL 演进有 preflight、backup、verify、rollback；每条持久记录保存 `schema_version`。

### 13.2 Canonical JSON 与哈希

所有契约中的 `*_hash`、幂等请求等价比较、签名输入和生成物稳定性比较，使用 RFC 8785 JSON Canonicalization Scheme（JCS）产生 UTF-8 bytes，再按字段规定计算哈希；未单独指定时哈希为 SHA-256 小写十六进制。不得使用语言默认对象序列化、格式化 JSON 或 map 插入顺序代替 RFC 8785。

### 13.3 Schema 源与生成物规则

#### TaggedUnion source profile

JSON Schema object `oneOf`只有在每branch经canonical `$ref` resolution后均为封闭object、共享同一discriminator属性、该属性在每branch required且为唯一string const、tag与union discriminator enum精确双射、unknown-field策略可由branch `additionalProperties:false`或组合层`unevaluatedProperties:false`证明时，才能lower为`TypeShape::TaggedUnion`。branch与tag必须一一对应；nested union逐层应用。普通nullable `oneOf(T,null)`、非判别oneOf与KCP conditional Envelope payload analysis分别走Nullable/unsupported/Envelope专用分析，绝不误判为TaggedUnion。unresolved/cross-target ref、开放或混合branch、零/多tag、重复const、enum缺/多支、collision、递归layout不可保真等整体fail closed且不写部分生成物。IR、Rust serde enum、未来TS discriminated union与正反测试矩阵的完整决策见ADR-0002；spec与ADR必须一致。

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

### 13.5 MethodVersionBinding权威结构与两阶段启用

MethodVersionBinding与manifest entry的`compatibility`是正交维度：binding描述某个KCP family/method的request/response版本选择与active/legacy validation lifecycle；`compatibility`只描述Schema entry相对既有wire合同的演化关系。不得把`compatibility`称作lifecycle字段，也不得从任一字段推导另一字段。

每条binding全部字段required：

```text
{
  family: command | query,
  method: <Catalog method>,
  active_request_versions: [positive integer, ...],
  legacy_validation_versions: [positive integer, ...],
  request_schema_id_by_version: {"<version>": <schema id>, ...},
  response_schema_id_by_version: {"<version>": <schema id>, ...}
}
```

完整validator必须接受合法非空registry，并实施以下合同：

- binding数组canonical排序固定为`family`（`command < query`），随后`method`按UTF-8 bytes升序；`family+method`唯一，method必须属于对应Envelope Catalog family；
- `active_request_versions`非空，`legacy_validation_versions`可空；两数组均为positive integer严格升序、唯一且互斥；
- version map key必须是无前导零的canonical decimal text；request map keys精确等于active∪legacy，response map keys精确等于active；
- request选择键是`family+method+request version`；response也必须按同一`family+method+request version`选择，不能只按method或response payload version猜测；legacy response若未来有named migration API必须另立binding，不能混入active response map；
- 每个map值必须命中唯一manifest entry。request entry kind=`kcp_request`，response entry kind=`kcp_response`；entry `version`和root `schema_version` required const必须都等于map key版本；root缺少唯一positive integer const时binding非法；
- active request/response不得引用`legacy-validation-only`或`legacy-read-only` entry；legacy request只能引用`legacy-validation-only` entry；internal projection/allocation与其它非KCP kind不得进入binding；
- binding引用的request、response及其全部`$ref`依赖必须属于binding catalog的generation target closure，且对应生成target完整；合法Schema ID但target closure不完整仍失败；
- active family structure authority不得再通过schema ID文件名或`*command_envelope.json`/`*query_envelope.json` suffix推断。registry必须对每个family按全部entry事实唯一选择：`component=kcp`、`kind=envelope`、title分别精确为`KcpCommandEnvelopeV2`/`KcpQueryEnvelopeV2`、ID分别精确为`https://schemas.shittim.local/kcp/command_envelope/v2`/`https://schemas.shittim.local/kcp/query_envelope/v2`、entry version `2`、root `schema_version`（若该root声明该字段）与entry version一致、`compatibility=breaking-replacement`；任一family候选为0或多于1都fail closed。选中后读取其root `command_type`/`query_type` discriminator enum作为active family method事实；retained v1 Envelope只负责legacy validation/typed decode，绝不是active method catalog authority；
- generated Envelope结构权威目录常量/API固定命名为`KCP_ENVELOPE_AUTHORITY_COMMAND_METHODS`、`KCP_ENVELOPE_AUTHORITY_QUERY_METHODS`与两者canonical合并的`KCP_ENVELOPE_AUTHORITY_METHODS`。这些常量只表达active family structure authority，不表达bound active version或dispatcher executable registration。不得继续用`KCP_COMMAND_METHODS`/`KCP_QUERY_METHODS`/`KCP_METHODS`等易被运行时误用的名称，也不得用`KCP_V1_METHODS`指代active总目录。retained legacy目录必须使用含`LEGACY`的独立名称，例如`KCP_LEGACY_V1_METHODS`，且不能被active preflight消费；
- 完整八方法覆盖不在代码或fixture手写第二份生产表：MethodVersionBinding validator直接从上述registry Envelope facts派生expected set，并证明每个method恰有一个binding、无遗漏/多余/跨family错配。validator不得反向读取generated `KCP_ENVELOPE_AUTHORITY_*`常量，否则会形成“用待验证生成物验证其自身来源”的循环；新Envelope本身没有payload conditional refs；
- generated binding catalog只提供library facts/选择器，不表示dispatcher registration、handler、repository、server或SDK已可用。

启用分两阶段（ADR-0009；不再使用已作废的`V2ProductionWriteCutover`叙事）：

1. **Schema/工具阶段**：实现上述完整validator、catalog生成与正反测试；测试使用合法非空的synthetic 8-method manifest/registry覆盖完整表。`SchemaRegistry::load`只负责与具体仓库阶段无关的通用完整validation，因此必须允许合法非空binding。在`V2InitialBuildActive`落地production非空bindings之前，production阶段约束由schema-tool production-profile模块中的单一owner `validate_production_manifest_stage(&SchemaRegistry)`承担：production CLI `check`与`generate`必须在registry load成功后、plan/render前调用它；仓库统一门`./scripts/check-schema.sh`通过这两个入口继承同一gate。该函数在bindings仍为空的实现切片中断言本repo production `schemas/manifest.json`的`method_version_bindings`精确为空；当本里程碑切片写入最终非空八方法binding时，同一stage gate改为验证bindings精确等于下表目标集合。非空synthetic binding catalog的end-to-end plan/render测试必须走library API并使用显式non-production registry profile，不得调用production CLI check/generate stage gate；synthetic registry不能靠路径、环境变量、fixture目录名或调用栈隐式绕过。
2. **V2InitialBuildActive 初始交付**：§13.7全部谓词满足时，production manifest启用非空八方法binding，并与v2 dispatcher registration、v2 repository write gate一并作为fresh baseline初始交付；不是从旧部署cutover。v1 runtime写路径删除而非双写切换。

最终production lifecycle目标如下；它由Envelope Catalog派生覆盖测试，不得转抄为第二份运行时目录：

| family | method | active | legacy |
|---|---|---|---|
| query | `system.ping` | `[1]` | `[]` |
| command | `task.create` | `[2]` | `[1]` |
| query | `task.get` | `[1]` | `[]` |
| query | `task.list` | `[1]` | `[]` |
| query | `event.subscribe` | `[1]` | `[]` |
| query | `event.poll` | `[1]` | `[]` |
| command | `stop.activate` | `[1]` | `[]` |
| query | `stop.status` | `[1]` | `[]` |

其中`task.create` active request/response分别绑定`TaskCreateRequestV2`/`TaskCreateResponseV2`，legacy request绑定retained `TaskCreateRequestV1`；其余方法继续绑定各自retained active v1 request/response。`KcpCommandEnvelopeV2`/`KcpQueryEnvelopeV2`是family结构验证入口，不作为method payload request/response map value。

### 13.6 Manifest namespace迁移合同

迁移已实施，manifest 只接受**schema_version=2**，不提供v1 alias或兼容解析。顶层字段固定为`schema_version,draft,id_base,components,method_version_bindings,schemas`，且`id_base`精确为`https://schemas.shittim.local/`。`components`为非空数组，每个对象固定字段如下：

```text
{ name, namespace, allowed_refs, retained_ids }
```

- `name`是非空 component 标识，唯一；`namespace`必须是 canonical absolute http(s) URL、无 fragment、以`/`结尾，且处于root `id_base`下的单一直接路径组件（`https://schemas.shittim.local/<name>/`）；不得使用prefix、dot segment、double slash、percent-encoded component或显式default port伪装组件；`name`必须等于该单一路径段。
- `allowed_refs`是严格升序、无重复的其它 component name 数组；不得声明自身或未知component。跨 component `$ref` 只有目标component在源component `allowed_refs`中才可解析；同component可引用。此component-ref gate独立于且不得替代generation target closure：gate通过后，依赖仍必须声明同一generation target。
- `retained_ids`是严格升序、无重复的canonical absolute schema IDs；一个retained ID恰好对应一个当前manifest entry及其完全未改写的source `$id`。因此retained ID禁止重绑到别的component、禁止孤儿、禁止重复；entry ID如落入任一retained set，必须恰好由其所属component保留。当前41个历史`/v1/` IDs以版本控制的不可变迁移ledger（`schemas/fixtures/manifest/retained_ids.v1.json`）逐项固定`id/component/source/source_sha256`，`SchemaRegistry::load`逐项计算实际source bytes SHA-256并同时核对manifest所有权/路径，因此ledger是每次load的生产gate，不能通过协同移动`retained_ids`和entry `component`、改路径或单独篡改仍合法的Schema bytes绕过。ledger只记录迁移时的41个历史ID；本批12个新component-native ID及以后所有新ID都不得加入。retained ID不得落在任何component namespace，component-native ID也不得落入retained集合；不允许新的非retained `/v1/` entry。
- `method_version_bindings`为manifest v2 required typed字段，元素wire shape就是§13.5的完整`MethodVersionBinding`。通用`SchemaRegistry::load`允许并完整验证合法非空synthetic registry；production `schemas/manifest.json`在`V2InitialBuildActive`完成前仍可为空，只由§13.5命名的`validate_production_manifest_stage`按当前阶段断言（空或最终八方法目标集）；完成后production bindings必须精确等于§13.5目标表。不得把“某一阶段为空”实现成通用empty-only loader规则。
- `compatibility`正式闭集固定且只允许`v1-stable | new-contract | breaking-replacement | legacy-validation-only | legacy-read-only`，所有production、synthetic、fixture与test manifest一律使用这五值，不得增加任何测试专用或预留的演化标签值，也不得设置测试旁路。一般判定规则为：`breaking-replacement`用于存在明确被替换旧wire contract的新独立版本；`new-contract`用于没有旧wire contract被替换的新对象，包括projection/allocation；`legacy-validation-only`只供显式历史输入Schema validation/fixture，不得用于production write、read或active response；`legacy-read-only`只供历史持久事实/response读取校验，不得作为request。`v1-stable`表示该retained合同仍按其引用方生命周期稳定使用。该字段与MethodVersionBinding lifecycle正交；尤其`new-contract`不表示public method可调用，也不等于internal visibility。正式闭集中没有`internal` compatibility：projection/allocation使用`new-contract`，依靠entry `kind`及“不得被MethodVersionBinding引用”的验证约束保持internal，不得增造第六种值或借compatibility表达可见性。
- 非retained component-native entry的ID必须**精确**为`https://schemas.shittim.local/<component>/<snake_case_name>/v<version>`，source必须精确镜像为`schemas/source/<component>/<snake_case_name>.v<version>.json`。一般情况下，`snake_case_name`由entry `title`移除末尾精确版本后缀`V<version>`后按项目canonical snake_case规则得到。KCP envelope是规范化命名空间已编码的固定规则：hard gate必须按`component=kcp`、`kind=envelope`与title精确为`KcpCommandEnvelopeV2`/`KcpQueryEnvelopeV2`应用规范stem，显式去掉领域前缀`Kcp`，分别固定为`command_envelope`/`query_envelope`；因此Command Envelope的ID/source固定为`https://schemas.shittim.local/kcp/command_envelope/v2`与`schemas/source/kcp/command_envelope.v2.json`，Query Envelope的ID/source固定为`https://schemas.shittim.local/kcp/query_envelope/v2`与`schemas/source/kcp/query_envelope.v2.json`，不得生成`kcp_kcp...`。这不是任意例外，而是component已编码kcp命名空间的规范stem规则。`snake_case_name`结果为一个或多个小写ASCII字母/数字片段以下划线连接，不允许大写、连字符、空片段。
production manifest的component-native entry分批次落地：首批为以下**正好12个**，title都带`V1`/`V2`后缀以避免Rust平面命名冲突；§13.6.2追加Event v2八项、§13.6.3追加root持久对象四项，现行合计24个。各批闭集及对应source/generated产物必须由manifest、生成漂移与Conformance持续验证：

| title | component | kind | compatibility | component-native ID |
|---|---|---|---|---|
| `InputContentOriginV1` | common | object | new-contract | `https://schemas.shittim.local/common/input_content_origin/v1` |
| `InputTaskScopeV1` | task | object | new-contract | `https://schemas.shittim.local/task/input_task_scope/v1` |
| `TaskCreateRequestV2` | kcp | kcp_request | breaking-replacement | `https://schemas.shittim.local/kcp/task_create_request/v2` |
| `NormalizedRootTaskCreatePayloadV2` | task | object | new-contract | `https://schemas.shittim.local/task/normalized_root_task_create_payload/v2` |
| `RootTaskCreateIdempotencyProjectionV1` | task | object | new-contract | `https://schemas.shittim.local/task/root_task_create_idempotency_projection/v1` |
| `TaskCreateResponseV2` | kcp | kcp_response | breaking-replacement | `https://schemas.shittim.local/kcp/task_create_response/v2` |
| `RootTaskCreateAllocationV2` | task | object | new-contract | `https://schemas.shittim.local/task/root_task_create_allocation/v2` |
| `ChildTaskProposalV1` | task | object | new-contract | `https://schemas.shittim.local/task/child_task_proposal/v1` |
| `NormalizedChildTaskProposalV1` | task | object | new-contract | `https://schemas.shittim.local/task/normalized_child_task_proposal/v1` |
| `ChildTaskMaterializationAllocationV1` | task | object | new-contract | `https://schemas.shittim.local/task/child_task_materialization_allocation/v1` |
| `KcpCommandEnvelopeV2` | kcp | envelope | breaking-replacement | `https://schemas.shittim.local/kcp/command_envelope/v2` |
| `KcpQueryEnvelopeV2` | kcp | envelope | breaking-replacement | `https://schemas.shittim.local/kcp/query_envelope/v2` |

本批12项的source级直接依赖与`$ref`写法固定如下；“无”表示document内不得出现`$ref`，不是说producer没有业务依赖。跨document一律使用absolute `$ref`，只有同document `$defs`使用local fragment：

| source root | component | 允许的直接`$ref`目标 | 直接/传递component依赖 |
|---|---|---|---|
| `InputContentOriginV1` | common | 无 | 无 |
| `InputTaskScopeV1` | task | 无 | 无 |
| `TaskCreateRequestV2` | kcp | `https://schemas.shittim.local/task/normalized_root_task_create_payload/v2#/$defs/{九字段}` | 直接`kcp→task`；经宿主defs传递`kcp→common` |
| `NormalizedRootTaskCreatePayloadV2` | task | root properties仅`#/$defs/{九字段}`；其`$defs/task_scope`→`https://schemas.shittim.local/task/input_task_scope/v1`，`$defs/origin`→`https://schemas.shittim.local/common/input_content_origin/v1` | 同component task；直接`task→common` |
| `RootTaskCreateIdempotencyProjectionV1` | task | retained Actor `https://schemas.shittim.local/v1/common/actor.json`、retained EntryPoint `https://schemas.shittim.local/v1/common/entry_point.json`、`https://schemas.shittim.local/task/normalized_root_task_create_payload/v2` | 直接`task→common`及task内引用 |
| `TaskCreateResponseV2` | kcp | retained `https://schemas.shittim.local/v1/task/task_spec.json` | 直接`kcp→task`；TaskSpec closure传递到common |
| `RootTaskCreateAllocationV2` | task | 无 | 无 |
| `ChildTaskProposalV1` | task | `https://schemas.shittim.local/task/normalized_root_task_create_payload/v2#/$defs/{九字段}` | task内；经宿主defs传递`task→common` |
| `NormalizedChildTaskProposalV1` | task | `https://schemas.shittim.local/task/normalized_root_task_create_payload/v2#/$defs/{九字段}` | task内；经宿主defs传递`task→common` |
| `ChildTaskMaterializationAllocationV1` | task | 无 | 无 |
| `KcpCommandEnvelopeV2` | kcp | retained Actor与EntryPoint common roots；不得有method payload conditional `$ref` | 直接`kcp→common` |
| `KcpQueryEnvelopeV2` | kcp | retained Actor与EntryPoint common roots；不得有method payload conditional `$ref` | 直接`kcp→common` |

`{九字段}`是表述集合，不是合法literal ref path；source中必须逐项写出精确fragment。`TaskCreateRequestV2`/`TaskCreateResponseV2`属于`kcp→task/common` closure，两个Envelope属于`kcp→common`，`RootTaskCreateIdempotencyProjectionV1`明确属于`task→common`，两个allocation文档无refs。

production manifest现已将retained `TaskCreateRequestV1`、`KcpCommandEnvelopeV1`、`KcpQueryEnvelopeV1` entry的`compatibility`标为`legacy-validation-only`，并将retained `TaskCreateResponseV1`标为`legacy-read-only`；其余retained 37项保持`v1-stable`。该compatibility改标只改变manifest演化标签，不得改41项retained ledger的ID/component/source/source SHA-256，也不得改对应source bytes；后续变更必须持续通过ledger、source hash与lifecycle测试证明这些不变量。

- registry在构造公开对象前调用单一权威`SchemaNode` walker。walker以pre-order提供canonical JSON Pointer、`is_root`与object/boolean node callback，并只遍历下列Draft 2020-12 Schema-bearing位置：map value `properties`、`patternProperties`、`dependentSchemas`、`$defs`、`definitions`；single Schema `additionalProperties`、`unevaluatedProperties`、`propertyNames`、`items`、`contains`、`unevaluatedItems`、`contentSchema`、`not`、`if`、`then`、`else`；Schema array `prefixItems`、`allOf`、`anyOf`、`oneOf`。存在但容器/node类型错误立即失败。loader对每个document用该walker建立私有不可变的authoritative SchemaNode pointer index；`resolve_ref`和public `schema_at`必须先验证目标pointer属于该index，raw JSON pointer lookup只能crate-private且命名明确。`$ref`指向`const/default/examples/enum`等实例位置时，即使目标JSON长得像Schema也fail closed；`$defs/properties/items`等合法Schema位置通过。restricted identity audit、registry `$ref` resolution/component gate、generation target dependency closure及restricted codegen support-profile audit均复用该walker callback，不得复制第二套通用位置闭集；target envelope提取只允许作为已命名的特定IR结构读取。`$ref`只在Schema node检查，禁止递归进入实例数据。map中的`$ref/$id/$schema/$dynamicRef`只是普通property/definition名称，其value仍作为Schema遍历。
- restricted identity/ref profile：仅root允许`$id`与`$schema`；任何nested非root `$id`/`$schema`立即失败；当前不支持的`$anchor`、`$dynamicAnchor`、`$dynamicRef`、`$recursiveAnchor`、`$recursiveRef`、`$vocabulary`均立即失败。上述invariant由`SchemaRegistry::load`统一实施，因此validate/check/generate不能绕过或延后。
- `generate`在plan/render全部成功后才进入artifact transaction。当前renderer的四个generated artifacts都在同一root，故`ArtifactPlan`和transaction入口要求distinct artifact roots**恰好一个**；0或多root明确unsupported，nested/multi-root泛化不在当前合同，未来实现必须采用新版本状态机并重新验收。transaction当前为**Linux real-platform verified**：repo root持久lock file使用Rust 1.97 `File::try_lock` OS advisory lock，文件描述符锁为权威，owner metadata只诊断；lock file不unlink，不依赖PID/stale回收。锁内先恢复旧journal，再在创建stage前durable写`Preparing`；stage、backup、rollback-discard采用固定transaction-id安全路径，均为artifact root同parent sibling，保证同filesystem。stage复制旧root（含非planned文件）、覆盖planned bytes并fsync tree，随后durable更新`Prepared`；已有root rename为backup并fsync parent，stage rename为root并fsync parent。rollback先durable写`RollingBack` substate，把new root rename到discard、恢复backup（或恢复原“不存在”）、再清discard，任意crash点重复执行不得删除已恢复old root。journal原子写为固定prefix temp regular file→file fsync→rename→repo dir fsync；write API必须区分rename未发生与rename已发生但dir fsync不确定，后者返回`commit outcome uncertain`、保留状态且绝不destructive rollback，下次按正式journal与可见路径恢复。durable `Committed`后只cleanup backup/stage/discard/journal；cleanup失败必须可见且下次可恢复。正式journal损坏fail closed并保留所有paths；无正式journal时只清固定temp prefix且验证regular non-symlink。成功允许持久lock file存在，但不得留下stage/backup/discard/journal/temp。其它平台尚未完成，必须contract-only/fail closed，不宣称跨平台durability。threat model只用advisory lock协调schema-tool generator；同用户恶意filesystem替换不在conformance安全边界，启动与每次操作仍检查symlink/类型，但不得宣称消除TOCTOU。

#### 13.6.1 Artifact transaction 与 LockPort 边界

artifact transaction 从 advisory FD lock 已成功取得且 owner metadata 已发布时开始。lock open/create/type/symlink/try-lock/truncate/seek/write/sync file/sync repo dir 是独立 LockPort 合同，不纳入 TransactionFs 矩阵；任一 owner metadata 失败不得读 journal、创建 stage 或改变 root，释放 FD 后下一次可成功。FD lock 权威，owner 仅诊断；lock file 永不 unlink，不按 PID/stale 回收。

TransactionFs 只覆盖语义 mutation/durability boundary，不宣称底层 OS syscall 穷举；read/metadata inspection 不在 fault 闭集。每个 target 产生 typed OperationEvent（semantic phase、operation、Before/AfterSuccess、logical path roles、journal target phase，动态 occurrence 仅区分重复目录/文件）。正式 Committed journal rename 是提交点：此前 existing=Old/absent=Absent，此后=New。FailureDisposition 为 NoMutation、RolledBackBeforeReturn、RecoveryRequired { RestoreOriginal | CleanResidue }、CommitOutcomeUncertain、CleanupDeferred、StoredStateInvalid；其中 StoredStateInvalid 表示正式journal存在但无法解码或验证，恢复必须fail closed且不得改变root或授权rollback/cleanup。crash sentinel 直接传播。普通补偿错误必须保留 primary+compensation，补偿 crash 不包装为普通错误。

矩阵从真实 typed trace 自动发现 reachable target，逐 target 验证 CrashBefore、CrashAfterSuccess、IoNoEffect 与已声明的代表性 IoPartial；不得把 after hook 当 I/O error。每例断言 target 精确一次、trace 前缀、即时完整 TreeSnapshot、返回 disposition、recover#1 精确终态及 recover#2 Noop/空 mutation trace。snapshot 包含 root/stage/backup/discard/formal journal/temp/lock，independent reference model 只消费 operation effect。control-flow/fault conformance 不是实际掉电介质模型；partial I/O 若无 journal-temp/stage-write/copy/remove 的明确 fake effect 不得声称覆盖。

registry按URL scheme/host/port/path组件校验上述归属，禁止裸prefix。relative `$ref`在新root base下解析后仍须命中registry，并先受component-ref gate、再受target closure约束。生成物catalog同时可解析旧retained ID与未来component ID。

测试覆盖manifest strict/v1拒绝、component prefix伪装、dot/double slash、percent encoding、default port、旧ID byte preservation、retained source合法Schema bytes篡改、retained重绑/协同搬家/真实孤儿/重复、retained与component namespace互斥、component-native精确三segment ID/source镜像/version一致、query/fragment/encoded/dot/empty/`.json` URL负例、source absolute/`..`/source-root外repo-relative/backslash/empty/dot/prefix trick、file/ancestor symlink、nested `$id` load拒绝、`$dynamicRef`跨component在validate/check/generate均load失败、跨component未声明ref、relative/absolute/local refs、target closure独立、CLI validate在registry load时拒绝未授权ref、synthetic合法非空binding与完整负例、production empty gate、两次生成稳定与失败无部分写。迁移ledger由每次registry load固定SHA-256验证41份source相对迁移前基线字节不变；这些generated hash不进入永久fixture，后续合法生成变更由Git基线审阅。

### 13.6.2 Event v2 八Schema实现合同（Schema/compiler/generated已落地）

本批已一次性加入恰好八项，该批落地后production manifest为61项（41 retained + 20 component-native；其后切片1a增至65项，见§13.6.3），production `method_version_bindings`继续为空。Schema source、manifest identity、single mapping IR、Event Catalog与typed decode已实现；后续第二实现段也已完成migration 0003、mixed Outbox API、strict stored decoder与savepoint poison。producer/Publisher/runtime仍未实现。

| title | component | kind | compatibility | exact ID | exact source | schema_version_field | direct refs |
|---|---|---|---|---|---|---|---|
| `ActionTransitionRefV1` | common | object | new-contract | `https://schemas.shittim.local/common/action_transition_ref/v1` | `schemas/source/common/action_transition_ref.v1.json` | null | 无 |
| `ConfirmationModeV1` | common | enum | new-contract | `https://schemas.shittim.local/common/confirmation_mode/v1` | `schemas/source/common/confirmation_mode.v1.json` | null | 无 |
| `CausationRefV2` | common | object | breaking-replacement | `https://schemas.shittim.local/common/causation_ref/v2` | `schemas/source/common/causation_ref.v2.json` | null | `ActionTransitionRefV1` |
| `ApprovalRecordKindV2` | policy | enum | new-contract | `https://schemas.shittim.local/policy/approval_record_kind/v2` | `schemas/source/policy/approval_record_kind.v2.json` | null | 无 |
| `ApprovalSubjectKindV2` | policy | enum | new-contract | `https://schemas.shittim.local/policy/approval_subject_kind/v2` | `schemas/source/policy/approval_subject_kind.v2.json` | null | 无 |
| `ActionStateChangedPayloadV1` | event | event_payload | new-contract | `https://schemas.shittim.local/event/action_state_changed_payload/v1` | `schemas/source/event/action_state_changed_payload.v1.json` | `schema_version` | retained `ActionStatus` |
| `ApprovalStateChangedPayloadV1` | event | event_payload | new-contract | `https://schemas.shittim.local/event/approval_state_changed_payload/v1` | `schemas/source/event/approval_state_changed_payload.v1.json` | `schema_version` | whole-schema root refs：`ConfirmationModeV1`、`ApprovalRecordKindV2`、`ApprovalSubjectKindV2` |
| `EventEnvelopeV2` | event | envelope | breaking-replacement | `https://schemas.shittim.local/event/event_envelope/v2` | `schemas/source/event/event_envelope.v2.json` | `schema_version` | whole-schema root refs：`CausationRefV2`与五payload roots |

`ConfirmationModeV1`属于common：它是PolicyRule、PermissionDecision、Approval request及公共事件共享的算法无关词表，不是Approval repository私有枚举；若放policy，common-level复用会形成错误的领域依赖。event `allowed_refs`已精确扩为`common,policy`，policy仍只指common，common不引用其它component；因此DAG保持`event→policy→common`无环。`ApprovalRecordKindV2`与`ApprovalSubjectKindV2`由policy拥有，因为它们描述Approval领域判别；event只能引用，不能复制平行enum。

`ActionTransitionRefV1`字段精确为`kind:"action_transition",action_id:UUID,transition_id:UUID`；`ConfirmationModeV1`闭集精确为`generic|local|system_authentication|remote_signature|plan_revision`；`ApprovalRecordKindV2`闭集精确为`request|resolution|invalidation`；`ApprovalSubjectKindV2`闭集精确为`operation|task_proposal|plan_revision`。以上三项enum闭集是本节权威，ADR-0008与API文档只能交叉引用，不得另立不同集合。`ApprovalStateChangedPayloadV1.change_kind`是该payload内部显式enum，闭集精确为`initial_request|resolution|invalidation_without_replacement|replacement_request`；它不是第九个Schema。`CausationRefV2`及两个event payload的完整shape与Schema/repository边界见§5.6、§6.14–6.15。EventEnvelopeV2字段不得新增`emitted_at`或把`delivered_at`塞入Envelope。本文“直接/whole-schema `$ref`”统一指：source属性值为仅含允许annotation sibling的`$ref`，fragment必须为空，解析后的canonical identity必须等于被引用manifest root；inline等价Schema与fragment ref都不算直接root引用。

active claimant、Catalog/typed生成及常量命名以§5.6为准；八项现已闭合五类active Event。compiler必须在registry load时把每个KCP/Event envelope解析一次并按schema id缓存同一strict `EnvelopeConditionalBinding` IR，Catalog authority、target closure与typed `EnvelopeWireBinding`只读取该缓存；通用IR位于独立`conditional_envelope`模块，`event_catalog`只做Event authority投影。active exact五项、legacy exact三项、target envelope/payload closure双向完整性继续硬验；不得增加第九个近义enum/schema，也不得把现有retained payload复制成component-native新版本。SQLite/producer后续未完成项必须按本合同实现，不能重新发明字段、命名、migration identity或KCP poll兼容策略。

### 13.6.3 V2InitialBuildActive切片1a：root持久对象四Schema（Schema/compiler/generated已落地）

切片1a在61项基线上新增四个component-native root；该批落地时production manifest为65项（41 retained + 24 component-native），切片1b后现行为70项（见§13.6.4）。`method_version_bindings`继续为空；repository/handler/producer不在本切片：

| title | component | kind | compatibility | exact ID | exact source | direct refs |
|---|---|---|---|---|---|---|
| `ContentOriginV2` | common | domain_object | breaking-replacement | `https://schemas.shittim.local/common/content_origin/v2` | `schemas/source/common/content_origin.v2.json` | retained `EntryPoint` |
| `AuditRecordV2` | audit | domain_object | breaking-replacement | `https://schemas.shittim.local/audit/audit_record/v2` | `schemas/source/audit/audit_record.v2.json` | retained `Actor`/`EntryPoint`、`CausationRefV2` |
| `AuditAllocationV2` | audit | object | new-contract | `https://schemas.shittim.local/audit/audit_allocation/v2` | `schemas/source/audit/audit_allocation.v2.json` | `CausationRefV2` |
| `TaskCreationProvenanceV1` | task | domain_object | new-contract | `https://schemas.shittim.local/task/task_creation_provenance/v1` | `schemas/source/task/task_creation_provenance.v1.json` | retained `Actor`/`EntryPoint` |

`AuditAllocationV2`按§6.16.0a的“正式required对象”、独立Audit路径Schema验证与跨语言传递要求source化；它不是仅在Rust内使用的allocation snapshot。`TaskCreationProvenanceV1`必须lower为schema-tool原生`TaggedUnion`，只允许`root_command_v2|child_action_v2`，不得恢复legacy/import分支或生成全部optional平面struct。

component DAG同步为：`common→[]`、`policy→[common]`、`audit→[common,policy]`、`task→[common,policy]`；后两者为本领域v2依赖闭包预留明确合法边，当前四source的实际direct refs仍只命中common。DAG无环，41 retained ownership/source bytes不变。

### 13.6.4 V2InitialBuildActive切片1b：Action/child授权五Schema（Schema/compiler/generated与pure crate已落地）

切片1b在65项基线上新增五个component-native root，production manifest现为70项（41 retained + 29 component-native），`method_version_bindings`继续为空；repository、handler、materializer与producer不在本切片：

| title | component | kind | compatibility | exact ID | exact source | direct refs |
|---|---|---|---|---|---|---|
| `ActionTransitionIntentV1` | task | domain_object | new-contract | `https://schemas.shittim.local/task/action_transition_intent/v1` | `schemas/source/task/action_transition_intent.v1.json` | retained `ActionStatus` |
| `ActionRequestV2` | task | domain_object | breaking-replacement | `https://schemas.shittim.local/task/action_request/v2` | `schemas/source/task/action_request.v2.json` | retained `ActionStatus`/`SideEffectClass` |
| `ChildTaskDeltaProjectionV1` | task | object | new-contract | `https://schemas.shittim.local/task/child_task_delta_projection/v1` | `schemas/source/task/child_task_delta_projection.v1.json` | none |
| `MaterialAuthorizationProjectionV1` | policy | object | new-contract | `https://schemas.shittim.local/policy/material_authorization_projection/v1` | `schemas/source/policy/material_authorization_projection.v1.json` | retained `Actor`/`EntryPoint`/`SideEffectClass` |
| `ObservationEvidenceProjectionV1` | policy | object | new-contract | `https://schemas.shittim.local/policy/observation_evidence_projection/v1` | `schemas/source/policy/observation_evidence_projection.v1.json` | none |

`ActionRequestV2`是§6.5/§6.5.1 active持久对象，不能复用retained v1：v1缺少`execution_generation`、`approval_chain_id`、result中的`materialized_child_task_ref`/`verification_result_refs`及Lease `generation`，且保留近义`approval_record_ref`。v2全部顶层字段required；可无事实的关系使用required-nullable或required nullable object，避免“漏写”与“已知无事实”混淆。

retained `VerificationResultV1`已完整承载child materialization所需的Kernel-local验证事实：UUID `id/action_id`、`strategy_used`、`outcome`、`verifier_kind`、observed/evidence refs、时间、结构化observations、`side_effect_confirmed`、`recommendation`与`created_at`；切片1b复用它，不新造v2。`ApprovalEventAllocationV1`属于Approval CAS与`approval.state_changed`批次，本批没有Approval repository/producer，因此明确留给切片1c。

新增pure crate `kernel-authorization`唯一拥有本批三投影的typed authoritative fact输入、构造、规则验证、Schema再验证、RFC8785 JCS与SHA-256。它依赖`kernel-contracts`与`domain-policy`唯一URI normalizer；不读SQLite/repository、不分配ID、不写存储、不替代Policy matcher。`SubjectProjectionV1`接口与实现一并留给切片1c，禁止本批放置无实现占位API。

IC §5.3.1要求的authorization projection official fixtures已落地为测试制品（wrapper非business Schema、不进manifest）：`schemas/fixtures/task/child_task_delta_projection.v1.json`、`schemas/fixtures/policy/material_authorization_projection.v1.json`、`schemas/fixtures/policy/observation_evidence_not_applicable.v1.json`、`schemas/fixtures/policy/observation_evidence_observed.v1.json`。每份含raw Facts JSON、normalized object、`jcs_utf8_hex`/`sha256`与tamper；`not_applicable`与`observed`各自独立，observed覆盖snapshot成对空值、证据排序与伪provider负例。shared wrapper owner为`schema-tool::official_fixture::ProjectionFixture`；production harness为`kernel-authorization` tests，CLI oracle为`schema-tool` tests。

component DAG不变：`policy→[common]`、`task→[common,policy]`均覆盖本批direct refs，保持无环；41 retained ownership/source bytes不变。

### 13.6.5 V2InitialBuildActive切片1c-i：授权核心五Schema（Schema/compiler/generated与Subject pure API已落地）

切片1c-i在70项基线上新增五个policy component-native root，production manifest现为75项（41 retained + 34 component-native），`method_version_bindings`继续为空；Credential/Challenge/Evidence/Remote signature家族明确留给1c-ii，repository、CAS、producer不在本切片：

| title | component | kind | compatibility | exact ID | exact source | direct refs |
|---|---|---|---|---|---|---|
| `PermissionDecisionV2` | policy | domain_object | breaking-replacement | `https://schemas.shittim.local/policy/permission_decision/v2` | `schemas/source/policy/permission_decision.v2.json` | `ConfirmationModeV1`、retained `SideEffectClass` |
| `PolicyRuleV2` | policy | domain_object | breaking-replacement | `https://schemas.shittim.local/policy/policy_rule/v2` | `schemas/source/policy/policy_rule.v2.json` | `ConfirmationModeV1`、retained `Actor`/`EntryPoint`/`SideEffectClass` |
| `ApprovalRecordV2` | policy | domain_object | breaking-replacement | `https://schemas.shittim.local/policy/approval_record/v2` | `schemas/source/policy/approval_record.v2.json` | `ConfirmationModeV1`、retained `Actor`/`EntryPoint`/`SideEffectClass` |
| `SubjectProjectionV1` | policy | object | new-contract | `https://schemas.shittim.local/policy/subject_projection/v1` | `schemas/source/policy/subject_projection.v1.json` | retained `SideEffectClass` |
| `ApprovalEventAllocationV1` | policy | object | new-contract | `https://schemas.shittim.local/policy/approval_event_allocation/v1` | `schemas/source/policy/approval_event_allocation.v1.json` | `CausationRefV2` |

`ApprovalRecordV2`由SchemaGraph lower为外层`record_kind`和内层`subject_kind`真正tagged enum；不得恢复全部optional平面对象。Schema硬约束request challenge nullability、invalidation expiry null、reason/evidence去重，以及resolution中零或单一专属证据槽；request id/expiry与顶层镜像、resolution mode与原request、exact evidence ref equality、approved/denied expiry、plan subject-mode一致性仍由未来repository在同事务验证，Schema不得伪称具备跨record历史事实。

`kernel-authorization::project_subject_projection(SubjectProjectionFactsV1)`现已覆盖operation/task_proposal/plan_revision三typed branch，负责字段规则验证、lowercase UUID构造、Schema再验证、RFC8785 JCS与SHA-256；不读repository、不分配ID、不执行Approval CAS。official fixture唯一路径为`schemas/fixtures/policy/subject_projection.v1.json`，三branch各保存完整subject、raw typed facts JSON、normalized projection、JCS UTF-8 hex、SHA-256与逐字段tamper；共享wrapper owner为`schema-tool::official_fixture::SubjectProjectionFixture`，production harness在`kernel-authorization`，CLI oracle在`schema-tool` tests。`ApprovalEventAllocationV1`按既有allocation惯例提供Schema-only official fixture `schemas/fixtures/policy/approval_event_allocation.v1.json`：保存allocation向量与shape tamper，不保存JCS/hash preimage；跨字段ID/opaque互异、真实causation与重放allocation复用留repository切片闭合。

component DAG保持`policy→[common]`无环；41 retained ownership/source bytes不变。

### 13.6.6 V2InitialBuildActive切片1c-ii：身份/挑战/证据与远程签名八Schema（Schema/compiler/generated已落地）

切片1c-ii在75项基线上新增八个policy component-native root，production manifest现为83项（41 retained + 42 component-native），`method_version_bindings`继续为空；repository、CAS、producer与真实验签路径不在本切片：

| title | component | kind | compatibility | exact ID | exact source | direct refs |
|---|---|---|---|---|---|---|
| `RemoteSignatureAlgorithmV1` | policy | object | new-contract | `https://schemas.shittim.local/policy/remote_signature_algorithm/v1` | `schemas/source/policy/remote_signature_algorithm.v1.json` | none |
| `CredentialRefV1` | policy | object | new-contract | `https://schemas.shittim.local/policy/credential_ref/v1` | `schemas/source/policy/credential_ref.v1.json` | `RemoteSignatureAlgorithmV1` |
| `RemoteApprovalChallengeV1` | policy | object | new-contract | `https://schemas.shittim.local/policy/remote_approval_challenge/v1` | `schemas/source/policy/remote_approval_challenge.v1.json` | `CredentialRefV1` |
| `SystemAuthenticationChallengeV1` | policy | object | new-contract | `https://schemas.shittim.local/policy/system_authentication_challenge/v1` | `schemas/source/policy/system_authentication_challenge.v1.json` | none |
| `LocalPresenceEvidenceV1` | policy | object | new-contract | `https://schemas.shittim.local/policy/local_presence_evidence/v1` | `schemas/source/policy/local_presence_evidence.v1.json` | retained `Actor`/`EntryPoint` |
| `SystemAuthenticationEvidenceV1` | policy | object | new-contract | `https://schemas.shittim.local/policy/system_authentication_evidence/v1` | `schemas/source/policy/system_authentication_evidence.v1.json` | none |
| `RemoteApprovalResponseV1` | policy | object | new-contract | `https://schemas.shittim.local/policy/remote_approval_response/v1` | `schemas/source/policy/remote_approval_response.v1.json` | `CredentialRefV1`、retained `Actor` |
| `RemoteApprovalSignaturePreimageV1` | policy | object | new-contract | `https://schemas.shittim.local/policy/remote_approval_signature_preimage/v1` | `schemas/source/policy/remote_approval_signature_preimage.v1.json` | none |

wire字段严格翻译§6.10.2–§6.10.4：`RemoteSignatureAlgorithmV1`只允许`ed25519` tagged branch；`allowed_decisions`是精确有序array const（remote=`["approved","denied"]`，system=`["approved"]`）；nonce为`base64url_no_pad`且编码长度≥43（解码≥32 bytes）；Ed25519 public key/signature编码长度固定43/86。状态机`issued→consumed|expired|revoked`与TTL≤5分钟是repository/CAS义务，Schema只编码闭集state与时间字段shape，不得伪称相对时间算术。schema-tool restricted profile为此批次原生支持string-array `const`与`maxLength` assertion；既有75项生成字节经drift gate保持稳定。

official fixtures位于`schemas/fixtures/policy/`：`remote_signature_algorithm.v1.json`、`credential_ref.v1.json`、`remote_approval_challenge.v1.json`、`system_authentication_challenge.v1.json`、`local_presence_evidence.v1.json`、`system_authentication_evidence.v1.json`、`remote_approval_response.v1.json`、`remote_approval_signature_preimage.v1.json`。每份含`valid_object`与逐字段/闭集/顺序/长度tamper；preimage fixture额外保存JCS UTF-8 hex与SHA-256。conformance harness为`kernel-contracts` tests `identity_challenge_evidence_v1_schema_conformance`。

component DAG保持`policy→[common]`无环；41 retained ownership/source bytes不变。

### 13.6.7 V2InitialBuildActive切片2：fresh SQLite 基线 + root TaskCreate v2 repository（已落地）

切片2不新增manifest Schema（仍为83项，production bindings仍空）。它在`kernel-sqlite`落地：

1. **migration 0004**（descriptor v1，`SchemaOnly` phase set）：`content_origins_v2`（canonical `record_json` + 生成列投影 + `content_origin_v2_parent_refs` 序列表）、`task_creation_provenances`（`record_json` + kind 投影 + 显式 `task_id` 列，读回强制列值与对应 Task 一致）、`audit_records_v2`、`root_task_create_idempotency_v2`（scope 四元组唯一 + `request_hash` + `created_task_id`/`creation_provenance_id`）。为允许 Task/Scope 引用 ContentOriginV2，0004 重建 `tasks`/`task_scopes`/`task_scope_source_refs`/`task_create_idempotency`，去掉对 v1 `content_origins` 的硬 FK，但恢复 `tasks.task_scope_ref → task_scopes` deferred FK（与 `task_scopes.task_id → tasks` 成环，同批重建可行）；origin 存在性由 repository canonical readback 证明。0001–0003 asset bytes 不变。
2. **root repository** `WriteTransaction::create_root_task_v2(RootTaskCreateV2Command)`：输入 `TaskCreateRequestV2` + `RootTaskCreateAllocationV2` + envelope facts + 调用方注入的唯一 `accepted_at`（repository 不读时钟）。流程固定为 `kernel-task-creation` normalize/receipt/idempotency/allocation validate → 统一 savepoint 内按顺序写 ContentOriginV2、TaskScope v1、TaskSpec v1（`parent_task_id=null`）、`TaskCreationProvenanceV1(root_command_v2)`、`AuditRecordV2(task.creation_recorded)` → 再写 idempotency → 最后 `append_active_event_v2(task.created)` → 与 Created 等价的全闭包 canonical readback（Task/Origin/Scope/Provenance↔task/Audit/Outbox Event/idempotency 交叉一致）。同 scope 四元组 + 同 hash 重放执行同一闭包读回并返回已存 Task（不产生新 Event）；异 hash → `idempotency_conflict`；mapping 存在但闭包不完整 → `stored_data_invalid`，不写新事实。失败整体回滚不占号。
3. **明确不在本切片**：KCP preflight/handler/bindings（切片3）；v1 写路径删除与旧库 reinitialize-required（切片6）；child materializer（切片5）。

### 13.7 V2InitialBuildActive唯一谓词

`V2InitialBuildActive`为当前v2从零构建里程碑的完成谓词（ADR-0009；**取代并作废**原`V2ProductionWriteCutover`）。当前存储格式就是v2 fresh baseline：不执行、也不支持v1业务数据迁移。仅当以下全部为真：

1. v2 source、manifest entries、generated Rust目标与MethodVersionBinding完整且drift clean；production bindings精确为§13.5目标表（`task.create` active=`[2]`、legacy validation=`[1]`仅用于判定`unsupported_schema_version`；其余七方法 active=`[1]`）；
2. root `task.create` v2是唯一active写路径：repository + handler + method-aware preflight + production bindings；
3. child唯一经`task.child.create` Action materializer；同一Action全生命周期最多一个child，bundle全有或全无；
4. Action / PermissionDecision / Approval v2 repositories实现真实持久化闭环（canonical readback、CAS、必要Outbox与错误映射）；
5. 有业务owner的active Event producer接入：root/child的`task.created`、`action.state_changed`、`approval.state_changed`；禁止伪造无owner的producer；
6. v1 runtime写路径已删除（v1 task.create handler/adapter/repository write、legacy direct-child写、`append_legacy_event_v1`、`StoredEventEnvelope::LegacyV1` production variant），而非保留版本分支；
7. 旧开发数据库open/start明确拒绝并返回稳定可诊断的`reinitialize-required`错误；禁止自动清库、隐式升级、读后补写。

**明确不在本里程碑**：Publisher loop、versioned KCP `event.poll` response/binding/handler；durable Outbox写入与异步发布是两个责任。retained `EventPollResponse` v1不得返回EventEnvelope v2。

阶段纪律：未满足本谓词前，不得启动可连接server；不得把空bindings或v1内部handler宣传为production可用。失败必须fail closed，不能进入“半迁移/半切换”状态。

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
- 不得凭不稳定外部观察或未验证的瞬时上下文引用执行副作用；具体 Profile 必须在自己的规范中定义稳定引用、过期判定与重新观察规则；
- 在恢复中重放未知外部副作用；
- Kernel Control Protocol 与 Extension RPC 混用同一 envelope；
- Delegation 越界自动降级为确认而不重新评估 Policy；
- `unknown_side_effect` 被当作"可能成功"而乐观继续后续 Action。
