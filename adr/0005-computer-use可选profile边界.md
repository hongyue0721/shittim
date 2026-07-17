# ADR-0005：Computer Use 可选 Profile 边界

- 状态：accepted
- 日期：2026-07-17

## 背景

早期架构草案把 Computer Use 表达成 Core 基础 crate、通用参考对象和 desktop-client 固定视图。这会让 Core 的启动、Schema、KCP、完成声明和发布门槛反向依赖桌面领域模型，也会把窗口、坐标、截图、desktop generation 等平台易变事实倒灌到通用能力合同。

统一 Extension / Capability SDK 的**规范**已经定义并预留 Profile/version negotiation、capability discovery/probe、Provider Registry、Object Handle、invoke 与 Conformance 挂载职责，但对应运行时、Schema、library、transport 和 Registry 实现尚未完成。Computer Use 未来可以复用这些已定义的公共合同，不需要成为 Core 模块或建立平行协议；本 ADR 只固定边界，**不声称 SDK 或 Computer Use 已实现**。仓库当前没有 Computer Use 的正式 Profile Schema、crate、KCP method 或可运行实现，因此必须诚实区分领域规范存在与能力已经实现。

## 决策

Computer Use 被接受为 **optional、claim-conditional 的 Extension Profile family**，不是 Shittim Core 能力、Core 基础 crate、KCP 固定方法集合或基础产品完成项。

源码 / Schema 依赖保持单向：

```text
Computer Use Profile / Provider package
  -> Extension SDK public contracts
    -> Core public contracts
```

运行时宿主控制 / 调用方向另行表达为：

```text
Core host
  -> SDK host boundary
    -> negotiated Computer Use Profile implementation
```

前一张图约束 import、Schema 生成与类型所有权；后一张图描述宿主进行 negotiation、grant、invoke、cancel 与监督的控制流。两者方向相反但语义不同，不得混称成一条“单一依赖方向”。

Shittim Core 与 Extension SDK Base 不得反向依赖 Computer Use 的 Display、Window、Snapshot、Coordinate、Input Session Lease、Operation Snapshot 或平台 Provider 类型。未来实现应作为 optional Profile/Extension package 分发；本决策不创建空目录、占位 crate、占位 Schema 或声明式假实现。

通用 `extension-protocol`、`provider-registry`、`object-store`、CapabilityDescriptor、Provider 选择、健康、grant 与 Object Handle infrastructure 保留在 SDK/Core 边界。它们只理解通用协商和路由元数据，不解释 Computer Use 私有 payload。

## 三层职责

1. Core

Core继续唯一拥有Task、Action、Policy、PermissionDecision、Approval、Action Lease、Resource Lock、Stop Fence、Audit、Verification与Recovery权威。新Child Task同样只能由Core的`kernel.task/task.child.create` Action原子物化；Computer Use/Profile不得直接创建。Approval v2的material等价判断、失效与resolution复用也只属于Core。

Core可以保存或传递稳定的capability id、provider id、stable resource ref、Schema ref、Object Handle和opaque typed result，但不得复制或解释Computer Use私有对象。当前没有正式Extension Event Schema、transport、persistence或subscription合同，因此本ADR不授权Core保存、索引、稳定转发Profile私有事件。

Core 不增加任何 Computer Use KCP command/query/event，不为桌面模型扩张 KCP Envelope、Task、Action、PermissionDecision、ApprovalRecord 或 AuditRecord 的固定顶层字段。通用副作用规则要求：不得凭不稳定外部观察或未经验证的瞬时上下文引用执行副作用；具体桌面焦点、generation、坐标过期和 reobserve 规则由 Profile 规范负责。

### 2. Extension / Capability SDK

SDK 规范负责定义扩展生命周期、Manifest 上限、SDK 与 Profile 版本协商、capability discovery/probe、Registry 注册、grant、host invoke 校验、取消、进度、健康、Object Handle 与 Conformance 挂载职责。当前运行时尚未实现这些职责；Profile 私有事件也只有未来协议职责，除非另立正式 Extension Event Schema、transport、persistence、subscription 与 Conformance 合同，否则不得声称 Kernel 已能保存、索引或稳定转发，也不得进入 Core EventEnvelope / Outbox。

CapabilityDescriptor 与 Provider 选择保持领域无关。Profile 专属结构只能通过已协商的 versioned operation payload/schema（包括 typed Extension result；未来正式 Extension Event 合同另行定义 event payload）或 Object Handle 所引用对象表达；窗口、坐标、截图、desktop generation 等字段不得进入通用 descriptor 或 Core 顶层合同。

### 3. Computer Use Profile / Provider

Profile 规范拥有统一桌面模型、Snapshot、Operation Snapshot、Coordinate/Transform、Target Resolution、Profile Readiness Gate、Profile-local Input Session Lease、平台 Provider、动作路径与观察—动作—验证细节。Profile 服从 Core 的 Task、Policy、Lease、Lock、Stop、Audit 与 Recovery，不建立平行权威，也不得直连 Privilege Broker。

Profile/Provider 必须用稳定 provider ref、semantic ref、Snapshot/generation ref、用户明确选择的候选 ref 或其他 Profile 定义的 stable target ref 管理目标。当前焦点窗口或元素只能作为候选排序信号或 Prepare 提示，不能成为唯一目标绑定；执行前必须 reobserve 并解析到稳定引用。违规必须返回结构化失败（如 `stale_state` / `target_not_found`），且 Profile Readiness Gate 失败。

## 启用与 claim 条件

相关 capability 只有在安装/enabled、SDK 与 Profile version negotiation、正式 versioned operation payload/result（声明 event 时还包括 event Schema）兼容协商、probe 全部成立后才可标记为 discovery-visible。当前调用是否 invocable 还要独立检查 grant、scope 和副作用边界；只读 observation 不虚构副作用 Action grant。

错误按事实固定：当前安装/发行/协商集合不提供为 `unsupported`；已支持但当前 disabled、quarantined、health/backend/session 暂不可用为 `unavailable`；SDK/Profile/operation Schema 版本无兼容交集为 `incompatible`；能力已 discovery-visible 但当前调用未获 grant 为 `capability_not_granted`。Manifest 声明、文档描述或 Profile family 名称都不是能力存在证明；不得静默 no-op、返回占位句柄或落入 Policy Default Allow。

本 ADR 采用 `COMPUTER_USE.md` 定义的语言中立 `ProfileClaim{claim_id,maturity,distribution_asserted,evidence_refs}` 概念，本阶段不新增 Schema。`contract-only | schema/SDK | composition | provider contract | real-platform` 是 maturity；`distribution_asserted` 是正交布尔事实，不是第六个成熟度。`IF_CLAIM`、`IF_REAL` 与发布门必须读取同一 claim 集合；对外宣称完整 Computer Use 时必须满足该规范固定的机械依赖闭包，不能用含糊的 `ALL` 标签、family 通配符或自然语言代替成员记录。

## Schema 依赖与 API 边界

Computer Use 的可运行支持必须先有独立、版本化的 Profile operation payload/result Schema；声明 `computer.event` 时还必须有独立 event Schema。它们都通过 SDK 的 Profile/version negotiation 绑定。Object Handle 只提供通用生命周期、content type、大小、哈希、权限与过期边界；句柄所指桌面对象的领域语义仍由 Profile Schema 定义。

当前仓库只有 `specs/COMPUTER_USE.md` 的领域合同，没有正式 Computer Use Profile Schema。因而当前不得：

- 把文档中的对象当作已发布 Core object 或 SDK generated type；
- 新增任何 KCP computer method、Core Event、Schema、crate 或实现声明；
- 用无 Schema JSON、私有约定或通用 `details` 字段冒充稳定 API；
- 在 CapabilityDescriptor 顶层加入窗口、坐标、截图或 Snapshot 字段。

未来若要把某个 Profile 私有事件或对象晋升为 Core 公共 API，必须另行增加正式 Core Schema、Catalog、兼容策略、Conformance 与新的 ADR；不能因 SDK 已能承载 opaque payload 就自动晋升。

## Conformance 与完成边界

Shittim Core / Extension SDK Base 的完成和发布必须验证：可选 Profile 全部缺失时 Core 仍能启动；Registry 诚实报告不可用；Core Schema 和 KCP 不含 Computer Use 私有固定字段；通用 SDK 的协商、probe、grant、Object Handle 和 enforcement 标注有效。这些是无条件 BASE 门。

Computer Use 的领域、Provider、平台和真机套件是 claim-conditional：只有实现或发行物作出相应 Profile/capability/platform claim 时，`specs/CONFORMANCE.md` 对应 `IF_CLAIM` / `IF_REAL` 套件才成为硬门。没有 claim 是合法状态，但不得把未运行套件表述为已支持。

完成声明必须按 `contract-only`、`schema/SDK`、`composition`、`provider contract` 与 `real-platform` maturity 分层，并独立记录 `distribution_asserted`。只完成本 ADR 和领域文档不代表 Schema、SDK 接线、Provider、平台支持或产品能力完成。

## Native enforcement 诚实性

Native Extension 的 Manifest 权限声明不等于 OS sandbox。每项能力与发行声明必须准确标注：

- **OS-enforced**：由操作系统、portal、WASI capability 或真实 sandbox 强制；
- **host-enforced**：由 agentd/SDK 在 negotiation、grant、invoke、lease、scope、handle 与 Stop Fence 边界强制；
- **declaration-only**：未沙箱 Native 可绕过 SDK，系统只能声明、展示和审计，不能声称已阻止旁路。

Computer Use 常涉及 capture、input、文件和网络权限；具体 enforcement class 必须按平台与运行形态报告。不得用签名、官方来源、Manifest 文本、host invoke 校验或 Conformance mock 测试冒充 Native 的 OS 级隔离。无法证明的约束必须明确标记为 declaration-only 或当前不支持。

## 迁移与兼容

当前仓库没有 Computer Use crate、正式 Profile Schema、持久对象、KCP method 或可运行实现，因此没有数据迁移、wire migration 或生成物迁移。本次只修正文档边界与索引。

不建立旧 `computer-core` 名称、旧对象形状或假 KCP method 的兼容层，也不创建 deprecated facade、空 crate 或占位 Schema。未来第一个实现直接按本 ADR、`specs/COMPUTER_USE.md` 与 `specs/EXTENSION_SDK.md` 建立版本化 Profile 合同；如果届时存在外部实验实现，其适配由 optional package 自己承担，不能污染 Core。

## 拒绝的替代方案

- **保留 Computer Use 为 Core 基础 crate**：拒绝；会让 Core 编译、启动、Schema 和完成声明依赖可选桌面域，并迫使平台模型进入 Kernel。
- **在 Core 保留通用桌面对象的“最小子集”**：拒绝；窗口、焦点、坐标、截图和 generation 不是跨 Profile 稳定通用事实，会形成字段倒灌与双重权威。
- **先创建空 crate/Schema/KCP method 占位**：拒绝；占位会被误读为已承诺 API 或实现进度，且在无版本化 operation contract 时无法验收。
- **让 desktop-client 固定拥有 Computer Use**：拒绝；client 是可选视图，是否展示 Operation Snapshot 必须由运行时 Profile 可用性决定。
- **仅凭 Manifest 注册能力**：拒绝；Manifest 是安装级上限，不能替代 negotiation、probe、health 与 grant。
- **把 Native Manifest 权限当作 sandbox**：拒绝；这会掩盖 declaration-only 旁路风险。
- **为旧草案建立兼容层**：拒绝；当前没有正式代码、Schema、数据或 wire consumers，兼容层只会固化尚未发布的错误边界。

## 影响

- `specs/IMPLEMENTATION_CONTRACTS.md` 的 Monorepo、Rust 模块、desktop-client 视图、参考对象、Provider 选择和禁止模式必须与本边界一致。
- `specs/COMPUTER_USE.md` 是 Computer Use 领域语义唯一事实源；`specs/EXTENSION_SDK.md` 是协商、probe、invoke、Object Handle 与 enforcement 的事实源；`specs/CONFORMANCE.md` 是条件性门控事实源。
- Core 保留通用 extension protocol、Provider Registry、Object Store 与 capability infrastructure，并允许未来挂载其他 optional Profile；这些基础设施不得被 Computer Use 数据模型绑定。
- 未来 Computer Use 实现需要独立 package、Profile Schema、SDK 接线、Provider 与条件性测试，但不需要修改 Core 的领域对象或 KCP 固定字段。
- 本 ADR 不声称任何 Computer Use 代码、Schema、平台 Provider 或真机支持已经完成。

## 验收

本决策完成时必须满足：

1. Monorepo 与 Rust 模块文档不再把 `computer-core` 列为 Core 基础 crate，且不创建空目录或占位实现；
2. 通用 `extension-protocol`、`provider-registry`、`object-store` 与 capability infrastructure 保留，并明确区分源码/Schema 依赖 `Profile package -> SDK public contracts -> Core public contracts` 与运行时控制流 `Core host -> SDK host boundary -> negotiated Profile implementation`；
3. desktop-client 的 Operation Snapshot 只作为 Profile 可用时的条件性视图，client 本身不等于 Computer Use；
4. CapabilityDescriptor 与 Provider 选择不含窗口、坐标、截图等 Profile 专属顶层字段；
5. 通用参考对象不重复定义 Operation Snapshot 字段，只引用 `specs/COMPUTER_USE.md`，并明确当前无正式 Schema；
6. Core 禁止项使用“不得凭不稳定外部观察引用执行副作用”的通用表述，桌面当前焦点的具体规则留在 Profile；
7. 未新增任何 KCP computer method、Core Event、Schema、crate、目录或实现声明；
8. Shittim Core/Extension SDK Base 与 Computer Use claim-conditional Conformance、Native enforcement class、无迁移/不建兼容层的边界均有明确记录；
9. 文档链接可解析，`git diff --check` 通过。
