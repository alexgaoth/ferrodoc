// Drive the page above in headless Chrome over the DevTools protocol.
//
// Two things are being proved and only a real browser can prove either:
// that the module instantiates and converts inside a tab, and that doing
// so makes **no network request** — the privacy claim is the reason this
// package exists, so it is the thing tested rather than asserted.
//
// Spoken to Chrome directly rather than through Puppeteer: the package
// has no dependencies and a test harness is a poor reason to acquire one.

import { spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const CHROME = process.env.CHROME ?? "google-chrome";
const page = fileURLToPath(new URL("./page.html", import.meta.url));
const profile = await mkdtemp(join(tmpdir(), "ferrodoc-chrome-"));
const port = 9222 + (process.pid % 900);

const chrome = spawn(CHROME, [
  "--headless=new", "--disable-gpu", "--no-sandbox",
  `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`,
  "--allow-file-access-from-files", `file://${page}`,
], { stdio: "ignore" });

// Chrome keeps writing to its profile for a moment after SIGTERM, so the
// directory has to be removed *after* it exits — and a leftover temp
// directory must never be what fails this test.
const cleanup = async () => {
  chrome.kill();
  await new Promise((resolve) => chrome.once("exit", resolve));
  await rm(profile, { recursive: true, force: true }).catch(() => {});
};
const fail = async (message) => {
  await cleanup();
  console.error(message);
  process.exit(1);
};

// The debugging port takes a moment to listen.
let target = null;
for (let i = 0; i < 100 && !target; i++) {
  await new Promise((r) => setTimeout(r, 100));
  try {
    const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
    target = list.find((t) => t.type === "page" && t.url.startsWith("file://"));
  } catch { /* not listening yet */ }
}
if (!target) await fail("headless Chrome never opened the page");

// Node only exposes `WebSocket` globally from 22 on, and CI ran 20: the
// driver died on a bare `ReferenceError` that said nothing about which
// Node was needed. This is a limit of the *driver*, not of the package —
// `package.json` says node >= 18 and means it.
if (typeof WebSocket === "undefined") {
  await fail("this driver needs Node 22+ for a global WebSocket (the package itself needs 18+)");
}
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });

let nextId = 1;
const pending = new Map();
const requests = [];
ws.onmessage = ({ data }) => {
  const message = JSON.parse(data);
  // Anything the page fetches over the network, including the wasm.
  if (message.method === "Network.requestWillBeSent") {
    requests.push(message.params.request.url);
  }
  const resolve = pending.get(message.id);
  if (resolve) { pending.delete(message.id); resolve(message.result); }
};
const send = (method, params = {}) =>
  new Promise((resolve) => {
    const id = nextId++;
    pending.set(id, resolve);
    ws.send(JSON.stringify({ id, method, params }));
  });

await send("Network.enable");
await send("Page.enable");
await send("Page.reload", { ignoreCache: true });

// Wait for the page's own verdict rather than a fixed sleep.
let results = null;
for (let i = 0; i < 200 && !results; i++) {
  await new Promise((r) => setTimeout(r, 100));
  const { result } = await send("Runtime.evaluate", {
    expression: "JSON.stringify(window.__results ?? null)",
    returnByValue: true,
  });
  if (result?.value && result.value !== "null") results = JSON.parse(result.value);
}
if (!results) await fail("the page never reported a result");

for (const r of results) {
  console.log(`${r.ok ? "ok  " : "FAIL"} ${r.name}${r.detail ? ` — ${r.detail}` : ""}`);
}

// `file://` loads are not network requests, which is the point: nothing
// left the machine. Anything with a scheme is a leak.
const network = requests.filter((url) => /^https?:/.test(url));
if (network.length) {
  console.log(`FAIL no network request — the page fetched ${network.join(", ")}`);
} else {
  console.log("ok   no network request — the document never left the client");
}

await cleanup();
process.exit(results.every((r) => r.ok) && !network.length ? 0 : 1);
