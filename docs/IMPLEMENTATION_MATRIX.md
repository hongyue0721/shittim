# Shittim 实现矩阵

> 本矩阵只汇总状态，不取代 `specs/` 中的唯一事实源。

| 领域 | 规范状态 | Schema | 实现 | 自动化测试 | 备注 |
|---|---|---|---|---|---|
| Task/Action 状态机 | 已消歧 | 已有首批类型 Schema | `domain-task` 纯领域实现 | NxN、证据、proptest | plan_version=0；success 字符串多重集合；parent_action_id 是补偿唯一事实 |
| Recovery/Verification | 已消歧 | Candidate/Attempt/Verification Schema | 验证摘要与 retry_original 合法性 | completed/failed/unknown/retry 测试 | 其它恢复候选只做枚举层接受，不代表授权或执行 |
| Policy matcher | 已消歧；ContentOrigin 多值同一-origin 语义已补充 | PolicyRule/PermissionDecision/ApprovalRecord Schema | `domain-policy` 纯领域实现；公开单项 `normalize_uri` / `normalize_uri_pattern`；`kernel-sqlite` 提供 transaction-bound 生产 RateLimitPort | URI/glob、公开 normalizer Task fixture、排序、Condition v1；SQLite 原子消费、多连接争最后 slot、winner-only integration | Default Allow；Stop/Recovery 独立 Blocked；无 PermissionDecision repository |
| ContentOrigin/Actor/EntryPoint | 已定义；task.create receipt payload 边界与 accepted_at 已消歧 | Schema + 生成类型；task.create executable hash fixture | 类型与运行时校验；`kernel-sqlite` ContentOrigin canonical repository 与严格 get | enum/未知字段/auth + fixture hash/projection + relation tamper | owner 是未认证预留标签；receipt hash 只含规范化完整 payload |
| PermissionDecision | 已定义 | Schema + 生成类型 | `domain-policy` 生成非持久 draft/binding/canonical input | decision mapping、default allow、RFC 8785 key params | ID/evaluated_at/decision revision/policy set revision/最终 context hash 由未来 agentd 持久层拥有 |
| AuditRecord | 已定义：任务创建固定 producer、外发状态/manifest refs、PD/policy context、权威 rollback projection、provider/model 分工及同事务失败回滚 | AuditRecord v1 + 2 个 wrapper 示例 + 生成 Rust 类型 | `kernel-sqlite` 不可变 canonical JSON Store；task.create 固定 producer 已实现 | canonical/immutable/contract invalid/rollback/读取重验；task.create 固定字段同事务测试 | PD 字段相等、rollback/provider 仍待对应 repository |
| Event/SQLite Outbox | 已消歧；sequence 首条已提交为 0、后续连续 +1、单次 append 失败自回滚且不占号 | EventEnvelope + 3 payload；task.created 示例 sequence=0 | `kernel-sqlite` 文件 migration、内部 savepoint 原子 sequence/position、cursor、at-least-once delivery storage；task.create 同事务生产唯一 task.created | 多聚合/多连接竞争/忽略 append 错误仍不留空洞/payload mismatch/cursor/重启前后 at-least-once；task.create 完整 Envelope/payload 断言 | 无 Publisher 循环、retention 或 claim lease；Task list/update 未实现 |
| KCP Envelope / Value preflight | Envelope 与 §5.11 Value 输入、固定优先级、response eligibility、五类 error、caller/internal classification 已闭合 | command/query/response/error；无 Schema 变更 | `kernel-contracts` 结构化 stage；`kernel-kcp::preflight_value` 全八方法 typed Accepted | priority/field/version/cross-family/八方法/fault seam/static assertions | generated family Catalog；最终 error response 门不可替换 |
| KCP 首批八方法 | Catalog 已定义；八方法合法请求均 typed Accepted，只有三方法可注册执行 | 8 组 request/response Schema + task.create hash fixture | `kernel-sqlite` task.create/get；`kernel-kcp` preflight/narrow/dispatcher/三方法 handler | 八方法 Accepted、五 known malformed/valid、三 registered | 五方法仍是本地不可序列化 KnownCatalogMethodNotImplemented；不是 wire error |
| KCP typed application handler / dispatcher | §5.10/§5.11 三步 API、注册集合与无损路由已实现 | 复用现有 Envelope/ping/create/get response/error Schema | private-state `TypedCatalogRequest`/`RegisteredRequest`、borrowing `TypedDispatcher`、handlers/ports/SQLite adapter、生产 `SystemKernelClock`/`RandomKernelIdGenerator` | handler、runtime clock epoch/范围/ID error channel、unknown-schema/final-response fault seam、dispatcher clock/response/ContractFailure/Created intent；`kernel-kcp` 46 tests | 只能库级不可连接；五方法正式 handler 前禁止 server |
| 首批三个事件 | 已定义 | 3 个 payload Schema | 尚无发布器 | 类型与 payload 错配测试 | 点号小写 |
| KCP 本地传输 | ADR accepted；受 typed-only 阶段门约束 | 不适用 | 未开始 | 未开始 | Unix Socket / Windows Named Pipe；本批不拍 path/frame 新事实、不允许可连接 server |
| Schema 生成链 | ADR accepted | 41 个 source + manifest；task.create executable fixture 不新增 Schema | schema-tool + kernel-contracts；optional/non-null 生成 serde omission | meta/$ref/drift/JCS + task.create receipt/idempotency hash + optional omission/required-null contract tests | 当前只生成 Rust；Approval target exactly-one 仍未由 Schema 强制 |
| Rust workspace | ADR accepted | 不适用 | kernel-contracts、schema-tool、domain-task、domain-policy、kernel-sqlite、kernel-kcp | fmt/clippy/workspace test | rustc/cargo 1.97.0；SQLite bundled |
| TypeScript workspace | ADR accepted | 尚无 TS 生成物 | 未开始 | 未开始 | Node 24.18.0 已可用 |
| Desktop client | 方向已定义 | 未开始 | 未开始 | 未开始 | 将使用 Tauri/React/AntD，蓝白配色 |
| Extension SDK | 规范已有 | 未开始 | 未开始 | 未开始 | 当前无可安装 SDK 包 |
| Provider/平台能力 | 仅接口边界 | 未开始 | 未开始 | 未开始 | 不伪造支持 |

## 状态含义

- **纯领域实现**：只计算规则和意图，不拥有持久化或外部副作用。
- **类型与运行时校验**：有 Schema/生成类型，不代表业务状态所有者已实现。
- **未开始**：没有对应实现或真实能力。

## 相关入口

- [进度](PROGRESS.md)
- [API 文档](api/README.md)
- [domain-task API](api/domain-task.md)
- [domain-policy API](api/domain-policy.md)
- [kernel-sqlite API](api/kernel-sqlite.md)
- [KCP Value preflight 与注册式 dispatcher](api/kcp-preflight-dispatcher.md)
- [kernel-kcp typed handler](api/kernel-kcp.md)
- [Task repository 创建契约](api/task-repository-contract.md)
- [AuditRecord v1](api/audit-record.md)
- [Schema 生成](api/schema-generation.md)
- [SDK 文档](sdk/README.md)
- [ADR 索引](../adr/README.md)
