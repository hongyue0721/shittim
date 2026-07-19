# AGENT.md — Companion Implementation Constitution

本文件是面向维护者和编码 Agent 的项目宪法，不复制领域字段、枚举或状态机；那些定义只在对应 `specs/` 中存在。

## 1. 全局不变量

- **Kernel Owns Reality**：`agentd` 是 Task、Action、Policy、Audit、Memory 事实、资源锁和恢复的唯一权威；模型、UI、Provider、Extension 输出不是事实或权限。
- **Freedom-first**：Policy 是 Default Allow。没有命中规则时必须 `allow`；`confirm`、`deny`、本地确认、系统认证和计划修订均只能来自匹配规则、底层系统机制或明确恢复状态，不能被风险等级偷偷默认化。
- **User-defined guardrails**：用户与 Companion 都可用自然语言创建、修改、撤销结构化 Policy Rule、Exploration Scope、Delegation、Trigger；第一版没有 Owner 唯一身份认证，不得假称“只有 Owner 能改”。所有变更保留 actor、entry point、来源证据与审计。
- **Core-prohibited/self-modification boundary**：Agent、Skill、Extension、Provider 不得读取后修改、补丁、热替换或重写 agentd Kernel、Task/Policy 解释器、Audit 完整性机制、Privilege Broker、Emergency Stop、Core Identity、安全/隔离边界或私有 Kernel API。正式开发者发布升级可更新 Core。
- **Growth is allowed**：Memory、Learned Style、Self Preferences、Skill、Trigger、Delegation、Governed Configuration、模型/Provider 路由、Extension 源码/安装/更新/回滚、Provider Adapter、任务策略及恢复知识可由 Companion 版本化生成和迭代，受 Policy、预算、停止条件、可观测性和回滚治理；这些不是默认审批。
- **Truthful boundaries**：必须区分 OS-enforced、host-enforced 与 declaration-only 权限，特别是未沙箱 Native Extension；不得把声明伪装为隔离。
- **Optional Profile independence**：任何可选 Profile（包括 Computer Use）都不得成为 Core 的隐含启动、运行、Schema、crate、KCP 或产品完成依赖。Core 面向 Optional Profile 暴露并治理通用 Task/Action/Policy/Scope/Lease/Lock/Stop/Verification/Audit/Recovery 与 Extension 生命周期原语，但这不削弱 Memory、Delegation、Initiative 等 Core 领域自身的事实权威；Core 不预埋 Profile 私有对象或字段。
- **Truthful capability absence**：Profile 未安装、未声明、未协商、probe 未通过或运行时不可用都是合法且必须显式表达的状态；不得用占位实现、空壳兼容层、默认成功或伪造能力档案掩盖缺失。
- **Verification over assertion**：Provider 成功不等于目标成功。副作用必须归属 Task/Action，并按规则和能力验证、审计、恢复。
- **Identity honesty**：Companion 承认自己是 AI，不冒充用户。机械转发用户原文可不标 AI；AI 起草且用户批准的最终文可按用户授权发送；按 Delegation 自主生成外发必须避免冒充并留存来源/审计。

## 2. 可信边界与依赖方向

允许：`client -> agentd`、`agent-runtime -> Kernel Control Protocol`、`agentd -> domain/extension protocol`、`extension -> extension protocol`、`Broker -> fixed system mechanism`。禁止 UI 或 runtime 直连平台能力、Extension 互调、Extension 直接使用 Broker 凭据、Provider 写 Kernel 事实、模型输出直写权限。

必须区分两张方向不同但含义不同的图，禁止合并成一句“唯一依赖方向”：

- **源码 / Schema 依赖**：`Optional Profile package -> Extension SDK public contracts -> Core public contracts`；Shittim Core 与 Extension SDK Base 不得反向 import、生成或固定任一 Profile 类型；
- **运行时宿主控制 / 调用**：`Core host -> SDK host boundary -> negotiated Profile implementation`；这是宿主发起协商、grant、invoke、cancel 与监督的控制流，不表示源码反向依赖 Profile。

第三方代码不得载入 agentd 地址空间。Extension 不得成为 Core 或绕过 Task、Policy、Scope、Lease、审计链。普通系统 Shell 与 Broker 固定特权 Action 是两条不同通道；Agent 不得用前者规避后者或改可信 Core。

## 3. 编码规则

- **禁止最小修复**：不得以补丁最小、改动行数最少或暂时通过测试为目标。所有修复必须从根因闭合，以职责清晰、易维护、易理解、可验证、可扩展、可持续迭代为完成标准；改动范围只服从方案完整性，不服从补丁大小。临时绕过、降格提交、复制平行语义、把已知结构性债务推迟给未来，均视为未完成。
- Rust stable/Tokio、SQLite + FTS5、版本化 JSON Schema；TypeScript strict、Node LTS、Pi；Tauri 可直接依赖。
- 每个持久对象和协议消息带 `schema_version`；可并发对象带 `revision`。Schema 是单一生成源，禁止手写平行类型。
- Schema 编译铁律：中立 graph 的 `ContractTypeId`（`$id` + 严格 JSON Pointer）与语言侧 `RustDeclarationId` 分离；`manifest.id_base` 是 entry `$id` 权威 URL path 命名空间（canonical absolute http(s)+trailing `/`，组件语义归属）；`$ref` 走 `Url::join`+percent-decode→RFC6901；canonical fragment 在 graph 中唯一，`RustProjection` 只 project 一次，renderer 按 use-site lineage 投影多个 declaration，并用 SCC `Option<Box<T>>` 处理 direct 递归；禁止把 language name/logical_title 写进中立 IR；response envelope intentionally untyped；`ArtifactPlan::try_new` path/root component-safe，外部输入错误走 `Result`，禁止生产路径 panic。
- 外部调用必须有 deadline、取消、幂等、结构化错误及恢复语义。不可安全重复的外部动作禁止盲目重放。
- 平台差异留在 Provider；Pi、A_Memorix、OmniParser 是可替换依赖或 Provider，MaiBot、Agent-S、UFO 仅作参考；MCP 是兼容桥，不是 Kernel 内部权威协议。
- 修改前读取本文件及相关 spec，先找既有状态所有者；修改后更新测试并运行规定检查。影响新常驻进程、核心协议、状态所有者、Core 边界或特权 Action 时写 ADR。
- **文档随实现持续更新**：代码、Schema、协议、错误、状态或完成度变化时，必须在同一切片主动更新 `docs/PROGRESS.md`、实现矩阵和相关 API/SDK/ADR/Conformance 文档；文档仍描述旧事实时，切片不得视为完成。
- **双仓库发布闭环**：主仓 `hongyue0721/shittim` 是唯一权威源。主仓验收、提交、推送并核对远端 SHA 后，必须从该已推送提交同步纯文档镜像 `hongyue0721/shittim-docs`；镜像不得独立演化或包含实现、Schema、构建产物和敏感文件。两仓同步与远端验证完成前不得报告任务完成。完整流程见 `docs/REPOSITORY_MAINTENANCE.md`。
- **本阶段编译/测试运行时目录**：在本主机执行编译或本阶段测试时，运行命令必须显式设置 `TMPDIR`（以及 Rust 的 `CARGO_TARGET_DIR`）到 `/mnt/data` 下可写目录；这是维护者/Agent 的运行约定，不是“全 workspace 禁止 `/tmp`”的库内硬门。Rust 库与测试应继续通过 `TMPDIR`/`std::env::temp_dir`/`tempfile` 等标准抽象取临时目录，不得硬编码 host path。双仓 sync 工具及其 Node 测试的固定 `/mnt/data/...` temp 路径合同见 `docs/REPOSITORY_MAINTENANCE.md` §6.1，范围仅限该工具链。
- **提交身份硬门（双仓）**：local `user.name`/`user.email` 与 commit author/committer 的 name/email 必须分别为 `小岳` / `2933634892@qq.com`；sync 工具以 `source_identity` / `docs_identity` 结构化失败，不得只校验邮箱。

## 4. 完成条件

实现必须保持唯一状态所有者与依赖方向；所有副作用可取消、超时、验证和审计；停止不依赖模型；删除和纠正传播到派生数据；扩展崩溃不破坏 Kernel；没有用“健壮兜底”掩盖缺失业务事实。

完成声明必须区分三类义务，并同时满足 `docs/REPOSITORY_MAINTENANCE.md` 的持续文档更新与双仓库发布闭环：

- **Core MUST**：基础产品无条件满足，不能依赖任何可选 Profile；
- **claim-conditional Profile MUST**：只有产品、发行物或 Extension 明确宣称支持某 Profile/version/facet 时才成为验收义务，并必须独立通过该 Profile 的 Conformance；
- **future direction**：尚未形成正式 Schema、Catalog、实现和测试的方向，不得写成已提供能力或基础产品完成项。

详情与可测试枚举见领域 specs。
