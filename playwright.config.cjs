const { defineConfig } = require("@playwright/test");
module.exports = defineConfig({
  testDir: "./extension/browser-tests",
  workers: 1,
  use: { headless: true, viewport: { width: 960, height: 700 } },
});
