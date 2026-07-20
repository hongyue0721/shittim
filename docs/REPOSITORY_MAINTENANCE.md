# 双仓库与持续文档维护

## 1. 仓库角色

Shittim 同时维护两个私有 GitHub 仓库：

- 主仓库：[`hongyue0721/shittim`](https://github.com/hongyue0721/shittim)
- 纯文档镜像：[`hongyue0721/shittim-docs`](https://github.com/hongyue0721/shittim-docs)

主仓库是唯一权威源，拥有源码、Schema、测试、脚本和文档。文档镜像只用于独立阅读与审阅，不得独立修改领域语义，也不得形成第二套事实源。字段、Schema 与可执行行为发生冲突时，一律以主仓库同一提交中的 `AGENT.md`、`specs/`、Schema 和测试为准。

## 2. 持续更新义务

文档不是功能完成后的可选整理项，而是每个变更切片的组成部分。维护者或编码 Agent 在修改实现、Schema、协议、状态、错误、兼容性、测试边界或完成度时，必须主动检查并同步更新受影响文档，包括但不限于：

- `docs/PROGRESS.md`：当前事实、已完成项、未完成项和阻塞；
- `docs/IMPLEMENTATION_MATRIX.md`：规范、Schema、library、composition、public/SDK、real-platform 等成熟度；
- `docs/api/`：内部 API、KCP、错误、事件、持久化与版本合同；
- `docs/sdk/`：可发布 SDK 表面和仍未提供的能力；
- `adr/`：新的架构决策及其状态；
- `specs/CONFORMANCE.md`：新增或变化的不变量与自动化验收锚点；
- `README.md`、导航页与 `FILE_MANIFEST.md`：入口、状态摘要和机械文件清单。

只更新代码而让文档继续描述旧事实，或者只更新进度文字却没有对应实现与测试证据，都属于未完成。历史事实不得被改写成当前能力；计划、contract-only、schema-source-present、library-implemented、composition-reachable、publicly-usable、SDK-publishable 和 real-platform-verified 必须明确区分。

### 2.1 完成状态单一来源（强制）

完成状态**只允许**写在：

1. `docs/IMPLEMENTATION_MATRIX.md` — 域状态表（规范/Schema/实现/测试）；
2. `docs/PROGRESS.md` — 当前里程碑、未完成 backlog、阻塞与下一步。

ADR 与 `docs/api/*` 页面**只允许**一枚状态徽章（如 `implemented` / `partial` / `contract-only`）+ 指向 MATRIX 与/或 PROGRESS 以及 `specs/` / 对应 ADR 的锚点链接。**禁止**在 ADR 文首/路线图段、API 导航页维护字段级复述（subject_hash、UUID 分配表、change_kind 真值表、projection 形状、IR/transaction 内部细节等）；那些事实只属于 `specs/*.md` 与 ADR 的决策段。

违反即文档债：同一完成状态手写多处必然漂移，必须先收敛到 MATRIX/PROGRESS 再改代码说明。

## 3. 每个切片的发布顺序

每个功能、修复、规范或文档切片按以下顺序闭环：

1. 在主仓库完成规范、实现、测试和相关文档更新；
2. 重新生成并校验 `FILE_MANIFEST.md`；
3. 执行该切片的 focused tests 与仓库统一门 `scripts/check-schema.sh`；
4. 独立验收通过后，以中文、功能范围明确的提交写入主仓库；
5. 推送主仓库，并验证远端分支 SHA 等于本地已验收 SHA；
6. 从该主仓已推送提交导出全部 tracked Markdown 与 `LICENSE`，同步到文档镜像；
7. 文档镜像以中文提交记录来源主仓完整 SHA，推送后验证远端 SHA、文件集合、内容和 `FILE_MANIFEST.md`；
8. 两个仓库都完成远端验证且工作区干净后，才可向用户报告该切片完成。

不得先更新文档镜像再让主仓语义追赶，不得从未提交工作区或未推送提交制作镜像，也不得在镜像中手工修订主仓不存在的内容。

## 4. 文档镜像闭集

`shittim-docs` 只允许包含：

- 主仓当前权威提交中的全部 tracked `*.md`；
- 主仓同一提交中的 `LICENSE`；
- 文档仓自身用于禁止实现和构建产物的 `.gitignore`。

不得包含 Rust/TypeScript 源码、`schemas/`、`scripts/`、依赖、构建产物、环境文件、凭据或密钥。镜像中的 Markdown 路径集合和文件字节必须与来源主仓提交完全一致；`FILE_MANIFEST.md` 的路径和行数必须继续成立。

文档镜像提交消息必须包含来源主仓完整 SHA，例如：

```text
文档: 同步shittim@<完整主仓SHA>
```

这样可以从任一文档镜像提交追溯其唯一主仓事实快照。

## 5. 失败处理与禁止事项

以下任一情况都必须停止发布并修复根因：

- 主仓测试、统一门或独立验收失败；
- 主仓远端在推送期间发生未预期变化；
- 文档镜像缺少、增加或改写任一主仓 Markdown；
- `FILE_MANIFEST.md` 与镜像实际文件不一致；
- 镜像包含实现、构建产物或敏感文件；
- 任一仓库本地/远端 SHA 不一致；
- Git author 或 committer 邮箱不是项目配置的 `2933634892@qq.com`，或名称不是 `小岳`。

禁止用强推覆盖未知远端变化。确需重写历史时，必须先确认协作状态，保存旧远端 SHA，并使用带明确 lease 的安全强推。普通持续同步只允许追加提交。

## 6. 同步工具（implemented）

主仓提供零依赖 Node 工具 `scripts/sync-docs-repository.mjs`，从**已推送**的主仓 `master` HEAD 导出文档闭集并同步纯文档镜像。工具**不**提交/推送主仓，**不**生成 `FILE_MANIFEST.md`，**不**使用 `gh` 或猜测认证；对于 push outcome unknown，会保留并在下轮严格恢复同一 staging commit，而不会自行改写或绕过未知远端。

### 6.1 固定路径与远端

| 角色 | 本地路径 | remote | 分支 |
|---|---|---|---|
| 主仓（唯一权威） | `/mnt/data/companion_architecture_v3` | `https://github.com/hongyue0721/shittim.git` | `master` |
| 文档 checkout | `/mnt/data/shittim-docs-export` | `https://github.com/hongyue0721/shittim-docs.git` | `master` |

锁文件：`/mnt/data/shittim-docs-repository.sync.lock`（`O_EXCL` 独占；**不**自动清除 stale）。

**双仓 sync 工具及其 Node 测试的临时目录合同（仅本工具链）**：`scripts/sync-docs-repository.mjs` 的 production `tempRoot`、文档同步测试根 `/mnt/data/shittim-docs-sync-tests`，以及 `update-file-manifest --self-test` 使用的 mode `0700` 根 `/mnt/data/shittim-file-manifest-tests`（mode `0600` 独占锁 `.self-test.lock`）均固定在 `/mnt/data` 下；每个 fixture 独立创建并在结束时递归清理，不读取 `os.tmpdir()`。该约束**只**约束上述双仓 sync / file-manifest Node 工具与其测试，**不得**被解读为“整个 workspace 禁止 `/tmp`”。

**本阶段仓库编译与测试运行时的主机约定（与上条不同层）**：当前维护者/Agent 在本主机跑编译或本阶段全量/focused 测试时，运行命令必须显式设置 `TMPDIR` 与（Rust 时）`CARGO_TARGET_DIR` 到 `/mnt/data` 下可写目录，例如：

```text
export TMPDIR=/mnt/data/shittim-build-tmp
export CARGO_TARGET_DIR=/mnt/data/shittim-cargo-target
mkdir -p "$TMPDIR" "$CARGO_TARGET_DIR"
```

Rust 库与测试代码应继续使用 `TMPDIR` / `std::env::temp_dir` / `tempfile` 等标准临时目录抽象，**不得**在库内硬编码 `/mnt/data` 或 `/tmp` 等 host path。

### 6.2 CLI 与 npm scripts

```text
export PATH="$HOME/.local/share/pnpm:$PATH"
pnpm run check:docs-repository   # node scripts/sync-docs-repository.mjs --check
pnpm run sync:docs-repository    # node scripts/sync-docs-repository.mjs --sync
pnpm run test:docs-repository    # node --test scripts/sync-docs-repository.test.mjs
```

- `--check`：真正只读验收。主仓 clean 且 HEAD 实时等于远端 `master` 后，在 `tempRoot` 下创建 mode 0700 的临时 bare 对象仓，fetch 并审计文档远端 history/tree，再清理；不会对 production docs checkout 执行 init/config/fetch/hash-object/update-ref。若本地 checkout 已存在，仅执行 status/config/cat-file 类只读检查；不存在则报告 `missing`，不创建。
- `--sync`：在同一 source 前置条件下规划并执行 bootstrap / 线性追加 / 同 tree 来源收据 / 幂等；普通 fast-forward push，禁止 force / force-with-lease。CLI 入口始终使用源码内固定 production contract；fixture 路径只通过 import 后的显式 module API 注入，环境变量不能覆盖 production 路径或 remote。
- `--self-test`：纯函数与合同冒烟；本地 bare-remote 集成覆盖见 `test:docs-repository`。CLI 必须恰好一个合法 flag，多余或未知参数均返回 `usage`。

所有 Git 子进程都会清除可重定向仓库、对象库、index 与配置的环境变量（包括 `GIT_DIR`、`GIT_WORK_TREE`、`GIT_COMMON_DIR`、`GIT_INDEX_FILE`、`GIT_OBJECT_DIRECTORY`、alternates 和 `GIT_CONFIG*` 注入），同时保留 `GIT_ASKPASS`、`GIT_SSH_COMMAND` 等认证传输变量。

### 6.3 文档闭集与对象导出

闭集 = 主仓**已推送 commit** 的 Git 对象中全部 tracked `*.md` + `LICENSE`，再加固定文档仓专用 `.gitignore` 字节。Markdown/LICENSE 的字节与 mode 只来自 `ls-tree` / `cat-file`，**不**复制 dirty worktree。拒绝 symlink、submodule/gitlink、非法路径与非 `100644`/`100755` mode。提交消息：

- 普通同步：`文档: 同步shittim@<完整SHA>`
- 兼容根提交：`文档: 从shittim@<完整SHA>建立纯文档镜像`

### 6.4 历史与推送门禁

- 文档仓 first-parent 历史必须线性，禁止 merge；每个 commit 的 author/committer **邮箱与名称**必须分别是 `2933634892@qq.com` 与 `小岳`（与 source local config / HEAD identity 对称硬门；身份失败走 `docs_identity`，结构/marker/ancestry 失败仍走 `docs_history`）。
- 来源权威索引从实时已推送 source `master` HEAD 以 `rev-list --first-parent --reverse` 构建。每个 marker SHA 必须属于该索引并严格递增；merge second-parent / 侧枝 SHA 即使在 DAG 可达也拒绝。
- 每个历史 commit 的 tree 必须精确等于该来源 SHA 的闭集 + 固定 `.gitignore`；`FILE_MANIFEST.md` 必须严格匹配 Markdown 路径闭集、Git 字节序、唯一条目、LF 行数与自身行数。
- push 前复验远端 tip，且待推 commit 的 parent 必须精确等于 expected remote parent（bootstrap 为零 parent）；push 后再次查询并区分 confirmed success、rejected、unknown。unknown 保留 `refs/heads/docs-sync-staging`；下一次 `--sync` 会先验证 staging 的 tree、marker、author/committer、parent 与当前 source/remote：remote 已等于 staging 时完成本地 checkout 并清理，remote 仍等于 parent 时续推同一 commit，第三值或 invalid staging 分别以 `docs_staging_diverged` / `docs_staging_invalid` fail-closed，绝不创建平行 commit。
- 本地 docs checkout 的独立状态机模块 `scripts/docs-checkout-transaction.mjs` 使用 `.git/docs-checkout-transaction.json` 原子 journal：先固定 `oldHead`/`targetSha`，再 materialize index/worktree、最后移动 branch 与 remote-tracking ref。`journal_written`、`worktree_materialized`、`branch_ref_updated`、`head_symbolic`、`remote_tracking_updated`、`journal_cleared` 每个边界注入中断后，下轮都以同一目标重跑并收敛；pending target 不同、journal 与 HEAD 冲突、initial partial tracked index，以及 `materialized` / `ref_updated` 后或 completion 前的树漂移均返回 `docs_checkout_recovery`，并逐字节保留用户文件，不执行覆盖。

### 6.5 结构化错误码（稳定）

失败时 stderr 先打 JSON（`ok:false, code, message, details`），再打人类可读一行。production 可达 `code`：`usage`、`lock_held`、`lock_io`、`source_not_repo`、`source_wrong_branch`、`source_wrong_remote`、`source_dirty`、`source_not_pushed`、`source_identity`、`source_snapshot`、`source_path_rejected`、`manifest_mismatch`、`docs_not_repo`、`docs_wrong_remote`、`docs_wrong_branch`、`docs_history`、`docs_identity`、`docs_tree`、`docs_remote_diverged`、`docs_push_rejected`、`docs_push_unknown`、`docs_staging_invalid`、`docs_staging_diverged`、`docs_checkout`、`docs_checkout_recovery`、`plan`、`internal`。

- `source_identity`：主仓 local `user.email`/`user.name` 或 HEAD author/committer email/name 任一不等于合同值。
- `docs_identity`：文档仓历史（或 staging recovery 审计中的身份字段）author/committer email/name 任一不等于合同值；staging 的 tree/parent/marker 违规仍归 `docs_staging_invalid`。
- `docs_history`：线性/merge/root parent/subject marker/source first-parent ancestry 等**结构与来源索引**问题，不含身份字段。

自动化覆盖见 `scripts/sync-docs-repository.test.mjs`（本地 bare remotes，不依赖 production 工作区 dirty 状态）：source dirty/not-pushed/wrong branch/wrong remote/email/name 与 HEAD author/committer name；Git 环境污染净化；manifest 路径集/排序/duplicate/LF 行数与 strict object bytes；source symlink/gitlink；docs 真实 merge/identity(email+name)/marker/tree/.gitignore/source first-parent；bootstrap/append/receipt/idempotent；push race/rejected/nonzero-but-confirmed/unknown/parent gate/plain argv；`runSync` 端到端证明 unknown 保留 staging、下一轮以 `recover_staging` 续推或完成已落远端的同一 commit、不会创建平行 commit，且 rejected/success 均清理 staging；check 临时仓不改本地 checkout；checkout initial/already-tip/FF/nonancestor/tracked edit/untracked collision、pending target/HEAD 冲突、initial partial tracked、materialized/ref_updated/completion tree drift逐字节保护，以及含 `journal_cleared` 的全部事务边界恢复；porcelain rename；lock held/I/O/release。该清单描述现有覆盖，不宣称穷尽所有 Git/网络故障矩阵。

### 6.6 当前配置事实

- 两个仓库默认分支：`master`；
- 两个仓库可见性：private（**人工事实**；同步脚本不验证、不修改 visibility）；
- Git author/committer 名称：`小岳`；
- Git author/committer 邮箱：`2933634892@qq.com`；
- 主仓远端：`https://github.com/hongyue0721/shittim.git`；
- 文档仓远端：`https://github.com/hongyue0721/shittim-docs.git`。

仓库地址、可见性、默认分支或提交身份发生变化时，必须先更新本文件、工具内 `PRODUCTION_CONTRACT` 与发布校验流程，再进行下一次同步。
