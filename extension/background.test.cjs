const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");

function harness(overrides = {}) {
  const calls = [];
  const event = () => ({ addListener() {} });
  const context = {
    URL, AbortSignal, setInterval() {},
    chrome: {
      storage: { local: { get: async () => ({ endpoint: "http://127.0.0.1:47821", token: "x".repeat(32), app: "chrome.exe", ...overrides.settings }) }, onChanged: event() },
      windows: {
        WINDOW_ID_NONE: -1,
        getLastFocused: async () => ({ id: 12, focused: false }),
        update: async (...args) => calls.push(["focus", ...args]), onFocusChanged: event(),
      },
      tabs: {
        query: async () => [{ id: 42, url: "https://example.com/learn", ...overrides.tab }],
        sendMessage: async (...args) => calls.push(["content", ...args]),
        create: async (...args) => calls.push(["new_tab", ...args]),
        onActivated: event(), onUpdated: event(),
      },
      runtime: { onStartup: event(), onInstalled: event(), onMessage: event() },
      alarms: { create() {}, onAlarm: event() }, action: { onClicked: event() },
    },
    fetch: async (url, options) => {
      calls.push(["request", url, options]);
      return { ok: true, json: async () => ({ new_tab: !!overrides.newTab, content: overrides.decision }) };
    },
  };
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(`${__dirname}/chromium/background.js`, "utf8"), context);
  return { context, calls };
}

test("reports last focused browser while the native overlay has focus", async () => {
  const { calls } = harness();
  await new Promise(setImmediate);
  assert.equal(calls.length, 1);
  const [, url, options] = calls[0];
  assert.equal(url, "http://127.0.0.1:47821/v1/activity");
  assert.equal(options.headers.Authorization, `Bearer ${"x".repeat(32)}`);
  assert.deepEqual(JSON.parse(options.body), { app: "chrome.exe", url: "https://example.com/learn" });
});

test("navigation reports the pending destination instead of earning on the old page", async () => {
  const { calls } = harness({ tab: { pendingUrl: "https://example.com/games" } });
  await new Promise(setImmediate);
  assert.equal(JSON.parse(calls[0][2].body).url, "https://example.com/games");
});

test("new tab action targets the reported browser window", async () => {
  const { calls } = harness({ newTab: true });
  await new Promise(setImmediate);
  assert.equal(calls[1][0], "new_tab");
  assert.equal(calls[1][1].windowId, 12);
  assert.equal(calls[2][0], "focus");
  assert.equal(calls[2][1], 12);
});

test("refuses to send the pairing token off the computer", async () => {
  const { calls } = harness({ settings: { endpoint: "https://evil.example" } });
  await new Promise(setImmediate);
  assert.equal(calls.length, 0);
});

test("does not report an unpaired browser", async () => {
  const { calls } = harness({ settings: { token: "" } });
  await new Promise(setImmediate);
  assert.equal(calls.length, 0);
});

test("Chromium and Firefox ship the same implementation", () => {
  for (const file of ["background.js", "content.js", "options.js", "options.html"]) {
    assert.equal(fs.readFileSync(`${__dirname}/chromium/${file}`, "utf8"), fs.readFileSync(`${__dirname}/firefox/${file}`, "utf8"));
  }
});

test("delivers content-only decisions to the reported tab's top frame", async () => {
  const { calls } = harness({ decision: { blocked: true, message: "Earn access" } });
  await new Promise(setImmediate);
  assert.equal(calls[1][0], "content");
  assert.equal(calls[1][1], 42);
  assert.equal(calls[1][2].blocked, true);
  assert.equal(calls[1][2].url, "https://example.com/learn");
  assert.equal(calls[1][3].frameId, 0);
});

test("sends an explicit release when the tab is allowed", async () => {
  const { calls } = harness({ decision: { blocked: false, message: "" } });
  await new Promise(setImmediate);
  assert.equal(calls[1][2].blocked, false);
});
