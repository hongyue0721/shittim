#!/usr/bin/env node
/**
 * Sync the pure-documentation mirror from an already-pushed main-repo commit.
 *
 * Production contract (fixed):
 * - Source authority: /mnt/data/companion_architecture_v3 → github.com/hongyue0721/shittim.git (master)
 * - Docs checkout:    /mnt/data/shittim-docs-export → github.com/hongyue0721/shittim-docs.git (master)
 * - Closed docs set: tracked *.md + LICENSE from the source Git object at HEAD, plus fixed docs .gitignore
 * - Never commits/pushes the main repo; never force-pushes docs; never invents remotes/auth.
 *
 * CLI:
 *   node scripts/sync-docs-repository.mjs --check
 *   node scripts/sync-docs-repository.mjs --sync
 *   node scripts/sync-docs-repository.mjs --self-test
 *
 * Exit: 0 on success; 1 on structured SyncError (printed as JSON line + human message).
 */
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  closeSync,
  constants as fsConstants,
  existsSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createHash } from "node:crypto";
import {
  docsWorktreeMatchesCommit as checkoutMatchesCommit,
  finalizeLocalCheckoutState,
} from "./docs-checkout-transaction.mjs";

// ---------------------------------------------------------------------------
// ProductionContract
// ---------------------------------------------------------------------------

/** Fixed docs-only .gitignore bytes (LF, no BOM). History-compatible ledger. */
export const DOCS_GITIGNORE_TEXT = [
  "# Documentation-only mirror: implementation and build outputs are forbidden.",
  "/rust/",
  "/schemas/",
  "/scripts/",
  "/ts/",
  "/node_modules/",
  "target/",
  "dist/",
  ".env",
  ".env.*",
  "*.pem",
  "*.key",
  ".schema-tool-generate.lock",
  ".idea/",
  ".vscode/",
  ".DS_Store",
  "",
].join("\n");

export const DOCS_GITIGNORE_BYTES = Buffer.from(DOCS_GITIGNORE_TEXT, "utf8");

export const PRODUCTION_CONTRACT = Object.freeze({
  sourceRepoRoot: "/mnt/data/companion_architecture_v3",
  sourceRemoteName: "origin",
  sourceRemoteUrl: "https://github.com/hongyue0721/shittim.git",
  sourceBranch: "master",
  docsCheckoutRoot: "/mnt/data/shittim-docs-export",
  docsRemoteName: "origin",
  docsRemoteUrl: "https://github.com/hongyue0721/shittim-docs.git",
  docsBranch: "master",
  requiredEmail: "2933634892@qq.com",
  requiredName: "小岳",
  lockPath: "/mnt/data/shittim-docs-repository.sync.lock",
  tempRoot: "/mnt/data/shittim-docs-sync-work",
  docsGitignoreRel: ".gitignore",
  docsGitignoreBytes: DOCS_GITIGNORE_BYTES,
  syncMessagePrefix: "文档: 同步shittim@",
  bootstrapMessagePrefix: "文档: 从shittim@",
  bootstrapMessageSuffix: "建立纯文档镜像",
  fullShaLength: 40,
});

/** Stable machine-readable error codes (fail-closed). */
export const ErrorCode = Object.freeze({
  USAGE: "usage",
  LOCK_HELD: "lock_held",
  LOCK_IO: "lock_io",
  SOURCE_NOT_REPO: "source_not_repo",
  SOURCE_WRONG_BRANCH: "source_wrong_branch",
  SOURCE_WRONG_REMOTE: "source_wrong_remote",
  SOURCE_DIRTY: "source_dirty",
  SOURCE_NOT_PUSHED: "source_not_pushed",
  /** Source local config + HEAD author/committer email and name. */
  SOURCE_IDENTITY: "source_identity",
  SOURCE_SNAPSHOT: "source_snapshot",
  SOURCE_PATH_REJECTED: "source_path_rejected",
  MANIFEST_MISMATCH: "manifest_mismatch",
  DOCS_NOT_REPO: "docs_not_repo",
  DOCS_WRONG_REMOTE: "docs_wrong_remote",
  DOCS_WRONG_BRANCH: "docs_wrong_branch",
  DOCS_HISTORY: "docs_history",
  /** Docs commit author/committer email and name (history + recovery metadata). */
  DOCS_IDENTITY: "docs_identity",
  DOCS_TREE: "docs_tree",
  DOCS_REMOTE_DIVERGED: "docs_remote_diverged",
  DOCS_PUSH_REJECTED: "docs_push_rejected",
  DOCS_PUSH_UNKNOWN: "docs_push_unknown",
  DOCS_STAGING_INVALID: "docs_staging_invalid",
  DOCS_STAGING_DIVERGED: "docs_staging_diverged",
  DOCS_CHECKOUT: "docs_checkout",
  DOCS_CHECKOUT_RECOVERY: "docs_checkout_recovery",
  PLAN: "plan",
  INTERNAL: "internal",
});

// ---------------------------------------------------------------------------
// SyncError
// ---------------------------------------------------------------------------

export class SyncError extends Error {
  /**
   * @param {string} code
   * @param {string} message
   * @param {Record<string, unknown>} [details]
   */
  constructor(code, message, details = {}) {
    super(message);
    this.name = "SyncError";
    this.code = code;
    this.details = details;
  }

  toJSON() {
    return {
      ok: false,
      code: this.code,
      message: this.message,
      details: this.details,
    };
  }
}

export function fail(code, message, details) {
  throw new SyncError(code, message, details);
}

// ---------------------------------------------------------------------------
// UTF-8 / path helpers (strict, fail-closed)
// ---------------------------------------------------------------------------

const strictUtf8Decoder = new TextDecoder("utf-8", { fatal: true });

/**
 * Decode Git -z path bytes as strict UTF-8 with roundtrip check.
 * @param {Buffer} pathBytes
 */
export function decodeGitPathBytes(pathBytes) {
  if (!Buffer.isBuffer(pathBytes)) {
    throw new Error("path segment must be a Buffer");
  }
  let text;
  try {
    text = strictUtf8Decoder.decode(pathBytes);
  } catch {
    throw new Error(`path is not valid UTF-8: ${pathBytes.toString("hex")}`);
  }
  const reencoded = Buffer.from(text, "utf8");
  if (reencoded.length !== pathBytes.length || !reencoded.equals(pathBytes)) {
    throw new Error(
      `path UTF-8 roundtrip mismatch: ${pathBytes.toString("hex")}`,
    );
  }
  if (text.includes("\0")) {
    throw new Error("path contains NUL");
  }
  return text;
}

/**
 * Split a Buffer on NUL (0x00), decoding each non-empty segment.
 * Empty segments (consecutive NULs) are dropped — for path lists only.
 * Trailing NUL is optional; incomplete trailing segment is accepted.
 * @param {Buffer} buf
 * @returns {string[]}
 */
export function splitNulPaths(buf) {
  if (!Buffer.isBuffer(buf)) {
    throw new Error("splitNulPaths expects Buffer");
  }
  const out = [];
  let start = 0;
  for (let i = 0; i < buf.length; i += 1) {
    if (buf[i] === 0) {
      if (i > start) {
        out.push(decodeGitPathBytes(buf.subarray(start, i)));
      }
      start = i + 1;
    }
  }
  if (start < buf.length) {
    out.push(decodeGitPathBytes(buf.subarray(start)));
  }
  return out;
}

/**
 * Split a Buffer on NUL preserving empty fields (for structured records).
 * A trailing NUL produces no extra empty field beyond the final delimiter.
 * @param {Buffer} buf
 * @returns {string[]}
 */
export function splitNulFields(buf) {
  if (!Buffer.isBuffer(buf)) {
    throw new Error("splitNulFields expects Buffer");
  }
  const out = [];
  let start = 0;
  for (let i = 0; i < buf.length; i += 1) {
    if (buf[i] === 0) {
      out.push(decodeGitPathBytes(buf.subarray(start, i)));
      start = i + 1;
    }
  }
  if (start < buf.length) {
    out.push(decodeGitPathBytes(buf.subarray(start)));
  }
  return out;
}

/**
 * Parse `git status --porcelain=v2 -z` without treating rename source paths as
 * independent records. Paths remain opaque UTF-8 strings, including spaces.
 * @param {Buffer} buf
 * @returns {{ kind: string, raw: string, path: string, originalPath?: string }[]}
 */
export function parsePorcelainV2Z(buf) {
  const fields = splitNulFields(buf);
  const records = [];
  for (let i = 0; i < fields.length; i += 1) {
    const raw = fields[i];
    if (raw === "") continue;
    const kind = raw[0];
    if (kind === "#") continue;
    if (kind === "?" || kind === "!") {
      if (raw.length < 3 || raw[1] !== " ") {
        fail(ErrorCode.INTERNAL, "malformed porcelain v2 untracked record", { raw });
      }
      records.push({ kind, raw, path: raw.slice(2) });
      continue;
    }
    if (kind !== "1" && kind !== "2" && kind !== "u") {
      fail(ErrorCode.INTERNAL, "unknown porcelain v2 record kind", { raw });
    }
    const fixedFields = kind === "1" ? 8 : kind === "2" ? 9 : 10;
    const parts = raw.split(" ");
    if (parts.length < fixedFields + 1) {
      fail(ErrorCode.INTERNAL, "malformed porcelain v2 tracked record", { raw });
    }
    const path = parts.slice(fixedFields).join(" ");
    const record = { kind, raw, path };
    if (kind === "2") {
      if (i + 1 >= fields.length || fields[i + 1] === "") {
        fail(ErrorCode.INTERNAL, "porcelain v2 rename lacks original path", { raw });
      }
      record.originalPath = fields[i + 1];
      i += 1;
    }
    records.push(record);
  }
  return records;
}

/**
 * Validate a repo-relative path for the docs closed set.
 * Rejects absolute, empty, `.`/`..`, backslash, drive, symlink-like, and control bytes.
 * @param {string} relPath
 */
export function assertSafeRepoRelativePath(relPath) {
  if (typeof relPath !== "string" || relPath.length === 0) {
    fail(ErrorCode.SOURCE_PATH_REJECTED, "empty path rejected", { relPath });
  }
  if (relPath.includes("\0")) {
    fail(ErrorCode.SOURCE_PATH_REJECTED, "NUL in path", { relPath });
  }
  if (relPath.startsWith("/") || relPath.startsWith("\\")) {
    fail(ErrorCode.SOURCE_PATH_REJECTED, "absolute path rejected", { relPath });
  }
  if (relPath.includes("\\") || relPath.includes(":")) {
    fail(ErrorCode.SOURCE_PATH_REJECTED, "illegal path separator/drive", {
      relPath,
    });
  }
  const parts = relPath.split("/");
  for (const part of parts) {
    if (part === "" || part === "." || part === "..") {
      fail(ErrorCode.SOURCE_PATH_REJECTED, "dot/empty path component", {
        relPath,
      });
    }
  }
  return relPath;
}

/** Compare paths as raw UTF-8 bytes (stable Git order). */
export function compareGitPath(a, b) {
  const ba = Buffer.from(a, "utf8");
  const bb = Buffer.from(b, "utf8");
  const n = Math.min(ba.length, bb.length);
  for (let i = 0; i < n; i += 1) {
    if (ba[i] !== bb[i]) return ba[i] - bb[i];
  }
  return ba.length - bb.length;
}

export function isFullSha(value) {
  return typeof value === "string" && /^[0-9a-f]{40}$/.test(value);
}

export function requireFullSha(value, label) {
  if (!isFullSha(value)) {
    fail(ErrorCode.INTERNAL, `${label} must be full lowercase SHA-1`, {
      value,
    });
  }
  return value;
}

// ---------------------------------------------------------------------------
// Commit message contract (pure)
// ---------------------------------------------------------------------------

/**
 * Parse a docs commit subject for source SHA.
 * Compatible with:
 *   文档: 同步shittim@<fullSHA>
 *   文档: 从shittim@<fullSHA>建立纯文档镜像
 * @param {string} subject
 * @returns {{ kind: "sync"|"bootstrap", sourceSha: string } | null}
 */
export function parseDocsCommitSubject(subject, contract = PRODUCTION_CONTRACT) {
  if (typeof subject !== "string") return null;
  const syncPrefix = contract.syncMessagePrefix;
  const bootPrefix = contract.bootstrapMessagePrefix;
  const bootSuffix = contract.bootstrapMessageSuffix;

  if (subject.startsWith(syncPrefix)) {
    const sha = subject.slice(syncPrefix.length);
    if (isFullSha(sha) && subject === `${syncPrefix}${sha}`) {
      return { kind: "sync", sourceSha: sha };
    }
    return null;
  }
  if (subject.startsWith(bootPrefix) && subject.endsWith(bootSuffix)) {
    const mid = subject.slice(
      bootPrefix.length,
      subject.length - bootSuffix.length,
    );
    if (isFullSha(mid) && subject === `${bootPrefix}${mid}${bootSuffix}`) {
      return { kind: "bootstrap", sourceSha: mid };
    }
    return null;
  }
  return null;
}

export function formatSyncCommitMessage(sourceSha, contract = PRODUCTION_CONTRACT) {
  requireFullSha(sourceSha, "sourceSha");
  return `${contract.syncMessagePrefix}${sourceSha}`;
}

export function formatBootstrapCommitMessage(
  sourceSha,
  contract = PRODUCTION_CONTRACT,
) {
  requireFullSha(sourceSha, "sourceSha");
  return `${contract.bootstrapMessagePrefix}${sourceSha}${contract.bootstrapMessageSuffix}`;
}

// ---------------------------------------------------------------------------
// GitPort — side-effect boundary
// ---------------------------------------------------------------------------

/**
 * @typedef {object} GitResult
 * @property {number} status
 * @property {Buffer} stdout
 * @property {Buffer} stderr
 * @property {string[]} args
 * @property {string} cwd
 */

// Git variables that can redirect repository discovery, object/index storage,
// refs, hooks, executables, or configuration. Authentication transports such
// as GIT_ASKPASS and GIT_SSH_COMMAND are deliberately preserved.
const REDIRECTING_GIT_ENV_EXACT = new Set([
  "GIT_DIR",
  "GIT_WORK_TREE",
  "GIT_COMMON_DIR",
  "GIT_INDEX_FILE",
  "GIT_OBJECT_DIRECTORY",
  "GIT_ALTERNATE_OBJECT_DIRECTORIES",
  "GIT_CONFIG",
  "GIT_CONFIG_SYSTEM",
  "GIT_CONFIG_GLOBAL",
  "GIT_CONFIG_NOSYSTEM",
  "GIT_CONFIG_PARAMETERS",
  "GIT_CONFIG_COUNT",
  "GIT_NAMESPACE",
  "GIT_QUARANTINE_PATH",
  "GIT_REPLACE_REF_BASE",
  "GIT_GRAFT_FILE",
  "GIT_SHALLOW_FILE",
  "GIT_CEILING_DIRECTORIES",
  "GIT_DISCOVERY_ACROSS_FILESYSTEM",
  "GIT_EXEC_PATH",
  "GIT_TEMPLATE_DIR",
  "GIT_ATTR_NOSYSTEM",
]);

function isRedirectingGitEnvironmentKey(key) {
  return (
    REDIRECTING_GIT_ENV_EXACT.has(key) ||
    /^GIT_CONFIG_(?:KEY|VALUE)_\d+$/.test(key)
  );
}

/** Remove ambient Git redirection/configuration while retaining auth transport. */
export function sanitizeGitEnvironment(...sources) {
  const sanitized = {};
  for (const source of sources) {
    if (!source) continue;
    for (const [key, value] of Object.entries(source)) {
      if (!isRedirectingGitEnvironmentKey(key) && value !== undefined) {
        sanitized[key] = value;
      }
    }
  }
  sanitized.GIT_TERMINAL_PROMPT = "0";
  sanitized.GIT_CONFIG_NOSYSTEM = "1";
  sanitized.LC_ALL = "C";
  sanitized.LANG = "C";
  return sanitized;
}

/**
 * Create a Git command runner bound to env safety defaults.
 * @param {{ env?: NodeJS.ProcessEnv, gitBin?: string }} [opts]
 */
export function createGitPort(opts = {}) {
  const gitBin = opts.gitBin || "git";
  const baseEnv = sanitizeGitEnvironment(process.env, opts.env);

  /**
   * @param {string} cwd
   * @param {string[]} args
   * @param {{ input?: Buffer|string, env?: NodeJS.ProcessEnv, allowNonZero?: boolean, maxBuffer?: number }} [runOpts]
   * @returns {GitResult}
   */
  function run(cwd, args, runOpts = {}) {
    const env = sanitizeGitEnvironment(baseEnv, runOpts.env);
    // Leave `encoding` unset so stdout/stderr are Buffers. String `input` must be
    // converted first: Node rejects encoding "buffer" when coercing string input.
    let input = runOpts.input;
    if (typeof input === "string") {
      input = Buffer.from(input, "utf8");
    }
    const result = spawnSync(gitBin, args, {
      cwd,
      env,
      input,
      maxBuffer: runOpts.maxBuffer ?? 64 * 1024 * 1024,
      stdio: ["pipe", "pipe", "pipe"],
    });
    if (result.error) {
      fail(ErrorCode.INTERNAL, `failed to spawn git: ${result.error.message}`, {
        args,
        cwd,
      });
    }
    const status = result.status ?? 1;
    const stdout = Buffer.isBuffer(result.stdout)
      ? result.stdout
      : Buffer.alloc(0);
    const stderr = Buffer.isBuffer(result.stderr)
      ? result.stderr
      : Buffer.alloc(0);
    if (status !== 0 && !runOpts.allowNonZero) {
      fail(
        ErrorCode.INTERNAL,
        `git ${args.join(" ")} failed (status ${status}): ${stderr.toString("utf8").trim() || stdout.toString("utf8").trim()}`,
        {
          args,
          cwd,
          status,
          stderr: stderr.toString("utf8"),
        },
      );
    }
    return { status, stdout, stderr, args, cwd };
  }

  /** @param {string} cwd @param {string[]} args */
  function text(cwd, args, runOpts = {}) {
    return run(cwd, args, runOpts).stdout.toString("utf8");
  }

  /** @param {string} cwd @param {string[]} args */
  function textTrim(cwd, args, runOpts = {}) {
    return text(cwd, args, runOpts).trim();
  }

  return { run, text, textTrim, gitBin, baseEnv };
}

// ---------------------------------------------------------------------------
// Exclusive lock (no stale auto-clear)
// ---------------------------------------------------------------------------

/**
 * Acquire an exclusive lock file under /mnt/data.
 * Does not clear stale locks: concurrent/crashed holders must be handled manually.
 * @param {string} lockPath
 * @returns {{ release: () => void }}
 */
export function acquireLock(lockPath, fsOps = {}) {
  const io = {
    mkdirSync,
    openSync,
    writeFileSync,
    closeSync,
    rmSync,
    ...fsOps,
  };
  const dir = dirname(lockPath);
  try {
    io.mkdirSync(dir, { recursive: true });
  } catch (error) {
    fail(ErrorCode.LOCK_IO, `cannot create lock parent: ${error.message}`, {
      lockPath,
    });
  }
  let fd;
  try {
    fd = io.openSync(
      lockPath,
      fsConstants.O_CREAT | fsConstants.O_EXCL | fsConstants.O_WRONLY,
      0o600,
    );
  } catch (error) {
    if (error && error.code === "EEXIST") {
      fail(ErrorCode.LOCK_HELD, `sync lock already held: ${lockPath}`, {
        lockPath,
      });
    }
    fail(ErrorCode.LOCK_IO, `cannot create lock: ${error.message}`, {
      lockPath,
    });
  }
  try {
    io.writeFileSync(
      fd,
      `pid=${process.pid}\nstarted_at=${new Date().toISOString()}\n`,
      "utf8",
    );
  } catch (error) {
    try {
      io.closeSync(fd);
    } catch {
      // The primary write error remains authoritative here.
    }
    try {
      io.rmSync(lockPath, { force: true });
    } catch {
      // The primary write error remains authoritative here.
    }
    fail(ErrorCode.LOCK_IO, `cannot write lock: ${error.message}`, {
      lockPath,
    });
  }
  let released = false;
  return {
    release() {
      if (released) return;
      released = true;
      const errors = [];
      try {
        io.closeSync(fd);
      } catch (error) {
        errors.push(`close: ${error.message}`);
      }
      try {
        io.rmSync(lockPath, { force: true });
      } catch (error) {
        errors.push(`remove: ${error.message}`);
      }
      if (errors.length > 0) {
        fail(ErrorCode.LOCK_IO, "cannot release sync lock", {
          lockPath,
          errors,
        });
      }
    },
  };
}

// ---------------------------------------------------------------------------
// SourceSnapshot — pure structure + builders over GitPort
// ---------------------------------------------------------------------------

/**
 * @typedef {object} SnapshotEntry
 * @property {string} path
 * @property {string} mode  // "100644" | "100755"
 * @property {string} blobSha
 * @property {Buffer} bytes
 */

/**
 * @typedef {object} SourceSnapshot
 * @property {string} sourceSha
 * @property {string} sourceTreeSha
 * @property {SnapshotEntry[]} entries  // closed set excluding .gitignore
 * @property {Map<string, SnapshotEntry>} byPath
 */

/**
 * Decide whether a tracked path belongs to the docs closed source set.
 * @param {string} path
 */
export function isDocsSourcePath(path) {
  if (path === "LICENSE") return true;
  if (path.endsWith(".md")) return true;
  return false;
}

/**
 * Parse `git ls-tree -r -z` records: MODE SP TYPE SP SHA \t PATH \0
 * @param {Buffer} buf
 * @returns {{ mode: string, type: string, sha: string, path: string }[]}
 */
export function parseLsTreeZ(buf) {
  const records = [];
  let start = 0;
  for (let i = 0; i < buf.length; i += 1) {
    if (buf[i] !== 0) continue;
    if (i === start) {
      start = i + 1;
      continue;
    }
    const rec = buf.subarray(start, i);
    const tab = rec.indexOf(0x09);
    if (tab < 0) {
      fail(ErrorCode.SOURCE_SNAPSHOT, "ls-tree record missing tab", {});
    }
    const meta = rec.subarray(0, tab).toString("utf8");
    const pathBytes = rec.subarray(tab + 1);
    const parts = meta.split(" ");
    if (parts.length !== 3) {
      fail(ErrorCode.SOURCE_SNAPSHOT, `ls-tree meta malformed: ${meta}`, {});
    }
    const [mode, type, sha] = parts;
    let path;
    try {
      path = decodeGitPathBytes(pathBytes);
    } catch (error) {
      fail(ErrorCode.SOURCE_PATH_REJECTED, error.message, {});
    }
    records.push({ mode, type, sha, path });
    start = i + 1;
  }
  if (start < buf.length) {
    fail(ErrorCode.SOURCE_SNAPSHOT, "ls-tree output not NUL-terminated", {});
  }
  return records;
}

/**
 * Build a closed SourceSnapshot from a source commit via Git objects only.
 * @param {ReturnType<typeof createGitPort>} git
 * @param {string} sourceRoot
 * @param {string} sourceSha
 * @returns {SourceSnapshot}
 */
export function buildSourceSnapshot(git, sourceRoot, sourceSha) {
  requireFullSha(sourceSha, "sourceSha");
  const treeSha = git.textTrim(sourceRoot, ["rev-parse", `${sourceSha}^{tree}`]);
  requireFullSha(treeSha, "sourceTreeSha");

  const ls = git.run(sourceRoot, ["ls-tree", "-r", "-z", sourceSha]);
  const records = parseLsTreeZ(ls.stdout);
  /** @type {SnapshotEntry[]} */
  const entries = [];
  const byPath = new Map();

  for (const rec of records) {
    if (!isDocsSourcePath(rec.path)) continue;
    assertSafeRepoRelativePath(rec.path);

    if (rec.type === "commit") {
      fail(ErrorCode.SOURCE_PATH_REJECTED, "submodule/gitlink rejected", {
        path: rec.path,
        mode: rec.mode,
      });
    }
    if (rec.type !== "blob") {
      fail(ErrorCode.SOURCE_PATH_REJECTED, `non-blob entry rejected: ${rec.type}`, {
        path: rec.path,
        mode: rec.mode,
        type: rec.type,
      });
    }
    if (rec.mode === "120000") {
      fail(ErrorCode.SOURCE_PATH_REJECTED, "symlink rejected", {
        path: rec.path,
      });
    }
    if (rec.mode !== "100644" && rec.mode !== "100755") {
      fail(ErrorCode.SOURCE_PATH_REJECTED, `unsupported file mode ${rec.mode}`, {
        path: rec.path,
        mode: rec.mode,
      });
    }
    if (!isFullSha(rec.sha)) {
      fail(ErrorCode.SOURCE_SNAPSHOT, "blob sha not full", { path: rec.path });
    }

    const blob = git.run(sourceRoot, ["cat-file", "blob", rec.sha]);
    const entry = {
      path: rec.path,
      mode: rec.mode,
      blobSha: rec.sha,
      bytes: blob.stdout,
    };
    if (byPath.has(rec.path)) {
      fail(ErrorCode.SOURCE_SNAPSHOT, `duplicate path ${rec.path}`, {});
    }
    byPath.set(rec.path, entry);
    entries.push(entry);
  }

  entries.sort((a, b) => compareGitPath(a.path, b.path));
  if (!byPath.has("LICENSE")) {
    fail(ErrorCode.SOURCE_SNAPSHOT, "LICENSE missing from source snapshot", {
      sourceSha,
    });
  }
  const mdCount = entries.filter((e) => e.path.endsWith(".md")).length;
  if (mdCount === 0) {
    fail(ErrorCode.SOURCE_SNAPSHOT, "no tracked Markdown in source snapshot", {
      sourceSha,
    });
  }

  return { sourceSha, sourceTreeSha: treeSha, entries, byPath };
}

const FILE_MANIFEST_REL = "FILE_MANIFEST.md";
const FILE_MANIFEST_HEADER = [
  "# FILE_MANIFEST",
  "",
  "> 非规范元数据。列出 Git source set 中的 Markdown（tracked `git ls-files '*.md'` + 标准 ignore 下 untracked source）；不含 ignored build 产物（如 target/、node_modules/）。行数以 UTF-8 文本 `wc -l` 等价结果为准。由 `scripts/update-file-manifest.mjs` 生成，禁止手改。",
  "",
];

function countLfBytes(bytes) {
  let count = 0;
  for (const byte of bytes) if (byte === 0x0a) count += 1;
  return count;
}

/** Validate FILE_MANIFEST.md against the exact Markdown closed set in a commit. */
export function validateFileManifest(snapshot) {
  const manifest = snapshot.byPath.get(FILE_MANIFEST_REL);
  if (!manifest) {
    fail(ErrorCode.MANIFEST_MISMATCH, `${FILE_MANIFEST_REL} is missing`, {});
  }
  if (
    manifest.bytes.includes(0x0d) ||
    manifest.bytes.length === 0 ||
    manifest.bytes[manifest.bytes.length - 1] !== 0x0a
  ) {
    fail(ErrorCode.MANIFEST_MISMATCH, `${FILE_MANIFEST_REL} must use LF and end with LF`, {});
  }
  let text;
  try {
    text = strictUtf8Decoder.decode(manifest.bytes);
  } catch {
    fail(ErrorCode.MANIFEST_MISMATCH, `${FILE_MANIFEST_REL} is not strict UTF-8`, {});
  }
  const lines = text.slice(0, -1).split("\n");
  const actualHeader = lines.slice(0, FILE_MANIFEST_HEADER.length);
  if (JSON.stringify(actualHeader) !== JSON.stringify(FILE_MANIFEST_HEADER)) {
    fail(ErrorCode.MANIFEST_MISMATCH, `${FILE_MANIFEST_REL} header mismatch`, {});
  }

  const expectedPaths = snapshot.entries
    .filter((entry) => entry.path.endsWith(".md"))
    .map((entry) => entry.path)
    .sort(compareGitPath);
  const entries = [];
  const seen = new Set();
  for (const line of lines.slice(FILE_MANIFEST_HEADER.length)) {
    const match = /^- `([^`]+)` — ([0-9]+) lines$/.exec(line);
    if (!match) {
      fail(ErrorCode.MANIFEST_MISMATCH, "malformed FILE_MANIFEST entry", { line });
    }
    const [, path, lineCountText] = match;
    assertSafeRepoRelativePath(path);
    if (seen.has(path)) {
      fail(ErrorCode.MANIFEST_MISMATCH, "duplicate FILE_MANIFEST path", { path });
    }
    seen.add(path);
    entries.push({ path, lineCount: Number(lineCountText) });
  }
  const actualPaths = entries.map((entry) => entry.path);
  if (JSON.stringify(actualPaths) !== JSON.stringify(expectedPaths)) {
    fail(ErrorCode.MANIFEST_MISMATCH, "FILE_MANIFEST path set/order mismatch", {
      expectedPaths,
      actualPaths,
    });
  }
  for (const entry of entries) {
    const sourceEntry = snapshot.byPath.get(entry.path);
    const actualLineCount = countLfBytes(sourceEntry.bytes);
    if (entry.lineCount !== actualLineCount) {
      fail(ErrorCode.MANIFEST_MISMATCH, "FILE_MANIFEST LF line count mismatch", {
        path: entry.path,
        expected: actualLineCount,
        actual: entry.lineCount,
      });
    }
  }
  return { pathCount: entries.length };
}

/**
 * Expected docs tree entries = source closed set + fixed .gitignore.
 * @param {SourceSnapshot} snapshot
 * @param {typeof PRODUCTION_CONTRACT} [contract]
 * @returns {{ path: string, mode: string, bytes: Buffer }[]}
 */
export function buildExpectedDocsEntries(
  snapshot,
  contract = PRODUCTION_CONTRACT,
) {
  /** @type {{ path: string, mode: string, bytes: Buffer }[]} */
  const out = [];
  for (const e of snapshot.entries) {
    out.push({ path: e.path, mode: e.mode, bytes: e.bytes });
  }
  out.push({
    path: contract.docsGitignoreRel,
    mode: "100644",
    bytes: Buffer.from(contract.docsGitignoreBytes),
  });
  out.sort((a, b) => compareGitPath(a.path, b.path));
  return out;
}

/**
 * Build a deterministic manifest: path → { mode, sha256, size }.
 * @param {{ path: string, mode: string, bytes: Buffer }[]} entries
 */
export function buildContentManifest(entries) {
  /** @type {Map<string, { mode: string, sha256: string, size: number }>} */
  const map = new Map();
  for (const e of entries) {
    if (map.has(e.path)) {
      fail(ErrorCode.INTERNAL, `duplicate manifest path ${e.path}`, {});
    }
    map.set(e.path, {
      mode: e.mode,
      sha256: createHash("sha256").update(e.bytes).digest("hex"),
      size: e.bytes.length,
    });
  }
  return map;
}

/**
 * Compare two content manifests strictly.
 * @param {Map<string, { mode: string, sha256: string, size: number }>} expected
 * @param {Map<string, { mode: string, sha256: string, size: number }>} actual
 */
export function diffContentManifests(expected, actual) {
  const missing = [];
  const extra = [];
  const changed = [];
  for (const [path, exp] of expected) {
    const act = actual.get(path);
    if (!act) {
      missing.push(path);
      continue;
    }
    if (
      act.mode !== exp.mode ||
      act.sha256 !== exp.sha256 ||
      act.size !== exp.size
    ) {
      changed.push(path);
    }
  }
  for (const path of actual.keys()) {
    if (!expected.has(path)) extra.push(path);
  }
  missing.sort(compareGitPath);
  extra.sort(compareGitPath);
  changed.sort(compareGitPath);
  return { missing, extra, changed, equal: missing.length + extra.length + changed.length === 0 };
}

// ---------------------------------------------------------------------------
// Tree builder via Git plumbing (mktree recursive)
// ---------------------------------------------------------------------------

/**
 * Insert a file path into a nested directory map.
 * @param {Map<string, any>} root
 * @param {string} path
 * @param {{ mode: string, blobSha: string }} file
 */
function insertTreePath(root, path, file) {
  const parts = path.split("/");
  let node = root;
  for (let i = 0; i < parts.length - 1; i += 1) {
    const part = parts[i];
    if (!node.has(part)) {
      node.set(part, new Map());
    }
    const next = node.get(part);
    if (!(next instanceof Map)) {
      fail(ErrorCode.INTERNAL, `path conflict at ${parts.slice(0, i + 1).join("/")}`, {});
    }
    node = next;
  }
  const base = parts[parts.length - 1];
  if (node.has(base)) {
    fail(ErrorCode.INTERNAL, `duplicate tree leaf ${path}`, {});
  }
  node.set(base, file);
}

/**
 * Materialize nested maps into git tree objects.
 * @param {ReturnType<typeof createGitPort>} git
 * @param {string} repoRoot  // any repo with object store write access
 * @param {Map<string, any>} node
 * @returns {string} tree sha
 */
function writeTreeNode(git, repoRoot, node) {
  /** @type {{ mode: string, type: string, sha: string, name: string }[]} */
  const items = [];
  for (const [name, value] of node.entries()) {
    if (value instanceof Map) {
      const sha = writeTreeNode(git, repoRoot, value);
      items.push({ mode: "040000", type: "tree", sha, name });
    } else {
      items.push({
        mode: value.mode,
        type: "blob",
        sha: value.blobSha,
        name,
      });
    }
  }
  items.sort((a, b) => {
    // Git tree sort: entries sort by name; trees are sorted as name+"/"
    const an = a.type === "tree" ? `${a.name}/` : a.name;
    const bn = b.type === "tree" ? `${b.name}/` : b.name;
    return compareGitPath(an, bn);
  });

  const lines = items
    .map((it) => `${it.mode} ${it.type} ${it.sha}\t${it.name}`)
    .join("\n");
  const input = lines.length === 0 ? "" : `${lines}\n`;
  const out = git.textTrim(repoRoot, ["mktree"], { input });
  requireFullSha(out, "treeSha");
  return out;
}

/**
 * Write blobs + tree for expected docs entries into a Git object store.
 * @param {ReturnType<typeof createGitPort>} git
 * @param {string} objectRepoRoot
 * @param {{ path: string, mode: string, bytes: Buffer }[]} entries
 * @returns {{ treeSha: string, entryBlobShas: Map<string, string> }}
 */
export function materializeDocsTree(git, objectRepoRoot, entries) {
  const root = new Map();
  /** @type {Map<string, string>} */
  const entryBlobShas = new Map();
  for (const e of entries) {
    assertSafeRepoRelativePath(e.path);
    const hashOut = git.run(objectRepoRoot, ["hash-object", "-w", "--stdin"], {
      input: e.bytes,
    });
    const blobSha = hashOut.stdout.toString("utf8").trim();
    requireFullSha(blobSha, "blobSha");
    entryBlobShas.set(e.path, blobSha);
    insertTreePath(root, e.path, { mode: e.mode, blobSha });
  }
  const treeSha = writeTreeNode(git, objectRepoRoot, root);
  return { treeSha, entryBlobShas };
}

/**
 * Read a docs commit tree into a content manifest (bytes from objects).
 * @param {ReturnType<typeof createGitPort>} git
 * @param {string} docsRoot
 * @param {string} commitSha
 */
export function readCommitContentManifest(git, docsRoot, commitSha) {
  requireFullSha(commitSha, "commitSha");
  const ls = git.run(docsRoot, ["ls-tree", "-r", "-z", commitSha]);
  const records = parseLsTreeZ(ls.stdout);
  /** @type {{ path: string, mode: string, bytes: Buffer }[]} */
  const entries = [];
  for (const rec of records) {
    assertSafeRepoRelativePath(rec.path);
    if (rec.type === "commit") {
      fail(ErrorCode.DOCS_TREE, "docs tree contains gitlink", { path: rec.path });
    }
    if (rec.type !== "blob") {
      fail(ErrorCode.DOCS_TREE, `docs tree non-blob ${rec.type}`, {
        path: rec.path,
      });
    }
    if (rec.mode === "120000") {
      fail(ErrorCode.DOCS_TREE, "docs tree contains symlink", { path: rec.path });
    }
    if (rec.mode !== "100644" && rec.mode !== "100755") {
      fail(ErrorCode.DOCS_TREE, `docs unsupported mode ${rec.mode}`, {
        path: rec.path,
      });
    }
    const blob = git.run(docsRoot, ["cat-file", "blob", rec.sha]);
    entries.push({ path: rec.path, mode: rec.mode, bytes: blob.stdout });
  }
  return buildContentManifest(entries);
}

// ---------------------------------------------------------------------------
// Source preflight
// ---------------------------------------------------------------------------

/**
 * @param {ReturnType<typeof createGitPort>} git
 * @param {typeof PRODUCTION_CONTRACT} contract
 * @param {{ sourceRoot?: string }} [override]
 */
export function inspectSourceRepository(git, contract, override = {}) {
  const sourceRoot = override.sourceRoot || contract.sourceRepoRoot;
  if (!existsSync(join(sourceRoot, ".git")) && !existsSync(sourceRoot)) {
    fail(ErrorCode.SOURCE_NOT_REPO, `source root missing: ${sourceRoot}`, {
      sourceRoot,
    });
  }

  const inside = git
    .textTrim(sourceRoot, ["rev-parse", "--is-inside-work-tree"], {
      allowNonZero: true,
    })
    .trim();
  if (inside !== "true") {
    fail(ErrorCode.SOURCE_NOT_REPO, `not a git work tree: ${sourceRoot}`, {
      sourceRoot,
    });
  }

  const branch = git.textTrim(sourceRoot, ["branch", "--show-current"]);
  if (branch !== contract.sourceBranch) {
    fail(
      ErrorCode.SOURCE_WRONG_BRANCH,
      `source branch must be ${contract.sourceBranch}, got ${JSON.stringify(branch)}`,
      { branch, expected: contract.sourceBranch },
    );
  }

  const remoteUrl = git.textTrim(
    sourceRoot,
    ["remote", "get-url", contract.sourceRemoteName],
    { allowNonZero: true },
  );
  if (remoteUrl !== contract.sourceRemoteUrl) {
    fail(
      ErrorCode.SOURCE_WRONG_REMOTE,
      `source remote ${contract.sourceRemoteName} must be ${contract.sourceRemoteUrl}, got ${JSON.stringify(remoteUrl)}`,
      { remoteUrl, expected: contract.sourceRemoteUrl },
    );
  }

  // Dirty detection: tracked diffs + untracked non-ignored + staged.
  const porcelain = git.run(sourceRoot, [
    "status",
    "--porcelain=v2",
    "-z",
    "--untracked-files=normal",
  ]);
  const dirtyRecords = parsePorcelainV2Z(porcelain.stdout);
  if (dirtyRecords.length > 0) {
    fail(ErrorCode.SOURCE_DIRTY, "source worktree is dirty", {
      sample: dirtyRecords.slice(0, 8),
      count: dirtyRecords.length,
    });
  }

  // Repo-local identity: email and name are symmetric hard gates.
  const userEmail = git.textTrim(sourceRoot, ["config", "--local", "--get", "user.email"], {
    allowNonZero: true,
  });
  if (userEmail !== contract.requiredEmail) {
    fail(
      ErrorCode.SOURCE_IDENTITY,
      `source local user.email must be ${contract.requiredEmail}, got ${JSON.stringify(userEmail)}`,
      { userEmail },
    );
  }
  const userName = git.textTrim(sourceRoot, ["config", "--local", "--get", "user.name"], {
    allowNonZero: true,
  });
  if (userName !== contract.requiredName) {
    fail(
      ErrorCode.SOURCE_IDENTITY,
      `source local user.name must be ${contract.requiredName}, got ${JSON.stringify(userName)}`,
      { userName },
    );
  }

  const headSha = git.textTrim(sourceRoot, ["rev-parse", "HEAD"]);
  requireFullSha(headSha, "HEAD");

  // One pretty record: ae, an, ce, cn — keeps author/committer fields paired.
  const headIdentityRaw = git.run(sourceRoot, [
    "show",
    "-s",
    "--format=%ae%x00%an%x00%ce%x00%cn",
    "--no-patch",
    headSha,
  ]).stdout;
  let headIdentityBuf = headIdentityRaw;
  if (headIdentityBuf.length > 0 && headIdentityBuf[headIdentityBuf.length - 1] === 0x0a) {
    headIdentityBuf = headIdentityBuf.subarray(0, headIdentityBuf.length - 1);
  }
  const headIdentityParts = splitNulFields(headIdentityBuf);
  if (headIdentityParts.length !== 4) {
    fail(ErrorCode.SOURCE_IDENTITY, "HEAD identity field count mismatch", {
      headSha,
      fieldCount: headIdentityParts.length,
    });
  }
  const [authorEmail, authorName, committerEmail, committerName] = headIdentityParts;
  if (authorEmail !== contract.requiredEmail) {
    fail(
      ErrorCode.SOURCE_IDENTITY,
      `HEAD author email must be ${contract.requiredEmail}, got ${JSON.stringify(authorEmail)}`,
      { authorEmail },
    );
  }
  if (authorName !== contract.requiredName) {
    fail(
      ErrorCode.SOURCE_IDENTITY,
      `HEAD author name must be ${contract.requiredName}, got ${JSON.stringify(authorName)}`,
      { authorName },
    );
  }
  if (committerEmail !== contract.requiredEmail) {
    fail(
      ErrorCode.SOURCE_IDENTITY,
      `HEAD committer email must be ${contract.requiredEmail}, got ${JSON.stringify(committerEmail)}`,
      { committerEmail },
    );
  }
  if (committerName !== contract.requiredName) {
    fail(
      ErrorCode.SOURCE_IDENTITY,
      `HEAD committer name must be ${contract.requiredName}, got ${JSON.stringify(committerName)}`,
      { committerName },
    );
  }

  // HEAD must equal remote-tracking tip (already pushed). Use ls-remote for live fact.
  const lsRemote = git.textTrim(
    sourceRoot,
    [
      "ls-remote",
      "--heads",
      contract.sourceRemoteName,
      `refs/heads/${contract.sourceBranch}`,
    ],
    { allowNonZero: true },
  );
  const remoteLine = lsRemote.split("\n").find((l) => l.includes(`refs/heads/${contract.sourceBranch}`));
  if (!remoteLine) {
    fail(ErrorCode.SOURCE_NOT_PUSHED, "source remote branch not found", {
      lsRemote,
    });
  }
  const remoteSha = remoteLine.split(/[\t ]+/)[0];
  requireFullSha(remoteSha, "sourceRemoteSha");
  if (remoteSha !== headSha) {
    fail(
      ErrorCode.SOURCE_NOT_PUSHED,
      `source HEAD ${headSha} is not equal to remote ${contract.sourceBranch} ${remoteSha}`,
      { headSha, remoteSha },
    );
  }

  return {
    sourceRoot,
    headSha,
    remoteSha,
    branch,
    remoteUrl,
  };
}

// ---------------------------------------------------------------------------
// Docs history audit
// ---------------------------------------------------------------------------

/**
 * @typedef {object} DocsHistoryCommit
 * @property {string} sha
 * @property {string} treeSha
 * @property {string[]} parents
 * @property {string} subject
 * @property {string} authorEmail
 * @property {string} authorName
 * @property {string} committerEmail
 * @property {string} committerName
 * @property {"sync"|"bootstrap"} kind
 * @property {string} sourceSha
 */

/**
 * List first-parent history oldest→newest for a branch tip.
 * @param {ReturnType<typeof createGitPort>} git
 * @param {string} docsRoot
 * @param {string} tipSha
 * @returns {DocsHistoryCommit[]}
 */
export function loadDocsFirstParentHistory(git, docsRoot, tipSha, contract = PRODUCTION_CONTRACT) {
  requireFullSha(tipSha, "tipSha");
  // Walk first-parent SHAs oldest→newest, then inspect each commit with a
  // single-commit pretty format. Avoids git-log's trailing newline field drift
  // when mixing %x00 with multi-commit output.
  const revList = git.textTrim(docsRoot, [
    "rev-list",
    "--first-parent",
    "--reverse",
    tipSha,
  ]);
  const shas = revList
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean);
  if (shas.length === 0) {
    fail(ErrorCode.DOCS_HISTORY, "docs rev-list returned no commits", { tipSha });
  }

  /** @type {DocsHistoryCommit[]} */
  const commits = [];
  for (const listedSha of shas) {
    requireFullSha(listedSha, "docsCommit");
    // One record: H, T, P, ae, an, ce, cn, s — NUL separated; root has empty %P.
    const fmt = "%H%x00%T%x00%P%x00%ae%x00%an%x00%ce%x00%cn%x00%s";
    const raw = git.run(docsRoot, [
      "show",
      "-s",
      "--format=" + fmt,
      "--no-patch",
      listedSha,
    ]).stdout;
    // Drop a single trailing newline that git pretty-printer may append.
    let buf = raw;
    if (buf.length > 0 && buf[buf.length - 1] === 0x0a) {
      buf = buf.subarray(0, buf.length - 1);
    }
    const parts = splitNulFields(buf);
    if (parts.length !== 8) {
      fail(ErrorCode.DOCS_HISTORY, "docs commit field count mismatch", {
        listedSha,
        fieldCount: parts.length,
        sample: parts,
      });
    }
    const [
      sha,
      treeSha,
      parentsRaw,
      authorEmail,
      authorName,
      committerEmail,
      committerName,
      subject,
    ] = parts;
    if (sha !== listedSha) {
      fail(ErrorCode.DOCS_HISTORY, "docs commit sha mismatch in show", {
        listedSha,
        sha,
      });
    }
    requireFullSha(treeSha, "docsTree");
    const parents = parentsRaw
      .split(" ")
      .map((p) => p.trim())
      .filter(Boolean);
    for (const p of parents) requireFullSha(p, "parent");
    const parsed = parseDocsCommitSubject(subject, contract);
    if (!parsed) {
      fail(
        ErrorCode.DOCS_HISTORY,
        `docs commit subject not a recognized source marker: ${JSON.stringify(subject)}`,
        { sha, subject },
      );
    }
    commits.push({
      sha,
      treeSha,
      parents,
      subject,
      authorEmail,
      authorName,
      committerEmail,
      committerName,
      kind: parsed.kind,
      sourceSha: parsed.sourceSha,
    });
  }
  return commits;
}

/**
 * Audit docs history against source ancestry and closed-set trees.
 * @param {ReturnType<typeof createGitPort>} git
 * @param {string} docsRoot
 * @param {string} sourceRoot
 * @param {DocsHistoryCommit[]} history
 * @param {typeof PRODUCTION_CONTRACT} contract
 */
export function buildSourceFirstParentIndex(git, sourceRoot, sourceHeadSha) {
  requireFullSha(sourceHeadSha, "sourceHeadSha");
  const output = git.textTrim(sourceRoot, [
    "rev-list",
    "--first-parent",
    "--reverse",
    sourceHeadSha,
  ]);
  const ordered = output.split("\n").map((sha) => sha.trim()).filter(Boolean);
  const positionBySha = new Map();
  for (let index = 0; index < ordered.length; index += 1) {
    requireFullSha(ordered[index], "sourceFirstParentSha");
    positionBySha.set(ordered[index], index);
  }
  if (ordered.at(-1) !== sourceHeadSha) {
    fail(ErrorCode.DOCS_HISTORY, "source first-parent index does not end at live master HEAD", {
      sourceHeadSha,
      last: ordered.at(-1) || null,
    });
  }
  return { headSha: sourceHeadSha, ordered, positionBySha };
}

export function auditDocsHistory(
  git,
  docsRoot,
  sourceRoot,
  history,
  contract,
  sourceFirstParentIndex = null,
) {
  if (history.length === 0) {
    fail(ErrorCode.DOCS_HISTORY, "docs history is empty", {});
  }

  for (const c of history) {
    if (c.parents.length > 1) {
      fail(ErrorCode.DOCS_HISTORY, "docs history must be linear (no merge commits)", {
        sha: c.sha,
        parents: c.parents,
      });
    }
  }

  const sourceIndex =
    sourceFirstParentIndex ||
    buildSourceFirstParentIndex(
      git,
      sourceRoot,
      git.textTrim(sourceRoot, ["rev-parse", contract.sourceBranch]),
    );
  let previousPosition = -1;

  for (let idx = 0; idx < history.length; idx += 1) {
    const c = history[idx];

    // Linear first-parent: root has 0 parents; others exactly 1 parent in first-parent walk.
    // (Merges on first-parent still show one parent; reject multi-parent commits entirely.)
    if (c.parents.length > 1) {
      fail(ErrorCode.DOCS_HISTORY, "docs history must be linear (no merge commits)", {
        sha: c.sha,
        parents: c.parents,
      });
    }
    if (idx === 0) {
      if (c.parents.length !== 0) {
        // first-parent reverse from tip should start at root
        fail(ErrorCode.DOCS_HISTORY, "docs root commit must have no parents", {
          sha: c.sha,
          parents: c.parents,
        });
      }
      if (c.kind !== "bootstrap" && c.kind !== "sync") {
        fail(ErrorCode.DOCS_HISTORY, "docs root marker invalid", { sha: c.sha });
      }
    } else if (c.parents.length !== 1) {
      fail(ErrorCode.DOCS_HISTORY, "non-root docs commit must have exactly one parent", {
        sha: c.sha,
      });
    }

    if (c.authorEmail !== contract.requiredEmail) {
      fail(ErrorCode.DOCS_IDENTITY, "docs author email mismatch", {
        sha: c.sha,
        authorEmail: c.authorEmail,
      });
    }
    if (c.authorName !== contract.requiredName) {
      fail(ErrorCode.DOCS_IDENTITY, "docs author name mismatch", {
        sha: c.sha,
        authorName: c.authorName,
      });
    }
    if (c.committerEmail !== contract.requiredEmail) {
      fail(ErrorCode.DOCS_IDENTITY, "docs committer email mismatch", {
        sha: c.sha,
        committerEmail: c.committerEmail,
      });
    }
    if (c.committerName !== contract.requiredName) {
      fail(ErrorCode.DOCS_IDENTITY, "docs committer name mismatch", {
        sha: c.sha,
        committerName: c.committerName,
      });
    }

    const sourcePosition = sourceIndex.positionBySha.get(c.sourceSha);
    if (sourcePosition === undefined) {
      fail(
        ErrorCode.DOCS_HISTORY,
        "docs source SHA is not on live source master first-parent history",
        { docsSha: c.sha, sourceSha: c.sourceSha, sourceHead: sourceIndex.headSha },
      );
    }
    if (sourcePosition <= previousPosition) {
      fail(ErrorCode.DOCS_HISTORY, "docs source markers must strictly advance", {
        docsSha: c.sha,
        sourceSha: c.sourceSha,
        sourcePosition,
        previousPosition,
      });
    }
    previousPosition = sourcePosition;

    // Closed-set tree audit against that source snapshot.
    const snapshot = buildSourceSnapshot(git, sourceRoot, c.sourceSha);
    const expectedEntries = buildExpectedDocsEntries(snapshot, contract);
    const expectedManifest = buildContentManifest(expectedEntries);
    const actualManifest = readCommitContentManifest(git, docsRoot, c.sha);
    const diff = diffContentManifests(expectedManifest, actualManifest);
    if (!diff.equal) {
      fail(ErrorCode.DOCS_TREE, "docs commit tree is not the closed source set", {
        docsSha: c.sha,
        sourceSha: c.sourceSha,
        missing: diff.missing,
        extra: diff.extra,
        changed: diff.changed,
      });
    }

    // Fixed .gitignore bytes ledger.
    const gi = actualManifest.get(contract.docsGitignoreRel);
    if (!gi) {
      fail(ErrorCode.DOCS_TREE, "docs commit missing .gitignore", { sha: c.sha });
    }
    const expectedGi = createHash("sha256")
      .update(contract.docsGitignoreBytes)
      .digest("hex");
    if (gi.sha256 !== expectedGi || gi.mode !== "100644") {
      fail(ErrorCode.DOCS_TREE, "docs .gitignore bytes/mode mismatch", {
        sha: c.sha,
        mode: gi.mode,
        sha256: gi.sha256,
        expectedSha256: expectedGi,
      });
    }
  }

  const lastSourceSha = history[history.length - 1].sourceSha;
  if (sourceIndex.positionBySha.get(lastSourceSha) === undefined) {
    fail(ErrorCode.DOCS_HISTORY, "last docs source marker is outside source first-parent history", {
      lastSourceSha,
      sourceHead: sourceIndex.headSha,
    });
  }
  return {
    tip: history[history.length - 1],
    length: history.length,
    lastSourceSha,
  };
}

// ---------------------------------------------------------------------------
// Sync plan
// ---------------------------------------------------------------------------

/**
 * @typedef {"noop_idempotent"|"append_sync"|"append_receipt"|"bootstrap"} PlanAction
 */

/**
 * @typedef {object} SyncPlan
 * @property {PlanAction} action
 * @property {string} sourceSha
 * @property {string} expectedTreeSha  // filled after materialize when needed
 * @property {string|null} docsRemoteSha
 * @property {string|null} docsLocalSha
 * @property {string} commitMessage
 * @property {string} reason
 */

/**
 * Decide what sync must do given audited docs tip and source HEAD.
 * @param {{ sourceSha: string, docsRemoteSha: string|null, docsRemoteSourceSha: string|null, docsRemoteTreeSha: string|null, expectedTreeSha: string|null, docsExists: boolean }} input
 */
export function planSyncAction(input) {
  const {
    sourceSha,
    docsRemoteSha,
    docsRemoteSourceSha,
    docsRemoteTreeSha,
    expectedTreeSha,
    docsExists,
  } = input;
  requireFullSha(sourceSha, "sourceSha");

  if (!docsExists || !docsRemoteSha) {
    return {
      action: /** @type {PlanAction} */ ("bootstrap"),
      sourceSha,
      expectedTreeSha,
      docsRemoteSha: null,
      docsLocalSha: null,
      commitMessage: formatBootstrapCommitMessage(sourceSha),
      reason: "docs remote has no master tip; bootstrap pure docs mirror",
    };
  }

  if (docsRemoteSourceSha === sourceSha) {
    // Idempotent if tree also matches; caller verifies tree.
    if (
      expectedTreeSha &&
      docsRemoteTreeSha &&
      expectedTreeSha === docsRemoteTreeSha
    ) {
      return {
        action: /** @type {PlanAction} */ ("noop_idempotent"),
        sourceSha,
        expectedTreeSha,
        docsRemoteSha,
        docsLocalSha: null,
        commitMessage: formatSyncCommitMessage(sourceSha),
        reason: "docs remote already records this source SHA with matching tree",
      };
    }
    // Same source marker but tree drift — fail closed (history audit should catch).
    fail(ErrorCode.DOCS_TREE, "docs tip marks source SHA but tree mismatches", {
      sourceSha,
      docsRemoteSha,
      docsRemoteTreeSha,
      expectedTreeSha,
    });
  }

  if (
    expectedTreeSha &&
    docsRemoteTreeSha &&
    expectedTreeSha === docsRemoteTreeSha
  ) {
    return {
      action: /** @type {PlanAction} */ ("append_receipt"),
      sourceSha,
      expectedTreeSha,
      docsRemoteSha,
      docsLocalSha: null,
      commitMessage: formatSyncCommitMessage(sourceSha),
      reason:
        "docs tree already matches source closed set; append source-SHA receipt commit",
    };
  }

  return {
    action: /** @type {PlanAction} */ ("append_sync"),
    sourceSha,
    expectedTreeSha,
    docsRemoteSha,
    docsLocalSha: null,
    commitMessage: formatSyncCommitMessage(sourceSha),
    reason: "append linear docs sync commit for new source closed-set tree",
  };
}

// ---------------------------------------------------------------------------
// Publisher + local checkout finalizer
// ---------------------------------------------------------------------------

/**
 * Ensure a docs work repo exists with correct remote (no unknown content wipe).
 * @param {ReturnType<typeof createGitPort>} git
 * @param {typeof PRODUCTION_CONTRACT} contract
 * @param {{ docsRoot?: string, docsRemoteUrl?: string }} [override]
 */
export function ensureDocsRepoShell(git, contract, override = {}) {
  const docsRoot = override.docsRoot || contract.docsCheckoutRoot;
  const remoteUrl = override.docsRemoteUrl || contract.docsRemoteUrl;

  if (!existsSync(docsRoot)) {
    mkdirSync(docsRoot, { recursive: true });
  }

  const gitDir = join(docsRoot, ".git");
  if (!existsSync(gitDir)) {
    // Safe first install: clone would overwrite; use init + remote + fetch.
    git.run(docsRoot, ["init", "-b", contract.docsBranch]);
    git.run(docsRoot, ["config", "--local", "user.email", contract.requiredEmail]);
    git.run(docsRoot, ["config", "--local", "user.name", contract.requiredName]);
    git.run(docsRoot, ["remote", "add", contract.docsRemoteName, remoteUrl]);
  } else {
    const url = git.textTrim(
      docsRoot,
      ["remote", "get-url", contract.docsRemoteName],
      { allowNonZero: true },
    );
    if (url !== remoteUrl) {
      fail(
        ErrorCode.DOCS_WRONG_REMOTE,
        `docs remote must be ${remoteUrl}, got ${JSON.stringify(url)}`,
        { url, expected: remoteUrl },
      );
    }
    // Ensure local identity for any commit we create in this checkout.
    git.run(docsRoot, ["config", "--local", "user.email", contract.requiredEmail]);
    git.run(docsRoot, ["config", "--local", "user.name", contract.requiredName]);
  }

  return { docsRoot, remoteUrl };
}

export function inspectExistingDocsCheckout(git, docsRoot, contract, remoteUrl) {
  if (!existsSync(docsRoot)) {
    return { state: "missing", docsRoot };
  }
  const inside = git.textTrim(docsRoot, ["rev-parse", "--is-inside-work-tree"], {
    allowNonZero: true,
  });
  if (inside !== "true") {
    fail(ErrorCode.DOCS_NOT_REPO, `existing docs path is not a Git work tree: ${docsRoot}`, {
      docsRoot,
    });
  }
  const actualRemote = git.textTrim(
    docsRoot,
    ["remote", "get-url", contract.docsRemoteName],
    { allowNonZero: true },
  );
  if (actualRemote !== remoteUrl) {
    fail(ErrorCode.DOCS_WRONG_REMOTE, `docs remote must be ${remoteUrl}`, {
      actual: actualRemote,
      expected: remoteUrl,
    });
  }
  const branch = git.textTrim(docsRoot, ["branch", "--show-current"], {
    allowNonZero: true,
  });
  const head = git.textTrim(docsRoot, ["rev-parse", "--verify", "HEAD"], {
    allowNonZero: true,
  });
  const status = parsePorcelainV2Z(
    git.run(docsRoot, ["status", "--porcelain=v2", "-z", "--untracked-files=normal"])
      .stdout,
  );
  return {
    state: "present",
    docsRoot,
    branch: branch || null,
    headSha: isFullSha(head) ? head : null,
    dirty: status.length > 0,
  };
}

export function createTemporaryDocsAuditRepo(git, contract, remoteUrl) {
  mkdirSync(contract.tempRoot, { recursive: true, mode: 0o700 });
  chmodSync(contract.tempRoot, 0o700);
  const root = mkdtempSync(join(contract.tempRoot, "check-"));
  chmodSync(root, 0o700);
  git.run(root, ["init", "--bare"]);
  git.run(root, ["remote", "add", contract.docsRemoteName, remoteUrl]);
  return {
    root,
    cleanup() {
      rmSync(root, { recursive: true, force: true });
    },
  };
}

/**
 * Fetch docs remote branch and return tip SHA or null if absent.
 * @param {ReturnType<typeof createGitPort>} git
 * @param {string} docsRoot
 * @param {typeof PRODUCTION_CONTRACT} contract
 */
export function queryDocsRemoteTip(git, docsRoot, contract) {
  const result = git.run(
    docsRoot,
    [
      "ls-remote",
      "--heads",
      contract.docsRemoteName,
      `refs/heads/${contract.docsBranch}`,
    ],
    { allowNonZero: true },
  );
  if (result.status !== 0) {
    fail(ErrorCode.DOCS_PUSH_UNKNOWN, "cannot query docs remote tip", {
      status: result.status,
      stderr: result.stderr.toString("utf8"),
    });
  }
  const line = result.stdout
    .toString("utf8")
    .split("\n")
    .map((value) => value.trim())
    .find((value) => value.endsWith(`refs/heads/${contract.docsBranch}`));
  if (!line) return null;
  const sha = line.split(/[\t ]+/)[0];
  requireFullSha(sha, "docsRemoteSha");
  return sha;
}

export function fetchDocsRemoteTip(git, docsRoot, contract) {
  const result = git.run(
    docsRoot,
    [
      "fetch",
      "--no-tags",
      contract.docsRemoteName,
      `refs/heads/${contract.docsBranch}`,
    ],
    { allowNonZero: true },
  );
  const tip = queryDocsRemoteTip(git, docsRoot, contract);
  if (result.status !== 0 && tip !== null) {
    fail(ErrorCode.DOCS_PUSH_UNKNOWN, "docs remote tip exists but fetch failed", {
      status: result.status,
      stderr: result.stderr.toString("utf8"),
      tip,
    });
  }
  return tip;
}

/**
 * Create a docs commit object (no worktree write required) parented at parentSha|null.
 * @param {ReturnType<typeof createGitPort>} git
 * @param {string} docsRoot
 * @param {{ treeSha: string, parentSha: string|null, message: string, email: string, name: string }} spec
 */
export function commitTree(git, docsRoot, spec) {
  requireFullSha(spec.treeSha, "treeSha");
  if (spec.parentSha) requireFullSha(spec.parentSha, "parentSha");
  const env = {
    GIT_AUTHOR_NAME: spec.name,
    GIT_AUTHOR_EMAIL: spec.email,
    GIT_COMMITTER_NAME: spec.name,
    GIT_COMMITTER_EMAIL: spec.email,
  };
  const args = ["commit-tree", spec.treeSha];
  if (spec.parentSha) {
    args.push("-p", spec.parentSha);
  }
  args.push("-m", spec.message);
  const sha = git.textTrim(docsRoot, args, { env });
  requireFullSha(sha, "newCommit");
  return sha;
}

/**
 * Fast-forward push only. Never force.
 * After push failure, re-query remote to classify success/not-pushed/unknown.
 * @param {ReturnType<typeof createGitPort>} git
 * @param {string} docsRoot
 * @param {string} localCommitSha
 * @param {string|null} expectedRemoteParent
 * @param {typeof PRODUCTION_CONTRACT} contract
 */
export function pushDocsFastForward(
  git,
  docsRoot,
  localCommitSha,
  expectedRemoteParent,
  contract,
) {
  requireFullSha(localCommitSha, "localCommitSha");
  if (expectedRemoteParent !== null) {
    requireFullSha(expectedRemoteParent, "expectedRemoteParent");
  }

  const parents = git.textTrim(docsRoot, ["show", "-s", "--format=%P", localCommitSha])
    .split(" ")
    .filter(Boolean);
  const expectedParents = expectedRemoteParent ? [expectedRemoteParent] : [];
  if (JSON.stringify(parents) !== JSON.stringify(expectedParents)) {
    fail(ErrorCode.DOCS_REMOTE_DIVERGED, "local docs commit parent does not equal expected remote parent", {
      localCommitSha,
      parents,
      expectedParents,
    });
  }

  const before = queryDocsRemoteTip(git, docsRoot, contract);
  if ((before || null) !== expectedRemoteParent) {
    fail(
      ErrorCode.DOCS_REMOTE_DIVERGED,
      "docs remote changed before push; refusing to force",
      { before, expectedRemoteParent },
    );
  }

  const refspec = `${localCommitSha}:refs/heads/${contract.docsBranch}`;
  const push = git.run(
    docsRoot,
    ["push", contract.docsRemoteName, refspec],
    { allowNonZero: true },
  );

  let after;
  try {
    after = queryDocsRemoteTip(git, docsRoot, contract);
  } catch (error) {
    fail(
      ErrorCode.DOCS_PUSH_UNKNOWN,
      "push result unknown: remote query failed after push attempt",
      {
        localCommitSha,
        pushStatus: push.status,
        pushStderr: push.stderr.toString("utf8"),
        queryMessage: error.message,
      },
    );
  }

  if (after === localCommitSha) {
    return { pushed: true, remoteSha: after };
  }
  if (push.status !== 0 && after === expectedRemoteParent) {
    fail(ErrorCode.DOCS_PUSH_REJECTED, "docs fast-forward push rejected", {
      localCommitSha,
      remoteSha: after,
      expectedRemoteParent,
      stderr: push.stderr.toString("utf8"),
    });
  }
  fail(ErrorCode.DOCS_PUSH_UNKNOWN, "push outcome cannot be proven", {
    localCommitSha,
    remoteSha: after,
    expectedRemoteParent,
    pushStatus: push.status,
    stderr: push.stderr.toString("utf8"),
  });
}

/**
 * Refuse local tracked content edits that would be clobbered by an update.
 * Incomplete checkout (worktree files simply missing/deleted vs HEAD) is allowed.
 * Untracked files are never deleted. They are allowed only for an empty initial
 * checkout; otherwise they are reported before materialization so Git cannot
 * partially overwrite a path and leave an ambiguous transaction state.
 * @param {ReturnType<typeof createGitPort>} git
 * @param {string} docsRoot
 */
export function assertDocsWorktreeSafeForFastForward(git, docsRoot) {
  const headExists = git.run(docsRoot, ["rev-parse", "--verify", "HEAD"], {
    allowNonZero: true,
  }).status === 0;
  const porcelain = git.run(docsRoot, [
    "status",
    "--porcelain=v2",
    "-z",
    "--untracked-files=normal",
  ]);
  const records = parsePorcelainV2Z(porcelain.stdout);
  const blockers = records
    .filter((record) => {
      if (record.kind === "!") return false;
      if (!headExists && record.kind === "?") return false;
      if (headExists && record.kind === "1") {
        const xy = record.raw.split(" ")[1];
        if (xy === ".D" || xy === "D.") return false;
      }
      return true;
    })
    .map((record) => ({
      kind: record.kind,
      path: record.path,
      originalPath: record.originalPath,
    }));
  if (blockers.length > 0) {
    fail(
      ErrorCode.DOCS_CHECKOUT,
      "docs worktree contains tracked edits or untracked paths; refusing overwrite",
      { sample: blockers.slice(0, 8), count: blockers.length },
    );
  }
}

/**
 * Point index + tracked worktree files at commitSha without removing untracked files.
 * Uses read-tree plumbing (not reset --hard / clean).
 * @param {ReturnType<typeof createGitPort>} git
 * @param {string} docsRoot
 * @param {string} commitSha
 */
export function materializeDocsWorktreeAtCommit(git, docsRoot, commitSha) {
  requireFullSha(commitSha, "commitSha");
  const result = git.run(
    docsRoot,
    ["read-tree", "-u", "--reset", commitSha],
    { allowNonZero: true },
  );
  if (result.status !== 0) {
    fail(
      ErrorCode.DOCS_CHECKOUT,
      "failed to materialize docs worktree at commit (possible untracked conflict)",
      {
        commitSha,
        stderr: result.stderr.toString("utf8"),
      },
    );
  }
}

/** True when index and tracked worktree bytes/modes exactly equal a commit tree. */
export function docsWorktreeMatchesCommit(git, docsRoot, commitSha) {
  return checkoutMatchesCommit(git, docsRoot, commitSha, {
    requireFullSha,
  });
}

/**
 * Finalize local docs checkout as a journaled, recoverable transaction.
 * Remote-tracking is deliberately the last ref written.
 */
export function finalizeLocalCheckout(git, docsRoot, targetSha, contract, options = {}) {
  return finalizeLocalCheckoutState(git, docsRoot, targetSha, contract, options, {
    ErrorCode,
    fail,
    isFullSha,
    requireFullSha,
    parsePorcelainV2Z,
    assertDocsWorktreeSafeForFastForward,
    materializeDocsWorktreeAtCommit,
  });
}

export const DOCS_STAGING_REF = "refs/heads/docs-sync-staging";

function readOptionalRef(git, docsRoot, ref) {
  const result = git.run(docsRoot, ["rev-parse", "--verify", ref], {
    allowNonZero: true,
  });
  if (result.status !== 0) return null;
  const sha = result.stdout.toString("utf8").trim();
  requireFullSha(sha, ref);
  return sha;
}

function deleteStagingRef(git, docsRoot, expectedSha = null) {
  const args = ["update-ref", "-d", DOCS_STAGING_REF];
  if (expectedSha) args.push(expectedSha);
  const result = git.run(docsRoot, args, { allowNonZero: true });
  if (result.status !== 0) {
    fail(ErrorCode.DOCS_STAGING_DIVERGED, "staging ref changed while recovery was finalizing", {
      expectedSha,
      stderr: result.stderr.toString("utf8"),
    });
  }
}

/** Validate and recover the one durable push-unknown staging commit. */
export function recoverDocsStaging(
  git,
  docsRoot,
  sourceRoot,
  contract,
  expectedEntries,
  options = {},
) {
  const stagingSha = readOptionalRef(git, docsRoot, DOCS_STAGING_REF);
  if (!stagingSha) return { disposition: "none" };

  const raw = git.run(docsRoot, [
    "show",
    "-s",
    "--format=%T%x00%P%x00%ae%x00%an%x00%ce%x00%cn%x00%s",
    stagingSha,
  ]).stdout;
  let buffer = raw;
  if (buffer.at(-1) === 0x0a) buffer = buffer.subarray(0, -1);
  const fields = splitNulFields(buffer);
  if (fields.length !== 7) {
    fail(ErrorCode.DOCS_STAGING_INVALID, "staging commit metadata is malformed", { stagingSha });
  }
  const [treeSha, parentsRaw, authorEmail, authorName, committerEmail, committerName, subject] =
    fields;
  const parents = parentsRaw.split(" ").filter(Boolean);
  const marker = parseDocsCommitSubject(subject, contract);
  const expectedManifest = buildContentManifest(expectedEntries);
  let actualManifest;
  try {
    actualManifest = readCommitContentManifest(git, docsRoot, stagingSha);
  } catch (error) {
    fail(ErrorCode.DOCS_STAGING_INVALID, "staging commit tree cannot be audited", {
      stagingSha,
      cause: error.code || error.message,
    });
  }
  const treeDiff = diffContentManifests(expectedManifest, actualManifest);

  // Identity failures are a separate production code from structural staging
  // invalidity (tree / parent shape / marker). Keep details field-scoped so a
  // single mismatch does not dump unrelated identity or tree payloads.
  if (authorEmail !== contract.requiredEmail) {
    fail(ErrorCode.DOCS_IDENTITY, "staging author email mismatch", {
      stagingSha,
      authorEmail,
    });
  }
  if (authorName !== contract.requiredName) {
    fail(ErrorCode.DOCS_IDENTITY, "staging author name mismatch", {
      stagingSha,
      authorName,
    });
  }
  if (committerEmail !== contract.requiredEmail) {
    fail(ErrorCode.DOCS_IDENTITY, "staging committer email mismatch", {
      stagingSha,
      committerEmail,
    });
  }
  if (committerName !== contract.requiredName) {
    fail(ErrorCode.DOCS_IDENTITY, "staging committer name mismatch", {
      stagingSha,
      committerName,
    });
  }

  if (
    !isFullSha(treeSha) ||
    parents.length > 1 ||
    !marker ||
    marker.sourceSha !== git.textTrim(sourceRoot, ["rev-parse", "HEAD"]) ||
    !treeDiff.equal
  ) {
    fail(ErrorCode.DOCS_STAGING_INVALID, "staging recovery commit violates the current sync contract", {
      stagingSha,
      parents,
      markerValid: Boolean(marker),
      markerSourceSha: marker?.sourceSha ?? null,
      treeEqual: treeDiff.equal,
    });
  }

  const parentSha = parents[0] || null;
  const remoteSha = queryDocsRemoteTip(git, docsRoot, contract);
  if (remoteSha === stagingSha) {
    finalizeLocalCheckout(git, docsRoot, stagingSha, contract, {
      operationPort: options.checkoutOperationPort,
    });
    deleteStagingRef(git, docsRoot, stagingSha);
    return { disposition: "remote_landed", stagingSha, remoteSha };
  }
  if ((remoteSha || null) !== parentSha) {
    fail(ErrorCode.DOCS_STAGING_DIVERGED, "staging parent no longer equals docs remote", {
      stagingSha,
      parentSha,
      remoteSha,
    });
  }

  let pushResult;
  try {
    pushResult = pushDocsFastForward(git, docsRoot, stagingSha, parentSha, contract);
  } catch (error) {
    if (error instanceof SyncError && error.code === ErrorCode.DOCS_PUSH_REJECTED) {
      deleteStagingRef(git, docsRoot, stagingSha);
    }
    throw error;
  }
  finalizeLocalCheckout(git, docsRoot, stagingSha, contract, {
    operationPort: options.checkoutOperationPort,
  });
  deleteStagingRef(git, docsRoot, stagingSha);
  return { disposition: "resumed_push", stagingSha, remoteSha: pushResult.remoteSha };
}

// ---------------------------------------------------------------------------
// Orchestration: check / sync
// ---------------------------------------------------------------------------

/**
 * @param {object} options
 * @param {typeof PRODUCTION_CONTRACT} [options.contract]
 * @param {ReturnType<typeof createGitPort>} [options.git]
 * @param {string} [options.sourceRoot]
 * @param {string} [options.docsRoot]
 * @param {string} [options.sourceRemoteUrl]
 * @param {string} [options.docsRemoteUrl]
 * @param {boolean} [options.skipLock]
 */
export function runCheck(options = {}) {
  const contract = options.contract || PRODUCTION_CONTRACT;
  const git = options.git || createGitPort();
  const sourceRoot = options.sourceRoot || contract.sourceRepoRoot;
  const docsRoot = options.docsRoot || contract.docsCheckoutRoot;

  let lock = null;
  if (!options.skipLock) {
    lock = acquireLock(contract.lockPath);
  }
  try {
    const source = inspectSourceRepository(git, {
      ...contract,
      sourceRepoRoot: sourceRoot,
      sourceRemoteUrl: options.sourceRemoteUrl || contract.sourceRemoteUrl,
    }, { sourceRoot });

    const snapshot = buildSourceSnapshot(git, sourceRoot, source.headSha);
    validateFileManifest(snapshot);
    const expectedEntries = buildExpectedDocsEntries(snapshot, contract);
    const expectedManifest = buildContentManifest(expectedEntries);
    const sourceFirstParentIndex = buildSourceFirstParentIndex(
      git,
      sourceRoot,
      source.headSha,
    );
    const docsRemoteUrl = options.docsRemoteUrl || contract.docsRemoteUrl;
    const localCheckout = inspectExistingDocsCheckout(
      git,
      docsRoot,
      contract,
      docsRemoteUrl,
    );
    const auditRepo = createTemporaryDocsAuditRepo(git, contract, docsRemoteUrl);
    try {
      const remoteSha = fetchDocsRemoteTip(git, auditRepo.root, contract);
      let historyAudit = null;
      let docsRemoteSourceSha = null;
      let docsRemoteTreeSha = null;
      if (remoteSha) {
        const history = loadDocsFirstParentHistory(git, auditRepo.root, remoteSha, contract);
        historyAudit = auditDocsHistory(
          git,
          auditRepo.root,
          sourceRoot,
          history,
          contract,
          sourceFirstParentIndex,
        );
        docsRemoteSourceSha = historyAudit.lastSourceSha;
        docsRemoteTreeSha = history.at(-1).treeSha;
      }

      const { treeSha: expectedTreeSha } = materializeDocsTree(
        git,
        auditRepo.root,
        expectedEntries,
      );
      const plan = planSyncAction({
        sourceSha: source.headSha,
        docsRemoteSha: remoteSha,
        docsRemoteSourceSha,
        docsRemoteTreeSha,
        expectedTreeSha,
        docsExists: Boolean(remoteSha),
      });
      if (plan.action !== "noop_idempotent") {
        fail(ErrorCode.PLAN, "docs repository is not in sync with source HEAD", {
          plan,
          sourceSha: source.headSha,
          docsRemoteSha: remoteSha,
          docsRemoteSourceSha,
          localCheckout,
        });
      }
      return {
        ok: true,
        mode: "check",
        inSync: true,
        sourceSha: source.headSha,
        expectedTreeSha,
        docsRemoteSha: remoteSha,
        docsRemoteSourceSha,
        plan,
        localCheckout,
        expectedPathCount: expectedEntries.length,
        historyLength: historyAudit?.length ?? 0,
        expectedManifestSize: expectedManifest.size,
      };
    } finally {
      auditRepo.cleanup();
    }
  } finally {
    if (lock) lock.release();
  }
}

/**
 * @param {object} options
 * same as runCheck; sync always performs a real push to the configured remote
 */
export function runSync(options = {}) {
  const contract = options.contract || PRODUCTION_CONTRACT;
  const git = options.git || createGitPort();
  const sourceRoot = options.sourceRoot || contract.sourceRepoRoot;
  const docsRoot = options.docsRoot || contract.docsCheckoutRoot;

  let lock = null;
  if (!options.skipLock) {
    lock = acquireLock(contract.lockPath);
  }
  try {
    const source = inspectSourceRepository(
      git,
      {
        ...contract,
        sourceRepoRoot: sourceRoot,
        sourceRemoteUrl: options.sourceRemoteUrl || contract.sourceRemoteUrl,
      },
      { sourceRoot },
    );

    const snapshot = buildSourceSnapshot(git, sourceRoot, source.headSha);
    validateFileManifest(snapshot);
    const sourceFirstParentIndex = buildSourceFirstParentIndex(
      git,
      sourceRoot,
      source.headSha,
    );
    const expectedEntries = buildExpectedDocsEntries(snapshot, contract);

    ensureDocsRepoShell(
      git,
      {
        ...contract,
        docsCheckoutRoot: docsRoot,
        docsRemoteUrl: options.docsRemoteUrl || contract.docsRemoteUrl,
      },
      {
        docsRoot,
        docsRemoteUrl: options.docsRemoteUrl || contract.docsRemoteUrl,
      },
    );

    const recovery = recoverDocsStaging(
      git,
      docsRoot,
      sourceRoot,
      contract,
      expectedEntries,
      { checkoutOperationPort: options.checkoutOperationPort },
    );
    if (recovery.disposition !== "none") {
      return {
        ok: true,
        mode: "sync",
        action: "recover_staging",
        sourceSha: source.headSha,
        docsCommitSha: recovery.stagingSha,
        docsRemoteSha: recovery.remoteSha,
        recovery,
        pushed: recovery.disposition === "resumed_push",
      };
    }

    const remoteSha = fetchDocsRemoteTip(git, docsRoot, {
      ...contract,
      docsRemoteUrl: options.docsRemoteUrl || contract.docsRemoteUrl,
    });

    let historyAudit = null;
    let docsRemoteSourceSha = null;
    let docsRemoteTreeSha = null;
    /** @type {import("./sync-docs-repository.mjs").DocsHistoryCommit[]|null} */
    let history = null;
    if (remoteSha) {
      history = loadDocsFirstParentHistory(git, docsRoot, remoteSha, contract);
      historyAudit = auditDocsHistory(
        git,
        docsRoot,
        sourceRoot,
        history,
        contract,
        sourceFirstParentIndex,
      );
      docsRemoteSourceSha = historyAudit.lastSourceSha;
      docsRemoteTreeSha = history[history.length - 1].treeSha;

      // Equal is the idempotent case handled by the plan; otherwise the marker
      // must precede live HEAD on the same first-parent index.
      const lastPosition = sourceFirstParentIndex.positionBySha.get(docsRemoteSourceSha);
      const headPosition = sourceFirstParentIndex.positionBySha.get(source.headSha);
      if (lastPosition === undefined || headPosition === undefined || lastPosition > headPosition) {
        fail(ErrorCode.DOCS_HISTORY, "last docs marker does not precede live source HEAD", {
          docsRemoteSourceSha,
          sourceHead: source.headSha,
        });
      }
    }

    const { treeSha: expectedTreeSha } = materializeDocsTree(
      git,
      docsRoot,
      expectedEntries,
    );

    const plan = planSyncAction({
      sourceSha: source.headSha,
      docsRemoteSha: remoteSha,
      docsRemoteSourceSha,
      docsRemoteTreeSha,
      expectedTreeSha,
      docsExists: Boolean(remoteSha),
    });

    if (plan.action === "noop_idempotent") {
      // Still ensure local checkout ff-only matches remote.
      const local = finalizeLocalCheckout(git, docsRoot, remoteSha, contract, {
        operationPort: options.checkoutOperationPort,
      });
      return {
        ok: true,
        mode: "sync",
        action: plan.action,
        sourceSha: source.headSha,
        docsCommitSha: remoteSha,
        expectedTreeSha,
        local,
        pushed: false,
      };
    }

    const parentSha = remoteSha;
    const message =
      plan.action === "bootstrap"
        ? formatBootstrapCommitMessage(source.headSha, contract)
        : formatSyncCommitMessage(source.headSha, contract);

    // Dual remote revalidation immediately before commit+push.
    const revalidated = fetchDocsRemoteTip(git, docsRoot, {
      ...contract,
      docsRemoteUrl: options.docsRemoteUrl || contract.docsRemoteUrl,
    });
    if ((revalidated || null) !== (remoteSha || null)) {
      fail(ErrorCode.DOCS_REMOTE_DIVERGED, "docs remote raced before commit", {
        revalidated,
        remoteSha,
      });
    }

    let retainRecovery = false;
    let newCommit = null;
    try {
      newCommit = commitTree(git, docsRoot, {
        treeSha: expectedTreeSha,
        parentSha,
        message,
        email: contract.requiredEmail,
        name: contract.requiredName,
      });
      git.run(docsRoot, ["update-ref", DOCS_STAGING_REF, newCommit]);

      let pushResult;
      try {
        pushResult = pushDocsFastForward(
          git,
          docsRoot,
          newCommit,
          parentSha,
          contract,
        );
      } catch (error) {
        retainRecovery = error instanceof SyncError && error.code === ErrorCode.DOCS_PUSH_UNKNOWN;
        throw error;
      }

      const finalRemoteSha = pushResult.remoteSha;
      const local = finalizeLocalCheckout(git, docsRoot, finalRemoteSha, contract, {
        operationPort: options.checkoutOperationPort,
      });

      return {
        ok: true,
        mode: "sync",
        action: plan.action,
        sourceSha: source.headSha,
        docsCommitSha: newCommit,
        expectedTreeSha,
        pushed: Boolean(pushResult.pushed),
        docsRemoteSha: finalRemoteSha,
        local,
        message,
      };
    } finally {
      if (!retainRecovery && newCommit) {
        deleteStagingRef(git, docsRoot, newCommit);
      }
    }
  } finally {
    if (lock) lock.release();
  }
}

// ---------------------------------------------------------------------------
// Self-test (lightweight pure + plumbing smoke; full matrix in *.test.mjs)
// ---------------------------------------------------------------------------

function assert(cond, msg) {
  if (!cond) throw new Error(msg);
}

export function runSelfTest() {
  // Pure parsers.
  const sha = "a".repeat(40);
  assert(
    parseDocsCommitSubject(`文档: 同步shittim@${sha}`)?.sourceSha === sha,
    "sync subject",
  );
  assert(
    parseDocsCommitSubject(`文档: 从shittim@${sha}建立纯文档镜像`)?.kind ===
      "bootstrap",
    "bootstrap subject",
  );
  assert(parseDocsCommitSubject("nope") === null, "reject unknown subject");
  assert(parseDocsCommitSubject(`文档: 同步shittim@${sha}x`) === null, "reject trailing");

  // Path safety.
  let threw = false;
  try {
    assertSafeRepoRelativePath("../x.md");
  } catch (e) {
    threw = e instanceof SyncError;
  }
  assert(threw, "reject .. path");

  // Manifest diff.
  const a = buildContentManifest([
    { path: "A.md", mode: "100644", bytes: Buffer.from("a\n") },
  ]);
  const b = buildContentManifest([
    { path: "A.md", mode: "100644", bytes: Buffer.from("b\n") },
  ]);
  const d = diffContentManifests(a, b);
  assert(!d.equal && d.changed.includes("A.md"), "manifest change detected");

  // Plan pure.
  const p1 = planSyncAction({
    sourceSha: sha,
    docsRemoteSha: null,
    docsRemoteSourceSha: null,
    docsRemoteTreeSha: null,
    expectedTreeSha: "b".repeat(40),
    docsExists: false,
  });
  assert(p1.action === "bootstrap", "plan bootstrap");

  const p2 = planSyncAction({
    sourceSha: sha,
    docsRemoteSha: "c".repeat(40),
    docsRemoteSourceSha: sha,
    docsRemoteTreeSha: "b".repeat(40),
    expectedTreeSha: "b".repeat(40),
    docsExists: true,
  });
  assert(p2.action === "noop_idempotent", "plan idempotent");

  const p3 = planSyncAction({
    sourceSha: sha,
    docsRemoteSha: "c".repeat(40),
    docsRemoteSourceSha: "d".repeat(40),
    docsRemoteTreeSha: "b".repeat(40),
    expectedTreeSha: "b".repeat(40),
    docsExists: true,
  });
  assert(p3.action === "append_receipt", "plan receipt");

  // Gitignore ledger hash stability.
  const giHash = createHash("sha256").update(DOCS_GITIGNORE_BYTES).digest("hex");
  assert(giHash.length === 64, "gitignore hash");
  assert(DOCS_GITIGNORE_BYTES.includes(Buffer.from("/schemas/\n", "utf8")), "gitignore schemas");

  // NUL split + UTF-8.
  const paths = splitNulPaths(Buffer.from("a.md\0b.md\0", "utf8"));
  assert(paths[0] === "a.md" && paths[1] === "b.md", "nul split");

  const markdownPaths = ["A.md", FILE_MANIFEST_REL].sort(compareGitPath);
  const manifestLineCount = FILE_MANIFEST_HEADER.length + markdownPaths.length;
  const entries = [];
  for (const path of markdownPaths) {
    let bytes;
    if (path === FILE_MANIFEST_REL) {
      bytes = Buffer.from(`${"x\n".repeat(manifestLineCount)}`, "utf8");
    } else {
      bytes = Buffer.from("x\n", "utf8");
    }
    entries.push({
      path,
      mode: "100644",
      blobSha: "0".repeat(40),
      bytes,
    });
  }
  entries.push({ path: "LICENSE", mode: "100644", blobSha: "0".repeat(40), bytes: Buffer.from("L\n") });
  const snapshot = {
    sourceSha: "0".repeat(40),
    sourceTreeSha: "0".repeat(40),
    entries,
    byPath: new Map(entries.map((entry) => [entry.path, entry])),
  };
  // Replace the synthetic manifest bytes with a valid minimal manifest generated
  // from its own listed paths and line counts.
  const body = markdownPaths.map((path) => {
    const count = path === FILE_MANIFEST_REL ? manifestLineCount : 1;
    return `- \`${path}\` — ${count} lines`;
  });
  snapshot.byPath.get(FILE_MANIFEST_REL).bytes = Buffer.from(
    `${FILE_MANIFEST_HEADER.join("\n")}\n${body.join("\n")}\n`,
    "utf8",
  );
  validateFileManifest(snapshot);

  return { ok: true, mode: "self-test" };
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

function printSuccess(result) {
  process.stdout.write(`${JSON.stringify({ ok: true, ...result })}\n`);
}

function printFailure(error) {
  if (error instanceof SyncError) {
    process.stderr.write(`${JSON.stringify(error.toJSON())}\n`);
    process.stderr.write(`sync-docs-repository: ${error.code}: ${error.message}\n`);
    return;
  }
  const wrapped = new SyncError(
    ErrorCode.INTERNAL,
    error && error.message ? error.message : String(error),
  );
  process.stderr.write(`${JSON.stringify(wrapped.toJSON())}\n`);
  process.stderr.write(`sync-docs-repository: ${wrapped.code}: ${wrapped.message}\n`);
}

export async function main(argv, importedTestSeam = null) {
  const handlers = importedTestSeam || {
    selfTest: runSelfTest,
    check: runCheck,
    sync: runSync,
  };
  const usageError = new SyncError(
    ErrorCode.USAGE,
    "usage: sync-docs-repository.mjs --check | --sync | --self-test",
  );
  if (!Array.isArray(argv) || argv.length !== 3) {
    printFailure(usageError);
    return 1;
  }
  const mode = argv[2];
  if (mode === "--self-test") {
    try {
      const result = handlers.selfTest();
      printSuccess(result);
      return 0;
    } catch (error) {
      printFailure(error);
      return 1;
    }
  }
  if (mode === "--check") {
    try {
      const result = handlers.check();
      printSuccess(result);
      return 0;
    } catch (error) {
      printFailure(error);
      return 1;
    }
  }
  if (mode === "--sync") {
    try {
      const result = handlers.sync();
      printSuccess(result);
      return 0;
    } catch (error) {
      printFailure(error);
      return 1;
    }
  }
  printFailure(usageError);
  return 1;
}

const isMain =
  process.argv[1] &&
  fileURLToPath(import.meta.url) === resolve(process.argv[1]);

if (isMain) {
  main(process.argv).then(
    (code) => process.exit(code),
    (error) => {
      printFailure(error);
      process.exit(1);
    },
  );
}
