# 开发交接手册

> 本文档面向在新设备上继续 Shittim 项目的开发者。clone 后按此手册准备环境、遵循流程、认领下一步任务。

## 1. 当前状态快照

- **主仓库**：<https://github.com/hongyue0721/shittim>（`master`）
- **文档镜像仓库**：<https://github.com/hongyue0721/shittim-docs>
- **当前 HEAD**：`da361420d62e637eaffde9da3603065f15d73c91`
- **工作树**：干净（无未提交改动）
- **里程碑 `V2InitialBuildActive`（ADR-0009）** 已完成切片：
  0 规范手术；1a root v2 持久对象 Schema×4；1b Action/child Schema×5 + `kernel-authorization` 纯库；1c-i 授权核心五 Schema；1c-ii 身份/挑战/证据八 Schema；2 root TaskCreate v2 仓库/migration 0004；3a production MethodVersionBindings 八方法集；3b KCP 切 active v2 删 v1 路径；3c sqlite v1 写路径删除/v2-only Outbox/旧库 reinitialize-required 拒绝；4a Action 仓库+`action.state_changed`/migration 0006；4b PolicyRule+PermissionDecision 仓库+评估编排/migration 0007；4c Approval 三 CAS 方法+Identity 仓库/migration 0008。
- **测试基线（主会话确认）**：`kernel-sqlite` 110、`kernel-authorization` 63、`domain-task` 45；全工作区测试绿；`./scripts/check-schema.sh` 全量门绿。
- **Schema 数量**：`schemas/manifest.json` 共 83 个 Schema。
- **事实单一来源**：`docs/IMPLEMENTATION_MATRIX.md` 与 `docs/PROGRESS.md`，ADR/API 文档只保留状态徽章与锚点链接。

## 2. 新设备环境准备

1. 安装 Rust toolchain（`rustup`），确保 `cargo`、`rustc` 在 PATH。
2. 安装 Node 与 pnpm：
   - 推荐已存在 `Node 24.18.0` + `pnpm 11.3.0`；pnpm 路径加入 `export PATH="$HOME/.local/share/pnpm:$PATH"`。
   - 若 Node 缺失，使用 pnpm 用户级 runtime 安装：`pnpm env use --global 24.18.0`。
   - 校验：`node scripts/check-node-toolchain.mjs`（读取 `package.json` 的 `engines` 与 `packageManager`，要求精确版本）。
3. 设置大/临时产物目录到 `/mnt/data`：

   ```bash
   export TMPDIR=/mnt/data/shittim-build-tmp
   export CARGO_TARGET_DIR=/mnt/data/shittim-cargo-target
   mkdir -p "$TMPDIR" "$CARGO_TARGET_DIR"
   ```

4. 首次验证命令（clone 后执行）：

   ```bash
   git rev-parse HEAD            # 应为 da36142...
   git status --short            # 应为空
   node scripts/check-node-toolchain.mjs
   cat schemas/manifest.json | python3 -c "import json,sys; print(len(json.load(sys.stdin)['schemas']))"
   # 应输出 83
   ```

5. 验收门前的特殊动作：
   - `git add -A`：因为 `./scripts/check-schema.sh` 的 generated-tree drift 检查对 index 做 diff，未 stage 的文档/生成物会被误判。
   - 文档变更后必须执行 `node scripts/update-file-manifest.mjs --write`，刷新 `FILE_MANIFEST.md`。

## 3. 标准工作流程

每个切片按以下顺序闭环，详情见 `docs/REPOSITORY_MAINTENANCE.md`：

1. **文档先行**：先更新规范/contract/ADR/API 文档，再写实现。
2. **实现**：按 contract 编码，优先复用现有仓库与模式。
3. **测试**：聚焦测试 + 全工作区 `cargo test`。
4. **独立验收**：必须 `0 Critical / 0 High / 0 Medium` 才 GO。
5. **提交**：中文、按功能域的提交；`./scripts/check-schema.sh` 全绿后再提交。
6. **推送**：验证远端 SHA 与本地已验收 SHA 一致。
7. **同步文档镜像**：`pnpm run sync:docs-repository` 并带 `--check` 验证镜像文件集合、内容与 `FILE_MANIFEST.md` 与主仓完全一致。

## 4. 下一步任务一：切片 5 child materializer

这是当前最高优先级任务，目标与约束如下：

### 4.1 目标

- **子 Task 唯一创建路径**：由 parent 发起 `kernel.task/task.child.create` Action（S1 kernel.task），经 Policy 评估后，由 Kernel 原子物化。
- root 直接创建只能建 root；v2 已强制 root-only，child 不再通过 KCP/TaskCreate 直接写入。

### 4.2 已有资产

- Schema 与生成类型：`ChildTaskProposalV1`、`NormalizedChildTaskProposalV1`、`ChildTaskMaterializationAllocationV1`（`schemas/`、`kernel-contracts` 生成类型）。
- `kernel-task-creation` 纯库：child 规范化、hash、10 UUID allocation 校验与官方 fixtures：
  - `schemas/fixtures/task/child_task_proposal_normalized_hash.v1.json`
  - `schemas/fixtures/task/task_creation_allocations.v1.json`
- `kernel-authorization` 投影：`project_child_task_delta`（`rust/crates/kernel-authorization/src/child_delta.rs`）、`project_material_authorization`（`material.rs`）、`project_observation_evidence`（`observation.rs`）。
- 已闭合的持久化基础设施：Action 仓库（4a/migration 0006）、PolicyRule+PermissionDecision 仓库+评估编排（4b/migration 0007）、Approval 三 CAS 方法+Identity 仓库（4c/migration 0008）。

### 4.3 待实现

1. `kernel-sqlite` 新增 child materialization 方法；如需要新表，按 migration 0004-0008 的 descriptor 模式写 `migrations/0009_child_materialization.sql`。
2. **原子性**：同一 `BEGIN IMMEDIATE` 事务内完成：
   - 创建 Action 标记为完成；
   - 写入子 `Task`、`TaskScope`、`ContentOrigin`、`Provenance`；
   - 写入 `VerificationResult`、`AuditRecord`、Outbox 事件。
3. 子 `Task` 的 sequence 0 事件为 `task.created`，其 `causation` 必须指向父 Action，不得伪造 command 或 event carrier。
4. 所有 UUID allocation 由上层注入，并经 `kernel-task-creation` 的 typed fixture/validator 校验；拒绝自由 UUID bag。
5. 严格 readback：事务提交后返回物化的子 Task 全量视图，供调用方验证。readback 必须至少覆盖：
   - `task` 表的 `task_id`、`parent_task_id`、`goal` 摘要；
   - `task_scope` 与 `content_origin` 的关联完整性；
   - `provenance` 的 sequence 0 与 `causation_ref` 指向父 Action；
   - `audit_record` 的 carrier 为 action 且 receipt hash 覆盖 `NormalizedChildTaskProposalV1` 的 JCS bytes；
   - `outbox` 的 `task.created` 事件已生成且 event_id 与 allocation 注入的预期一致。

6. 禁止引入新的 v1 路径或允许直接写 child 的 API；任何新增 public 方法必须有对应的 contract 测试。

### 4.4 合同锚点

- `specs/IMPLEMENTATION_CONTRACTS.md`：§5.3（child proposal 与 TaskCreate 字段同构）、§13.6（切片 1b/2/4a-4c 的约束与仓库边界）、§13.7（`V2InitialBuildActive` 谓词清单）。
- `specs/CORE_ARCHITECTURE.md` §9.2（child task 父子关系与权威入口）。
- `specs/CONFORMANCE.md` 中关于 child proposal 的字段约束、allocation 互异校验与 Schema-valid/domain-invalid 边界（第 3 章与第 5 章）。
- `adr/0006-child-task权威与taskcreate-v2迁移.md`（`task.child.create` 是 Kernel-local Action operation、atomic materialization、allocation 结构）。
- `adr/0009-v2从零构建并取消v1数据迁移.md`（v2-only 基线，取消 v1 迁移）。

### 4.5 验收门槛

- 新增测试全绿；`kernel-task-creation` 的 allocation/validator 测试覆盖边界；`kernel-sqlite` 新增 integration 测试覆盖原子事务与 readback。
- `./scripts/check-schema.sh` 全量门绿。
- 独立验收 `0 Critical/High/Medium` 后才提交。
- 提交前：`git add -A && ./scripts/check-schema.sh`，确保 generated tree 与 `FILE_MANIFEST.md` 都已 stage 并通过漂移检查。

## 5. 下一步任务二：切片 6 最终清理 + §13.7 闭合

1. 逐条核对 `specs/IMPLEMENTATION_CONTRACTS.md` §13.7 的谓词；未闭合的项必须显式记录，不得隐藏。
2. 只有在 §13.7 全部闭合后，才解锁以下五方法 handler：
   - `task.list`
   - `event.subscribe`
   - `event.poll`
   - `stop.activate`
   - `stop.status`
3. 这些 handler 各自有前置合同，§13.7 纪律要求：谓词未闭合不得启动 server。

## 6. 再后续路线图

仅概述，五方法 handler 后才能进入：

- `task.list`：游标/排序合同。
- `event.subscribe` / `event.poll`：订阅关系与 Outbox 游标合同。
- `stop.activate` / `stop.status`：stop fence 持久化合同；**特别注意**：`stop.activate` 的副作用与重启和解合同尚未闭合，属合同先行任务。
- Publisher loop。
- 多语言 SDK。
- 桌面端。

## 7. 验收债务（必须先补）

切片 4b 与 4c 因子代理通道故障由主会话亲自实现并自验提交，但**缺少独立验收**。恢复后必须补做：

- **4b 复核**（提交 `e96ae7c`）：Terra 级复核，重点：评估编排的单事务性；material/observation 指纹写入；PermissionDecision 与 PolicyRule 的边界。
- **4c 复核**（提交 `a72a757`）：Terra 级复核，重点：Approval 三 CAS 方法边界；证据校验面；Action revision bump 的因果一致性。

复核通过后再继续切片 5，避免在未经验证的 4b/4c 基础上叠加物化逻辑。

## 8. 已知问题

- `schema-tool` 测试 `artifact_transaction lock_conformance real_cross_process_holder_crash_releases_fd_lock` 在并行高负载下偶发时序 flake，单跑必过。与业务逻辑无关，待修复。

## 9. 子代理使用政策

- `inherit_context` 必须为 `false`。
- prompt 必须自包含：工作区、目标、范围、约束、步骤、验收标准、输出格式。
- 子代理报告返回后，必须对照实际工作区核验（曾出现假阳性与假阴性）。
- 推荐模型分工以 `docs/REPOSITORY_MAINTENANCE.md` 与主会话政策为准：快速探索用轻量模型，合同/推理用强模型，实现继承父会话或指定 `proxy/grok-4.5` 等。
- 给子代理的 `prompt` 必须显式禁止：
  - 未读仓库就臆想接口；
  - 用兜底默认值掩盖 missing fact；
  - 修改超出 prompt 范围的文件；
  - 未实际跑测试就报告“完成”。
