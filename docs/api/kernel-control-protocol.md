# Kernel Control Protocol

> 状态：仅规范，未实现。本文是导航摘要；请求/响应字段的唯一事实源是 [`IMPLEMENTATION_CONTRACTS.md` 第 5 节](../../specs/IMPLEMENTATION_CONTRACTS.md#5-kernel-control-protocol)。

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

完整请求/响应 payload、排序、cursor 与方法专属错误见权威规范。`task.create` 已拍板只规范化 origin URI 和 TaskScope URI patterns，使用完整规范化 payload 计算 receipt hash，并使用精确 Envelope 业务投影计算幂等 hash；物化 Task/Scope/Origin/Audit/Event 的初值与单事务关系见 [Task repository 创建契约](task-repository-contract.md)。这些契约尚无 repository 或 handler 实现。首批 KCP 没有清除 Stop Fence 的方法；未来解除流程必须有独立恢复契约。

## Cursor

Event cursor 只使用十进制字符串表示的全局 `outbox_position`。`sequence` 只用于聚合内顺序，不能作为全局 cursor。列表 cursor 是 Kernel 生成的不透明字符串；`task.list` 的具体 cursor 编码尚未选择，必须在 Task repository 实现前通过 ADR/API 拍板。

## 当前不可用项

- 没有 Socket/Pipe server；
- 没有可运行的 agentd 或方法处理实现；
- 没有 TypeScript client 包；
- 没有认证扩展；
- 没有 TCP/HTTP/JSON-RPC KCP endpoint。

## 已有契约产物

- KCP Envelope 与八方法 request/response JSON Schema：`schemas/source/kcp/`；
- 生成的 Rust 类型、manifest catalog，以及 Command/Query/Event 的 typed envelope decode 与运行时校验：`kernel-contracts`（见 [schema-generation.md](schema-generation.md)）；
- Response Envelope 只按 `status = ok | error` 校验。它不携带原始方法 discriminator，因此不生成方法级 typed envelope；客户端必须根据已配对的 `request_id` 和原请求方法，再用对应 `*_response.json` Schema 校验并解码 `payload`；
- 这不表示 KCP server 已可用。
