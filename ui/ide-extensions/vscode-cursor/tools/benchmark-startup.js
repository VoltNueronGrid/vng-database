#!/usr/bin/env node

/**
 * Lightweight startup benchmark harness.
 * Measures cold load time for out/extension.js in isolated Node processes.
 */

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const runsArgIndex = process.argv.indexOf("--runs");
const runs = runsArgIndex >= 0 ? Number(process.argv[runsArgIndex + 1]) : 10;

if (!Number.isInteger(runs) || runs <= 0) {
  console.error("Invalid --runs value. Use a positive integer, e.g. --runs 10");
  process.exit(1);
}

const extensionEntry = path.join(process.cwd(), "out", "extension.js");
if (!fs.existsSync(extensionEntry)) {
  console.error(`Missing compiled extension entry: ${extensionEntry}`);
  console.error("Run `npm run build` first.");
  process.exit(1);
}

const childScript = `
  const path = require("node:path");
  const Module = require("node:module");
  const extensionPath = process.argv[1];

  function createCallableProxy() {
    let proxy;
    const fn = function () {
      return proxy;
    };
    proxy = new Proxy(fn, {
      get(_target, prop) {
        if (prop === Symbol.toPrimitive) {
          return () => "";
        }
        if (prop === "toString") {
          return () => "";
        }
        if (prop === "valueOf") {
          return () => 0;
        }
        return proxy;
      },
      apply() {
        return proxy;
      },
      construct() {
        return proxy;
      },
    });
    return proxy;
  }

  const vscodeMock = createCallableProxy();
  const originalLoad = Module._load;
  Module._load = function(request, parent, isMain) {
    if (request === "vscode") {
      return vscodeMock;
    }
    return originalLoad(request, parent, isMain);
  };

  const start = process.hrtime.bigint();
  require(path.resolve(extensionPath));
  const end = process.hrtime.bigint();
  const elapsedMs = Number(end - start) / 1e6;
  process.stdout.write(JSON.stringify({ elapsedMs }));
`;

const samples = [];
for (let i = 0; i < runs; i += 1) {
  const result = spawnSync(process.execPath, ["-e", childScript, extensionEntry], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });

  if (result.status !== 0) {
    console.error(`Benchmark run ${i + 1} failed.`);
    console.error(result.stderr || result.stdout);
    process.exit(result.status || 1);
  }

  let parsed;
  try {
    parsed = JSON.parse(result.stdout.trim());
  } catch (error) {
    console.error(`Benchmark run ${i + 1} produced non-JSON output: ${result.stdout}`);
    process.exit(1);
  }

  samples.push(parsed.elapsedMs);
}

samples.sort((a, b) => a - b);

function percentile(values, p) {
  if (values.length === 0) {
    return 0;
  }
  const idx = Math.min(values.length - 1, Math.ceil((p / 100) * values.length) - 1);
  return values[idx];
}

const total = samples.reduce((acc, value) => acc + value, 0);
const summary = {
  benchmark: "cold-module-load",
  entry: "out/extension.js",
  runs,
  minMs: samples[0],
  avgMs: total / samples.length,
  p95Ms: percentile(samples, 95),
  maxMs: samples[samples.length - 1],
};

console.log(JSON.stringify(summary, null, 2));