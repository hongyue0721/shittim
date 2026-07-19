import {
  existsSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";

const ZERO_SHA = "0".repeat(40);

function checkoutJournalPath(docsRoot) {
  return join(docsRoot, ".git", "docs-checkout-transaction.json");
}

function readCheckoutJournal(docsRoot, deps) {
  const path = checkoutJournalPath(docsRoot);
  if (!existsSync(path)) return null;
  let value;
  try {
    value = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    deps.fail(deps.ErrorCode.DOCS_CHECKOUT_RECOVERY, "checkout journal is unreadable", {
      path,
      message: error.message,
    });
  }
  if (
    value?.version !== 1 ||
    !["prepared", "materialized", "ref_updated"].includes(value.phase) ||
    (value.oldHead !== null && !deps.isFullSha(value.oldHead)) ||
    !deps.isFullSha(value.targetSha)
  ) {
    deps.fail(deps.ErrorCode.DOCS_CHECKOUT_RECOVERY, "checkout journal is invalid", { path });
  }
  return value;
}

function writeCheckoutJournal(docsRoot, journal) {
  const path = checkoutJournalPath(docsRoot);
  const temporary = `${path}.new`;
  writeFileSync(temporary, `${JSON.stringify(journal)}\n`, { mode: 0o600 });
  renameSync(temporary, path);
}

function removeCheckoutJournal(docsRoot) {
  rmSync(checkoutJournalPath(docsRoot), { force: true });
}

function operationBoundary(operationPort, name, state) {
  if (operationPort?.after) operationPort.after(name, state);
}

function currentHeadOrNull(git, docsRoot, deps) {
  const result = git.run(docsRoot, ["rev-parse", "--verify", "HEAD"], {
    allowNonZero: true,
  });
  if (result.status !== 0) return null;
  const sha = result.stdout.toString("utf8").trim();
  deps.requireFullSha(sha, "localHead");
  return sha;
}

function assertInitialCheckoutStillUntouched(git, docsRoot, deps) {
  const records = deps.parsePorcelainV2Z(
    git.run(docsRoot, ["status", "--porcelain=v2", "-z", "--untracked-files=normal"]).stdout,
  );
  const tracked = records.filter((record) => record.kind !== "?" && record.kind !== "!");
  if (tracked.length > 0) {
    deps.fail(
      deps.ErrorCode.DOCS_CHECKOUT_RECOVERY,
      "initial checkout has an unknown partial index/worktree state",
      { sample: tracked.slice(0, 8) },
    );
  }
}

/** True when index and tracked worktree bytes/modes exactly equal a commit tree. */
export function docsWorktreeMatchesCommit(git, docsRoot, commitSha, deps) {
  deps.requireFullSha(commitSha, "commitSha");
  const index = git.run(docsRoot, ["diff", "--cached", "--quiet", commitSha, "--"], {
    allowNonZero: true,
  });
  const worktree = git.run(docsRoot, ["diff", "--quiet", commitSha, "--"], {
    allowNonZero: true,
  });
  return index.status === 0 && worktree.status === 0;
}

/** Journaled checkout state machine. Remote-tracking is the last ref written. */
export function finalizeLocalCheckoutState(git, docsRoot, targetSha, contract, options, deps) {
  deps.requireFullSha(targetSha, "targetSha");
  const operationPort = options.operationPort;
  const branchRef = `refs/heads/${contract.docsBranch}`;
  const remoteTrackingRef = `refs/remotes/${contract.docsRemoteName}/${contract.docsBranch}`;

  const branch = git.textTrim(docsRoot, ["branch", "--show-current"], {
    allowNonZero: true,
  });
  const startingHead = currentHeadOrNull(git, docsRoot, deps);
  if (startingHead !== null && branch !== contract.docsBranch) {
    deps.fail(
      deps.ErrorCode.DOCS_WRONG_BRANCH,
      `docs local branch must be ${contract.docsBranch}, got ${JSON.stringify(branch)}`,
      { branch },
    );
  }

  let journal = readCheckoutJournal(docsRoot, deps);
  if (journal && journal.targetSha !== targetSha) {
    deps.fail(deps.ErrorCode.DOCS_CHECKOUT_RECOVERY, "pending checkout targets a different commit", {
      pendingTarget: journal.targetSha,
      requestedTarget: targetSha,
    });
  }

  if (!journal) {
    if (startingHead !== null && startingHead !== targetSha) {
      const isAncestor = git.run(
        docsRoot,
        ["merge-base", "--is-ancestor", startingHead, targetSha],
        { allowNonZero: true },
      );
      if (isAncestor.status !== 0) {
        deps.fail(
          deps.ErrorCode.DOCS_CHECKOUT,
          "local docs HEAD is not an ancestor of remote tip; refusing reset",
          { localSha: startingHead, targetSha },
        );
      }
    }
    if (startingHead === null) assertInitialCheckoutStillUntouched(git, docsRoot, deps);
    else deps.assertDocsWorktreeSafeForFastForward(git, docsRoot);
    journal = { version: 1, phase: "prepared", oldHead: startingHead, targetSha };
    writeCheckoutJournal(docsRoot, journal);
    operationBoundary(operationPort, "journal_written", journal);
  }

  const currentHead = currentHeadOrNull(git, docsRoot, deps);
  if (currentHead !== journal.oldHead && currentHead !== targetSha) {
    deps.fail(deps.ErrorCode.DOCS_CHECKOUT_RECOVERY, "checkout journal conflicts with current HEAD", {
      oldHead: journal.oldHead,
      currentHead,
      targetSha,
    });
  }

  const targetMaterialized = docsWorktreeMatchesCommit(git, docsRoot, targetSha, deps);
  if (!targetMaterialized) {
    if (journal.phase !== "prepared") {
      deps.fail(
        deps.ErrorCode.DOCS_CHECKOUT_RECOVERY,
        "checkout is in an unknown mixed worktree/index state",
        { phase: journal.phase, oldHead: journal.oldHead, currentHead, targetSha },
      );
    }
    if (journal.oldHead === null) {
      assertInitialCheckoutStillUntouched(git, docsRoot, deps);
    }
    deps.assertDocsWorktreeSafeForFastForward(git, docsRoot);
    deps.materializeDocsWorktreeAtCommit(git, docsRoot, targetSha);
    journal = { ...journal, phase: "materialized" };
    writeCheckoutJournal(docsRoot, journal);
    operationBoundary(operationPort, "worktree_materialized", journal);
  }

  const headBeforeRefUpdate = currentHeadOrNull(git, docsRoot, deps);
  if (headBeforeRefUpdate !== journal.oldHead && headBeforeRefUpdate !== targetSha) {
    deps.fail(deps.ErrorCode.DOCS_CHECKOUT_RECOVERY, "checkout journal conflicts with current HEAD", {
      oldHead: journal.oldHead,
      currentHead: headBeforeRefUpdate,
      targetSha,
    });
  }
  if (headBeforeRefUpdate !== targetSha) {
    if (!docsWorktreeMatchesCommit(git, docsRoot, targetSha, deps)) {
      deps.fail(
        deps.ErrorCode.DOCS_CHECKOUT_RECOVERY,
        "checkout tree changed before branch ref update",
        { targetSha },
      );
    }
    git.run(docsRoot, ["update-ref", branchRef, targetSha, journal.oldHead || ZERO_SHA]);
    journal = { ...journal, phase: "ref_updated" };
    writeCheckoutJournal(docsRoot, journal);
    operationBoundary(operationPort, "branch_ref_updated", journal);
  }
  git.run(docsRoot, ["symbolic-ref", "HEAD", branchRef]);
  operationBoundary(operationPort, "head_symbolic", journal);

  if (currentHeadOrNull(git, docsRoot, deps) !== targetSha) {
    deps.fail(
      deps.ErrorCode.DOCS_CHECKOUT_RECOVERY,
      "checkout HEAD changed before transaction completion",
      { targetSha },
    );
  }
  if (!docsWorktreeMatchesCommit(git, docsRoot, targetSha, deps)) {
    deps.fail(
      deps.ErrorCode.DOCS_CHECKOUT_RECOVERY,
      "checkout tree changed before transaction completion",
      { targetSha },
    );
  }
  git.run(docsRoot, ["update-ref", remoteTrackingRef, targetSha]);
  operationBoundary(operationPort, "remote_tracking_updated", journal);
  removeCheckoutJournal(docsRoot);
  operationBoundary(operationPort, "journal_cleared", journal);

  return {
    mode: journal.oldHead === null
      ? "initial_checkout"
      : journal.oldHead === targetSha
        ? "already_at_tip"
        : "ff_only",
    sha: targetSha,
  };
}
