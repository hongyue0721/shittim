# Shittim 实现矩阵

> 本矩阵只汇总状态，不取代 `specs/` 中的唯一事实源。

| 领域 | 规范状态 | Schema | 实现 | 自动化测试 | 备注 |
|---|---|---|---|---|---|
| Task/Action状态机 | active合同已升级：root v2/child Action与Approval v2；现有状态图仍有效 | 首批TaskCreate/Child proposal/allocation相关Schema与generated root types已落地；Action/Approval及其余v2仍未开始 | `domain-task`是v1字段接口；无child materializer | 现有测试只证明现有状态图与首批Schema shape，不证明v2持久shape | contract-only差距明确 |
| Child Task authority | ADR-0006 accepted；fixture/task-creation owner合同已闭合 | ChildTaskProposal/NormalizedChild/Allocation v1 Schema与generated types已落地；`expires_at`双门Schema已实现，Delta/Authorization/Observation等其余仍未开始 | `kernel-task-creation`已实现child proposal normalization/hash与allocation validation；direct-child v1 repository仍是legacy write path，active materializer未实现 | Schema conformance、inline tests、official child/allocation fixtures与production owner + 独立CLI Schema路径 + stored preimage自一致性已覆盖 | 本阶段crate只含proposal/receipt与allocation；authorization projections另切片 |
| Approval/身份/失效 | ADR-0007 accepted | Approval/PD/auth evidence v2未开始；v1 target exactly-one缺口仅legacy | 无repository | 未开始 | immutable union/current-head CAS/fingerprint/remote challenge contract-only |
| Recovery/Verification | v1合同已消歧；v2 producer引用规则已定义 | Candidate/Attempt/Verification Schema与生成类型均为v1 | v1验证摘要与 retry_original 合法性 | v1 completed/failed/unknown/retry 测试 | 其它恢复候选只做枚举层接受，不代表授权或执行 |
| Policy matcher | v1 matcher已消歧；v2 material/observation projection合同已定义 | PolicyRule/PermissionDecision/ApprovalRecord Schema与生成类型均为v1 | `domain-policy` v1纯领域实现；v2 PD/Approval repository无实现 | v1 URI/glob/Default Allow/rate-limit tests | v1对象不得冒充v2 authorization事实 |
| ContentOrigin/Actor/EntryPoint | v1已实现；active ContentOrigin v2 carrier合同已定义 | InputContentOriginV1与InputTaskScopeV1已落地；stored ContentOriginV2未开始 | v1类型、normalizer与SQLite canonical repository | v1 + Input* boundary tests | Actor/EntryPoint仍v1；owner未认证；stored v2 child carrier待实现 |
| PermissionDecision | v1代码事实 + active v2完整projection/lease合同 | PermissionDecision Schema/生成类型仅v1；v2未开始 | `domain-policy`只生成v1非持久draft/canonical input | v1 decision/default allow/key params tests | v2 ID/revisions/material/observation/Approval binding必须由未来repository实现 |
| AuditRecord | v1 legacy + active v2义务已定义 | v1存在；v2未开始 | v1 canonical Store与legacy task.create producer | v1 tests | child Action causation/provenance/fingerprint producer待实现 |
| Event/SQLite Outbox | ADR-0008 accepted；v2八Schema、`change_kind` Approval真值表、Causation oneOf whole-ref、reserved claimant、`EventTypeBinding`单源bindings→types、exact 0003 descriptor与统一Outbox/KCP poll边界已闭合 | 当前仍只有v1 Envelope/3 payload；下一八项contract-only，未进manifest | 当前v1 Outbox；migration 0003、fixed mixed API与active producer未实现 | 现有仅v1 tests；v2/migration测试是Conformance future gate | 0003=`versioned_event_outbox`/single exact asset；单表、单position/sequence；retained poll v1不能返回v2 |
| KCP Envelope / Value preflight | active合同使用Command/Query Envelope V2结构门 + MethodVersionBinding业务门；合同已闭合 | 首批12项source/entries/generated types已落地；production bindings仍空；retained v1 envelopes bytes未改 | schema-tool library + production V2 authority已生效；kernel-kcp仍走retained v1 preflight/dispatcher | business-v2 schema conformance与synthetic authority/binding负例已覆盖；runtime cutover测试未开始 | active task.create只接受2；升级前禁止server |
| KCP首批八方法 | Catalog保留；task.create active语义升v2 root-only | v1 8组Schema保留；TaskCreate Request/Response v2与Envelope V2已生成；production bindings空 | v1 create/get与三方法dispatcher仍是legacy内部代码 | v1 tests + business-v2 schema tests | 五方法缺handler；v1 create还必须退出active registration；bindings/cutover未完成 |
| KCP typed application handler / dispatcher | §5.10/§5.11 三步 API、注册集合与无损路由已实现 | 复用现有 Envelope/ping/create/get response/error Schema | private-state `TypedCatalogRequest`/`RegisteredRequest`、borrowing `TypedDispatcher`、handlers/ports/SQLite adapter、生产 `SystemKernelClock`/`RandomKernelIdGenerator` | handler行为、闭集错误映射、runtime clock epoch/范围/ID error channel、unknown-schema/final-response fault seam、dispatcher clock/response/ContractFailure/Created intent均有回归覆盖 | 只能库级不可连接；五方法正式 handler 前禁止 server |
| 首批active事件 | 五类合同与ADR-0008权威闭合：新增`action.state_changed`与`approval.state_changed`，Approval有显式`change_kind`，Envelope/payload版本正交 | 当前仍是3 retained payload + v1 Envelope；exact八Schema未实现；`ActionTransitionIntentV1`明确后续独立批次 | v1 Outbox；无active producer/Publisher | v2 claimant/payload/migration/producer tests未开始 | future generated固定`EventTypeBinding`+active/legacy bindings并单源投影TYPE slices；当前`EVENT_V1_TYPES`仍是legacy代码事实 |
| KCP 本地传输 | ADR accepted；受 typed-only 阶段门约束 | 不适用 | 未开始 | 未开始 | Unix Socket / Windows Named Pipe；本批不拍 path/frame 新事实、不允许可连接 server |
| Schema生成链 | ADR accepted；manifest v2/walker/transaction完成；component-native exact、五值compatibility、MethodVersionBinding完整validator、KCP V2 Envelope authority、production stage gate与task creation fixture合同已闭合；Event v2 claimant与binding Rust符号已精确固定 | production仍=53 entries（41 retained + 12 component-native）；Event v2实现后目标61；production bindings `[]` | schema-tool现有实现未识别active Event claimant；current catalog仍生成`EVENT_V1_TYPES`；pointer/CLI与Task helper已实现 | 现有生成链/fixture测试通过；Event v2 claimant/whole-schema mapping/bindings const projection/typed tests未开始 | docs-only未改manifest/generated；下一切片生成`EventTypeBinding`与两套bindings，删除模糊alias/平行数据 |
| Rust workspace | ADR accepted；`kernel-task-creation`已加入workspace | 不适用 | kernel-contracts、schema-tool、domain-task、domain-policy、kernel-sqlite、kernel-kcp、kernel-task-creation；新crate只做task proposal/receipt/idempotency/allocation纯逻辑 | fmt/clippy/workspace test + crate inline tests | URI复用domain-policy唯一实现，全部事实caller注入；authorization projections另切片；尚未repository composition |
| TypeScript workspace | ADR accepted | 尚无 TS 生成物 | 仅根零依赖基座（`package.json` / `pnpm-workspace.yaml` / lockfile / `check:toolchain` / `update-file-manifest` / `sync-docs-repository`）；无 `ts/*` 包 | `pnpm run check:toolchain`；`pnpm run test:file-manifest` / `check:file-manifest`；`pnpm run test:docs-repository` / `check:docs-repository` / `sync:docs-repository`；统一门 `PATH`+`./scripts/check-schema.sh`（先 Node 硬门） | Node exact 24.18.0、pnpm exact 11.3.0；入口 `~/.local/share/pnpm/node`；Corepack 不可用；无 deps/SDK/client；无跨平台 npm `check:all`；`FILE_MANIFEST` 只列 Git Markdown source set（路径严格 UTF-8 fail closed）；文档镜像同步工具已 library/CLI implemented，不进入统一门默认步骤 |
| Desktop client | 方向已定义 | 未开始 | 未开始 | 未开始 | 将使用 Tauri/React/AntD，蓝白配色 |
| Extension SDK Base | contract-only；统一 SDK 的唯一规范边界已定义 | 未开始 | 未开始：没有 library、composition、public API 或 SDK 包 | 未开始 | **基础产品 Core 阻塞项**；不得把规范、根 Node 工作区或未来 `desktop-client` 宣传为 SDK 实现 |
| Optional Computer Use Profile | contract-only；桌面专属事实源为 `COMPUTER_USE.md` | 未开始：没有专用 Schema | 未开始：没有 crate、SDK composition、Provider 或 real-platform 能力 | 未开始：没有 Provider/真机测试 | 不阻塞 Core 完成；`desktop-client` 不等同于 Computer Use；启用后仍投影 Task/Policy/Scope/Lease/Stop Fence/Audit |
| Provider/平台能力 | 仅接口边界 | 未开始 | 未开始 | 未开始 | 不伪造支持；Computer Use Provider 属于 optional Profile，不代表 Extension SDK Base 已完成 |

## 状态含义

- **ProfileClaim maturity**：`contract-only | schema/SDK | composition | provider contract | real-platform`；按精确 claim id 独立记录。
- **distribution assertion**：与 maturity 正交的布尔对外声明事实；不是成熟度，不能自动升级能力。
- **library**：可复用实现已存在；不自动表示已完成运行时组合或对外发布。
- **composition**：实现已接入其宿主并可按契约协作；不自动表示已公开或已在真实平台验证。
- **public / SDK**：稳定对外入口或可安装 SDK 包已交付；不自动表示所有 Profile 或平台均已支持。
- **real-platform**：目标真实平台/Provider 的集成验证已完成；模拟、Schema 或纯领域测试不能替代。
- **纯领域实现**：只计算规则和意图，不拥有持久化或外部副作用。
- **类型与运行时校验**：有 Schema/生成类型，不代表业务状态所有者已实现。
- **未开始**：没有对应实现或真实能力。

## 相关入口

- [进度](PROGRESS.md)
- [双仓库与持续文档维护](REPOSITORY_MAINTENANCE.md)
- [API 文档](api/README.md)
- [domain-task API](api/domain-task.md)
- [domain-policy API](api/domain-policy.md)
- [kernel-sqlite API](api/kernel-sqlite.md)
- [KCP Value preflight 与注册式 dispatcher](api/kcp-preflight-dispatcher.md)
- [kernel-kcp typed handler](api/kernel-kcp.md)
- [Task repository 创建契约](api/task-repository-contract.md)
- [AuditRecord版本合同](api/audit-record.md)
- [Event Catalog](api/event-catalog.md)
- [Error Catalog](api/error-catalog.md)
- [Approval v2合同](api/approval-contract.md)
- [Schema 生成](api/schema-generation.md)
- [SDK 文档](sdk/README.md)
- [ADR 索引](../adr/README.md)
