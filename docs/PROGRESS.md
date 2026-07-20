# Shittim 实现进度

> 状态日期：2026-07-20（ADR-0008 前两实现段已落地：Event v2 八 Schema/catalog/typed envelope，以及 migration 0003 descriptor v1、版本化统一 Outbox、mixed API、stored decoder 与 savepoint poison。production MethodVersionBindings 仍为空；active business producer、Publisher、KCP poll cutover 仍未实现。）

域状态表唯一来源：[`IMPLEMENTATION_MATRIX.md`](IMPLEMENTATION_MATRIX.md)。本文只保留当前切片事实、未完成 backlog 与下一步；逐切片编年史由 git log 与 ADR 承载。

## 当前里程碑

**已实现（代码/Schema 事实）**

- 规范与工程基线：Freedom-first / Kernel Owns Reality 合同、Apache-2.0、双仓同步 library/CLI、Node/pnpm 零依赖根基座（exact Node 24.18.0 / pnpm 11.3.0）、统一门 `scripts/check-schema.sh`。
- Schema/Rust 契约：Rust workspace、Draft 2020-12 + manifest v2（production=61 entries，41 retained + 20 component-native）、`schema-tool` 单 root transaction / target-scoped IR / TaggedUnion / string enum `ALL` / RFC 8785；production `METHOD_VERSION_BINDINGS=[]`。
- 纯领域：`domain-task`（Task/Action 状态图、revision/plan_version）、`domain-policy`（URI/glob/Default Allow/rate-limit draft，Stop Fence/Recovery 独立 Blocked）。
- 持久化：`kernel-sqlite` migration 0001–0003；Audit canonical Store；legacy Task create/get；版本化统一 Outbox + mixed v1/v2 append/read + 严格 stored decoder + savepoint poison。
- KCP 库级：`kernel-kcp` retained v1 Value preflight、三方法 registration/dispatcher/handler（`system.ping` / legacy `task.create` / `task.get`）与 SQLite adapter；不可连接，无 bytes/frame/server。
- ADR-0006 首批：12 business-v2 Schema + `kernel-task-creation` pure library + official fixtures/harness + schema-tool strict pointer CLI；**未**接 repository/handler。
- ADR-0008 前两段：Event v2 八 Schema、`EventTypeBinding`/active·legacy catalog、typed EventEnvelope v1/v2、migration 0003 descriptor v1 与 mixed Outbox API。

**未实现（不得宣称完成）**

- `ActionTransitionIntentV1` Schema/Action 持久对象/repository/migration（IC §6.14）；八 Schema 批次只交付 ref/wire。
- active Action 升级：`approval_chain_id`、execution generation、materialized child result、Approval v2 消费；现有 `approval_record_ref` 是 legacy。
- 其余 v2 对象：四投影/SubjectProjection/CreationProvenance、ContentOrigin/Audit v2、ApprovalRecord/PermissionDecision/PolicyRule v2、signature/credential/challenge/evidence 等。
- method-aware payload version preflight；active `task.create` 只接受 2；替换 registered v1 handler。
- root v2 repository/handler、child Action materializer（同 Action 最多一 child、bundle 全有或全无、canonical readback/reconciliation）。
- Action/PermissionDecision/Approval v2 repositories、current-head CAS、fingerprint 失效/复用、身份 challenge、plan 重评。
- legacy direct-child provenance migration（不伪造历史 Action/Approval/Verification）。
- active business **producer**、**Publisher**、versioned KCP **poll** response/binding/handler（retained poll v1 不得返回 v2）。
- 其余 Command 幂等/乐观锁；**五方法**正式 handler（完成前 `KnownCatalogMethodNotImplemented` 仅本地注册结果，server 阶段门关闭）。
- Unix Domain Socket / Windows Named Pipe KCP **server/client**；**agentd** 组合根与首批八方法处理。
- `ts/*` 包、SDK client、Pi `agent-runtime`；统一 **Extension SDK Base**（仍 `contract-only`，阻塞基础产品）；optional Computer Use Profile（`contract-only`，不阻塞 Core）；Tauri 桌面客户端；Provider/Memory/Initiative/Broker。
- `specs/CONFORMANCE.md` 全部 BASE 与声明的 Profile/Platform 条件套件。
- V2ProductionWriteCutover；production 非空 MethodVersionBindings。

`domain-task` / `domain-policy` 只计算意图与 draft；`kernel-sqlite` 不伪造尚无权威表的跨对象一致性。Extension SDK Base 完成不要求任何 Profile 的 `provider contract` / `real-platform`；`desktop-client` 不等同 Computer Use。

## 未完成 backlog

- [ ] `ActionTransitionIntentV1` + Action 持久对象/transition repository/migration
- [ ] active Action 合同升级（Approval v2 消费字段）
- [ ] 其余 v2 Schema（投影/Origin/Audit/Approval/PD/Policy/auth evidence 等）
- [ ] method-aware preflight；active `task.create` v2 替换 v1 registration
- [ ] root v2 repository/handler + child Action materializer
- [ ] Action/PD/Approval v2 repositories + current-head CAS + challenge/plan 重评
- [ ] legacy direct-child provenance migration
- [ ] active Event business producers + Publisher + versioned KCP poll
- [ ] 五方法正式 handler；可连接 KCP server/client；`agentd`
- [ ] 其它 Command 幂等与乐观锁；Task list cursor ADR 拍板后实现
- [ ] Extension SDK Base → `schema/SDK`；TS 包 / SDK client / `agent-runtime`
- [ ] optional Computer Use Profile；桌面客户端；Provider/Memory/Initiative/Broker
- [ ] CONFORMANCE 全量 BASE + 声明 Profile 套件；V2ProductionWriteCutover

## 当前阻塞

- active contract migration：v1 `task.create` 须退出 production dispatcher；v2 Schema/handler/repository 与 method-aware preflight 完成前禁止启动 server。
- legacy v1 repository 的 Delegation 正向路径未实现（非 null 固定 not found）；active v2 必须实现真实 Delegation authority。
- Task list cursor 编码须先 ADR/API 拍板。
- Audit 仍缺跨对象 PermissionDecision/policy context 相等、rollback 权威投影、Provider/ModelCall 一致性与其它业务 producer。
- Extension SDK Base / Computer Use 仍为 `contract-only`；不得把规范或根 Node 工作区冒充 SDK 实现。
- 真实 Provider/Channel/Privilege Broker 需要后续真实环境；当前无伪造支持。
- 默认 PATH 可能不是 Node 24.18.0；须显式 `~/.local/share/pnpm`。

## 下一步

1. 独立实现 `ActionTransitionIntentV1` + Action 持久对象/transition repository/migration；再做 method-aware preflight、root v2 repository/handler、child Action 原子 materializer 与 V2ProductionWriteCutover。
2. 实现 Approval/PermissionDecision/Action repositories 及 current-head CAS；接入 active Event business producers。
3. 实现 Publisher 与 versioned KCP poll response/binding/handler；retained poll v1 仍不得返回 v2。
4. 完成 Task creation provenance migration 与 reconciliation，不伪造历史 Action/Approval/Verification。
5. 再实现剩余五个 Catalog handler 与可连接 server；禁止先接 v1 server。
6. 随后实现 Extension SDK Base 与 TypeScript/client。

## 最近验证

本切片（文档瘦身）验证命令：

```text
export PATH="$HOME/.local/share/pnpm:$PATH"
export TMPDIR=/mnt/data/shittim-build-tmp
node scripts/update-file-manifest.mjs --check
pnpm run test:docs-repository
git diff --check
```

历史实现切片的 full gate（`./scripts/check-schema.sh`、workspace tests）记录在对应提交的 git log；本页不重放编年史。

## 事实来源

- 域状态表：[`IMPLEMENTATION_MATRIX.md`](IMPLEMENTATION_MATRIX.md)
- 全局不变量：[`../AGENT.md`](../AGENT.md)
- 状态机与恢复：[`../specs/CORE_ARCHITECTURE.md`](../specs/CORE_ARCHITECTURE.md)
- 实现契约：[`../specs/IMPLEMENTATION_CONTRACTS.md`](../specs/IMPLEMENTATION_CONTRACTS.md)
- 验收：[`../specs/CONFORMANCE.md`](../specs/CONFORMANCE.md)
- API 导航：[`api/README.md`](api/README.md)
- ADR 索引：[`../adr/README.md`](../adr/README.md)
