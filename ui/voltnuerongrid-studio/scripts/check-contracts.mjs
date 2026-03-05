import fs from "node:fs";

const required = [
  "src/api/types.ts",
  "src/api/client.ts",
];

for (const file of required) {
  if (!fs.existsSync(new URL(`../${file}`, import.meta.url))) {
    console.error(`missing file: ${file}`);
    process.exit(1);
  }
}

const typesContent = fs.readFileSync(new URL("../src/api/types.ts", import.meta.url), "utf8");
const clientContent = fs.readFileSync(new URL("../src/api/client.ts", import.meta.url), "utf8");
const requiredSnippets = [
  "trace_id",
  "/api/v1/sql/execute",
  "/api/v1/autonomous/actions/authorize",
  "/api/v1/audit/events",
  "/api/v1/autonomous/actions/records",
];

for (const snippet of requiredSnippets) {
  if (!typesContent.includes(snippet) && !clientContent.includes(snippet)) {
    console.error(`missing contract snippet: ${snippet}`);
    process.exit(1);
  }
}

console.log("studio contract check passed");
