# Kernel Control Protocol

> 状态：Envelope/Schema 与三个方法的 typed application handler 合同已闭合，但 Rust handler 尚未实现，仍无可连接 server。字段与行为的唯一事实源是 [`IMPLEMENTATION_CONTRACTS.md` 第 5 节](../../specs/IMPLEMENTATION_CONTRACTS.md#5-kernel-control-protocol)，其中三个方法的实现边界见 [§5.10](../../specs/IMPLEMENTATION_CONTRACTS.md#510-systemping--taskcreate--taskget-typed-application-handler)。

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

完整请求/响应 payload、排序、cursor 与方法专属错误见权威规范。`task.create` 已由 `kernel-sqlite` repository 实现规范化、receipt/idempotency hash 与 Task/Scope/Origin/Audit/Event 单事务物化；`system.ping` / `task.create` / `task.get` 的下一步 typed handler 合同已经闭合，但代码与 server 均未实现，详见 [Task repository 创建与读取契约](task-repository-contract.md)。首批 KCP 没有清除 Stop Fence 的方法；未来解除流程必须有独立恢复契约。

## 三方法 typed handler 边界

- 输入已通过对应 Envelope Schema、方法 payload Schema 与 typed decode；raw JSON、frame、protocol/auth/method/schema preflight 不在此层，`invalid_request` 不由 typed handler 产生。
- 输出 payload 先按原方法 response Schema 校验，再将最终成功/错误 Response Envelope 按通用 Schema 校验。
- 响应固定 `protocol_version=1.0`、`message_kind=response`、request ID 原样；success/error 互斥。Response 无 method discriminator，调用方依原请求方法校验成功 payload。
- `KernelClock`、`KernelIdGenerator` 与闭集 `BackendError` 的 Task backend 可注入；backend 只暴露 create/get 高阶操作，不暴露 SQLite transaction 或 SQL。SQLite adapter 必须逐项把 `StoreErrorCode` 转成公开 backend 分类或 Internal，禁止消息匹配。
- deadline 将 Envelope RFC 3339 文本解析为 UTC instant 后比较，禁止字符串比较；解析失败在 ID/backend 前返回 `internal_error`。
- `task.create` 的第一次时钟读取同时是入口 deadline 检查和唯一 `accepted_at`。六个对象 ID 是合法、两两不同的唯一 UUID，版本不固定；correlation/dedup 是独立生成的非空 opaque 值，不从 caller 字段派生。
- SQLite 创建事务不可中途取消。commit 后到期仍返回 `deadline_exceeded`，但事实保留；客户端用同一 idempotency key 重放或用已知 Task ID 查询。
- Created/Replayed 都返回当前 Task；Created 的 **backend 结果**还必须返回与本次 operation 中 Event UUID 相等的 `committed_event_id`（它不是 wire `TaskCreateResponse` 字段），仅据此产生一个 post-commit Publisher wake-up intent。它不表示 Event 已 delivered；后续 deadline/internal/response contract failure 仍保留 intent，通知失败不回滚 durable Outbox。

## 实现阶段门

下一小功能只能是不可连接的库级 typed handler 与 fake-port conformance 测试。raw preflight 与全 Catalog 可用性合同关闭前，不得启动 server，也不新增 `method_unavailable`。


## Cursor

Event cursor 只使用十进制字符串表示的全局 `outbox_position`。`sequence` 只用于聚合内顺序，不能作为全局 cursor。列表 cursor 是 Kernel 生成的不透明字符串；`task.list` 的具体 cursor 编码尚未选择，必须在 Task repository 实现前通过 ADR/API 拍板。

## 当前不可用项

- 没有 typed application handler Rust 实现；
- 没有 raw JSON/frame/preflight 与全 Catalog dispatcher；
- 没有 Socket/Pipe server；
- 没有可运行的 agentd 或方法处理实现；
- 没有 TypeScript client 包；
- 没有认证扩展；
- 没有 TCP/HTTP/JSON-RPC KCP endpoint。

## 已有契约产物

- KCP Envelope 与八方法 request/response JSON Schema：`schemas/source/kcp/`；
- 生成的 Rust 类型、manifest catalog，以及 Command/Query/Event 的 typed envelope decode 与运行时校验：`kernel-contracts`（见 [schema-generation.md](schema-generation.md)）；
- Response Envelope 只按 `status = ok | error` 校验。它不携带原始方法 discriminator，因此不生成方法级 typed envelope；handler/客户端必须根据原请求方法用对应 response Schema 校验成功 `payload`，再校验通用 Response Envelope；
- 这不表示 KCP handler 或 server 已可用。
