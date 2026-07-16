# kernel-kcp typed application handler

> 相关下一层合同：[`kcp-preflight-dispatcher.md`](kcp-preflight-dispatcher.md)。该 Value preflight/registration/dispatcher 尚未有 Rust 实现。

`kernel-kcp` 当前实现是一个**不可连接**的 Rust 库级 application handler crate。它只接收 `kernel-contracts` 已完成 Schema preflight 与 typed decode 的 envelope；未来 §5.11 dispatcher 会在调用前再完成三方法 registration narrow。当前公共 `handle_*` 仍以本地 `InputMethodMismatch` 防御错误调用，并实现：

- `system.ping`；
- `task.create`；
- `task.get`。

它当前不接受 `serde_json::Value` 或 transport frame，也不提供 preflight、registration narrow、dispatcher、Socket、Named Pipe、server、`agentd`、Event/Stop 或 `task.list` handler。§5.11 已定义未来不可连接 dispatcher 必须如何调用本 crate 的三个公共 handler。

## 边界

handler 通过高阶端口依赖：

- `KernelClock`：返回已解析的 `DateTime<Utc>`；
- `KernelIdGenerator`：按 purpose 分配六个 UUID 文本和两个 opaque ID；
- `TaskApplicationBackend`：只暴露 create/get 与闭集 `BackendError`；
- Response Schema 门是 handler 内置生产事实：公共 `handle_*` API 不接收 validator，也不允许调用方替换或绕过；故障注入 seam 仅存在于 crate 私有测试代码，不是 public API、feature 或 SDK port。

`sqlite_adapter::SqliteTaskBackend` 是单独的组合边界。handler 核心模块不依赖 SQL、transaction、normalize/hash 或 repository producer 细节；adapter 将完整 `TaskCreateOperation` 无损转换为 `kernel_sqlite::TaskCreateCommand`，并只通过 `SqliteStore::with_write_transaction` 调用 `create_task`。repository 只有在 Event append/verify 完成后才返回 `Created`，外层 transaction commit 后 adapter 才返回 operation Event UUID；真实 SQLite 测试通过公开 Store API绑定 intent、Outbox、Audit、Task、Scope 与 Origin。

## Deadline 与提交语义

三个 handler 的第一个可观察操作都是 clock。deadline 解析为 UTC instant 后以 `now >= deadline` 判断。`task.create` 的第一次读时钟同时是唯一 `accepted_at`；backend 返回后才进行第二次完成检查，不在 SQLite 事务内轮询或取消。

只有 `Created` 返回一个 `TaskCreatedCommitted { task_id, event_id }` notification intent。该 intent 在 post-commit deadline、clock failure、response Schema failure或最终本地 `ContractFailure` 时仍保留；`Replayed` 不产生 intent。

## Response

方法成功 payload 先用对应 response Schema 校验，再装入生成的 `KcpResponseEnvelope` 并通过通用 response Schema 校验。成功 payload 失败会折叠成固定 `internal_error`；最终 error envelope 也失败时返回本地 `HandlerContractFailure`，不发送未验证响应。

固定 error code/message/details/retryable 来自 `IMPLEMENTATION_CONTRACTS.md` §5.10.5；handler 不拼接 storage message、SQL、路径或 payload。
