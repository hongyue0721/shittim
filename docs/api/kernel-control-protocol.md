# Kernel Control Protocol

> 状态：active KCP合同要求method-aware payload version，`task.create` active版本为v2 root-only；Envelope V2 / TaskCreate v2 Schema、generated root types、**production MethodVersionBindings（切片3a）** 与 **kernel-kcp method-aware runtime（切片3b）** 已落地。`V2InitialBuildActive` 完整谓词与可连接 server 仍未完成（五方法无 handler；Publisher/poll 未实现）。字段与行为的唯一事实源是 [`IMPLEMENTATION_CONTRACTS.md` 第5节](../../specs/IMPLEMENTATION_CONTRACTS.md#5-kernel-control-protocol)。

## 定位

KCP 是 `desktop-client`、`agent-runtime` 和其他内部客户端访问 `agentd` 的唯一控制协议。KCP 与 Extension RPC 分离，也不等同于 JSON-RPC。

首批本地传输选择：

- Unix：Unix Domain Socket；
- Windows：Named Pipe；
- 帧与连接决策见 [`ADR-0003`](../../adr/0003-kcp本地传输.md)。

## Envelope 要点

active结构合同使用`KcpCommandEnvelopeV2`=`https://schemas.shittim.local/kcp/command_envelope/v2`与`KcpQueryEnvelopeV2`=`https://schemas.shittim.local/kcp/query_envelope/v2`；两份production source与generated untyped root types已实现，且各有0个method payload conditional `$ref`。schema-tool library已实现claimant式authority发现、partial impostor拒绝及同target closure验证。顶层`protocol_version`仍是`1.0`。新Envelope只验证结构、family method与根payload positive `schema_version`，各有0个method payload conditional `$ref`。因此不生成`TypedKcpCommandEnvelopeV2`/`TypedKcpQueryEnvelopeV2`及其0-ref同义wrapper；这不禁止其它有正式判别映射的`Typed*V2`。payload业务Schema由MethodVersionBinding按family+method+request version选择。retained v1 Envelope不得修改，只负责legacy validation/typed decode，不是active Catalog authority；其legacy catalog与未来active catalog正交共存。Query payload当前仍v1，但两阶段验证架构本身是breaking change，因此Query Envelope V2保留。

- `protocol_version`：第一版为 `1.0`；
- `actor`：保留 `source`，不包含 EntryPoint；
- `entry_point`：只在 Envelope；
- `auth`：v1必须为`null`。在实际执行Value preflight auth判定时，非null返回`unsupported_auth_schema`；official root fixture harness不执行preflight，非null raw Schema拒绝固定投影为`invalid_request`/details null。
- `actor.kind = owner`：只是未来 Owner/授权系统的预留标签，第一版不据此认定已认证或授予任何权限；
- `deadline`：必填，过期返回 `deadline_exceeded`，不得静默丢弃；已开始且不能安全取消的外部动作先进入恢复待查；
- Command 带 `idempotency_key`；Query 不带；
- payload 带独立 `schema_version`。

## 首批方法

| 方法 | 类型 | 状态/副作用 | 幂等说明 |
|---|---|---|---|
| `system.ping` | Query | 只读 | 不适用 |
| `task.create` | Command | active v2只创建root Task | v2精确projection；kcp production 入口仅 v2；v1 → `unsupported_schema_version`；legacy sqlite create write 已删（切片3c） |
| `task.get` | Query | 只读 | 不适用 |
| `task.list` | Query | 只读 | 不适用 |
| `event.subscribe` | Query | 创建连接级临时订阅句柄，无领域副作用 | 不适用 |
| `event.poll` | Query | 只读长轮询 | 不适用 |
| `stop.activate` | Command | 激活 Kernel Stop Fence，并执行 Emergency Stop 的 Kernel 副作用集 | 当前全局 generation |
| `stop.status` | Query | 只读 | 不适用 |

完整请求/响应payload、排序、cursor与方法专属错误见权威规范。新Child Task只通过父Task Action原子materialization。当前`kernel-sqlite`已实现统一 **v2-only** Outbox（legacy append 已删）及 root `create_root_task_v2`；`kernel-kcp` runtime 已接 method-aware preflight 与 active root create v2 / get / ping。child Action materializer、其余 active producer、Publisher 与 versioned poll 未实现。

## Value preflight 与 registration 合同

- 输入只接受调用方已经解析的 `serde_json::Value`，不接 bytes/UTF-8/JSON parse/frame。
- Value preflight、registration 与 dispatcher 是 method-aware 库级路径（切片3b）：V2 Envelope 结构门 + `select_request_version` 业务门。最终优先级、完整 Schema/generated decode 与 production binding/`V2InitialBuildActive` 以 IC §5.11、§13.5、§13.7 为准。
- active `task.create`只接受v2；v1虽有known legacy Schema，也必须返回`unsupported_schema_version`且不能进入active registration（已实现）。
- MethodVersionBinding完整validator已在工具阶段以synthetic 8-method非空manifest测试；expected family/method集合直接从registry V2 Envelope facts派生。切片3a起production manifest由`validate_production_manifest_stage`断言精确等于IC §13.5目标表；`KCP_ENVELOPE_AUTHORITY_*`只表达family structure authority；`METHOD_VERSION_BINDINGS`表达bound version；**registration/handler 可用性由 kernel-kcp 显式实现，不由 bindings 暗示 server 可连接**。
- active `task.create` v2的task creation纯逻辑正式owner为crate`kernel-task-creation`；repository `create_root_task_v2` 与 kcp handler/adapter 已接入（切片2/3b）。authorization projections / child materializer 另切片。
- task creation三份official测试制品已位于root `schemas/fixtures/kcp/task_create_normalized_hash.v2.json`、child `schemas/fixtures/task/child_task_proposal_normalized_hash.v1.json`、allocation `schemas/fixtures/task/task_creation_allocations.v1.json`。child materializer 与 §13.7 完整谓词仍未完成。
- request ID 不可关联时本地拒绝且不发响应；可关联的五类 preflight error 使用固定安全 message、`details=null`、`retryable=false`，并经过不可替换 Response Schema 门。
- 八方法合法请求都必须先成为 generated typed Accepted；三方法 narrow 为 `RegisteredRequest`，其余五个得到本地不可序列化 `KnownCatalogMethodNotImplemented`，不是 wire error。
- 公开调用分成 `preflight_value -> narrow_to_registered -> TypedDispatcher.dispatch`，已在 `kernel-kcp` 实现；详细 API 见 [`kcp-preflight-dispatcher.md`](kcp-preflight-dispatcher.md)。

## 三方法 typed handler 边界

- 输入已通过对应 Envelope Schema、方法 payload Schema 与 typed decode；正常 dispatcher 路径还会先 narrow 为 `RegisteredRequest`。现有公共 `handle_*` 对错误 family/variant 返回本地 InputMethodMismatch；`serde_json::Value` preflight、bytes/frame、protocol/auth/method/schema 分类不在 handler 内，`invalid_request` 不由 typed handler 产生。
- 输出 payload 先按原方法 response Schema 校验，再将最终成功/错误 Response Envelope 按通用 Schema 校验。
- 响应固定 `protocol_version=1.0`、`message_kind=response`、request ID 原样；success/error 互斥。Response 无 method discriminator，调用方依原请求方法校验成功 payload。
- `KernelClock`、`KernelIdGenerator` 与闭集 `BackendError` 的 Task backend 可注入；backend 只暴露 create/get 高阶操作，不暴露 SQLite transaction 或 SQL。SQLite adapter 必须逐项把 `StoreErrorCode` 转成公开 backend 分类或 Internal，禁止消息匹配。
- deadline 将 Envelope RFC 3339 文本解析为 UTC instant 后比较，禁止字符串比较；解析失败在 ID/backend 前返回 `internal_error`。
- `task.create` 的第一次时钟读取同时是入口 deadline 检查和唯一 `accepted_at`。**当前已实现 active root v2 handler**：backend 前按 purpose 分配 **七个** UUID（`Task`/`TaskScope`/`ContentOrigin`/`KernelReceipt`/`CreationProvenance`/`AuditRecord`/`Event`）与两个 non-empty opaque ID；映射 `RootTaskCreateAllocationV2`。repository 侧再以 typed `RootTaskCreateExternalUuidRefsV1` 验证内部 UUID 互异、external 碰撞与 opaque 独立。root-only：Envelope `task_id`/`expected_revision` 必须 null；成功响应为 `TaskCreateResponseV2`。
- SQLite 创建事务不可中途取消。commit 后到期仍返回 `deadline_exceeded`，但事实保留；客户端用同一 idempotency key 重放或用已知 Task ID 查询。
- Created/Replayed 都返回当前 Task；Created 的 **backend 结果**还必须返回与本次 operation 中 Event UUID 相等的 `committed_event_id`（它不是 wire `TaskCreateResponse` 字段），仅据此产生一个 post-commit Publisher wake-up intent。它不表示 Event 已 delivered；后续 deadline/internal/response contract failure 仍保留 intent，通知失败不回滚 durable Outbox。

## 实现阶段门

当前已实现 method-aware Value preflight/registration/dispatcher 与三个 typed application handler（ping / create v2 / get）；五个 Catalog 方法仍无正式 handler。bytes/frame/transport/server 生命周期均未完成，因此不得启动 server，也不新增 `method_unavailable`。


## Cursor

Event cursor 只使用十进制字符串表示的全局 `outbox_position`。`sequence` 只用于聚合内顺序，不能作为全局 cursor。列表 cursor 是 Kernel 生成的不透明字符串；`task.list` 的具体 cursor 编码尚未选择，必须在 Task repository 实现前通过 ADR/API 拍板。

## 当前不可用项

- 已有 Value preflight/registration/dispatcher 与三个 typed application handler Rust 实现；公共 raw 边界只接受调用方已解析的 `Value`；
- 其余五个 Catalog 方法没有正式 handler；
- 没有 Socket/Pipe server；
- 没有可运行的 `agentd` 组合根；
- 没有 TypeScript client 包；
- 没有认证扩展；
- 没有 TCP/HTTP/JSON-RPC KCP endpoint。

## 已有契约产物

- KCP retained v1 Envelope与八方法v1 request/response JSON Schema：`schemas/source/kcp/`；首批12个Schema的source/manifest entries与generated Rust root types已落地，包括两Envelope V2与TaskCreate request/response V2；production MethodVersionBindings为IC §13.5八方法集（切片3a），generated `select_request_version`可用。
- 生成的 Rust 类型与manifest catalog，以及现有 Command/Query/Event typed decode：`kernel-contracts`（见 [schema-generation.md](schema-generation.md)）。通用`decode_validated`、结构化post-Schema decode taxonomy与共享format assertion配置已完成；当前运行时仍以retained v1路径为准，active method-aware 初始交付尚未完成；
- Response Envelope 只按 `status = ok | error` 校验。它不携带原始方法 discriminator，因此不生成方法级 typed envelope；handler/客户端必须根据原请求方法用对应 response Schema 校验成功 `payload`，再校验通用 Response Envelope；
- 这表示不可连接 Value preflight、三方法 dispatcher/handler 已可供未来组合根调用；不表示五个缺失 handler或 KCP server 已可用。
