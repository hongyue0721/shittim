# Kernel Control Protocol

> 状态：Envelope/Schema、`serde_json::Value` preflight、三方法 registration/dispatcher 与 `system.ping` / `task.create` / `task.get` 的不可连接 Rust typed application handler 已实现；仍无可连接 server。字段与行为的唯一事实源是 [`IMPLEMENTATION_CONTRACTS.md` 第 5 节](../../specs/IMPLEMENTATION_CONTRACTS.md#5-kernel-control-protocol)。

## 定位

KCP 是 `desktop-client`、`agent-runtime` 和其他内部客户端访问 `agentd` 的唯一控制协议。KCP 与 Extension RPC 分离，也不等同于 JSON-RPC。

首批本地传输选择：

- Unix：Unix Domain Socket；
- Windows：Named Pipe；
- 帧与连接决策见 [`ADR-0003`](../../adr/0003-kcp本地传输.md)。

## Envelope 要点

- `protocol_version`：第一版为 `1.0`；
- `actor`：保留 `source`，不包含 EntryPoint；
- `entry_point`：只在 Envelope；
- `auth`：v1 必须为 `null`，非 null 返回 `unsupported_auth_schema`；
- `actor.kind = owner`：只是未来 Owner/授权系统的预留标签，第一版不据此认定已认证或授予任何权限；
- `deadline`：必填，过期返回 `deadline_exceeded`，不得静默丢弃；已开始且不能安全取消的外部动作先进入恢复待查；
- Command 带 `idempotency_key`；Query 不带；
- payload 带独立 `schema_version`。

## 首批方法

| 方法 | 类型 | 状态/副作用 | 幂等说明 |
|---|---|---|---|
| `system.ping` | Query | 只读 | 不适用 |
| `task.create` | Command | 创建 Task Kernel 事实，不执行外部副作用 | actor.id/entry_point/command_type/idempotency_key scope；精确 projection + JCS hash 见下文 |
| `task.get` | Query | 只读 | 不适用 |
| `task.list` | Query | 只读 | 不适用 |
| `event.subscribe` | Query | 创建连接级临时订阅句柄，无领域副作用 | 不适用 |
| `event.poll` | Query | 只读长轮询 | 不适用 |
| `stop.activate` | Command | 激活 Kernel Stop Fence，并执行 Emergency Stop 的 Kernel 副作用集 | 当前全局 generation |
| `stop.status` | Query | 只读 | 不适用 |

完整请求/响应 payload、排序、cursor 与方法专属错误见权威规范。`task.create` 已由 `kernel-sqlite` repository 实现规范化、receipt/idempotency hash 与 Task/Scope/Origin/Audit/Event 单事务物化；`kernel-kcp` 已实现 `system.ping` / `task.create` / `task.get` typed handler 与 SQLite adapter，详见 [`kernel-kcp.md`](kernel-kcp.md) 和 [Task repository 创建与读取契约](task-repository-contract.md)。首批 KCP 没有清除 Stop Fence 的方法；未来解除流程必须有独立恢复契约。

## Value preflight 与 registration 合同

- 输入只接受调用方已经解析的 `serde_json::Value`，不接 bytes/UTF-8/JSON parse/frame。
- 固定优先级为 request_id 可关联性、message family、protocol、auth、family method、根 payload schema version、完整 Schema/generated decode。
- request ID 不可关联时本地拒绝且不发响应；可关联的五类 preflight error 使用固定安全 message、`details=null`、`retryable=false`，并经过不可替换 Response Schema 门。
- 八方法合法请求都必须先成为 generated typed Accepted；三方法 narrow 为 `RegisteredRequest`，其余五个得到本地不可序列化 `KnownCatalogMethodNotImplemented`，不是 wire error。
- 公开调用分成 `preflight_value -> narrow_to_registered -> TypedDispatcher.dispatch`，已在 `kernel-kcp` 实现；详细 API 见 [`kcp-preflight-dispatcher.md`](kcp-preflight-dispatcher.md)。

## 三方法 typed handler 边界

- 输入已通过对应 Envelope Schema、方法 payload Schema 与 typed decode；正常 dispatcher 路径还会先 narrow 为 `RegisteredRequest`。现有公共 `handle_*` 对错误 family/variant 返回本地 InputMethodMismatch；`serde_json::Value` preflight、bytes/frame、protocol/auth/method/schema 分类不在 handler 内，`invalid_request` 不由 typed handler 产生。
- 输出 payload 先按原方法 response Schema 校验，再将最终成功/错误 Response Envelope 按通用 Schema 校验。
- 响应固定 `protocol_version=1.0`、`message_kind=response`、request ID 原样；success/error 互斥。Response 无 method discriminator，调用方依原请求方法校验成功 payload。
- `KernelClock`、`KernelIdGenerator` 与闭集 `BackendError` 的 Task backend 可注入；backend 只暴露 create/get 高阶操作，不暴露 SQLite transaction 或 SQL。SQLite adapter 必须逐项把 `StoreErrorCode` 转成公开 backend 分类或 Internal，禁止消息匹配。
- deadline 将 Envelope RFC 3339 文本解析为 UTC instant 后比较，禁止字符串比较；解析失败在 ID/backend 前返回 `internal_error`。
- `task.create` 的第一次时钟读取同时是入口 deadline 检查和唯一 `accepted_at`。六个对象 ID 是合法、两两不同的唯一 UUID，版本不固定；correlation/dedup 是独立生成的非空 opaque 值，不从 caller 字段派生。
- SQLite 创建事务不可中途取消。commit 后到期仍返回 `deadline_exceeded`，但事实保留；客户端用同一 idempotency key 重放或用已知 Task ID 查询。
- Created/Replayed 都返回当前 Task；Created 的 **backend 结果**还必须返回与本次 operation 中 Event UUID 相等的 `committed_event_id`（它不是 wire `TaskCreateResponse` 字段），仅据此产生一个 post-commit Publisher wake-up intent。它不表示 Event 已 delivered；后续 deadline/internal/response contract failure 仍保留 intent，通知失败不回滚 durable Outbox。

## 实现阶段门

当前 Value preflight/registration/dispatcher 与三方法 typed handler 阶段门均已完成，但五个 Catalog 方法仍缺正式 handler。即使已有 Value 边界，在八方法 registration 完整、bytes/frame/transport/server 生命周期关闭前不得启动 server，也不新增 `method_unavailable`。


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

- KCP Envelope 与八方法 request/response JSON Schema：`schemas/source/kcp/`；
- 生成的 Rust 类型、manifest catalog，Command/Query/Event typed envelope decode，以及 `decode_after_validation` 的结构化 post-Schema 错误：`kernel-contracts`（见 [schema-generation.md](schema-generation.md)）；
- Response Envelope 只按 `status = ok | error` 校验。它不携带原始方法 discriminator，因此不生成方法级 typed envelope；handler/客户端必须根据原请求方法用对应 response Schema 校验成功 `payload`，再校验通用 Response Envelope；
- 这表示不可连接 Value preflight、三方法 dispatcher/handler 已可供未来组合根调用；不表示五个缺失 handler或 KCP server 已可用。
