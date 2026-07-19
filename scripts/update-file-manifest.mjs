#!/usr/bin/env node
/**
 * Generate or check FILE_MANIFEST.md from the Git Markdown source set.
 *
 * Source set = tracked `git ls-files '*.md'` plus untracked non-ignored
 * `git ls-files --others --exclude-standard '*.md'`. Never scans ignored
 * build products (target/, node_modules/, .git, ...). Paths use Git -z
 * bytes decoded as strict UTF-8 (no soft replacement, no C-style quote
 * escaping). Order follows Git ls-files for non-manifest paths;
 * FILE_MANIFEST.md is re-inserted in path-sorted position with a closed-form
 * self line count.
 *
 * Usage:
 *   node scripts/update-file-manifest.mjs --write
 *   node scripts/update-file-manifest.mjs --check
 *   node scripts/update-file-manifest.mjs --self-test
 */
import { execFileSync } from "node:child_process";
import {
  chmodSync,
  closeSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const MANIFEST_REL = "FILE_MANIFEST.md";
const SELF_TEST_ROOT = "/mnt/data/shittim-file-manifest-tests";
const SELF_TEST_LOCK = join(SELF_TEST_ROOT, ".self-test.lock");
const HEADER_LINES = [
  "# FILE_MANIFEST",
  "",
  "> 非规范元数据。列出 Git source set 中的 Markdown（tracked `git ls-files '*.md'` + 标准 ignore 下 untracked source）；不含 ignored build 产物（如 target/、node_modules/）。行数以 UTF-8 文本 `wc -l` 等价结果为准。由 `scripts/update-file-manifest.mjs` 生成，禁止手改。",
  "",
];

const scriptDir = dirname(fileURLToPath(import.meta.url));
const defaultRepoRoot = join(scriptDir, "..");

/** Strict UTF-8 decoder: invalid sequences throw (fail closed, no soft replace). */
const strictUtf8Decoder = new TextDecoder("utf-8", { fatal: true });

function fail(message) {
  console.error(`update-file-manifest: ${message}`);
  process.exit(1);
}

function usage() {
  fail("usage: update-file-manifest.mjs --write | --check | --self-test");
}

/**
 * Decode a Git -z path segment as strict UTF-8.
 * Fail closed on invalid UTF-8 (no soft replacement).
 * Also rejects Buffer→string soft-decode mismatches via roundtrip check.
 */
function decodeGitPathBytes(pathBytes) {
  if (!Buffer.isBuffer(pathBytes)) {
    throw new Error("path segment must be a Buffer");
  }
  let text;
  try {
    text = strictUtf8Decoder.decode(pathBytes);
  } catch {
    throw new Error(
      `path is not valid UTF-8: ${pathBytes.toString("hex")}`,
    );
  }
  // Roundtrip: reject any decoder/string edge that would not re-encode identically.
  const reencoded = Buffer.from(text, "utf8");
  if (
    reencoded.length !== pathBytes.length ||
    !reencoded.equals(pathBytes)
  ) {
    throw new Error(
      `path UTF-8 roundtrip mismatch: ${pathBytes.toString("hex")}`,
    );
  }
  return text;
}

/** UTF-8 wc -l equivalent: count LF bytes in string content. */
function countNewlines(content) {
  let n = 0;
  for (let i = 0; i < content.length; i += 1) {
    if (content.charCodeAt(i) === 10) n += 1;
  }
  return n;
}

/** UTF-8 wc -l equivalent for a file on disk (byte LF count). */
function countFileNewlines(absPath) {
  const buf = readFileSync(absPath);
  let n = 0;
  for (let i = 0; i < buf.length; i += 1) {
    if (buf[i] === 10) n += 1;
  }
  return n;
}

function gitLsMarkdown(repoRoot, extraArgs) {
  const args = ["-C", repoRoot, "ls-files", "-z", ...extraArgs, "--", "*.md"];
  let stdout;
  try {
    stdout = execFileSync("git", args, {
      encoding: "buffer",
      stdio: ["ignore", "pipe", "pipe"],
      maxBuffer: 32 * 1024 * 1024,
    });
  } catch (error) {
    const errText = error.stderr?.toString("utf8")?.trim() || error.message;
    fail(`git ${args.slice(2).join(" ")} failed: ${errText}`);
  }

  const paths = [];
  let start = 0;
  for (let i = 0; i < stdout.length; i += 1) {
    if (stdout[i] === 0) {
      if (i > start) {
        try {
          paths.push(decodeGitPathBytes(stdout.subarray(start, i)));
        } catch (error) {
          fail(error.message);
        }
      }
      start = i + 1;
    }
  }
  if (start < stdout.length) {
    try {
      paths.push(decodeGitPathBytes(stdout.subarray(start)));
    } catch (error) {
      fail(error.message);
    }
  }
  return paths;
}

/** Compare paths as raw UTF-8 bytes (matches `git ls-files` sort). */
function compareGitPath(a, b) {
  const ba = Buffer.from(a, "utf8");
  const bb = Buffer.from(b, "utf8");
  const n = Math.min(ba.length, bb.length);
  for (let i = 0; i < n; i += 1) {
    if (ba[i] !== bb[i]) return ba[i] - bb[i];
  }
  return ba.length - bb.length;
}

/**
 * Git source Markdown paths: tracked + untracked non-ignored, Git byte order,
 * excluding FILE_MANIFEST.md (caller re-inserts it with a stable line count).
 */
function listSourceMarkdown(repoRoot) {
  const tracked = gitLsMarkdown(repoRoot, []);
  const untracked = gitLsMarkdown(repoRoot, ["--others", "--exclude-standard"]);
  const seen = new Set();
  const ordered = [];
  for (const path of [...tracked, ...untracked]) {
    if (path === MANIFEST_REL) continue;
    if (seen.has(path)) continue;
    seen.add(path);
    ordered.push(path);
  }
  ordered.sort(compareGitPath);
  return ordered;
}

function entryLine(path, lines) {
  return `- \`${path}\` — ${lines} lines`;
}

/** Insert MANIFEST_REL into a Git-byte-ordered path list. */
function insertManifestPath(orderedOthers) {
  const withManifest = [];
  let inserted = false;
  for (const path of orderedOthers) {
    if (!inserted && compareGitPath(MANIFEST_REL, path) < 0) {
      withManifest.push(MANIFEST_REL);
      inserted = true;
    }
    withManifest.push(path);
  }
  if (!inserted) withManifest.push(MANIFEST_REL);
  return withManifest;
}

/**
 * Build manifest text. Self line count is closed-form:
 * header lines + entry lines (one per path including self), trailing newline.
 */
function buildManifestText(repoRoot) {
  const others = listSourceMarkdown(repoRoot);
  const lineByPath = new Map();

  for (const path of others) {
    const abs = join(repoRoot, path);
    try {
      lineByPath.set(path, countFileNewlines(abs));
    } catch (error) {
      fail(`failed to read ${path}: ${error.message}`);
    }
  }

  const withManifest = insertManifestPath(others);
  const totalLines = HEADER_LINES.length + withManifest.length;
  lineByPath.set(MANIFEST_REL, totalLines);

  const body = withManifest.map((path) =>
    entryLine(path, lineByPath.get(path)),
  );
  return `${HEADER_LINES.join("\n")}\n${body.join("\n")}\n`;
}

function writeManifest(repoRoot) {
  const text = buildManifestText(repoRoot);
  writeFileSync(join(repoRoot, MANIFEST_REL), text, "utf8");
  const lines = countNewlines(text);
  const entries = withManifestEntryCount(text);
  console.log(
    `update-file-manifest: wrote ${MANIFEST_REL} (${entries} entries, ${lines} lines)`,
  );
}

function withManifestEntryCount(text) {
  let n = 0;
  for (const line of text.split("\n")) {
    if (line.startsWith("- `")) n += 1;
  }
  return n;
}

function checkManifest(repoRoot) {
  const expected = buildManifestText(repoRoot);
  const abs = join(repoRoot, MANIFEST_REL);
  let actual;
  try {
    actual = readFileSync(abs, "utf8");
  } catch (error) {
    fail(`missing ${MANIFEST_REL}: ${error.message}`);
  }
  if (actual !== expected) {
    fail(
      `${MANIFEST_REL} is out of date; run: node scripts/update-file-manifest.mjs --write`,
    );
  }
  console.log("update-file-manifest: check ok");
}

function runGit(cwd, args) {
  return execFileSync("git", args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function assertThrows(fn, messageSubstring) {
  let threw = false;
  try {
    fn();
  } catch (error) {
    threw = true;
    if (messageSubstring && !String(error.message).includes(messageSubstring)) {
      throw new Error(
        `expected error containing ${JSON.stringify(messageSubstring)}, got ${JSON.stringify(error.message)}`,
      );
    }
  }
  if (!threw) {
    throw new Error(`expected throw for: ${messageSubstring || "predicate"}`);
  }
}

function createSelfTestWorkspace(prefix) {
  mkdirSync(SELF_TEST_ROOT, { recursive: true, mode: 0o700 });
  chmodSync(SELF_TEST_ROOT, 0o700);
  const root = mkdtempSync(join(SELF_TEST_ROOT, prefix));
  chmodSync(root, 0o700);
  return root;
}

function acquireSelfTestLock() {
  mkdirSync(SELF_TEST_ROOT, { recursive: true, mode: 0o700 });
  chmodSync(SELF_TEST_ROOT, 0o700);
  let fd;
  try {
    fd = openSync(SELF_TEST_LOCK, "wx", 0o600);
    writeFileSync(fd, `${process.pid}\n`, "utf8");
  } catch (error) {
    if (fd !== undefined) closeSync(fd);
    throw new Error(`cannot acquire self-test lock ${SELF_TEST_LOCK}: ${error.message}`);
  }
  return {
    release() {
      closeSync(fd);
      rmSync(SELF_TEST_LOCK, { force: true });
    },
  };
}

function selfTestUtf8Helper() {
  // Valid UTF-8 path bytes roundtrip.
  const valid = Buffer.from("docs/中文.md", "utf8");
  assert(
    decodeGitPathBytes(valid) === "docs/中文.md",
    "valid multi-byte UTF-8 path must decode",
  );

  // Invalid sequences must fail closed (no soft U+FFFD replacement).
  const invalids = [
    Buffer.from([0x80]), // lone continuation
    Buffer.from([0xc3]), // truncated 2-byte
    Buffer.from([0xe2, 0x82]), // truncated 3-byte
    Buffer.from([0xf0, 0x9f, 0x92]), // truncated 4-byte
    Buffer.from([0xff]), // illegal byte
    Buffer.from([0xed, 0xa0, 0x80]), // UTF-16 surrogate half encoded as UTF-8
    Buffer.from([0x62, 0x61, 0x64, 0x2d, 0xff, 0x2e, 0x6d, 0x64]), // bad-\xff.md
  ];
  for (const bad of invalids) {
    assertThrows(() => decodeGitPathBytes(bad), "not valid UTF-8");
  }
}

/**
 * Best-effort: create a Git-tracked path whose on-disk name is invalid UTF-8
 * and assert each NUL path segment from `git ls-files -z` fails closed under
 * decodeGitPathBytes. Does not call fail()/process.exit.
 * If the OS/filesystem refuses the name, unit helper coverage still holds.
 */
function selfTestInvalidUtf8GitPathIfPossible() {
  const root = createSelfTestWorkspace("badutf8-");
  try {
    runGit(root, ["init"]);
    runGit(root, ["config", "user.email", "test@example.com"]);
    runGit(root, ["config", "user.name", "test"]);

    // On Unix, Node can write a Buffer path with illegal UTF-8 bytes.
    // Stage via plumbing (update-index --index-info) so raw path bytes enter the index
    // without shell/pathspec UTF-8 assumptions.
    const badRel = Buffer.from([
      0x62, 0x61, 0x64, 0x2d, 0xff, 0x2e, 0x6d, 0x64,
    ]); // bad-\xff.md
    const badAbs = Buffer.concat([Buffer.from(`${root}/`, "utf8"), badRel]);
    let created = false;
    try {
      writeFileSync(badAbs, "x\n");
      const blob = execFileSync(
        "git",
        ["-C", root, "hash-object", "-w", "--stdin"],
        {
          input: Buffer.from("x\n"),
          stdio: ["pipe", "pipe", "pipe"],
        },
      )
        .toString("utf8")
        .trim();
      // index-info line: mode SP sha1 TAB path LF  (path is raw bytes)
      const indexLine = Buffer.concat([
        Buffer.from(`100644 ${blob}\t`, "utf8"),
        badRel,
        Buffer.from("\n", "utf8"),
      ]);
      execFileSync("git", ["-C", root, "update-index", "--index-info"], {
        input: indexLine,
        stdio: ["pipe", "pipe", "pipe"],
      });
      created = true;
    } catch {
      // Filesystem or git refused invalid path; unit helper still covers fail-closed.
      created = false;
    }

    if (!created) {
      return { attempted: true, created: false };
    }

    const stdout = execFileSync(
      "git",
      ["-C", root, "ls-files", "-z", "--", "*.md"],
      { encoding: "buffer", stdio: ["ignore", "pipe", "pipe"] },
    );
    let sawInvalid = false;
    let start = 0;
    for (let i = 0; i < stdout.length; i += 1) {
      if (stdout[i] === 0) {
        if (i > start) {
          const seg = stdout.subarray(start, i);
          try {
            decodeGitPathBytes(seg);
          } catch (error) {
            assert(
              String(error.message).includes("not valid UTF-8") ||
                String(error.message).includes("UTF-8"),
              `expected UTF-8 failure, got: ${error.message}`,
            );
            sawInvalid = true;
          }
        }
        start = i + 1;
      }
    }
    if (start < stdout.length) {
      try {
        decodeGitPathBytes(stdout.subarray(start));
      } catch (error) {
        assert(
          String(error.message).includes("not valid UTF-8") ||
            String(error.message).includes("UTF-8"),
          `expected UTF-8 failure, got: ${error.message}`,
        );
        sawInvalid = true;
      }
    }
    assert(
      sawInvalid,
      "invalid UTF-8 git path must fail closed under decodeGitPathBytes",
    );
    return { attempted: true, created: true };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function selfTest() {
  const lock = acquireSelfTestLock();
  try {
    selfTestUtf8Helper();

    // Git-level invalid path: best-effort on Unix; helper unit is authoritative.
    const gitUtf8 = selfTestInvalidUtf8GitPathIfPossible();
    assert(gitUtf8.attempted, "invalid UTF-8 git probe must run");

    const root = createSelfTestWorkspace("fixture-");
    try {
      runGit(root, ["init"]);
      runGit(root, ["config", "user.email", "test@example.com"]);
      runGit(root, ["config", "user.name", "test"]);
      writeFileSync(
        join(root, ".gitignore"),
        "target/\nnode_modules/\n",
        "utf8",
      );
      writeFileSync(join(root, "README.md"), "# hi\n\nbody\n", "utf8");
      writeFileSync(join(root, "AGENT.md"), "agent\n", "utf8");
      mkdirSync(join(root, "docs"), { recursive: true });
      writeFileSync(join(root, "docs", "a.md"), "one\n", "utf8");
      mkdirSync(join(root, "target", "doc"), { recursive: true });
      writeFileSync(
        join(root, "target", "doc", "junk.md"),
        "ignored build product\n",
        "utf8",
      );
      runGit(root, ["add", ".gitignore", "README.md", "AGENT.md", "docs/a.md"]);
      runGit(root, ["commit", "-m", "init"]);

      // Untracked non-ignored source md must enter.
      writeFileSync(join(root, "docs", "new.md"), "new\nfile\n", "utf8");

      const listed = listSourceMarkdown(root);
      assert(
        !listed.some((p) => p.includes("target")),
        "ignored target md must not enter source set",
      );
      assert(
        listed.includes("docs/new.md"),
        "untracked non-ignored md must enter",
      );
      assert(listed.includes("README.md"), "tracked md must enter");
      assert(!listed.includes(MANIFEST_REL), "manifest excluded before reinsert");

      const text = buildManifestText(root);
      assert(
        !/^- `[^`]*target\//m.test(text),
        "generated manifest must not list target paths",
      );
      assert(text.includes("- `docs/new.md`"), "manifest must list untracked md");
      assert(
        text.includes(`- \`${MANIFEST_REL}\``),
        "manifest must list itself",
      );

      const expectedSelf = countNewlines(text);
      const selfMatch = text.match(
        new RegExp(`^- \\\`${MANIFEST_REL}\\\` — (\\d+) lines$`, "m"),
      );
      assert(selfMatch, "self entry missing");
      assert(
        Number(selfMatch[1]) === expectedSelf,
        `self lines ${selfMatch[1]} !== ${expectedSelf}`,
      );

      writeFileSync(join(root, MANIFEST_REL), text, "utf8");
      assert(
        buildManifestText(root) === text,
        "regenerate must be stable after write",
      );

      writeFileSync(
        join(root, MANIFEST_REL),
        text.replace("README.md", "README.MD"),
        "utf8",
      );
      assert(
        readFileSync(join(root, MANIFEST_REL), "utf8") !==
          buildManifestText(root),
        "check must detect drift",
      );

      writeFileSync(join(root, MANIFEST_REL), buildManifestText(root), "utf8");
      assert(
        readFileSync(join(root, MANIFEST_REL), "utf8") ===
          buildManifestText(root),
        "check should pass after rewrite",
      );

      console.log("update-file-manifest: self-test ok");
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  } finally {
    lock.release();
  }
}

function main(argv) {
  const mode = argv[2];
  if (mode === "--self-test") {
    try {
      selfTest();
    } catch (error) {
      fail(`self-test failed: ${error.message}`);
    }
    return;
  }

  const repoRoot = defaultRepoRoot;
  if (mode === "--write") {
    writeManifest(repoRoot);
    return;
  }
  if (mode === "--check") {
    checkManifest(repoRoot);
    return;
  }
  usage();
}

main(process.argv);
