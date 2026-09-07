// A modal in the document's top layer covers the viewport, not browser chrome.
// The closed shadow root keeps page styles from changing the blocking UI.
(() => {
  if (globalThis.__timeTollContent) return;
  globalThis.__timeTollContent = true;
  const api = globalThis.browser ?? chrome;
  let host;
  let dialog;
  let description;
  let blocked = false;
  let expires = 0;

  function create() {
    host = document.createElement("div");
    const shadow = host.attachShadow({ mode: "closed" });
    const style = document.createElement("style");
    style.textContent = `
      :host { all: initial !important; }
      dialog { all: initial; position: fixed; inset: 0; box-sizing: border-box;
        width: 100vw; height: 100vh; max-width: none; max-height: none;
        margin: 0; padding: 24px; border: 0; background: #121721; color: #fff;
        overflow: auto; font: 18px/1.6 system-ui, sans-serif; }
      dialog[open] { display: grid; place-items: center; }
      dialog::backdrop { background: #121721; }
      section { max-width: 620px; }
      h1 { font: 600 30px/1.2 system-ui, sans-serif; margin: 0 0 24px; }
      p { margin: 0; white-space: pre-wrap; }
    `;
    dialog = document.createElement("dialog");
    dialog.setAttribute("aria-label", "TimeToll blocked content");
    const section = document.createElement("section");
    const title = document.createElement("h1");
    title.textContent = "TimeToll";
    description = document.createElement("p");
    section.append(title, description);
    dialog.append(section);
    dialog.addEventListener("cancel", event => { if (blocked) event.preventDefault(); });
    shadow.append(style, dialog);
  }

  function hide() {
    blocked = false;
    if (dialog?.open) dialog.close();
    host?.remove();
  }

  function sameDocument(url) {
    try {
      const expected = new URL(url);
      const actual = new URL(location.href);
      expected.hash = "";
      actual.hash = "";
      return expected.href === actual.href;
    } catch { return false; }
  }

  function show(message) {
    if (!document.documentElement) return;
    if (!host) create();
    if (!host.isConnected) document.documentElement.append(host);
    description.textContent = message;
    if (!dialog.open) dialog.showModal();
  }

  api.runtime.onMessage.addListener((message, _sender, reply) => {
    if (message.type !== "content-decision" || !sameDocument(message.url)) return false;
    expires = Date.now() + 4000;
    blocked = message.blocked === true;
    if (blocked) show(message.message || "Access is locked.");
    else hide();
    reply({ applied: true });
    return false;
  });

  // Fullscreen media is also in the top layer. Promote the existing blocker
  // after a fullscreen change, without changing focus on every heartbeat.
  document.addEventListener("fullscreenchange", () => {
    if (blocked && dialog) { dialog.close(); dialog.showModal(); }
  });
  addEventListener("pagehide", hide);
  function tick() {
    if (blocked && Date.now() > expires) hide();
    if (blocked && !host?.isConnected) show(description?.textContent || "Access is locked.");
    if (document.visibilityState === "visible") {
      try { api.runtime.sendMessage({ type: "content-ready" }).catch(() => hide()); }
      catch { hide(); } // The extension may have been reloaded or disabled.
    }
  }
  document.addEventListener("visibilitychange", tick);
  setInterval(tick, 1000);
  tick();
})();
