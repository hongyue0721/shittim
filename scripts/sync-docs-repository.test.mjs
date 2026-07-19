#!/usr/bin/env node
/**
 * Full local bare-remote matrix for scripts/sync-docs-repository.mjs.
 * All temp paths live under /mnt/data (never /tmp).
 *
 * Run:
 *   PATH="$HOME/.local/share/pnpm:$PATH" TMPDIR=/mnt/data/shittim-docs-sync-tests \
 *     node --test scripts/sync-docs-repository.test.mjs
 */
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { createHash } from "node:crypto";

import {
  PRODUCTION_CONTRACT,
  DOCS_GITIGNORE_BYTES,
  DOCS_GITIGNORE_TEXT,
  ErrorCode,
  SyncError,
  sanitizeGitEnvironment,
  createGitPort,
  decodeGitPathBytes,
  splitNulPaths,
  assertSafeRepoRelativePath,
  parseDocsCommitSubject,
  formatSyncCommitMessage,
  formatBootstrapCommitMessage,
  parseLsTreeZ,
  isDocsSourcePath,
  buildSourceSnapshot,
  validateFileManifest,
  buildSourceFirstParentIndex,
  buildExpectedDocsEntries,
  buildContentManifest,
  diffContentManifests,
  materializeDocsTree,
  readCommitContentManifest,
  inspectSourceRepository,
  loadDocsFirstParentHistory,
  auditDocsHistory,
  planSyncAction,
  acquireLock,
  parsePorcelainV2Z,
  inspectExistingDocsCheckout,
  runCheck,
  runSync,
  runSelfTest,
  commitTree,
  recoverDocsStaging,
  DOCS_STAGING_REF,
  finalizeLocalCheckout,
  fetchDocsRemoteTip,
  ensureDocsRepoShell,
  pushDocsFastForward,
} from "./sync-docs-repository.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(HERE, "..");
const TEST_ROOT_BASE = "/mnt/data/shittim-docs-sync-tests";
const EMAIL = "2933634892@qq.com";
const NAME = "小岳";

mkdirSync(TEST_ROOT_BASE, { recursive: true });

const git = createGitPort();

function tempDir(prefix) {
  mkdirSync(TEST_ROOT_BASE, { recursive: true });
  return mkdtempSync(join(TEST_ROOT_BASE, prefix));
}

function write(path, content) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content, "utf8");
}

function gitOk(cwd, args, opts = {}) {
  const r = git.run(cwd, args, opts);
  assert.equal(r.status, 0, `git ${args.join(" ")}: ${r.stderr.toString("utf8")}`);
  return r;
}

function gitText(cwd, args, opts = {}) {
  return git.textTrim(cwd, args, opts);
}

function initSourceRepo(root, { remoteUrl, branch = "master" } = {}) {
  mkdirSync(root, { recursive: true });
  gitOk(root, ["init", "-b", branch]);
  gitOk(root, ["config", "--local", "user.email", EMAIL]);
  gitOk(root, ["config", "--local", "user.name", NAME]);
  // Disable autocrlf so worktree bytes match objects in tests.
  gitOk(root, ["config", "--local", "core.autocrlf", "false"]);
  if (remoteUrl) {
    gitOk(root, ["remote", "add", "origin", remoteUrl]);
  }
}

function commitAll(root, message) {
  gitOk(root, ["add", "-A"]);
  gitOk(root, ["-c", `user.email=${EMAIL}`, "-c", `user.name=${NAME}`, "commit", "-m", message]);
  return gitText(root, ["rev-parse", "HEAD"]);
}

function bareInit(path, branch = "master") {
  mkdirSync(dirname(path), { recursive: true });
  gitOk(TEST_ROOT_BASE, ["init", "--bare", "-b", branch, path]);
  return path;
}

function pushHead(root, remote = "origin", branch = "master") {
  gitOk(root, ["push", "-u", remote, `HEAD:refs/heads/${branch}`]);
}

function seedMinimalSource(sourceRoot) {
  const markdown = new Map([
    ["AGENT.md", "# agent\n"],
    ["FILE_MANIFEST.md", ""],
    ["README.md", "# readme\n"],
    ["docs/a.md", "docs-a\n"],
  ]);
  const header = [
    "# FILE_MANIFEST",
    "",
    "> 非规范元数据。列出 Git source set 中的 Markdown（tracked `git ls-files '*.md'` + 标准 ignore 下 untracked source）；不含 ignored build 产物（如 target/、node_modules/）。行数以 UTF-8 文本 `wc -l` 等价结果为准。由 `scripts/update-file-manifest.mjs` 生成，禁止手改。",
    "",
  ];
  const paths = [...markdown.keys()].sort((a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)));
  const manifestLines = header.length + paths.length;
  const body = paths.map((path) => {
    const lines = path === "FILE_MANIFEST.md"
      ? manifestLines
      : Buffer.from(markdown.get(path)).filter((byte) => byte === 0x0a).length;
    return `- \`${path}\` — ${lines} lines`;
  });
  markdown.set("FILE_MANIFEST.md", `${header.join("\n")}\n${body.join("\n")}\n`);
  write(join(sourceRoot, "LICENSE"), "Apache-2.0 license text\n");
  for (const [path, bytes] of markdown) write(join(sourceRoot, path), bytes);
  write(join(sourceRoot, "rust", "x.rs"), "fn main(){}\n");
  write(join(sourceRoot, "schemas", "x.json"), "{}\n");
}

function makeContract(paths) {
  return {
    ...PRODUCTION_CONTRACT,
    sourceRepoRoot: paths.sourceRoot,
    sourceRemoteUrl: paths.sourceRemoteUrl,
    docsCheckoutRoot: paths.docsRoot,
    docsRemoteUrl: paths.docsRemoteUrl,
    lockPath: paths.lockPath,
    tempRoot: paths.tempRoot,
  };
}

/**
 * Full fixture: bare source remote, bare docs remote, source worktree pushed,
 * optional bootstrap of docs remote from source HEAD closed set.
 */
function createFixture(opts = {}) {
  const base = tempDir("fixture-");
  const sourceBare = bareInit(join(base, "source.git"));
  const docsBare = bareInit(join(base, "docs.git"));
  const sourceRoot = join(base, "source-work");
  const docsRoot = join(base, "docs-work");
  const lockPath = join(base, "sync.lock");
  const tempRoot = join(base, "tmp");
  mkdirSync(tempRoot, { recursive: true });

  initSourceRepo(sourceRoot, { remoteUrl: sourceBare });
  seedMinimalSource(sourceRoot);
  if (opts.extraFiles) {
    for (const [rel, body] of Object.entries(opts.extraFiles)) {
      write(join(sourceRoot, rel), body);
    }
  }
  const sourceSha = commitAll(sourceRoot, "init source");
  pushHead(sourceRoot);

  const contract = makeContract({
    sourceRoot,
    sourceRemoteUrl: sourceBare,
    docsRoot,
    docsRemoteUrl: docsBare,
    lockPath,
    tempRoot,
  });

  const ctx = {
    base,
    sourceBare,
    docsBare,
    sourceRoot,
    docsRoot,
    lockPath,
    tempRoot,
    contract,
    sourceSha,
    cleanup() {
      rmSync(base, { recursive: true, force: true });
    },
  };

  if (opts.bootstrapDocs) {
    bootstrapDocsFromSource(ctx);
  }

  return ctx;
}

function bootstrapDocsFromSource(ctx, sourceSha = ctx.sourceSha) {
  const snapshot = buildSourceSnapshot(git, ctx.sourceRoot, sourceSha);
  const entries = buildExpectedDocsEntries(snapshot, ctx.contract);
  // Prepare docs work shell pointing at bare.
  ensureDocsRepoShell(git, ctx.contract, {
    docsRoot: ctx.docsRoot,
    docsRemoteUrl: ctx.docsBare,
  });
  const { treeSha } = materializeDocsTree(git, ctx.docsRoot, entries);
  const message = formatBootstrapCommitMessage(sourceSha, ctx.contract);
  const commit = commitTree(git, ctx.docsRoot, {
    treeSha,
    parentSha: null,
    message,
    email: EMAIL,
    name: NAME,
  });
  gitOk(ctx.docsRoot, ["update-ref", "refs/heads/master", commit]);
  gitOk(ctx.docsRoot, ["push", "-u", "origin", "HEAD:refs/heads/master"]);
  ctx.docsBootstrapSha = commit;
  ctx.sourceSha = sourceSha;
  return commit;
}

function regenerateFixtureManifest(sourceRoot) {
  const paths = git.run(
    sourceRoot,
    ["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", "*.md"],
  ).stdout
    .toString("utf8")
    .split("\0")
    .filter(Boolean)
    .sort((a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)));
  const header = [
    "# FILE_MANIFEST",
    "",
    "> 非规范元数据。列出 Git source set 中的 Markdown（tracked `git ls-files '*.md'` + 标准 ignore 下 untracked source）；不含 ignored build 产物（如 target/、node_modules/）。行数以 UTF-8 文本 `wc -l` 等价结果为准。由 `scripts/update-file-manifest.mjs` 生成，禁止手改。",
    "",
  ];
  const total = header.length + paths.length;
  const lines = paths.map((path) => {
    const count = path === "FILE_MANIFEST.md"
      ? total
      : readFileSync(join(sourceRoot, path)).filter((byte) => byte === 0x0a).length;
    return `- \`${path}\` — ${count} lines`;
  });
  write(join(sourceRoot, "FILE_MANIFEST.md"), `${header.join("\n")}\n${lines.join("\n")}\n`);
}

function appendSourceCommit(ctx, mutator, message = "source change") {
  mutator(ctx.sourceRoot);
  regenerateFixtureManifest(ctx.sourceRoot);
  const sha = commitAll(ctx.sourceRoot, message);
  pushHead(ctx.sourceRoot);
  ctx.sourceSha = sha;
  return sha;
}

function expectSyncError(fn, code) {
  let err;
  try {
    fn();
  } catch (e) {
    err = e;
  }
  assert.ok(err instanceof SyncError, `expected SyncError, got ${err}`);
  assert.equal(err.code, code, `expected code ${code}, got ${err.code}: ${err.message}`);
  return err;
}

function makeScriptedGit({ parentLine, pushStatus, tips, queryFailureAt = null }) {
  let queryCount = 0;
  const calls = [];
  return {
    calls,
    textTrim(_cwd, args) {
      if (args[0] === "show") return parentLine;
      throw new Error(`unexpected textTrim: ${args.join(" ")}`);
    },
    run(cwd, args) {
      calls.push([...args]);
      if (args[0] === "ls-remote") {
        queryCount += 1;
        if (queryFailureAt === queryCount) {
          return { status: 1, stdout: Buffer.alloc(0), stderr: Buffer.from("query failed"), args, cwd };
        }
        const tip = tips.shift() ?? null;
        const stdout = tip
          ? Buffer.from(`${tip}\trefs/heads/master\n`)
          : Buffer.alloc(0);
        return { status: 0, stdout, stderr: Buffer.alloc(0), args, cwd };
      }
      if (args[0] === "push") {
        return {
          status: pushStatus,
          stdout: Buffer.alloc(0),
          stderr: Buffer.from(pushStatus === 0 ? "" : "rejected"),
          args,
          cwd,
        };
      }
      throw new Error(`unexpected run: ${args.join(" ")}`);
    },
  };
}

function makeValidManifestSnapshot(extra = {}) {
  const markdown = new Map([
    ["A.md", Buffer.from("a\n")],
    ["FILE_MANIFEST.md", Buffer.alloc(0)],
    ...Object.entries(extra).map(([path, body]) => [path, Buffer.from(body)]),
  ]);
  const paths = [...markdown.keys()].sort((a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)));
  const header = [
    "# FILE_MANIFEST",
    "",
    "> 非规范元数据。列出 Git source set 中的 Markdown（tracked `git ls-files '*.md'` + 标准 ignore 下 untracked source）；不含 ignored build 产物（如 target/、node_modules/）。行数以 UTF-8文本 `wc -l` 等价结果为准。由 `scripts/update-file-manifest.mjs` 生成，禁止手改。",
    "",
  ];
  // Use production header bytes, not the near-match above.
  header[2] = "> 非规范元数据。列出 Git source set 中的 Markdown（tracked `git ls-files '*.md'` + 标准 ignore 下 untracked source）；不含 ignored build 产物（如 target/、node_modules/）。行数以 UTF-8 文本 `wc -l` 等价结果为准。由 `scripts/update-file-manifest.mjs` 生成，禁止手改。";
  const total = header.length + paths.length;
  const body = paths.map((path) => {
    const lines = path === "FILE_MANIFEST.md"
      ? total
      : markdown.get(path).filter((byte) => byte === 0x0a).length;
    return `- \`${path}\` — ${lines} lines`;
  });
  markdown.set("FILE_MANIFEST.md", Buffer.from(`${header.join("\n")}\n${body.join("\n")}\n`));
  const entries = [...markdown].map(([path, bytes]) => ({ path, mode: "100644", blobSha: "0".repeat(40), bytes }));
  entries.push({ path: "LICENSE", mode: "100644", blobSha: "0".repeat(40), bytes: Buffer.from("L\n") });
  return { sourceSha: "0".repeat(40), sourceTreeSha: "0".repeat(40), entries, byPath: new Map(entries.map((entry) => [entry.path, entry])) };
}

// ---------------------------------------------------------------------------
// Pure unit layer
// ---------------------------------------------------------------------------

test("pure: docs commit subject parsers and formatters", () => {
  const sha = "0123456789abcdef0123456789abcdef01234567";
  assert.equal(formatSyncCommitMessage(sha), `文档: 同步shittim@${sha}`);
  assert.equal(
    formatBootstrapCommitMessage(sha),
    `文档: 从shittim@${sha}建立纯文档镜像`,
  );
  assert.deepEqual(parseDocsCommitSubject(formatSyncCommitMessage(sha)), {
    kind: "sync",
    sourceSha: sha,
  });
  assert.deepEqual(parseDocsCommitSubject(formatBootstrapCommitMessage(sha)), {
    kind: "bootstrap",
    sourceSha: sha,
  });
  assert.equal(parseDocsCommitSubject(`文档: 同步shittim@${sha} `), null);
  assert.equal(parseDocsCommitSubject(`文档: 同步shittim@${sha.slice(0, 7)}`), null);
  assert.equal(parseDocsCommitMessageCompat(), null);

  function parseDocsCommitMessageCompat() {
    return parseDocsCommitSubject("chore: something else");
  }
});

test("pure: path safety and UTF-8 NUL decoding", () => {
  assert.equal(assertSafeRepoRelativePath("docs/a.md"), "docs/a.md");
  assert.throws(() => assertSafeRepoRelativePath("../x"), SyncError);
  assert.throws(() => assertSafeRepoRelativePath("/abs"), SyncError);
  assert.throws(() => assertSafeRepoRelativePath("a\\b.md"), SyncError);
  assert.throws(() => assertSafeRepoRelativePath("a/../../b"), SyncError);
  assert.throws(() => decodeGitPathBytes(Buffer.from([0xff])), /UTF-8/);
  assert.deepEqual(splitNulPaths(Buffer.from("a\0b\0", "utf8")), ["a", "b"]);
  assert.equal(isDocsSourcePath("LICENSE"), true);
  assert.equal(isDocsSourcePath("x.md"), true);
  assert.equal(isDocsSourcePath("x.rs"), false);
  assert.equal(isDocsSourcePath(".gitignore"), false);
});

test("pure: content manifest equality and plan actions", () => {
  const e1 = [
    { path: "LICENSE", mode: "100644", bytes: Buffer.from("L\n") },
    { path: "A.md", mode: "100644", bytes: Buffer.from("a\n") },
  ];
  const m1 = buildContentManifest(e1);
  const m2 = buildContentManifest(e1);
  assert.equal(diffContentManifests(m1, m2).equal, true);
  const m3 = buildContentManifest([
    ...e1,
    { path: "B.md", mode: "100644", bytes: Buffer.from("b\n") },
  ]);
  const d = diffContentManifests(m1, m3);
  assert.equal(d.equal, false);
  assert.deepEqual(d.extra, ["B.md"]);

  const sha = "a".repeat(40);
  const tree = "b".repeat(40);
  assert.equal(
    planSyncAction({
      sourceSha: sha,
      docsRemoteSha: null,
      docsRemoteSourceSha: null,
      docsRemoteTreeSha: null,
      expectedTreeSha: tree,
      docsExists: false,
    }).action,
    "bootstrap",
  );
  assert.equal(
    planSyncAction({
      sourceSha: sha,
      docsRemoteSha: "c".repeat(40),
      docsRemoteSourceSha: sha,
      docsRemoteTreeSha: tree,
      expectedTreeSha: tree,
      docsExists: true,
    }).action,
    "noop_idempotent",
  );
  assert.equal(
    planSyncAction({
      sourceSha: sha,
      docsRemoteSha: "c".repeat(40),
      docsRemoteSourceSha: "d".repeat(40),
      docsRemoteTreeSha: tree,
      expectedTreeSha: tree,
      docsExists: true,
    }).action,
    "append_receipt",
  );
  assert.equal(
    planSyncAction({
      sourceSha: sha,
      docsRemoteSha: "c".repeat(40),
      docsRemoteSourceSha: "d".repeat(40),
      docsRemoteTreeSha: "e".repeat(40),
      expectedTreeSha: tree,
      docsExists: true,
    }).action,
    "append_sync",
  );
});

test("pure: fixed docs gitignore ledger bytes", () => {
  assert.equal(DOCS_GITIGNORE_TEXT.endsWith("\n"), true);
  assert.ok(DOCS_GITIGNORE_TEXT.includes("/schemas/"));
  assert.ok(DOCS_GITIGNORE_TEXT.includes("/scripts/"));
  assert.equal(
    createHash("sha256").update(DOCS_GITIGNORE_BYTES).digest("hex"),
    createHash("sha256").update(Buffer.from(DOCS_GITIGNORE_TEXT, "utf8")).digest("hex"),
  );
});

test("self-test entrypoint passes", () => {
  const r = runSelfTest();
  assert.equal(r.ok, true);
});

test("pure: porcelain v2 -z rename parser consumes original path", () => {
  const record = "2 R. N... 100644 100644 100644 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb R100 new name.md";
  assert.deepEqual(parsePorcelainV2Z(Buffer.from(`${record}\0old name.md\0? loose.md\0`)), [
    { kind: "2", raw: record, path: "new name.md", originalPath: "old name.md" },
    { kind: "?", raw: "? loose.md", path: "loose.md" },
  ]);
});

test("manifest validator enforces path set, order, duplicate, LF count, and self entry", () => {
  const valid = makeValidManifestSnapshot({ "docs/B.md": "b\nline\n" });
  assert.equal(validateFileManifest(valid).pathCount, 3);

  const badPath = makeValidManifestSnapshot({ "docs/B.md": "b\n" });
  badPath.byPath.get("FILE_MANIFEST.md").bytes = Buffer.from(
    badPath.byPath.get("FILE_MANIFEST.md").bytes.toString("utf8").replace("docs/B.md", "docs/C.md"),
  );
  expectSyncError(() => validateFileManifest(badPath), ErrorCode.MANIFEST_MISMATCH);

  const badLines = makeValidManifestSnapshot();
  badLines.byPath.get("FILE_MANIFEST.md").bytes = Buffer.from(
    badLines.byPath.get("FILE_MANIFEST.md").bytes.toString("utf8").replace("A.md` — 1", "A.md` — 2"),
  );
  expectSyncError(() => validateFileManifest(badLines), ErrorCode.MANIFEST_MISMATCH);
});

test("GitPort strips repository/config redirection but preserves authentication transport", () => {
  const clean = sanitizeGitEnvironment(
    {
      PATH: process.env.PATH,
      GIT_DIR: "/wrong/repo",
      GIT_WORK_TREE: "/wrong/tree",
      GIT_COMMON_DIR: "/wrong/common",
      GIT_INDEX_FILE: "/wrong/index",
      GIT_OBJECT_DIRECTORY: "/wrong/objects",
      GIT_ALTERNATE_OBJECT_DIRECTORIES: "/wrong/alternates",
      GIT_CONFIG_GLOBAL: "/wrong/config",
      GIT_CONFIG_COUNT: "1",
      GIT_CONFIG_KEY_0: "core.hooksPath",
      GIT_CONFIG_VALUE_0: "/wrong/hooks",
      GIT_ASKPASS: "/credential/helper",
      GIT_SSH_COMMAND: "ssh -F /credential/config",
    },
  );
  for (const key of [
    "GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR", "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY", "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG_GLOBAL", "GIT_CONFIG_COUNT", "GIT_CONFIG_KEY_0", "GIT_CONFIG_VALUE_0",
  ]) assert.equal(clean[key], undefined, key);
  assert.equal(clean.GIT_ASKPASS, "/credential/helper");
  assert.equal(clean.GIT_SSH_COMMAND, "ssh -F /credential/config");

  const ctx = createFixture();
  try {
    const polluted = createGitPort({ env: { GIT_DIR: join(ctx.base, "missing") } });
    assert.equal(polluted.textTrim(ctx.sourceRoot, ["rev-parse", "--show-toplevel"]), ctx.sourceRoot);
  } finally {
    ctx.cleanup();
  }
});

test("manifest validator rejects a duplicate entry line", () => {
  const snapshot = makeValidManifestSnapshot();
  const manifest = snapshot.byPath.get("FILE_MANIFEST.md");
  manifest.bytes = Buffer.from(
    manifest.bytes.toString("utf8").replace("- `A.md` — 1 lines\n", "- `A.md` — 1 lines\n- `A.md` — 1 lines\n"),
  );
  expectSyncError(() => validateFileManifest(snapshot), ErrorCode.MANIFEST_MISMATCH);
});


test("push state machine validates parent, argv, and outcomes", () => {
  const parent = "a".repeat(40);
  const local = "b".repeat(40);
  const contract = { ...PRODUCTION_CONTRACT, docsRemoteName: "origin", docsBranch: "master" };

  const successDespiteStatus = makeScriptedGit({ parentLine: parent, pushStatus: 1, tips: [parent, local] });
  assert.deepEqual(pushDocsFastForward(successDespiteStatus, "/repo", local, parent, contract), {
    pushed: true,
    remoteSha: local,
  });
  const pushArgv = successDespiteStatus.calls.find((args) => args[0] === "push");
  assert.deepEqual(pushArgv, ["push", "origin", `${local}:refs/heads/master`]);

  const rejected = makeScriptedGit({ parentLine: parent, pushStatus: 1, tips: [parent, parent] });
  expectSyncError(
    () => pushDocsFastForward(rejected, "/repo", local, parent, contract),
    ErrorCode.DOCS_PUSH_REJECTED,
  );
  const mismatch = makeScriptedGit({ parentLine: parent, pushStatus: 0, tips: [parent, "c".repeat(40)] });
  expectSyncError(
    () => pushDocsFastForward(mismatch, "/repo", local, parent, contract),
    ErrorCode.DOCS_PUSH_UNKNOWN,
  );
  const queryFails = makeScriptedGit({ parentLine: parent, pushStatus: 0, tips: [parent], queryFailureAt: 2 });
  expectSyncError(
    () => pushDocsFastForward(queryFails, "/repo", local, parent, contract),
    ErrorCode.DOCS_PUSH_UNKNOWN,
  );
  const badParent = makeScriptedGit({ parentLine: "c".repeat(40), pushStatus: 0, tips: [] });
  expectSyncError(
    () => pushDocsFastForward(badParent, "/repo", local, parent, contract),
    ErrorCode.DOCS_REMOTE_DIVERGED,
  );
});

test("lock I/O and release failures use lock_io", () => {
  const ctx = createFixture();
  try {
    expectSyncError(
      () => acquireLock(ctx.lockPath, { openSync() { throw new Error("open failed"); } }),
      ErrorCode.LOCK_IO,
    );
    const lock = acquireLock(ctx.lockPath, {
      rmSync() { throw new Error("remove failed"); },
    });
    expectSyncError(() => lock.release(), ErrorCode.LOCK_IO);
    rmSync(ctx.lockPath, { force: true });
  } finally {
    ctx.cleanup();
  }
});

test("push bootstrap requires zero-parent commit", () => {
  const local = "b".repeat(40);
  const contract = { ...PRODUCTION_CONTRACT, docsRemoteName: "origin", docsBranch: "master" };
  const bootstrap = makeScriptedGit({ parentLine: "", pushStatus: 0, tips: [null, local] });
  assert.equal(pushDocsFastForward(bootstrap, "/repo", local, null, contract).remoteSha, local);
  const invalid = makeScriptedGit({ parentLine: "a".repeat(40), pushStatus: 0, tips: [] });
  expectSyncError(
    () => pushDocsFastForward(invalid, "/repo", local, null, contract),
    ErrorCode.DOCS_REMOTE_DIVERGED,
  );
});

test("stable preflight/parser errors cover source_not_repo, source_snapshot, and internal", () => {
  const ctx = createFixture();
  try {
    expectSyncError(
      () => inspectSourceRepository(git, ctx.contract, { sourceRoot: join(ctx.base, "missing") }),
      ErrorCode.SOURCE_NOT_REPO,
    );
    expectSyncError(
      () => parseLsTreeZ(Buffer.from("100644 blob deadbeef no-tab\0")),
      ErrorCode.SOURCE_SNAPSHOT,
    );
    expectSyncError(
      () => parsePorcelainV2Z(Buffer.from("x unknown\0")),
      ErrorCode.INTERNAL,
    );
  } finally {
    ctx.cleanup();
  }
});

// ---------------------------------------------------------------------------
// Source snapshot from Git objects (not dirty worktree)
// ---------------------------------------------------------------------------

test("source snapshot uses Git object bytes, ignores dirty worktree content", () => {
  const ctx = createFixture();
  try {
    // Dirty the worktree README without committing.
    write(join(ctx.sourceRoot, "README.md"), "# dirty worktree\n");
    // Snapshot must still use committed blob.
    // inspect would fail dirty; buildSourceSnapshot takes explicit SHA.
    const snap = buildSourceSnapshot(git, ctx.sourceRoot, ctx.sourceSha);
    const readme = snap.byPath.get("README.md");
    assert.ok(readme);
    assert.equal(readme.bytes.toString("utf8"), "# readme\n");
    assert.equal(snap.byPath.has("rust/x.rs"), false);
    assert.equal(snap.byPath.has("schemas/x.json"), false);
    assert.ok(snap.byPath.has("LICENSE"));
    const expected = buildExpectedDocsEntries(snap, ctx.contract);
    assert.ok(expected.some((e) => e.path === ".gitignore"));
    assert.ok(
      expected.find((e) => e.path === ".gitignore").bytes.equals(DOCS_GITIGNORE_BYTES),
    );
    // Materialize tree deterministic.
    ensureDocsRepoShell(git, ctx.contract, {
      docsRoot: ctx.docsRoot,
      docsRemoteUrl: ctx.docsBare,
    });
    const t1 = materializeDocsTree(git, ctx.docsRoot, expected).treeSha;
    const t2 = materializeDocsTree(git, ctx.docsRoot, expected).treeSha;
    assert.equal(t1, t2);
  } finally {
    ctx.cleanup();
  }
});

test("source snapshot rejects symlink blob mode", () => {
  const ctx = createFixture();
  try {
    // Create a symlink file and commit via plumbing-ish: git add symlink.
    gitOk(ctx.sourceRoot, ["rm", "-f", "README.md"]);
    // replace README with symlink
    spawnSync("ln", ["-s", "AGENT.md", "README.md"], { cwd: ctx.sourceRoot });
    // force add
    gitOk(ctx.sourceRoot, ["add", "-A"]);
    gitOk(ctx.sourceRoot, [
      "-c",
      `user.email=${EMAIL}`,
      "-c",
      `user.name=${NAME}`,
      "commit",
      "-m",
      "symlink",
    ]);
    const sha = gitText(ctx.sourceRoot, ["rev-parse", "HEAD"]);
    expectSyncError(
      () => buildSourceSnapshot(git, ctx.sourceRoot, sha),
      ErrorCode.SOURCE_PATH_REJECTED,
    );
  } finally {
    ctx.cleanup();
  }
});

test("source snapshot rejects gitlink even when its path ends in Markdown", () => {
  const ctx = createFixture();
  try {
    gitOk(ctx.sourceRoot, ["update-index", "--add", "--cacheinfo", `160000,${ctx.sourceSha},linked.md`]);
    gitOk(ctx.sourceRoot, [
      "-c", `user.email=${EMAIL}`,
      "-c", `user.name=${NAME}`,
      "commit", "-m", "gitlink markdown path",
    ]);
    const sha = gitText(ctx.sourceRoot, ["rev-parse", "HEAD"]);
    expectSyncError(() => buildSourceSnapshot(git, ctx.sourceRoot, sha), ErrorCode.SOURCE_PATH_REJECTED);
  } finally {
    ctx.cleanup();
  }
});

// ---------------------------------------------------------------------------
// Source preflight matrix
// ---------------------------------------------------------------------------

test("source preflight: dirty worktree fails source_dirty", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    write(join(ctx.sourceRoot, "EXTRA.md"), "untracked\n");
    expectSyncError(
      () =>
        inspectSourceRepository(git, ctx.contract, { sourceRoot: ctx.sourceRoot }),
      ErrorCode.SOURCE_DIRTY,
    );
    expectSyncError(
      () =>
        runCheck({
          contract: ctx.contract,
          sourceRoot: ctx.sourceRoot,
          docsRoot: ctx.docsRoot,
          sourceRemoteUrl: ctx.sourceBare,
          docsRemoteUrl: ctx.docsBare,
          skipLock: true,
        }),
      ErrorCode.SOURCE_DIRTY,
    );
  } finally {
    ctx.cleanup();
  }
});

test("source preflight: not pushed fails source_not_pushed", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    write(join(ctx.sourceRoot, "docs", "a.md"), "local only\n");
    commitAll(ctx.sourceRoot, "not pushed");
    // HEAD advanced, remote still old.
    expectSyncError(
      () =>
        inspectSourceRepository(git, ctx.contract, { sourceRoot: ctx.sourceRoot }),
      ErrorCode.SOURCE_NOT_PUSHED,
    );
  } finally {
    ctx.cleanup();
  }
});

test("source preflight: wrong branch fails", () => {
  const ctx = createFixture();
  try {
    gitOk(ctx.sourceRoot, ["checkout", "-b", "develop"]);
    // still need remote tracking facts — branch check fires first.
    expectSyncError(
      () =>
        inspectSourceRepository(git, ctx.contract, { sourceRoot: ctx.sourceRoot }),
      ErrorCode.SOURCE_WRONG_BRANCH,
    );
  } finally {
    ctx.cleanup();
  }
});

test("source preflight: wrong remote URL fails", () => {
  const ctx = createFixture();
  try {
    gitOk(ctx.sourceRoot, [
      "remote",
      "set-url",
      "origin",
      "https://example.com/wrong.git",
    ]);
    // Contract still expects bare path; override contract remote to production-like and set wrong.
    const contract = {
      ...ctx.contract,
      sourceRemoteUrl: "https://github.com/hongyue0721/shittim.git",
    };
    expectSyncError(
      () =>
        inspectSourceRepository(git, contract, { sourceRoot: ctx.sourceRoot }),
      ErrorCode.SOURCE_WRONG_REMOTE,
    );
  } finally {
    ctx.cleanup();
  }
});

test("source preflight: wrong local email fails", () => {
  const ctx = createFixture();
  try {
    gitOk(ctx.sourceRoot, ["config", "--local", "user.email", "other@example.com"]);
    expectSyncError(
      () =>
        inspectSourceRepository(git, ctx.contract, { sourceRoot: ctx.sourceRoot }),
      ErrorCode.SOURCE_IDENTITY,
    );
  } finally {
    ctx.cleanup();
  }
});

test("source preflight: wrong local name fails", () => {
  const ctx = createFixture();
  try {
    gitOk(ctx.sourceRoot, ["config", "--local", "user.name", "not-xiaoyue"]);
    expectSyncError(
      () =>
        inspectSourceRepository(git, ctx.contract, { sourceRoot: ctx.sourceRoot }),
      ErrorCode.SOURCE_IDENTITY,
    );
  } finally {
    ctx.cleanup();
  }
});

function rewriteSourceHeadIdentity(ctx, env) {
  // Fixture source may be a single root commit; rewrite HEAD without requiring a parent.
  const tree = gitText(ctx.sourceRoot, ["rev-parse", "HEAD^{tree}"]);
  const parents = gitText(ctx.sourceRoot, ["rev-list", "--parents", "-n", "1", "HEAD"])
    .split(/\s+/)
    .slice(1)
    .filter(Boolean);
  const message = gitText(ctx.sourceRoot, ["log", "-1", "--format=%B", "HEAD"]);
  const args = ["commit-tree", tree];
  for (const parent of parents) {
    args.push("-p", parent);
  }
  args.push("-m", message);
  const bad = gitText(ctx.sourceRoot, args, { env });
  gitOk(ctx.sourceRoot, ["update-ref", "HEAD", bad]);
  gitOk(ctx.sourceRoot, ["push", "--force", "origin", "HEAD:refs/heads/master"]);
  return bad;
}

test("source preflight: wrong HEAD author name fails", () => {
  const ctx = createFixture();
  try {
    rewriteSourceHeadIdentity(ctx, {
      GIT_AUTHOR_NAME: "wrong-author",
      GIT_AUTHOR_EMAIL: EMAIL,
      GIT_COMMITTER_NAME: NAME,
      GIT_COMMITTER_EMAIL: EMAIL,
    });
    expectSyncError(
      () =>
        inspectSourceRepository(git, ctx.contract, { sourceRoot: ctx.sourceRoot }),
      ErrorCode.SOURCE_IDENTITY,
    );
  } finally {
    ctx.cleanup();
  }
});

test("source preflight: wrong HEAD committer name fails", () => {
  const ctx = createFixture();
  try {
    rewriteSourceHeadIdentity(ctx, {
      GIT_AUTHOR_NAME: NAME,
      GIT_AUTHOR_EMAIL: EMAIL,
      GIT_COMMITTER_NAME: "wrong-committer",
      GIT_COMMITTER_EMAIL: EMAIL,
    });
    expectSyncError(
      () =>
        inspectSourceRepository(git, ctx.contract, { sourceRoot: ctx.sourceRoot }),
      ErrorCode.SOURCE_IDENTITY,
    );
  } finally {
    ctx.cleanup();
  }
});

// ---------------------------------------------------------------------------
// End-to-end sync matrix on bare remotes
// ---------------------------------------------------------------------------

test("sync bootstrap then idempotent check/sync", () => {
  const ctx = createFixture({ bootstrapDocs: false });
  try {
    // First sync bootstraps docs remote.
    const r1 = runSync({
      contract: ctx.contract,
      sourceRoot: ctx.sourceRoot,
      docsRoot: ctx.docsRoot,
      sourceRemoteUrl: ctx.sourceBare,
      docsRemoteUrl: ctx.docsBare,
      skipLock: true,
    });
    assert.equal(r1.ok, true);
    assert.equal(r1.action, "bootstrap");
    assert.equal(r1.pushed, true);
    assert.equal(r1.sourceSha, ctx.sourceSha);
    assert.equal(r1.local.mode, "initial_checkout");

    const remoteTip = gitText(ctx.docsBare, ["rev-parse", "refs/heads/master"]);
    assert.equal(remoteTip, r1.docsCommitSha);

    // Subject + identity (email and name).
    const subject = gitText(ctx.docsBare, ["log", "-1", "--format=%s", remoteTip]);
    assert.equal(subject, formatBootstrapCommitMessage(ctx.sourceSha));
    const ae = gitText(ctx.docsBare, ["log", "-1", "--format=%ae", remoteTip]);
    const an = gitText(ctx.docsBare, ["log", "-1", "--format=%an", remoteTip]);
    const ce = gitText(ctx.docsBare, ["log", "-1", "--format=%ce", remoteTip]);
    const cn = gitText(ctx.docsBare, ["log", "-1", "--format=%cn", remoteTip]);
    assert.equal(ae, EMAIL);
    assert.equal(an, NAME);
    assert.equal(ce, EMAIL);
    assert.equal(cn, NAME);

    // Tree closed set.
    const hist = loadDocsFirstParentHistory(git, ctx.docsRoot, remoteTip, ctx.contract);
    auditDocsHistory(git, ctx.docsRoot, ctx.sourceRoot, hist, ctx.contract);

    // Idempotent check.
    const c1 = runCheck({
      contract: ctx.contract,
      sourceRoot: ctx.sourceRoot,
      docsRoot: ctx.docsRoot,
      sourceRemoteUrl: ctx.sourceBare,
      docsRemoteUrl: ctx.docsBare,
      skipLock: true,
    });
    assert.equal(c1.inSync, true);
    assert.equal(c1.plan.action, "noop_idempotent");

    // Idempotent sync.
    const r2 = runSync({
      contract: ctx.contract,
      sourceRoot: ctx.sourceRoot,
      docsRoot: ctx.docsRoot,
      sourceRemoteUrl: ctx.sourceBare,
      docsRemoteUrl: ctx.docsBare,
      skipLock: true,
    });
    assert.equal(r2.action, "noop_idempotent");
    assert.equal(r2.local.mode, "already_at_tip");
    assert.equal(r2.docsCommitSha, remoteTip);
  } finally {
    ctx.cleanup();
  }
});

test("sync append on content change with linear history", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    const firstDocs = gitText(ctx.docsBare, ["rev-parse", "refs/heads/master"]);
    const newSha = appendSourceCommit(
      ctx,
      (root) => {
        write(join(root, "docs", "a.md"), "docs-a-v2\n");
        write(join(root, "docs", "b.md"), "new\n");
      },
      "docs update",
    );

    const r = runSync({
      contract: ctx.contract,
      sourceRoot: ctx.sourceRoot,
      docsRoot: ctx.docsRoot,
      sourceRemoteUrl: ctx.sourceBare,
      docsRemoteUrl: ctx.docsBare,
      skipLock: true,
    });
    assert.equal(r.action, "append_sync");
    assert.equal(r.local.mode, "ff_only");
    assert.equal(r.sourceSha, newSha);
    assert.equal(r.pushed, true);

    const tip = gitText(ctx.docsBare, ["rev-parse", "refs/heads/master"]);
    assert.equal(tip, r.docsCommitSha);
    const parent = gitText(ctx.docsBare, ["rev-parse", `${tip}^`]);
    assert.equal(parent, firstDocs);

    const subject = gitText(ctx.docsBare, ["log", "-1", "--format=%s", tip]);
    assert.equal(subject, formatSyncCommitMessage(newSha));

    // Ensure new file present and old content updated; no rust/schemas.
    const ls = gitText(ctx.docsBare, ["ls-tree", "-r", "--name-only", tip]);
    assert.ok(ls.includes("docs/b.md"));
    assert.ok(!ls.includes("rust/"));
    assert.ok(!ls.includes("schemas/"));
    assert.ok(ls.includes(".gitignore"));

    const hist = loadDocsFirstParentHistory(git, ctx.docsRoot, tip, ctx.contract);
    assert.equal(hist.length, 2);
    auditDocsHistory(git, ctx.docsRoot, ctx.sourceRoot, hist, ctx.contract);
  } finally {
    ctx.cleanup();
  }
});

test("sync append_receipt when tree unchanged but source SHA advanced", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    // Commit that does not change docs closed set (only rust/).
    const newSha = appendSourceCommit(
      ctx,
      (root) => {
        write(join(root, "rust", "x.rs"), "fn main(){ /* v2 */ }\n");
      },
      "rust only",
    );

    const beforeTree = gitText(ctx.docsBare, [
      "rev-parse",
      `${gitText(ctx.docsBare, ["rev-parse", "refs/heads/master"])}^{tree}`,
    ]);

    const r = runSync({
      contract: ctx.contract,
      sourceRoot: ctx.sourceRoot,
      docsRoot: ctx.docsRoot,
      sourceRemoteUrl: ctx.sourceBare,
      docsRemoteUrl: ctx.docsBare,
      skipLock: true,
    });
    assert.equal(r.action, "append_receipt");
    assert.equal(r.sourceSha, newSha);

    const tip = gitText(ctx.docsBare, ["rev-parse", "refs/heads/master"]);
    const afterTree = gitText(ctx.docsBare, ["rev-parse", `${tip}^{tree}`]);
    assert.equal(beforeTree, afterTree);
    assert.equal(
      gitText(ctx.docsBare, ["log", "-1", "--format=%s", tip]),
      formatSyncCommitMessage(newSha),
    );

    // History audit still holds: receipt tree matches new source closed set (same bytes).
    const hist = loadDocsFirstParentHistory(git, ctx.docsRoot, tip, ctx.contract);
    auditDocsHistory(git, ctx.docsRoot, ctx.sourceRoot, hist, ctx.contract);
  } finally {
    ctx.cleanup();
  }
});

test("remote race: pre-push divergence rejects without force", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    appendSourceCommit(
      ctx,
      (root) => write(join(root, "docs", "a.md"), "race-me\n"),
      "for race",
    );

    // Simulate concurrent remote update by pushing a different commit to docs bare
    // after we read plan but before push — inject via monkeying fetchDocs path:
    // We perform a manual competing commit on docs bare.
    const snap = buildSourceSnapshot(git, ctx.sourceRoot, ctx.sourceSha);
    // Use OLD source sha tree but different message parent — simpler: create empty-change fake by
    // committing same tree with a valid receipt for previous source after local plan starts.
    // Instead: directly update docs bare ref to a new sibling commit while runSync is about to push.
    // We'll call lower-level steps to assert pushDocsFastForward classification.

    ensureDocsRepoShell(git, ctx.contract, {
      docsRoot: ctx.docsRoot,
      docsRemoteUrl: ctx.docsBare,
    });
    const remoteBefore = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
    const entries = buildExpectedDocsEntries(snap, ctx.contract);
    const { treeSha } = materializeDocsTree(git, ctx.docsRoot, entries);
    const localCommit = commitTree(git, ctx.docsRoot, {
      treeSha,
      parentSha: remoteBefore,
      message: formatSyncCommitMessage(ctx.sourceSha, ctx.contract),
      email: EMAIL,
      name: NAME,
    });

    // Competing commit from same parent. Use a distinct tree so the SHA cannot collide
    // with localCommit even when timestamps match within the same second.
    const competitorEntries = [
      ...entries,
      {
        path: "docs/race-competitor.md",
        mode: "100644",
        bytes: Buffer.from("competitor-only\n"),
      },
    ];
    const { treeSha: competitorTree } = materializeDocsTree(
      git,
      ctx.docsRoot,
      competitorEntries,
    );
    const competitor = commitTree(git, ctx.docsRoot, {
      treeSha: competitorTree,
      parentSha: remoteBefore,
      message: formatSyncCommitMessage(ctx.sourceSha, ctx.contract),
      email: EMAIL,
      name: NAME,
    });
    assert.notEqual(competitor, localCommit);
    gitOk(ctx.docsRoot, ["push", "origin", `${competitor}:refs/heads/master`]);

    // Now push localCommit expecting parent remoteBefore — should fail remote diverged or push rejected.
    const err = expectSyncError(
      () =>
        pushDocsFastForward(
          git,
          ctx.docsRoot,
          localCommit,
          remoteBefore,
          ctx.contract,
        ),
      ErrorCode.DOCS_REMOTE_DIVERGED,
    );
    assert.equal(err.code, ErrorCode.DOCS_REMOTE_DIVERGED);

    // Bare tip is competitor, not force-overwritten by localCommit.
    const tip = gitText(ctx.docsBare, ["rev-parse", "refs/heads/master"]);
    assert.equal(tip, competitor);
    assert.notEqual(tip, localCommit);
  } finally {
    ctx.cleanup();
  }
});

test("lock held fails closed and does not auto-clear stale lock", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    // Create lock file exclusively.
    const lock = acquireLock(ctx.lockPath);
    try {
      expectSyncError(() => acquireLock(ctx.lockPath), ErrorCode.LOCK_HELD);
      expectSyncError(
        () =>
          runCheck({
            contract: ctx.contract,
            sourceRoot: ctx.sourceRoot,
            docsRoot: ctx.docsRoot,
            sourceRemoteUrl: ctx.sourceBare,
            docsRemoteUrl: ctx.docsBare,
            skipLock: false,
          }),
        ErrorCode.LOCK_HELD,
      );
    } finally {
      lock.release();
    }
    // After release, lock can be acquired again.
    const lock2 = acquireLock(ctx.lockPath);
    lock2.release();
  } finally {
    ctx.cleanup();
  }
});

test("docs history rejects an actual two-parent merge commit", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    ensureDocsRepoShell(git, ctx.contract, { docsRoot: ctx.docsRoot, docsRemoteUrl: ctx.docsBare });
    const tip = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
    const tree = gitText(ctx.docsRoot, ["rev-parse", `${tip}^{tree}`]);
    const side = commitTree(git, ctx.docsRoot, {
      treeSha: tree,
      parentSha: tip,
      message: formatSyncCommitMessage(ctx.sourceSha, ctx.contract),
      email: EMAIL,
      name: NAME,
    });
    const merge = gitText(ctx.docsRoot, [
      "commit-tree", tree,
      "-p", tip,
      "-p", side,
      "-m", formatSyncCommitMessage(ctx.sourceSha, ctx.contract),
    ], {
      env: {
        GIT_AUTHOR_NAME: NAME,
        GIT_AUTHOR_EMAIL: EMAIL,
        GIT_COMMITTER_NAME: NAME,
        GIT_COMMITTER_EMAIL: EMAIL,
      },
    });
    const history = loadDocsFirstParentHistory(git, ctx.docsRoot, merge, ctx.contract);
    expectSyncError(
      () => auditDocsHistory(git, ctx.docsRoot, ctx.sourceRoot, history, ctx.contract),
      ErrorCode.DOCS_HISTORY,
    );
  } finally {
    ctx.cleanup();
  }
});

test("docs history rejects bad subject marker", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    // Create a bad commit on docs and push.
    ensureDocsRepoShell(git, ctx.contract, {
      docsRoot: ctx.docsRoot,
      docsRemoteUrl: ctx.docsBare,
    });
    const tip = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
    const tree = gitText(ctx.docsRoot, ["rev-parse", `${tip}^{tree}`]);
    const bad = commitTree(git, ctx.docsRoot, {
      treeSha: tree,
      parentSha: tip,
      message: "bad subject without marker",
      email: EMAIL,
      name: NAME,
    });
    gitOk(ctx.docsRoot, ["push", "origin", `${bad}:refs/heads/master`]);

    expectSyncError(() => {
      const hist = loadDocsFirstParentHistory(git, ctx.docsRoot, bad, ctx.contract);
      auditDocsHistory(git, ctx.docsRoot, ctx.sourceRoot, hist, ctx.contract);
    }, ErrorCode.DOCS_HISTORY);
  } finally {
    ctx.cleanup();
  }
});

test("docs history rejects wrong committer email", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    ensureDocsRepoShell(git, ctx.contract, {
      docsRoot: ctx.docsRoot,
      docsRemoteUrl: ctx.docsBare,
    });
    const tip = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
    const tree = gitText(ctx.docsRoot, ["rev-parse", `${tip}^{tree}`]);
    const env = {
      GIT_AUTHOR_NAME: NAME,
      GIT_AUTHOR_EMAIL: EMAIL,
      GIT_COMMITTER_NAME: NAME,
      GIT_COMMITTER_EMAIL: "evil@example.com",
    };
    const bad = gitText(
      ctx.docsRoot,
      [
        "commit-tree",
        tree,
        "-p",
        tip,
        "-m",
        formatSyncCommitMessage(ctx.sourceSha, ctx.contract),
      ],
      { env },
    );
    gitOk(ctx.docsRoot, ["push", "origin", `${bad}:refs/heads/master`]);
    expectSyncError(() => {
      const hist = loadDocsFirstParentHistory(git, ctx.docsRoot, bad, ctx.contract);
      auditDocsHistory(git, ctx.docsRoot, ctx.sourceRoot, hist, ctx.contract);
    }, ErrorCode.DOCS_IDENTITY);
  } finally {
    ctx.cleanup();
  }
});

test("docs history rejects wrong author name", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    ensureDocsRepoShell(git, ctx.contract, {
      docsRoot: ctx.docsRoot,
      docsRemoteUrl: ctx.docsBare,
    });
    const tip = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
    const tree = gitText(ctx.docsRoot, ["rev-parse", `${tip}^{tree}`]);
    const env = {
      GIT_AUTHOR_NAME: "wrong-author",
      GIT_AUTHOR_EMAIL: EMAIL,
      GIT_COMMITTER_NAME: NAME,
      GIT_COMMITTER_EMAIL: EMAIL,
    };
    const bad = gitText(
      ctx.docsRoot,
      [
        "commit-tree",
        tree,
        "-p",
        tip,
        "-m",
        formatSyncCommitMessage(ctx.sourceSha, ctx.contract),
      ],
      { env },
    );
    gitOk(ctx.docsRoot, ["push", "origin", `${bad}:refs/heads/master`]);
    expectSyncError(() => {
      const hist = loadDocsFirstParentHistory(git, ctx.docsRoot, bad, ctx.contract);
      auditDocsHistory(git, ctx.docsRoot, ctx.sourceRoot, hist, ctx.contract);
    }, ErrorCode.DOCS_IDENTITY);
  } finally {
    ctx.cleanup();
  }
});

test("docs history rejects wrong committer name", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    ensureDocsRepoShell(git, ctx.contract, {
      docsRoot: ctx.docsRoot,
      docsRemoteUrl: ctx.docsBare,
    });
    const tip = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
    const tree = gitText(ctx.docsRoot, ["rev-parse", `${tip}^{tree}`]);
    const env = {
      GIT_AUTHOR_NAME: NAME,
      GIT_AUTHOR_EMAIL: EMAIL,
      GIT_COMMITTER_NAME: "wrong-committer",
      GIT_COMMITTER_EMAIL: EMAIL,
    };
    const bad = gitText(
      ctx.docsRoot,
      [
        "commit-tree",
        tree,
        "-p",
        tip,
        "-m",
        formatSyncCommitMessage(ctx.sourceSha, ctx.contract),
      ],
      { env },
    );
    gitOk(ctx.docsRoot, ["push", "origin", `${bad}:refs/heads/master`]);
    expectSyncError(() => {
      const hist = loadDocsFirstParentHistory(git, ctx.docsRoot, bad, ctx.contract);
      auditDocsHistory(git, ctx.docsRoot, ctx.sourceRoot, hist, ctx.contract);
    }, ErrorCode.DOCS_IDENTITY);
  } finally {
    ctx.cleanup();
  }
});

test("docs history rejects source marker on merge second parent", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    gitOk(ctx.sourceRoot, ["checkout", "-b", "side"]);
    write(join(ctx.sourceRoot, "docs", "side.md"), "side\n");
    regenerateFixtureManifest(ctx.sourceRoot);
    const sideSha = commitAll(ctx.sourceRoot, "side source");
    gitOk(ctx.sourceRoot, ["checkout", "master"]);
    write(join(ctx.sourceRoot, "docs", "main.md"), "main\n");
    regenerateFixtureManifest(ctx.sourceRoot);
    commitAll(ctx.sourceRoot, "main source");
    const merge = git.run(ctx.sourceRoot, ["merge", "--no-commit", "--no-ff", "side"], {
      allowNonZero: true,
    });
    assert.equal(merge.status, 1, "fixture intentionally conflicts only in manifest");
    gitOk(ctx.sourceRoot, ["checkout", "--ours", "FILE_MANIFEST.md"]);
    regenerateFixtureManifest(ctx.sourceRoot);
    commitAll(ctx.sourceRoot, "merge side");
    pushHead(ctx.sourceRoot);

    ensureDocsRepoShell(git, ctx.contract, { docsRoot: ctx.docsRoot, docsRemoteUrl: ctx.docsBare });
    const tip = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
    const sideSnapshot = buildSourceSnapshot(git, ctx.sourceRoot, sideSha);
    const { treeSha } = materializeDocsTree(git, ctx.docsRoot, buildExpectedDocsEntries(sideSnapshot, ctx.contract));
    const bad = commitTree(git, ctx.docsRoot, {
      treeSha,
      parentSha: tip,
      message: formatSyncCommitMessage(sideSha, ctx.contract),
      email: EMAIL,
      name: NAME,
    });
    const history = loadDocsFirstParentHistory(git, ctx.docsRoot, bad, ctx.contract);
    const index = buildSourceFirstParentIndex(git, ctx.sourceRoot, gitText(ctx.sourceRoot, ["rev-parse", "HEAD"]));
    expectSyncError(
      () => auditDocsHistory(git, ctx.docsRoot, ctx.sourceRoot, history, ctx.contract, index),
      ErrorCode.DOCS_HISTORY,
    );
  } finally {
    ctx.cleanup();
  }
});

test("docs history rejects source SHA not advancing", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    // Create second source commit and third, push all.
    const sha2 = appendSourceCommit(
      ctx,
      (root) => write(join(root, "docs", "a.md"), "v2\n"),
      "v2",
    );
    const sha3 = appendSourceCommit(
      ctx,
      (root) => write(join(root, "docs", "a.md"), "v3\n"),
      "v3",
    );

    // Bootstrap already has sha1. Manually append docs for sha3 then sha2 (regression).
    ensureDocsRepoShell(git, ctx.contract, {
      docsRoot: ctx.docsRoot,
      docsRemoteUrl: ctx.docsBare,
    });
    let tip = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);

    function appendDocsFor(sourceSha, parent) {
      const snap = buildSourceSnapshot(git, ctx.sourceRoot, sourceSha);
      const entries = buildExpectedDocsEntries(snap, ctx.contract);
      const { treeSha } = materializeDocsTree(git, ctx.docsRoot, entries);
      return commitTree(git, ctx.docsRoot, {
        treeSha,
        parentSha: parent,
        message: formatSyncCommitMessage(sourceSha, ctx.contract),
        email: EMAIL,
        name: NAME,
      });
    }

    const c3 = appendDocsFor(sha3, tip);
    const c2 = appendDocsFor(sha2, c3); // sha2 is ancestor of sha3 — not advancing
    gitOk(ctx.docsRoot, ["push", "origin", `${c2}:refs/heads/master`]);

    expectSyncError(() => {
      const hist = loadDocsFirstParentHistory(git, ctx.docsRoot, c2, ctx.contract);
      auditDocsHistory(git, ctx.docsRoot, ctx.sourceRoot, hist, ctx.contract);
    }, ErrorCode.DOCS_HISTORY);
  } finally {
    ctx.cleanup();
  }
});

test("docs history rejects tree that adds non-closed-set path", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    ensureDocsRepoShell(git, ctx.contract, {
      docsRoot: ctx.docsRoot,
      docsRemoteUrl: ctx.docsBare,
    });
    const tip = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
    const badSourceSha = appendSourceCommit(
      ctx,
      (root) => write(join(root, "docs", "a.md"), "tree-audit-source\n"),
      "tree audit source",
    );
    const snap = buildSourceSnapshot(git, ctx.sourceRoot, badSourceSha);
    const entries = [
      ...buildExpectedDocsEntries(snap, ctx.contract),
      { path: "rust/leak.rs", mode: "100644", bytes: Buffer.from("leak\n") },
    ];
    const { treeSha } = materializeDocsTree(git, ctx.docsRoot, entries);
    const bad = commitTree(git, ctx.docsRoot, {
      treeSha,
      parentSha: tip,
      message: formatSyncCommitMessage(badSourceSha, ctx.contract),
      email: EMAIL,
      name: NAME,
    });
    gitOk(ctx.docsRoot, ["push", "origin", `${bad}:refs/heads/master`]);
    // Production push remains plain fast-forward; this invalid commit is linear.
    expectSyncError(() => {
      const hist = loadDocsFirstParentHistory(git, ctx.docsRoot, bad, ctx.contract);
      auditDocsHistory(git, ctx.docsRoot, ctx.sourceRoot, hist, ctx.contract);
    }, ErrorCode.DOCS_TREE);
  } finally {
    ctx.cleanup();
  }
});

test("--check audits in temp repo and never mutates existing docs checkout", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    ensureDocsRepoShell(git, ctx.contract, { docsRoot: ctx.docsRoot, docsRemoteUrl: ctx.docsBare });
    fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
    const beforeGit = gitText(ctx.docsRoot, ["for-each-ref", "--format=%(refname) %(objectname)"]);
    const beforeConfig = readFileSync(join(ctx.docsRoot, ".git", "config"));
    const result = runCheck({
      contract: ctx.contract,
      sourceRoot: ctx.sourceRoot,
      docsRoot: ctx.docsRoot,
      sourceRemoteUrl: ctx.sourceBare,
      docsRemoteUrl: ctx.docsBare,
      skipLock: true,
    });
    assert.equal(result.inSync, true);
    assert.equal(gitText(ctx.docsRoot, ["for-each-ref", "--format=%(refname) %(objectname)"]), beforeGit);
    assert.ok(readFileSync(join(ctx.docsRoot, ".git", "config")).equals(beforeConfig));
    assert.deepEqual(
      existsSync(ctx.tempRoot) ? readdirSync(ctx.tempRoot) : [],
      [],
      "temporary check repository must be removed",
    );
  } finally {
    ctx.cleanup();
  }
});

test("existing docs path that is not a repo fails docs_not_repo", () => {
  const ctx = createFixture();
  try {
    mkdirSync(ctx.docsRoot, { recursive: true });
    write(join(ctx.docsRoot, "unknown.txt"), "keep\n");
    expectSyncError(
      () => inspectExistingDocsCheckout(git, ctx.docsRoot, ctx.contract, ctx.docsBare),
      ErrorCode.DOCS_NOT_REPO,
    );
  } finally {
    ctx.cleanup();
  }
});

test("docs checkout remote/branch gates and check plan code are reachable", () => {
  const ctx = createFixture();
  try {
    ensureDocsRepoShell(git, ctx.contract, { docsRoot: ctx.docsRoot, docsRemoteUrl: ctx.docsBare });
    expectSyncError(
      () => inspectExistingDocsCheckout(git, ctx.docsRoot, ctx.contract, join(ctx.base, "other.git")),
      ErrorCode.DOCS_WRONG_REMOTE,
    );
    expectSyncError(
      () => runCheck({
        contract: ctx.contract,
        sourceRoot: ctx.sourceRoot,
        docsRoot: join(ctx.base, "missing-docs-checkout"),
        sourceRemoteUrl: ctx.sourceBare,
        docsRemoteUrl: ctx.docsBare,
        skipLock: true,
      }),
      ErrorCode.PLAN,
    );

    const ctx2 = createFixture({ bootstrapDocs: true });
    try {
      ensureDocsRepoShell(git, ctx2.contract, { docsRoot: ctx2.docsRoot, docsRemoteUrl: ctx2.docsBare });
      const tip = fetchDocsRemoteTip(git, ctx2.docsRoot, ctx2.contract);
      finalizeLocalCheckout(git, ctx2.docsRoot, tip, ctx2.contract);
      gitOk(ctx2.docsRoot, ["checkout", "-b", "other"]);
      expectSyncError(
        () => finalizeLocalCheckout(git, ctx2.docsRoot, tip, ctx2.contract),
        ErrorCode.DOCS_WRONG_BRANCH,
      );
    } finally {
      ctx2.cleanup();
    }
  } finally {
    ctx.cleanup();
  }
});

test("local checkout finalizer refuses tracked edit and untracked collision", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    ensureDocsRepoShell(git, ctx.contract, { docsRoot: ctx.docsRoot, docsRemoteUrl: ctx.docsBare });
    const tip = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
    finalizeLocalCheckout(git, ctx.docsRoot, tip, ctx.contract);
    write(join(ctx.docsRoot, "README.md"), "tracked edit\n");
    expectSyncError(() => finalizeLocalCheckout(git, ctx.docsRoot, tip, ctx.contract), ErrorCode.DOCS_CHECKOUT);

    gitOk(ctx.docsRoot, ["checkout", "--", "README.md"]);
    const newSha = appendSourceCommit(ctx, (root) => write(join(root, "docs", "collision.md"), "remote\n"));
    const snapshot = buildSourceSnapshot(git, ctx.sourceRoot, newSha);
    const { treeSha } = materializeDocsTree(git, ctx.docsRoot, buildExpectedDocsEntries(snapshot, ctx.contract));
    const advanced = commitTree(git, ctx.docsRoot, {
      treeSha,
      parentSha: tip,
      message: formatSyncCommitMessage(newSha, ctx.contract),
      email: EMAIL,
      name: NAME,
    });
    write(join(ctx.docsRoot, "docs", "collision.md"), "local untracked\n");
    expectSyncError(() => finalizeLocalCheckout(git, ctx.docsRoot, advanced, ctx.contract), ErrorCode.DOCS_CHECKOUT);
  } finally {
    ctx.cleanup();
  }
});

test("local checkout finalizer is ff-only and refuses non-ancestor", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    // Local docs at bootstrap.
    ensureDocsRepoShell(git, ctx.contract, {
      docsRoot: ctx.docsRoot,
      docsRemoteUrl: ctx.docsBare,
    });
    const tip = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
    gitOk(ctx.docsRoot, ["fetch", "origin", "master"]);
    gitOk(ctx.docsRoot, ["checkout", "-B", "master", tip]);

    // Advance remote with new commit.
    const newSha = appendSourceCommit(
      ctx,
      (root) => write(join(root, "README.md"), "# v2\n"),
      "readme v2",
    );
    const snap = buildSourceSnapshot(git, ctx.sourceRoot, newSha);
    const entries = buildExpectedDocsEntries(snap, ctx.contract);
    const { treeSha } = materializeDocsTree(git, ctx.docsRoot, entries);
    const advanced = commitTree(git, ctx.docsRoot, {
      treeSha,
      parentSha: tip,
      message: formatSyncCommitMessage(newSha, ctx.contract),
      email: EMAIL,
      name: NAME,
    });
    gitOk(ctx.docsRoot, ["push", "origin", `${advanced}:refs/heads/master`]);

    const r = finalizeLocalCheckout(git, ctx.docsRoot, advanced, ctx.contract);
    assert.equal(r.mode, "ff_only");
    assert.equal(gitText(ctx.docsRoot, ["rev-parse", "HEAD"]), advanced);

    // Create divergent local commit not ancestor of a fake target.
    write(join(ctx.docsRoot, "README.md"), "# local diverge\n");
    gitOk(ctx.docsRoot, ["add", "README.md"]);
    gitOk(ctx.docsRoot, [
      "-c",
      `user.email=${EMAIL}`,
      "-c",
      `user.name=${NAME}`,
      "commit",
      "-m",
      formatSyncCommitMessage(newSha, ctx.contract),
    ]);
    const diverged = gitText(ctx.docsRoot, ["rev-parse", "HEAD"]);
    // Target is old tip (not descendant of diverged).
    expectSyncError(
      () => finalizeLocalCheckout(git, ctx.docsRoot, tip, ctx.contract),
      ErrorCode.DOCS_CHECKOUT,
    );
    assert.equal(gitText(ctx.docsRoot, ["rev-parse", "HEAD"]), diverged);
  } finally {
    ctx.cleanup();
  }
});

test("local checkout transaction recovers after every operation boundary", () => {
  const boundaries = [
    "journal_written",
    "worktree_materialized",
    "branch_ref_updated",
    "head_symbolic",
    "remote_tracking_updated",
    "journal_cleared",
  ];
  for (const boundary of boundaries) {
    const ctx = createFixture({ bootstrapDocs: true });
    try {
      ensureDocsRepoShell(git, ctx.contract, { docsRoot: ctx.docsRoot, docsRemoteUrl: ctx.docsBare });
      const oldTip = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
      finalizeLocalCheckout(git, ctx.docsRoot, oldTip, ctx.contract);
      const newSha = appendSourceCommit(ctx, (root) => write(join(root, "docs", "tx.md"), "transaction\n"));
      const snapshot = buildSourceSnapshot(git, ctx.sourceRoot, newSha);
      const { treeSha } = materializeDocsTree(git, ctx.docsRoot, buildExpectedDocsEntries(snapshot, ctx.contract));
      const target = commitTree(git, ctx.docsRoot, {
        treeSha,
        parentSha: oldTip,
        message: formatSyncCommitMessage(newSha, ctx.contract),
        email: EMAIL,
        name: NAME,
      });
      let injected = false;
      assert.throws(
        () => finalizeLocalCheckout(git, ctx.docsRoot, target, ctx.contract, {
          operationPort: {
            after(name) {
              if (!injected && name === boundary) {
                injected = true;
                throw new Error(`injected:${name}`);
              }
            },
          },
        }),
        new RegExp(`injected:${boundary}`),
      );
      const recovered = finalizeLocalCheckout(git, ctx.docsRoot, target, ctx.contract);
      assert.equal(recovered.sha, target);
      assert.equal(gitText(ctx.docsRoot, ["rev-parse", "HEAD"]), target);
      assert.equal(gitText(ctx.docsRoot, ["rev-parse", "refs/remotes/origin/master"]), target);
      assert.equal(existsSync(join(ctx.docsRoot, ".git", "docs-checkout-transaction.json")), false);
    } finally {
      ctx.cleanup();
    }
  }
});

function createCheckoutTarget(ctx, relativePath = "README.md", content = "# target\n") {
  ensureDocsRepoShell(git, ctx.contract, {
    docsRoot: ctx.docsRoot,
    docsRemoteUrl: ctx.docsBare,
  });
  const oldTip = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
  finalizeLocalCheckout(git, ctx.docsRoot, oldTip, ctx.contract);
  const sourceSha = appendSourceCommit(ctx, (root) => write(join(root, relativePath), content));
  const snapshot = buildSourceSnapshot(git, ctx.sourceRoot, sourceSha);
  const { treeSha } = materializeDocsTree(
    git,
    ctx.docsRoot,
    buildExpectedDocsEntries(snapshot, ctx.contract),
  );
  const target = commitTree(git, ctx.docsRoot, {
    treeSha,
    parentSha: oldTip,
    message: formatSyncCommitMessage(sourceSha, ctx.contract),
    email: EMAIL,
    name: NAME,
  });
  return { oldTip, sourceSha, target };
}

function readCheckoutJournal(ctx) {
  return JSON.parse(
    readFileSync(join(ctx.docsRoot, ".git", "docs-checkout-transaction.json"), "utf8"),
  );
}

function writeCheckoutJournal(ctx, journal) {
  write(
    join(ctx.docsRoot, ".git", "docs-checkout-transaction.json"),
    `${JSON.stringify(journal)}\n`,
  );
}

function expectRecoveryPreservesBytes(ctx, target, path, expectedBytes) {
  expectSyncError(
    () => finalizeLocalCheckout(git, ctx.docsRoot, target, ctx.contract),
    ErrorCode.DOCS_CHECKOUT_RECOVERY,
  );
  assert.deepEqual(readFileSync(join(ctx.docsRoot, path)), Buffer.from(expectedBytes));
}

test("invalid checkout journal reaches docs_checkout_recovery", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    ensureDocsRepoShell(git, ctx.contract, { docsRoot: ctx.docsRoot, docsRemoteUrl: ctx.docsBare });
    const tip = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
    finalizeLocalCheckout(git, ctx.docsRoot, tip, ctx.contract);
    write(join(ctx.docsRoot, ".git", "docs-checkout-transaction.json"), "not-json\n");
    expectSyncError(
      () => finalizeLocalCheckout(git, ctx.docsRoot, tip, ctx.contract),
      ErrorCode.DOCS_CHECKOUT_RECOVERY,
    );
  } finally {
    ctx.cleanup();
  }
});

test("prepared checkout journal with pending different target fails recovery before mutation", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    const { target } = createCheckoutTarget(ctx);
    assert.throws(
      () => finalizeLocalCheckout(git, ctx.docsRoot, target, ctx.contract, {
        operationPort: { after(name) { if (name === "journal_written") throw new Error("crash"); } },
      }),
      /crash/,
    );
    const otherTarget = "f".repeat(40);
    const bytesBefore = readFileSync(join(ctx.docsRoot, "README.md"));
    expectSyncError(
      () => finalizeLocalCheckout(git, ctx.docsRoot, otherTarget, ctx.contract),
      ErrorCode.DOCS_CHECKOUT_RECOVERY,
    );
    assert.deepEqual(readFileSync(join(ctx.docsRoot, "README.md")), bytesBefore);
    assert.equal(readCheckoutJournal(ctx).targetSha, target);
  } finally {
    ctx.cleanup();
  }
});

test("checkout journal conflicting with current HEAD fails recovery without overwriting bytes", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    const { target } = createCheckoutTarget(ctx);
    assert.throws(
      () => finalizeLocalCheckout(git, ctx.docsRoot, target, ctx.contract, {
        operationPort: { after(name) { if (name === "journal_written") throw new Error("crash"); } },
      }),
      /crash/,
    );
    const journal = readCheckoutJournal(ctx);
    const conflictingHead = commitTree(git, ctx.docsRoot, {
      treeSha: gitText(ctx.docsRoot, ["rev-parse", `${journal.oldHead}^{tree}`]),
      parentSha: journal.oldHead,
      message: "conflicting local head",
      email: EMAIL,
      name: NAME,
    });
    gitOk(ctx.docsRoot, ["update-ref", "refs/heads/master", conflictingHead]);
    write(join(ctx.docsRoot, "README.md"), "conflict-owned bytes\n");
    expectRecoveryPreservesBytes(ctx, target, "README.md", "conflict-owned bytes\n");
  } finally {
    ctx.cleanup();
  }
});

test("materialized and ref_updated checkout journals reject later user edits and preserve exact bytes", () => {
  for (const boundary of ["worktree_materialized", "branch_ref_updated"]) {
    const ctx = createFixture({ bootstrapDocs: true });
    try {
      const { target } = createCheckoutTarget(ctx);
      assert.throws(
        () => finalizeLocalCheckout(git, ctx.docsRoot, target, ctx.contract, {
          operationPort: { after(name) { if (name === boundary) throw new Error(`crash:${name}`); } },
        }),
        new RegExp(`crash:${boundary}`),
      );
      const journal = readCheckoutJournal(ctx);
      assert.equal(
        journal.phase,
        boundary === "worktree_materialized" ? "materialized" : "ref_updated",
      );
      const userBytes = Buffer.from([0x75, 0x73, 0x65, 0x72, 0x00, 0xff, 0x0a]);
      writeFileSync(join(ctx.docsRoot, "README.md"), userBytes);
      expectRecoveryPreservesBytes(ctx, target, "README.md", userBytes);
    } finally {
      ctx.cleanup();
    }
  }
});

test("checkout tree drift after materialization but before branch update fails recovery", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    const { target } = createCheckoutTarget(ctx);
    const userBytes = Buffer.from("completion-boundary user bytes\n");
    expectSyncError(
      () => finalizeLocalCheckout(git, ctx.docsRoot, target, ctx.contract, {
        operationPort: {
          after(name) {
            if (name === "worktree_materialized") {
              writeFileSync(join(ctx.docsRoot, "README.md"), userBytes);
            }
          },
        },
      }),
      ErrorCode.DOCS_CHECKOUT_RECOVERY,
    );
    assert.deepEqual(readFileSync(join(ctx.docsRoot, "README.md")), userBytes);
    assert.equal(gitText(ctx.docsRoot, ["rev-parse", "HEAD"]), readCheckoutJournal(ctx).oldHead);
  } finally {
    ctx.cleanup();
  }
});

test("initial checkout journal with a partial tracked index fails docs_checkout_recovery without overwriting bytes", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    ensureDocsRepoShell(git, ctx.contract, {
      docsRoot: ctx.docsRoot,
      docsRemoteUrl: ctx.docsBare,
    });
    const target = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
    write(join(ctx.docsRoot, "README.md"), "partial tracked bytes\n");
    gitOk(ctx.docsRoot, ["add", "README.md"]);
    writeCheckoutJournal(ctx, {
      version: 1,
      phase: "prepared",
      oldHead: null,
      targetSha: target,
    });
    expectRecoveryPreservesBytes(ctx, target, "README.md", "partial tracked bytes\n");
  } finally {
    ctx.cleanup();
  }
});

test("prepared checkout with a new tracked user edit fails docs_checkout and preserves user bytes", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    ensureDocsRepoShell(git, ctx.contract, { docsRoot: ctx.docsRoot, docsRemoteUrl: ctx.docsBare });
    const oldTip = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
    finalizeLocalCheckout(git, ctx.docsRoot, oldTip, ctx.contract);
    const newSha = appendSourceCommit(ctx, (root) => write(join(root, "README.md"), "# target\n"));
    const snapshot = buildSourceSnapshot(git, ctx.sourceRoot, newSha);
    const { treeSha } = materializeDocsTree(git, ctx.docsRoot, buildExpectedDocsEntries(snapshot, ctx.contract));
    const target = commitTree(git, ctx.docsRoot, {
      treeSha,
      parentSha: oldTip,
      message: formatSyncCommitMessage(newSha, ctx.contract),
      email: EMAIL,
      name: NAME,
    });
    assert.throws(
      () => finalizeLocalCheckout(git, ctx.docsRoot, target, ctx.contract, {
        operationPort: { after(name) { if (name === "journal_written") throw new Error("crash"); } },
      }),
      /crash/,
    );
    write(join(ctx.docsRoot, "README.md"), "user-owned bytes\n");
    expectSyncError(
      () => finalizeLocalCheckout(git, ctx.docsRoot, target, ctx.contract),
      ErrorCode.DOCS_CHECKOUT,
    );
    assert.equal(readFileSync(join(ctx.docsRoot, "README.md"), "utf8"), "user-owned bytes\n");
  } finally {
    ctx.cleanup();
  }
});


function createUnknownOutcomeGitPort({ pushRemote = true } = {}) {
  const real = createGitPort();
  let queryAfterPush = false;
  return {
    ...real,
    run(cwd, args, options = {}) {
      if (args[0] === "push" && args[1] === "origin") {
        const result = pushRemote
          ? real.run(cwd, args, options)
          : {
              status: 1,
              stdout: Buffer.alloc(0),
              stderr: Buffer.from("injected transport uncertainty"),
              args,
              cwd,
            };
        queryAfterPush = true;
        return result;
      }
      if (queryAfterPush && args[0] === "ls-remote") {
        queryAfterPush = false;
        return {
          status: 1,
          stdout: Buffer.alloc(0),
          stderr: Buffer.from("injected remote query failure"),
          args,
          cwd,
        };
      }
      return real.run(cwd, args, options);
    },
  };
}

function createRejectingPushGitPort() {
  const real = createGitPort();
  return {
    ...real,
    run(cwd, args, options = {}) {
      if (args[0] === "push" && args[1] === "origin") {
        return {
          status: 1,
          stdout: Buffer.alloc(0),
          stderr: Buffer.from("injected rejection"),
          args,
          cwd,
        };
      }
      return real.run(cwd, args, options);
    },
  };
}

test("runSync preserves unknown staging and next run recovers the same commit without a parallel commit", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    appendSourceCommit(ctx, (root) => write(join(root, "docs", "unknown.md"), "unknown\n"));
    const unknownGit = createUnknownOutcomeGitPort();
    expectSyncError(
      () => runSync({
        contract: ctx.contract,
        sourceRoot: ctx.sourceRoot,
        docsRoot: ctx.docsRoot,
        sourceRemoteUrl: ctx.sourceBare,
        docsRemoteUrl: ctx.docsBare,
        skipLock: true,
        git: unknownGit,
      }),
      ErrorCode.DOCS_PUSH_UNKNOWN,
    );
    const staged = gitText(ctx.docsRoot, ["rev-parse", DOCS_STAGING_REF]);
    const commitCountAfterUnknown = Number(gitText(ctx.docsRoot, ["rev-list", "--all", "--count"]));
    const recovered = runSync({
      contract: ctx.contract,
      sourceRoot: ctx.sourceRoot,
      docsRoot: ctx.docsRoot,
      sourceRemoteUrl: ctx.sourceBare,
      docsRemoteUrl: ctx.docsBare,
      skipLock: true,
    });
    assert.equal(recovered.action, "recover_staging");
    assert.equal(recovered.docsCommitSha, staged);
    assert.equal(recovered.recovery.disposition, "remote_landed");
    assert.equal(Number(gitText(ctx.docsRoot, ["rev-list", "--all", "--count"])), commitCountAfterUnknown);
    assert.notEqual(
      git.run(ctx.docsRoot, ["rev-parse", "--verify", DOCS_STAGING_REF], { allowNonZero: true }).status,
      0,
    );
  } finally {
    ctx.cleanup();
  }
});

test("runSync unknown before confirmed remote update resumes the same staged commit on the next run", () => {
  const ctx = createFixture({ bootstrapDocs: true });
  try {
    appendSourceCommit(ctx, (root) => write(join(root, "docs", "resume.md"), "resume\n"));
    const unknownGit = createUnknownOutcomeGitPort({ pushRemote: false });
    expectSyncError(
      () => runSync({
        contract: ctx.contract,
        sourceRoot: ctx.sourceRoot,
        docsRoot: ctx.docsRoot,
        sourceRemoteUrl: ctx.sourceBare,
        docsRemoteUrl: ctx.docsBare,
        skipLock: true,
        git: unknownGit,
      }),
      ErrorCode.DOCS_PUSH_UNKNOWN,
    );
    const staged = gitText(ctx.docsRoot, ["rev-parse", DOCS_STAGING_REF]);
    const parent = gitText(ctx.docsRoot, ["rev-parse", `${staged}^`]);
    assert.equal(gitText(ctx.docsBare, ["rev-parse", "refs/heads/master"]), parent);
    const recovered = runSync({
      contract: ctx.contract,
      sourceRoot: ctx.sourceRoot,
      docsRoot: ctx.docsRoot,
      sourceRemoteUrl: ctx.sourceBare,
      docsRemoteUrl: ctx.docsBare,
      skipLock: true,
    });
    assert.equal(recovered.action, "recover_staging");
    assert.equal(recovered.recovery.disposition, "resumed_push");
    assert.equal(recovered.docsCommitSha, staged);
    assert.equal(gitText(ctx.docsBare, ["rev-parse", "refs/heads/master"]), staged);
  } finally {
    ctx.cleanup();
  }
});

test("runSync rejected push clears staging while successful push also leaves no staging ref", () => {
  const rejected = createFixture({ bootstrapDocs: true });
  try {
    appendSourceCommit(rejected, (root) => write(join(root, "docs", "rejected.md"), "rejected\n"));
    expectSyncError(
      () => runSync({
        contract: rejected.contract,
        sourceRoot: rejected.sourceRoot,
        docsRoot: rejected.docsRoot,
        sourceRemoteUrl: rejected.sourceBare,
        docsRemoteUrl: rejected.docsBare,
        skipLock: true,
        git: createRejectingPushGitPort(),
      }),
      ErrorCode.DOCS_PUSH_REJECTED,
    );
    assert.notEqual(
      git.run(rejected.docsRoot, ["rev-parse", "--verify", DOCS_STAGING_REF], { allowNonZero: true }).status,
      0,
    );
  } finally {
    rejected.cleanup();
  }

  const successful = createFixture({ bootstrapDocs: true });
  try {
    appendSourceCommit(successful, (root) => write(join(root, "docs", "success.md"), "success\n"));
    const result = runSync({
      contract: successful.contract,
      sourceRoot: successful.sourceRoot,
      docsRoot: successful.docsRoot,
      sourceRemoteUrl: successful.sourceBare,
      docsRemoteUrl: successful.docsBare,
      skipLock: true,
    });
    assert.equal(result.pushed, true);
    assert.notEqual(
      git.run(successful.docsRoot, ["rev-parse", "--verify", DOCS_STAGING_REF], { allowNonZero: true }).status,
      0,
    );
  } finally {
    successful.cleanup();
  }
});

test("docs staging recovery resumes push, finalizes landed remote, and fails closed on invalid/diverged", () => {
  function stageNext(ctx) {
    ensureDocsRepoShell(git, ctx.contract, { docsRoot: ctx.docsRoot, docsRemoteUrl: ctx.docsBare });
    const parent = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
    const snapshot = buildSourceSnapshot(git, ctx.sourceRoot, ctx.sourceSha);
    const entries = buildExpectedDocsEntries(snapshot, ctx.contract);
    const { treeSha } = materializeDocsTree(git, ctx.docsRoot, entries);
    const staged = commitTree(git, ctx.docsRoot, {
      treeSha,
      parentSha: parent,
      message: formatSyncCommitMessage(ctx.sourceSha, ctx.contract),
      email: EMAIL,
      name: NAME,
    });
    gitOk(ctx.docsRoot, ["update-ref", DOCS_STAGING_REF, staged]);
    return { parent, staged, entries };
  }

  const resumed = createFixture({ bootstrapDocs: true });
  try {
    appendSourceCommit(resumed, (root) => write(join(root, "docs", "recover.md"), "recover\n"));
    const staged = stageNext(resumed);
    const result = recoverDocsStaging(git, resumed.docsRoot, resumed.sourceRoot, resumed.contract, staged.entries);
    assert.equal(result.disposition, "resumed_push");
    assert.equal(gitText(resumed.docsBare, ["rev-parse", "refs/heads/master"]), staged.staged);
    assert.notEqual(git.run(resumed.docsRoot, ["rev-parse", "--verify", DOCS_STAGING_REF], { allowNonZero: true }).status, 0);
  } finally { resumed.cleanup(); }

  const landed = createFixture({ bootstrapDocs: true });
  try {
    appendSourceCommit(landed, (root) => write(join(root, "docs", "landed.md"), "landed\n"));
    const staged = stageNext(landed);
    gitOk(landed.docsRoot, ["push", "origin", `${staged.staged}:refs/heads/master`]);
    const result = recoverDocsStaging(git, landed.docsRoot, landed.sourceRoot, landed.contract, staged.entries);
    assert.equal(result.disposition, "remote_landed");
  } finally { landed.cleanup(); }

  const invalid = createFixture({ bootstrapDocs: true });
  try {
    appendSourceCommit(invalid, (root) => write(join(root, "docs", "invalid.md"), "invalid\n"));
    const staged = stageNext(invalid);
    const bad = commitTree(git, invalid.docsRoot, {
      treeSha: gitText(invalid.docsRoot, ["rev-parse", `${staged.staged}^{tree}`]),
      parentSha: staged.parent,
      message: "invalid staging marker",
      email: EMAIL,
      name: NAME,
    });
    gitOk(invalid.docsRoot, ["update-ref", DOCS_STAGING_REF, bad]);
    expectSyncError(
      () => recoverDocsStaging(git, invalid.docsRoot, invalid.sourceRoot, invalid.contract, staged.entries),
      ErrorCode.DOCS_STAGING_INVALID,
    );
  } finally { invalid.cleanup(); }

  const diverged = createFixture({ bootstrapDocs: true });
  try {
    appendSourceCommit(diverged, (root) => write(join(root, "docs", "diverged.md"), "diverged\n"));
    const staged = stageNext(diverged);
    const competitor = commitTree(git, diverged.docsRoot, {
      treeSha: gitText(diverged.docsRoot, ["rev-parse", `${staged.parent}^{tree}`]),
      parentSha: staged.parent,
      message: formatSyncCommitMessage(diverged.sourceSha, diverged.contract),
      email: EMAIL,
      name: NAME,
    });
    gitOk(diverged.docsRoot, ["push", "origin", `${competitor}:refs/heads/master`]);
    expectSyncError(
      () => recoverDocsStaging(git, diverged.docsRoot, diverged.sourceRoot, diverged.contract, staged.entries),
      ErrorCode.DOCS_STAGING_DIVERGED,
    );
  } finally { diverged.cleanup(); }
});

test("docs staging recovery identity field mismatches raise docs_identity without mutating remote or staging", () => {
  const cases = [
    {
      label: "author email",
      env: {
        GIT_AUTHOR_NAME: NAME,
        GIT_AUTHOR_EMAIL: "evil-author@example.com",
        GIT_COMMITTER_NAME: NAME,
        GIT_COMMITTER_EMAIL: EMAIL,
      },
      detailKey: "authorEmail",
      detailValue: "evil-author@example.com",
    },
    {
      label: "author name",
      env: {
        GIT_AUTHOR_NAME: "wrong-author",
        GIT_AUTHOR_EMAIL: EMAIL,
        GIT_COMMITTER_NAME: NAME,
        GIT_COMMITTER_EMAIL: EMAIL,
      },
      detailKey: "authorName",
      detailValue: "wrong-author",
    },
    {
      label: "committer email",
      env: {
        GIT_AUTHOR_NAME: NAME,
        GIT_AUTHOR_EMAIL: EMAIL,
        GIT_COMMITTER_NAME: NAME,
        GIT_COMMITTER_EMAIL: "evil-committer@example.com",
      },
      detailKey: "committerEmail",
      detailValue: "evil-committer@example.com",
    },
    {
      label: "committer name",
      env: {
        GIT_AUTHOR_NAME: NAME,
        GIT_AUTHOR_EMAIL: EMAIL,
        GIT_COMMITTER_NAME: "wrong-committer",
        GIT_COMMITTER_EMAIL: EMAIL,
      },
      detailKey: "committerName",
      detailValue: "wrong-committer",
    },
  ];

  for (const tc of cases) {
    const ctx = createFixture({ bootstrapDocs: true });
    try {
      appendSourceCommit(ctx, (root) =>
        write(join(root, "docs", `staging-identity-${tc.label.replace(/\s+/g, "-")}.md`), `${tc.label}\n`),
      );
      ensureDocsRepoShell(git, ctx.contract, {
        docsRoot: ctx.docsRoot,
        docsRemoteUrl: ctx.docsBare,
      });
      const parent = fetchDocsRemoteTip(git, ctx.docsRoot, ctx.contract);
      const remoteBefore = gitText(ctx.docsBare, ["rev-parse", "refs/heads/master"]);
      assert.equal(remoteBefore, parent);
      const snapshot = buildSourceSnapshot(git, ctx.sourceRoot, ctx.sourceSha);
      const entries = buildExpectedDocsEntries(snapshot, ctx.contract);
      const { treeSha } = materializeDocsTree(git, ctx.docsRoot, entries);
      const args = [
        "commit-tree",
        treeSha,
        "-p",
        parent,
        "-m",
        formatSyncCommitMessage(ctx.sourceSha, ctx.contract),
      ];
      const bad = gitText(ctx.docsRoot, args, { env: tc.env });
      gitOk(ctx.docsRoot, ["update-ref", DOCS_STAGING_REF, bad]);

      const err = expectSyncError(
        () => recoverDocsStaging(git, ctx.docsRoot, ctx.sourceRoot, ctx.contract, entries),
        ErrorCode.DOCS_IDENTITY,
      );
      assert.equal(err.details.stagingSha, bad);
      assert.equal(err.details[tc.detailKey], tc.detailValue);
      // Only the mismatched field is exposed; other identity / tree payloads stay out.
      for (const key of ["authorEmail", "authorName", "committerEmail", "committerName"]) {
        if (key !== tc.detailKey) {
          assert.equal(Object.hasOwn(err.details, key), false, `${tc.label}: leaked ${key}`);
        }
      }
      assert.equal(Object.hasOwn(err.details, "treeDiff"), false, `${tc.label}: leaked treeDiff`);
      assert.equal(Object.hasOwn(err.details, "parents"), false, `${tc.label}: leaked parents`);
      assert.equal(Object.hasOwn(err.details, "marker"), false, `${tc.label}: leaked marker`);

      assert.equal(
        gitText(ctx.docsBare, ["rev-parse", "refs/heads/master"]),
        remoteBefore,
        `${tc.label}: remote must not change`,
      );
      assert.equal(
        gitText(ctx.docsRoot, ["rev-parse", DOCS_STAGING_REF]),
        bad,
        `${tc.label}: staging ref must remain`,
      );
    } finally {
      ctx.cleanup();
    }
  }
});

test("production script never force-pushes: push args lack force flags", () => {
  const src = [
    readFileSync(join(HERE, "sync-docs-repository.mjs"), "utf8"),
    readFileSync(join(HERE, "docs-checkout-transaction.mjs"), "utf8"),
  ].join("\n");
  assert.equal(src.includes("force-with-lease"), false);
  assert.equal(src.includes("--force"), false);
  assert.ok(src.includes('sanitized.GIT_TERMINAL_PROMPT = "0"'));
  assert.ok(src.includes("commit-tree"));
  assert.ok(src.includes("merge-base"));
  assert.ok(src.includes("read-tree"));
});

test("CLI --self-test and usage error codes", () => {
  const node = process.execPath;
  const script = join(HERE, "sync-docs-repository.mjs");
  const self = spawnSync(node, [script, "--self-test"], {
    encoding: "utf8",
    env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
  });
  assert.equal(self.status, 0, self.stderr);
  assert.ok(self.stdout.includes('"ok":true'));

  const usage = spawnSync(node, [script], {
    encoding: "utf8",
    env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
  });
  assert.equal(usage.status, 1);
  assert.ok(usage.stderr.includes(ErrorCode.USAGE));

  const extraArg = spawnSync(node, [script, "--self-test", "extra"], {
    encoding: "utf8",
    env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
  });
  assert.equal(extraArg.status, 1);
  assert.ok(extraArg.stderr.includes(ErrorCode.USAGE));
  const usageLines = extraArg.stderr.split("\n").filter((line) => line.includes("usage:"));
  assert.equal(usageLines.length, 2);
  assert.ok(usageLines[0].startsWith('{"ok":false'));
  assert.ok(usageLines[1].startsWith("sync-docs-repository: usage:"));

  const unknown = spawnSync(node, [script, "--unknown"], {
    encoding: "utf8",
    env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
  });
  assert.equal(unknown.status, 1);
  assert.ok(unknown.stderr.includes(ErrorCode.USAGE));
});

test("CLI state tests use an imported fixture seam while production entry stays fixed", () => {
  const script = join(HERE, "sync-docs-repository.mjs");
  const moduleUrl = new URL("./sync-docs-repository.mjs", import.meta.url).href;
  const program = [
    `import { main } from ${JSON.stringify(moduleUrl)};`,
    "const code = await main(['node', 'sync-docs-repository.mjs', '--check'], {",
    "  selfTest() { throw new Error('wrong handler'); },",
    "  check() { return { mode: 'fixture_check', contract: 'injected_module_api' }; },",
    "  sync() { throw new Error('wrong handler'); },",
    "});",
    "process.exit(code);",
  ].join("\n");
  const r = spawnSync(process.execPath, ["--input-type=module", "--eval", program], {
    encoding: "utf8",
    cwd: REPO_ROOT,
    env: {
      ...process.env,
      GIT_DIR: join(TEST_ROOT_BASE, "must-not-redirect"),
      GIT_WORK_TREE: join(TEST_ROOT_BASE, "must-not-redirect-tree"),
    },
  });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.ok(r.stdout.includes('"mode":"fixture_check"'), r.stdout);
  assert.ok(r.stdout.includes('"contract":"injected_module_api"'), r.stdout);

  const source = readFileSync(script, "utf8");
  assert.ok(source.includes("main(process.argv).then("));
  assert.equal(source.includes("main(process.argv, "), false);
});

test("parseLsTreeZ roundtrip on real tree", () => {
  const ctx = createFixture();
  try {
    const buf = git.run(ctx.sourceRoot, ["ls-tree", "-r", "-z", "HEAD"]).stdout;
    const recs = parseLsTreeZ(buf);
    assert.ok(recs.some((r) => r.path === "README.md"));
    assert.ok(recs.every((r) => r.sha.length === 40));
  } finally {
    ctx.cleanup();
  }
});

test("manifest rejects mode/byte drift", () => {
  const a = buildContentManifest([
    { path: "LICENSE", mode: "100644", bytes: Buffer.from("x\n") },
  ]);
  const b = buildContentManifest([
    { path: "LICENSE", mode: "100755", bytes: Buffer.from("x\n") },
  ]);
  const d = diffContentManifests(a, b);
  assert.deepEqual(d.changed, ["LICENSE"]);
});
