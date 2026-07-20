# Shittim 实现矩阵

> 本矩阵只汇总状态，不取代 `specs/` 中的唯一事实源。

| 领域 | 规范状态 | Schema | 实现 | 自动化测试 | 备注 |
|---|---|---|---|---|---|
| Task/Action状态机 | active合同：root v2/child Action与Approval v2；`V2InitialBuildActive`（ADR-0009） | 首批TaskCreate/Child proposal/allocation、ActionRequestV2、ActionTransitionIntentV1、Approval核心五Schema及身份/证据八Schema/generated root types已落地 | root TaskCreate v2 repository（migration 0004）已落地；`domain-task`是v1字段接口；无child materializer；v1 create write待删除；Action repository未实现 | Schema/typed conformance + root v2 原子写入/幂等/回滚/corruption tests | 不从v1迁数据；旧库reinitialize-required |
| Child Task authority | ADR-0006 accepted（§7 migration/provenance被ADR-0009 supersede）；fixture/task-creation owner合同已闭合 | ChildTaskProposal/NormalizedChild/Allocation与ChildTaskDeltaProjection Schema/generated types已落地 | `kernel-task-creation`负责proposal/allocation；`kernel-authorization`负责delta/material/observation/subject projection；active materializer未实现 | Schema conformance、pure crate hash/normalization/negative tests、official task fixtures及child-delta/subject official projection fixtures覆盖 | 无legacy direct-child provenance/migration；身份/证据Schema已落地，repository未开始 |
| Approval/身份/失效 | ADR-0007 accepted（v1 migration生产要求被ADR-0009 supersede） | ApprovalRecordV2、PermissionDecisionV2、PolicyRuleV2、SubjectProjectionV1、ApprovalEventAllocationV1与身份/证据八Schema已落地；v1仅历史验证资产 | SubjectProjection pure API已实现；身份/证据typed Schema已落地；无repository/CAS/producer | 双层tagged union、字段/闭集/v1泄漏、三subject fixture/JCS/hash/tamper、身份/挑战/证据/远程签名fixture与array-const顺序已覆盖 | immutable union/current-head CAS/fingerprint/remote challenge；无v1数据迁移 |
| Recovery/Verification | v1合同已消歧；v2 producer引用规则已定义 | Candidate/Attempt/Verification Schema与生成类型均为v1 | v1验证摘要与 retry_original 合法性 | v1 completed/failed/unknown/retry 测试 | 其它恢复候选只做枚举层接受，不代表授权或执行 |
| Policy matcher | v1 matcher已消歧；v2 material/observation/subject projection合同已定义 | MaterialAuthorizationProjectionV1、ObservationEvidenceProjectionV1、SubjectProjectionV1、PolicyRuleV2、PermissionDecisionV2、ApprovalRecordV2与身份/证据八Schema已落地 | `domain-policy` v1纯领域matcher；`kernel-authorization`构造/验证/hash四projection，不替代matcher；v2 PD/Approval repository无实现 | URI/set/multiset/joint-null/pseudo-provider/JCS/hash及Subject三branch official fixtures覆盖 | 身份/证据Schema已落地，repository未开始 |
| ContentOrigin/Actor/EntryPoint | v1已实现；active ContentOrigin v2 carrier合同已定义 | InputContentOriginV1、InputTaskScopeV1与stored ContentOriginV2已落地 | root v2 写路径持久化 ContentOriginV2；v1 ContentOrigin repository write 待删除 | Input边界 + stored v2 exact wire/typed/JCS + root v2 origin readback | Actor/EntryPoint仍复用retained v1；v2 child carrier=action由未来producer证明 |
| PermissionDecision | v1代码事实 + active v2完整projection/lease合同 | PermissionDecisionV2 Schema/生成类型已落地 | `domain-policy`只生成v1非持久draft/canonical input | v1 decision/default allow/key params tests | v2 ID/revisions/material/observation/Approval binding必须由未来repository实现 |
| AuditRecord | v1 legacy + active v2义务已定义 | AuditRecordV2与Schema-validated AuditAllocationV2已落地；v1 retained不动 | root task.create v2 写入 AuditRecordV2；v1 canonical Store与legacy producer待删除 | exact v2 wire + root v2 creation_recorded 投影一致性测试 | 其它 producer 的跨对象一致性仍未实现 |
| Event/SQLite Outbox | ADR-0008 accepted（§7/§8 legacy production API被ADR-0009 supersede）；v2八Schema与统一Outbox shape已闭合 | Event v2八Schema + retained v1 已进manifest/generated；catalog/typed decode已落地 | migration 0003/0004、descriptor v1、单表mixed read/write、严格stored decoder、delivery gate与savepoint poison已实现；root task.create v2 是首个 active `task.created` producer；`append_legacy_event_v1`/`LegacyV1`待删除；Publisher未实现 | descriptor/ledger/升级路径、mixed事件、root v2 causation/correlation/readback、stored corruption与poison tests已覆盖 | retained poll v1不能返回v2；Publisher/poll不在V2InitialBuildActive |
| KCP Envelope / Value preflight | active合同使用Command/Query Envelope V2结构门 + MethodVersionBinding业务门；IC §13.5/§13.7为V2InitialBuildActive口径 | 首批12项source/entries/generated types已落地；production bindings仍空；retained v1 envelopes bytes未改 | schema-tool library + production V2 authority已生效；kernel-kcp仍走retained v1 preflight/dispatcher | business-v2 schema conformance与synthetic authority/binding负例已覆盖；runtime初始交付测试未开始 | active task.create只接受2；升级前禁止server |
| KCP首批八方法 | Catalog保留；task.create active语义升v2 root-only | v1 8组Schema保留；TaskCreate Request/Response v2与Envelope V2已生成；production bindings空 | v1 create/get与三方法dispatcher仍是待删除/待替换legacy内部代码 | v1 tests + business-v2 schema tests | 五方法缺handler；v1 create必须删除；bindings为初始交付而非cutover |
| KCP typed application handler / dispatcher | §5.10/§5.11 三步 API、注册集合与无损路由已实现 | 复用现有 Envelope/ping/create/get response/error Schema | private-state `TypedCatalogRequest`/`RegisteredRequest`、borrowing `TypedDispatcher`、handlers/ports/SQLite adapter、生产 `SystemKernelClock`/`RandomKernelIdGenerator` | handler行为、闭集错误映射、runtime clock epoch/范围/ID error channel、unknown-schema/final-response fault seam、dispatcher clock/response/ContractFailure/Created intent均有回归覆盖 | 只能库级不可连接；五方法正式 handler 前禁止 server |
| 首批active事件 | 五类合同与ADR-0008/0009权威闭合 | 八Schema + retained三payload + Envelope v1/v2 已落地 | mixed Outbox active append API已实现但没有业务owner producer/Publisher | mixed五类active + legacy、aggregate mismatch及corrupt relation覆盖 | storage API不等于producer；里程碑要求有owner的producer，不伪造 |
| KCP 本地传输 | ADR accepted；受 typed-only 阶段门约束 | 不适用 | 未开始 | 未开始 | Unix Socket / Windows Named Pipe；本批不拍 path/frame 新事实、不允许可连接 server |
| Schema生成链 | ADR accepted；manifest v2/walker/transaction、KCP/Event authority与bindings生成已实现 | production=83 entries；41 retained + 42 component-native；production bindings `[]` | schema-tool扩展restricted profile接受`maxItems`/`maxLength`与string-array const；切片1a–1c-ii进入同一生成链 | 生成链/fixtures/Event claimant + 切片1a/1b/1c-i/1c-ii identity/DAG/typed/JCS conformance覆盖 | V2InitialBuildActive完成后bindings等于§13.5目标表 |
| Rust workspace | ADR accepted；`kernel-task-creation`与`kernel-authorization`已加入workspace | 不适用 | kernel-contracts、schema-tool、domain-task、domain-policy、kernel-sqlite、kernel-kcp、kernel-task-creation、kernel-authorization | fmt/clippy/workspace test + pure crate conformance + official projection fixtures/harness/oracle | URI复用domain-policy唯一实现，全部事实caller typed注入；两crate均不读repo/不写存储 |
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
- [SDK 文档](sdk/extension-sdk.md)
- [ADR 索引](../adr/README.md)
