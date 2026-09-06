/* The OS monitor decides whether this browser is foreground. Keep reporting
 * its last focused window while the native blocking window has keyboard focus.
 * Tab API calls also keep Chromium's MV3 worker active between heartbeats.
 */
const api = globalThis.browser ?? chrome;
let sending = false;
let lastError = "Not paired";

async function heartbeat() {
  if (sending) return;
  sending = true;
  try {
    const settings = await api.storage.local.get(["endpoint", "token", "app"]);
    if (!settings.token || !settings.app) {
      lastError = "Run timetoll pair and save the pairing settings.";
      return;
    }
    const endpoint = new URL(settings.endpoint || "http://127.0.0.1:47821");
    if (endpoint.protocol !== "http:" || endpoint.hostname !== "127.0.0.1" || endpoint.username || endpoint.password) {
      throw new Error("Bridge URL must use http://127.0.0.1");
    }
    const window = await api.windows.getLastFocused({ windowTypes: ["normal"] });
    if (!window || window.id === api.windows.WINDOW_ID_NONE) return;
    const tabs = await api.tabs.query({ active: true, windowId: window.id });
    const tab = tabs[0];
    const url = tab?.pendingUrl || tab?.url;
    if (!url) return;
    const response = await fetch(`${endpoint.origin}/v1/activity`, {
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${settings.token}` },
      body: JSON.stringify({ app: settings.app, url }),
      signal: AbortSignal.timeout(2000),
    });
    if (!response.ok) throw new Error(`Bridge returned ${response.status}. Check the token and app identifier.`);
    const result = await response.json();
    lastError = "Connected";
    if (result.new_tab) {
      await api.tabs.create({ windowId: window.id, active: true });
      await api.windows.update(window.id, { focused: true });
    }
  } catch (error) {
    lastError = error.message || String(error);
  } finally {
    sending = false;
  }
}

api.tabs.onActivated.addListener(heartbeat);
api.tabs.onUpdated.addListener((_id, change, tab) => {
  if (tab.active && (change.url || change.status)) heartbeat();
});
api.windows.onFocusChanged.addListener(heartbeat);
api.storage.onChanged.addListener(heartbeat);
api.runtime.onStartup.addListener(heartbeat);
api.runtime.onInstalled.addListener(() => {
  api.alarms.create("heartbeat", { periodInMinutes: 0.5 });
  heartbeat();
});
api.alarms.onAlarm.addListener(heartbeat);
(api.action ?? api.browserAction).onClicked.addListener(() => api.runtime.openOptionsPage());
api.runtime.onMessage.addListener((message, _sender, reply) => {
  if (message.type === "status") {
    heartbeat().then(() => reply({ status: lastError }));
    return true;
  }
  return false;
});
api.alarms.create("heartbeat", { periodInMinutes: 0.5 });
setInterval(heartbeat, 1000);
heartbeat();
