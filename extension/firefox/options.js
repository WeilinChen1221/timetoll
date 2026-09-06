const api = globalThis.browser ?? chrome;
const fields = ["endpoint", "token", "app"];
const status = document.getElementById("status");

async function load() {
  const settings = await api.storage.local.get(fields);
  for (const field of fields) {
    if (settings[field]) document.getElementById(field).value = settings[field];
  }
  if (!settings.app) {
    const mac = navigator.platform.startsWith("Mac");
    const firefox = navigator.userAgent.includes("Firefox/");
    const edge = navigator.userAgent.includes("Edg/");
    document.getElementById("app").value = firefox ? (mac ? "org.mozilla.firefox" : "firefox.exe")
      : edge ? (mac ? "com.microsoft.edgemac" : "msedge.exe")
      : mac ? "com.google.Chrome" : "chrome.exe";
  }
}

document.getElementById("pairing").addEventListener("submit", async (event) => {
  event.preventDefault();
  try {
    const settings = Object.fromEntries(fields.map(field => [field, document.getElementById(field).value.trim()]));
    const endpoint = new URL(settings.endpoint);
    if (endpoint.protocol !== "http:" || endpoint.hostname !== "127.0.0.1" || endpoint.username || endpoint.password || endpoint.pathname !== "/" || endpoint.search || endpoint.hash) {
      throw new Error("Use a loopback bridge URL, for example http://127.0.0.1:47821");
    }
    if (!/^[a-zA-Z0-9]{32,128}$/.test(settings.token)) throw new Error("Copy the complete token from timetoll pair.");
    await api.storage.local.set(settings);
    // A storage change triggers a heartbeat. Let it finish before testing.
    await new Promise(resolve => setTimeout(resolve, 2200));
    const result = await api.runtime.sendMessage({ type: "status" });
    status.textContent = result.status;
  } catch (error) {
    status.textContent = error.message || String(error);
  }
});
load().catch(error => { status.textContent = error.message; });
