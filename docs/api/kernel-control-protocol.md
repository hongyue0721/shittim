# Kernel Control Protocol

> 状态：active KCP合同要求method-aware payload version，`task.create` active版本为v2 root-only；首批 Envelope V2 / TaskCreate v2 Schema与generated root types 已落地，但production MethodVersionBindings 仍为空。当前 `kernel-kcp` 的Value preflight/dispatcher/handler仍是retained v1库级实现，active method-aware preflight、runtime cutover与可连接server均未完成。字段与行为的唯一事实源是 [`IMPLEMENTATION_CONTRACTS.md` 第5节](../../specs/IMPLEMENTATION_CONTRACTS.md#5-kernel-control-protocol)。

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
| `task.create` | Command | active v2只创建root Task | v2精确projection；v1 legacy frozen，不进入active dispatcher |
| `task.get` | Query | 只读 | 不适用 |
| `task.list` | Query | 只读 | 不适用 |
| `event.subscribe` | Query | 创建连接级临时订阅句柄，无领域副作用 | 不适用 |
| `event.poll` | Query | 只读长轮询 | 不适用 |
| `stop.activate` | Command | 激活 Kernel Stop Fence，并执行 Emergency Stop 的 Kernel 副作用集 | 当前全局 generation |
| `stop.status` | Query | 只读 | 不适用 |

完整请求/响应payload、排序、cursor与方法专属错误见权威规范。新Child Task只通过父Task Action原子materialization。当前`kernel-sqlite`已实现migration 0003与mixed Outbox，但`kernel-kcp` runtime仍只有legacy v1 create/get；active runtime、child Action、Audit v2、active producer与mixed poll均未实现。

## Value preflight 与 registration 合同

- 输入只接受调用方已经解析的 `serde_json::Value`，不接 bytes/UTF-8/JSON parse/frame。
- 现有 Value preflight、registration 与 dispatcher 是 retained v1 库级路径；active method-aware payload version preflight 仍待完成。最终优先级、完整 Schema/generated decode 与 production binding/cutover 以 IC §5.11、§13.5 为准。
- active `task.create`只接受v2；v1虽有known legacy Schema，也必须返回`unsupported_schema_version`且不能进入active registration。当前Rust实现尚未完成该升级。
- MethodVersionBinding完整validator已在工具阶段以synthetic 8-method非空manifest测试；expected family/method集合直接从registry V2 Envelope facts派生，不读取generated catalog形成循环。production manifest仍由`validate_production_manifest_stage`在check/generate入口保持空，synthetic registry不走该stage gate，最终V2ProductionWriteCutover才启用。`KCP_ENVELOPE_AUTHORITY_*`只表达family structure authority；`METHOD_VERSION_BINDINGS`表达bound version；两者都不代表registration/handler/server可用。不得以旧`KCP_METHODS`类名称混淆三层职责。
- active `task.create` v2的task creation纯逻辑正式owner已实现为crate`kernel-task-creation`：本阶段只负责root/child proposal normalize、root receipt/idempotency、child proposal/receipt hash与root/child allocation validation；authorization projections另切片。它依赖`kernel-contracts`并调用`domain-policy`唯一URI parser，不依赖KCP/SQLite、不分配ID、不读repo、不写存储，全部事实由caller typed input注入；handler/未来SQLite adapter只调用，不复制。当前尚未接入handler/repository。
- task creation三份official测试制品已位于root `schemas/fixtures/kcp/task_create_normalized_hash.v2.json`、child `schemas/fixtures/task/child_task_proposal_normalized_hash.v1.json`、allocation `schemas/fixtures/task/task_creation_allocations.v1.json`；wrapper不是business Schema。official fixtures/harness已完成：production owner负责业务hash关系，独立CLI进程/Schema路径负责中立validate/canonicalize，stored bytes/hash自一致性共享唯一JCS authority；repository/handler/materializer/cutover仍未完成。
- request ID 不可关联时本地拒绝且不发响应；可关联的五类 preflight error 使用固定安全 message、`details=null`、`retryable=false`，并经过不可替换 Response Schema 门。
- 八方法合法请求都必须先成为 generated typed Accepted；三方法 narrow 为 `RegisteredRequest`，其余五个得到本地不可序列化 `KnownCatalogMethodNotImplemented`，不是 wire error。
- 公开调用分成 `preflight_value -> narrow_to_registered -> TypedDispatcher.dispatch`，已在 `kernel-kcp` 实现；详细 API 见 [`kcp-preflight-dispatcher.md`](kcp-preflight-dispatcher.md)。

## 三方法 typed handler 边界

- 输入已通过对应 Envelope Schema、方法 payload Schema 与 typed decode；正常 dispatcher 路径还会先 narrow 为 `RegisteredRequest`。现有公共 `handle_*` 对错误 family/variant 返回本地 InputMethodMismatch；`serde_json::Value` preflight、bytes/frame、protocol/auth/method/schema 分类不在 handler 内，`invalid_request` 不由 typed handler 产生。
- 输出 payload 先按原方法 response Schema 校验，再将最终成功/错误 Response Envelope 按通用 Schema 校验。
- 响应固定 `protocol_version=1.0`、`message_kind=response`、request ID 原样；success/error 互斥。Response 无 method discriminator，调用方依原请求方法校验成功 payload。
- `KernelClock`、`KernelIdGenerator` 与闭集 `BackendError` 的 Task backend 可注入；backend 只暴露 create/get 高阶操作，不暴露 SQLite transaction 或 SQL。SQLite adapter 必须逐项把 `StoreErrorCode` 转成公开 backend 分类或 Internal，禁止消息匹配。
- deadline 将 Envelope RFC 3339 文本解析为 UTC instant 后比较，禁止字符串比较；解析失败在 ID/backend 前返回 `internal_error`。
- `task.create` 的第一次时钟读取同时是入口 deadline 检查和唯一 `accepted_at`。**本段描述的当前已实现 handler 是 legacy v1**：在 backend 前按 purpose 分配六个对象 ID（Task/Scope/Origin/receipt/Audit/Event），为合法、两两不同的唯一 UUID，版本不固定；correlation/dedup 是独立生成的非空 opaque 值，不从 caller 字段派生。**active `task.create` v2 不使用该六 UUID 口径**，必须由自身`schema_version=2`的`RootTaskCreateAllocationV2`分配**七个** UUID（`task_id,task_scope_id,content_origin_id,kernel_receipt_id,creation_provenance_id,audit_record_id,task_created_event_id`，含 provenance）与两个non-empty opaque ID；schema_version不计入七UUID。未来helper先验证Schema，再用typed `RootTaskCreateExternalUuidRefsV1 {command_request_id,delegation_ref,parent_origin_refs}`验证内部UUID互异、external碰撞与opaque独立；不接受自由UUID bag，该Rust input也不进manifest。legacy 六 UUID 与 active 七 UUID不得混写。
- SQLite 创建事务不可中途取消。commit 后到期仍返回 `deadline_exceeded`，但事实保留；客户端用同一 idempotency key 重放或用已知 Task ID 查询。
- Created/Replayed 都返回当前 Task；Created 的 **backend 结果**还必须返回与本次 operation 中 Event UUID 相等的 `committed_event_id`（它不是 wire `TaskCreateResponse` 字段），仅据此产生一个 post-commit Publisher wake-up intent。它不表示 Event 已 delivered；后续 deadline/internal/response contract failure 仍保留 intent，通知失败不回滚 durable Outbox。

## 实现阶段门

当前已实现的是 retained v1 Value preflight/registration/dispatcher 与三个 legacy typed application handler；五个 Catalog 方法仍无正式 handler。active method-aware preflight、八方法 registration 完整闭合、bytes/frame/transport/server 生命周期均未完成，因此不得启动 server，也不新增 `method_unavailable`。


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

- KCP retained v1 Envelope与八方法v1 request/response JSON Schema：`schemas/source/kcp/`；首批12个Schema的source/manifest entries与generated Rust root types已落地，包括两Envelope V2与TaskCreate request/response V2；production MethodVersionBindings仍为空。
- 生成的 Rust 类型与manifest catalog，以及现有 Command/Query/Event typed decode：`kernel-contracts`（见 [schema-generation.md](schema-generation.md)）。通用`decode_validated`、结构化post-Schema decode taxonomy与共享format assertion配置已完成；当前运行时仍以retained v1路径为准，active method-aware cutover尚未完成；
- Response Envelope 只按 `status = ok | error` 校验。它不携带原始方法 discriminator，因此不生成方法级 typed envelope；handler/客户端必须根据原请求方法用对应 response Schema 校验成功 `payload`，再校验通用 Response Envelope；
- 这表示不可连接 Value preflight、三方法 dispatcher/handler 已可供未来组合根调用；不表示五个缺失 handler或 KCP server 已可用。
