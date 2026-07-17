#!/usr/bin/env node
/**
 * Zero-dependency smoke for the root Node/pnpm toolchain.
 * Reads package.json engines + packageManager, then verifies the running
 * process Node version and the pnpm executable version resolved from PATH.
 * Exit 1 on any mismatch or missing fact.
 */
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const rootDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const packageJsonPath = join(rootDir, "package.json");

function fail(message) {
  console.error(`check-node-toolchain: ${message}`);
  process.exit(1);
}

function readPackageJson() {
  let raw;
  try {
    raw = readFileSync(packageJsonPath, "utf8");
  } catch (error) {
    fail(`failed to read ${packageJsonPath}: ${error.message}`);
  }

  try {
    return JSON.parse(raw);
  } catch (error) {
    fail(`failed to parse ${packageJsonPath}: ${error.message}`);
  }
}

function requireExactEngine(engines, key) {
  const value = engines?.[key];
  if (typeof value !== "string" || value.length === 0) {
    fail(`package.json engines.${key} must be an exact version string`);
  }
  if (/[<>|=*xX~^]/.test(value) || value.includes(" ")) {
    fail(
      `package.json engines.${key} must be an exact version, got ${JSON.stringify(value)}`,
    );
  }
  return value;
}

function readActualPnpmVersion() {
  let output;
  try {
    output = execFileSync("pnpm", ["--version"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
  } catch (error) {
    fail(`failed to execute pnpm --version: ${error.message}`);
  }

  if (!/^\d+\.\d+\.\d+$/.test(output)) {
    fail(`pnpm --version must return an exact version, got ${JSON.stringify(output)}`);
  }
  return output;
}

const pkg = readPackageJson();
const expectedNode = requireExactEngine(pkg.engines, "node");
const expectedPnpm = requireExactEngine(pkg.engines, "pnpm");
const expectedPackageManager = `pnpm@${expectedPnpm}`;

if (pkg.packageManager !== expectedPackageManager) {
  fail(
    `packageManager must be ${JSON.stringify(expectedPackageManager)}, got ${JSON.stringify(pkg.packageManager)}`,
  );
}

const actualNode = process.versions.node;
if (actualNode !== expectedNode) {
  fail(`Node must be ${expectedNode}, got ${actualNode}`);
}

const actualPnpm = readActualPnpmVersion();
if (actualPnpm !== expectedPnpm) {
  fail(`pnpm must be ${expectedPnpm}, got ${actualPnpm}`);
}

console.log(
  `check-node-toolchain: ok (node ${actualNode}, pnpm ${actualPnpm}, packageManager ${pkg.packageManager})`,
);
