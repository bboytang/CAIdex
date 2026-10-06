import { accessSync, appendFileSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";

const root = path.resolve(process.argv[2] === "--github-env" || !process.argv[2]
  ? ".tools/codex/node_modules/@openai/codex" : process.argv[2]);
const metadata = JSON.parse(readFileSync(path.join(root, "package.json"), "utf8"));
const lock = JSON.parse(readFileSync("upstream/codex/lock.json", "utf8"));
if (metadata.version !== lock.cliVersion) throw new Error("Codex package version differs from lock.json");

const targets = {
  "linux-x64": ["x86_64-unknown-linux-musl", "@openai/codex-linux-x64"],
  "linux-arm64": ["aarch64-unknown-linux-musl", "@openai/codex-linux-arm64"],
  "darwin-x64": ["x86_64-apple-darwin", "@openai/codex-darwin-x64"],
  "darwin-arm64": ["aarch64-apple-darwin", "@openai/codex-darwin-arm64"],
  "win32-x64": ["x86_64-pc-windows-msvc", "@openai/codex-win32-x64"],
};
const target = targets[`${process.platform}-${process.arch}`];
if (!target) throw new Error("Unsupported verification platform");
const require = createRequire(path.join(root, "package.json"));
const platformPackage = path.dirname(require.resolve(`${target[1]}/package.json`));
const executable = path.join(platformPackage, "vendor", target[0], "bin", process.platform === "win32" ? "codex.exe" : "codex");
accessSync(executable);
if (process.argv.includes("--github-env")) {
  if (!process.env.GITHUB_ENV) throw new Error("GITHUB_ENV is missing");
  appendFileSync(process.env.GITHUB_ENV, `CAIDEX_CODEX_BIN=${executable}\n`);
}
console.log(executable);
