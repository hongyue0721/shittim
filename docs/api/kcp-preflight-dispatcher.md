# KCP Value preflight 与注册式 dispatcher

> 状态：当前Rust已实现legacy v1不可连接边界；active合同已升级为method-aware payload version与TaskCreate v2（`V2InitialBuildActive`），因此本实现不能作为未来server的active preflight，必须先升级并删除v1 create写路径。

## 范围

本边界只接收调用方已经解析得到的 `serde_json::Value`。它不负责 bytes、UTF-8、JSON parse、4-byte length prefix frame、最大 frame、transport、连接身份、clock、backend 或 Kernel ID。实现位于现有 `kernel-kcp`，复用 handlers、ports、generated Catalog 与 response contract 门，没有建立平行 crate abstraction。

公开调用链固定分三步：

```rust
preflight_value(value)
-> narrow_to_registered(request)
-> TypedDispatcher::new(clock, ids, backend).dispatch(request)
```

没有 `process_value` 或语义等价的一站式全 Catalog API。首批八方法都有正式 Schema/Catalog，但目前只有三个方法拥有 typed handler。

## Public API

- `preflight_value(Value) -> PreflightResult`
- `TypedCatalogRequest::family()` / `method()`：只读查看已经确认的 family 与 generated discriminator；内部 envelope variant 私有，只能由 preflight 构造。
- `narrow_to_registered(TypedCatalogRequest) -> RegistrationResult`
- `RegisteredRequest::method()`：只读查看 registered method；内部 variant 私有，只能由 narrow 构造。
- `TypedDispatcher<'a, C, G, B>::new(&C, &G, &B)` / `dispatch(RegisteredRequest)`：借用现有 `KernelClock`、`KernelIdGenerator`、`TaskApplicationBackend`。

`PreflightLocalRejection`、`KnownCatalogMethodNotImplemented`、`TypedCatalogRequest` 与 `RegisteredRequest` 均不实现 `Serialize`；测试使用负 trait assertion 锚定。生产 API 不接收 validator/catalog/bypass flag，response fault seam 只存在于 crate 私有单元测试。

## Preflight 结果与优先级

`preflight_value` 只产生：

- `Accepted(TypedCatalogRequest)`：完整 generated Envelope/方法 Schema 与 `decode_after_validation` 已通过；
- `Response(KcpResponseEnvelope)`：request ID 可关联的固定 preflight wire error，且最终 response 已通过 generated Response Schema；
- `LocalRejection(PreflightLocalRejection)`：不能关联 request，或 catalog/schema/generated/response 出现内部合同失败；不得发送 wire response。

固定短路优先级：

1. request ID response eligibility；
2. message kind / family；
3. protocol；
4. auth；
5. family-specific method；
6. 根 `payload.schema_version`；
7. 完整 Envelope/方法 Schema + generated typed decode。

顶层 `request_id` 必须是 UUID parser 接受的 string；wire error 中逐字保留原字符串，不重新格式化。非 object、缺失/非 string/非法 UUID 都得到 `UncorrelatableRequest`。

当前Rust实现把根payload version全局固定为1；这只描述legacy代码缺口，不是规范规则。active实现必须读取generated `MethodVersionBinding`：`task.create active=[2], legacy=[1]`（v1仅判定`unsupported_schema_version`），其余首批方法active=[1]；按family+method+version选择request及response Schema。v1不得进入dispatcher。

preflight的`unsupported_auth_schema`只在实际执行auth判定时适用。它不能被root official fixture harness借用：root harness执行raw Envelope Schema → payload Schema → normalization/projection/hash，auth非null固定为raw schema rejection并投影为`invalid_request`、details为null，两个hash不计算。

## 结构化 contract error

`kernel-contracts` 公开：

- `ContractFailureStage`：`CallerSchemaValidation`、`WireDecodeAfterSchema`、`PayloadDecodeAfterSchema`、`GeneratedDiscriminatorMapping`、`SchemaCatalog`；
- `ContractFailureClassification`：`CallerInvalid` / `InternalContractFailure`；
- `ContractError::stage()` 与 `classification_for_preflight()`。

schema-tool 生成的 typed decoder 现在先由 `decode` 验证 Schema，再调用公开且有明确前置条件的 `decode_after_validation`。generated raw wire、payload 与 discriminator default 分别返回独立结构化变体。preflight 先单独 `validate_json`；只有 `SchemaValidation` 是 caller invalid，后续任何 decode/catalog 失败都本地 fail closed，不匹配错误文本。

## 固定错误

这里的错误表只描述Value preflight实际执行的判定层；official root fixture harness不调用preflight，不能复用本表的preflight-only code。root fixture的raw Schema拒绝由其自身合同固定投影为`invalid_request`，而不是`unsupported_auth_schema`。

| code | 固定 message | details | retryable |
|---|---|---|---:|
| `invalid_request` | `request is invalid` | null | false |
| `unsupported_protocol_version` | `protocol version is not supported` | null | false |
| `unsupported_schema_version` | `payload schema version is not supported` | null | false |
| `unsupported_method` | `method is not supported` | null | false |
| `unsupported_auth_schema` | `authentication schema is not supported` | null | false |

response 构造复用 `kernel-kcp` crate-private 通用 validated error finalizer。最终 error response 若不能通过 generated Response Schema，则返回固定本地 `ContractFailure`，不发送未验证 response。

## Registration 与 dispatcher

全部八方法合法请求都先得到 typed `Accepted`。narrow 结果：

- registered：`system.ping`、`task.create`、`task.get`；
- known-unimplemented：`task.list`、`event.subscribe`、`event.poll`、`stop.activate`、`stop.status`。

当前narrow把v1 `task.create`列为registered；active升级后必须改为v2 typed variant，v1不得Accepted/registered且handler写路径删除。其余registered/known集合不变，直到各方法另行升级。

`TypedDispatcher` 只调用现有公共 `handle_system_ping`、`handle_task_create`、`handle_task_get`，不重复 Schema/protocol/auth/method/payload version/deadline 检查，不改写 `HandlerResult` 或 post-commit intents，也不创建平行端口。

## 阶段门

五个 Catalog 方法仍缺正式 handler，bytes/frame/transport/server 生命周期也未关闭。因此当前仍禁止启动 Socket/Named Pipe server；known-unimplemented 不能作为“先启动 server、收到请求后再报错”的兜底。
