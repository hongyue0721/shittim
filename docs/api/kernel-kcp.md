# kernel-kcp legacy v1不可连接库边界

`kernel-kcp`当前实现的是**legacy v1** Rust库级KCP边界。它包含v1 Value preflight/dispatcher和`task.create v1` handler；ADR-0006后的active合同要求TaskCreate v2 root-only与method-aware version，因此当前crate在升级前不得接入未来server。

- v1已解析`serde_json::Value` structured preflight；
- v1全八方法generated typed Accepted；
- 三方法registration narrow（其中task.create仍为legacy v1）；
- `TypedDispatcher`；
- `system.ping`、`task.create`、`task.get` typed handlers；
- `SqliteTaskBackend` 组合 adapter。

它不接受 bytes 或 transport frame，不提供 UTF-8/JSON parser、frame codec、Socket、Named Pipe、server 或 `agentd`。其余五个 Catalog 方法没有 handler，仍阻塞 server 启动。

## Active升级要求

- generated method-version catalog；
- `task.create v2` typed payload/response与handler；
- v1 create从active Accepted/registration移除，仅migration/test可用；
- child Task不增加KCP方法，由Action repository/materializer实现；
- CausationRef/EventEnvelope/ContentOrigin/Audit v2 producer接线。

其余response门、clock/ID/backend分层可复用，不得复制第二套KCP栈。

## 分步入口

```rust
let preflight = kernel_kcp::preflight_value(value);
// Accepted 后：
let registration = kernel_kcp::narrow_to_registered(request);
// Registered 后：
let dispatcher = kernel_kcp::TypedDispatcher::new(clock, ids, backend);
let result = dispatcher.dispatch(request);
```

禁止一站式全 Catalog 执行入口。`TypedCatalogRequest` 与 `RegisteredRequest` 内部 variant 私有，分别只能由 preflight/narrow 构造，因此正常 dispatcher 路径不能构造 family/discriminator/payload variant 错配。既有公共 `handle_*` 仍保留本地 `InputMethodMismatch`，用于防御绕过正常路径的直接错误调用。

详细 Value 分类、固定 wire error、结构化 contract failure 与 registration 集合见 [`kcp-preflight-dispatcher.md`](kcp-preflight-dispatcher.md)。

## 端口边界

handler/dispatcher 复用：

- `KernelClock`：返回已解析的 `DateTime<Utc>`；公开 `SystemKernelClock` 使用 OS 系统时钟，并以显式 epoch 前借位、整数溢出和 chrono 可表示范围检查把异常映射为 `ClockError`，不走隐式 panic 转换；
- `KernelIdGenerator`：legacy v1 `task.create`按purpose分配六个UUID文本和两个opaque ID；active root v2改由`RootTaskCreateAllocationV2`分配七个UUID和两个opaque ID。公开`RandomKernelIdGenerator`使用可失败的OS随机源，随机源失败映射为`IdGenerationError`而不是panic；当前UUID使用v4只是实现细节，不形成协议版本承诺，opaque ID是无caller派生信息的128-bit小写十六进制文本；
- `TaskApplicationBackend`：只暴露 create/get 与闭集 `BackendError`。

`TypedDispatcher` 借用这三个端口，并按 variant 只把所需能力传给对应 public handler。它不创建平行接口、不重复 deadline 或 Schema 检查，也不改写 `HandlerResult`。

Response Schema 门是内置生产事实：公共 API 不接收 validator，也不允许调用方替换或绕过。故障注入 seam 仅存在于 crate 私有测试代码，不是 public API、feature 或 SDK port。preflight wire error 与 handler error 共用 crate-private final response Schema 门。

`sqlite_adapter::SqliteTaskBackend` 将完整 `TaskCreateOperation` 无损转换为 `kernel_sqlite::TaskCreateCommand`，并只通过 `SqliteStore::with_write_transaction` 调用 repository。handler 核心不依赖 SQL、transaction、normalize/hash 或 producer 细节。

## Deadline 与提交语义

preflight 不读取 clock、不比较 deadline。三个 registered handler 的第一个可观察操作仍是 clock。deadline 解析为 UTC instant 后以 `now >= deadline` 判断。`task.create` 第一次读时钟同时是唯一 `accepted_at`；backend 返回后才进行第二次完成检查，不在 SQLite 事务内轮询或取消。

只有 `Created` 返回一个 `TaskCreatedCommitted { task_id, event_id }` notification intent。该 intent 在 post-commit deadline、clock failure、response Schema failure或最终本地 `ContractFailure` 时仍保留；`Replayed` 不产生 intent。dispatcher 原样透传这些结果。

## Response

方法成功 payload 先用对应 response Schema 校验，再装入 generated `KcpResponseEnvelope` 并通过通用 response Schema 校验。成功 payload 失败折叠成固定 `internal_error`；最终 error envelope 也失败时返回本地 `HandlerContractFailure`，不发送未验证响应。

固定 handler error code/message/details/retryable 来自 `IMPLEMENTATION_CONTRACTS.md` §5.10.5；固定 preflight wire error 来自 §5.11.4。实现不拼接 storage/schema/serde message、SQL、路径或 payload。
