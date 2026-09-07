const { test, expect } = require("@playwright/test");
const path = require("node:path");

test.beforeEach(async ({ page }) => {
  await page.route("http://timetoll.test/**", route => route.fulfill({
    contentType: "text/html",
    body: `<!doctype html><style>body { margin:0; } button { position:fixed; left:10px; top:10px; width:200px; height:60px; }</style>
      <button id="under" onclick="window.clicks++">Page action</button>
      <input id="input"><button id="full" style="top:100px" onclick="document.documentElement.requestFullscreen()">Fullscreen</button>
      <script>window.clicks=0</script>`,
  }));
  await page.addInitScript(() => {
    // Keep a test-only reference to the otherwise closed shadow root.
    const attach = Element.prototype.attachShadow;
    Element.prototype.attachShadow = function (options) {
      const root = attach.call(this, options);
      window.testRoot = root;
      return root;
    };
    window.chrome = { runtime: {
      onMessage: { addListener: callback => { window.deliver = callback; } },
      sendMessage: async () => ({ ok: true }),
    } };
  });
  await page.goto("http://timetoll.test/blocked");
  await page.clock.install();
  await page.addScriptTag({ path: path.join(__dirname, "../chromium/content.js") });
});

async function decision(page, blocked, url) {
  return page.evaluate(({ blocked, url }) => window.deliver({
    type: "content-decision", blocked, url: url || location.href,
    message: "Access is locked. Use the tabs or address bar to go elsewhere.",
  }, {}, () => {}), { blocked, url });
}

async function bounds(page) {
  return page.evaluate(() => {
    const dialog = window.testRoot.querySelector("dialog");
    const rect = dialog.getBoundingClientRect();
    return { x: rect.x, y: rect.y, width: rect.width, height: rect.height, open: dialog.open, modal: dialog.matches(":modal") };
  });
}

test("covers exactly the viewport and blocks page input while locked", async ({ page }) => {
  await page.locator("#input").focus();
  await decision(page, true);
  expect(await bounds(page)).toEqual({ x: 0, y: 0, width: 960, height: 700, open: true, modal: true });
  await page.mouse.click(50, 30);
  await page.keyboard.type("blocked typing");
  await page.keyboard.press("Escape");
  expect(await page.evaluate(() => window.clicks)).toBe(0);
  await expect(page.locator("#input")).toHaveValue("");
  expect((await bounds(page)).open).toBe(true);
  // Repeated decisions do not reopen the modal or refocus page controls.
  await decision(page, true);
  await decision(page, false);
  await page.mouse.click(50, 30);
  expect(await page.evaluate(() => window.clicks)).toBe(1);
});

test("tracks viewport resize, zoom, and fullscreen without native-window offsets", async ({ page }) => {
  await page.locator("#full").click();
  await expect.poll(() => page.evaluate(() => !!document.fullscreenElement)).toBe(true);
  await decision(page, true);
  const fullscreenSize = await page.evaluate(() => ({ width: innerWidth, height: innerHeight }));
  expect(await bounds(page)).toMatchObject({ x: 0, y: 0, ...fullscreenSize });
  await page.evaluate(() => document.exitFullscreen());
  await expect.poll(() => page.evaluate(() => !!document.fullscreenElement)).toBe(false);
  expect((await bounds(page)).modal).toBe(true);
  for (const size of [{ width: 520, height: 360 }, { width: 1280, height: 800 }]) {
    await page.setViewportSize(size);
    await decision(page, true);
    expect(await bounds(page)).toMatchObject({ x: 0, y: 0, ...size });
  }
  await page.evaluate(() => { document.body.style.zoom = "175%"; });
  expect(await bounds(page)).toMatchObject({ x: 0, y: 0, width: 1280, height: 800 });
});

test("ignores decisions for a previous page after navigation", async ({ page }) => {
  await page.evaluate(() => history.pushState({}, "", "/allowed"));
  await decision(page, true, "http://timetoll.test/blocked");
  expect(await page.evaluate(() => !!window.testRoot)).toBe(false);
  await decision(page, false);
  await page.mouse.click(50, 30);
  expect(await page.evaluate(() => window.clicks)).toBe(1);
});

test("releases an overlay when the monitor stops renewing its lease", async ({ page }) => {
  await decision(page, true);
  await page.clock.fastForward(5000);
  expect(await page.evaluate(() => window.testRoot.querySelector("dialog").open)).toBe(false);
  await page.mouse.click(50, 30);
  expect(await page.evaluate(() => window.clicks)).toBe(1);
});

test("an allowed tab has its own document and stays usable", async ({ page, context }) => {
  await decision(page, true);
  const other = await context.newPage();
  await other.route("http://timetoll.test/**", route => route.fulfill({ body: "<button onclick='this.textContent=\"clicked\"'>Allowed tab</button>" }));
  await other.goto("http://timetoll.test/allowed");
  await other.locator("button").click();
  await expect(other.locator("button")).toHaveText("clicked");
  expect((await bounds(page)).open).toBe(true);
});
