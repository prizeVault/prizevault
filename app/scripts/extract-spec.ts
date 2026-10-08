// Extracts the contract spec (the `contractspecv0` custom section: an XDR
// stream of ScSpecEntry) from the built wasm and writes it, base64 encoded, to
// src/spec.json. The CLI and the offline tests construct a contract.Client
// from it without fetching the wasm over RPC.
//
// Usage: node scripts/extract-spec.ts [path/to/prizevault.wasm]
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { contract } from "@stellar/stellar-sdk";

const here = dirname(fileURLToPath(import.meta.url));
const wasmPath = resolve(
  process.argv[2] ?? resolve(here, "../../target/wasm32v1-none/release/prizevault.wasm"),
);
const outPath = resolve(here, "../src/spec.json");

const wasm = readFileSync(wasmPath);
const module = await WebAssembly.compile(wasm);
const sections = WebAssembly.Module.customSections(module, "contractspecv0");
if (sections.length === 0) {
  throw new Error(`${wasmPath} has no contractspecv0 section`);
}
const stream = Buffer.concat(sections.map((s) => Buffer.from(s)));
const specBase64 = stream.toString("base64");

// Round-trip through the SDK to make sure the stream parses.
const spec = new contract.Spec(specBase64);
const functions = spec.funcs().map((f) => String(f.name)).sort();

writeFileSync(
  outPath,
  JSON.stringify({ source: "prizevault.wasm", wasm_bytes: wasm.length, functions, spec_base64: specBase64 }, null, 2) +
    "\n",
);
console.log(`wrote ${outPath}: ${functions.length} functions, ${stream.length} spec bytes (${wasm.length} wasm bytes)`);
