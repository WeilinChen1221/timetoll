# TimeToll

A Rust CLI for macOS and Windows that trades time in selected apps and websites for access to distracting ones.

By default, 15 minutes in the earning group earns 10 minutes of access to the blocked group. Credit accumulates in one shared balance. TimeToll covers the blocked target's content when the balance is empty. Browser overlays live inside the tab viewport. Desktop app overlays leave the title bar and its window controls exposed.

It runs in your desktop session without administrator access, Accessibility permission, Screen Recording permission, a VPN, or changes to the hosts file. Website tracking and blocking use the included browser extension, with permission to read tab URLs and run its content overlay on websites. App tracking works without the extension.

## Build and start

Install stable Rust. Building on macOS also requires Xcode Command Line Tools. On Windows, use the MSVC Rust toolchain with Visual Studio C++ Build Tools and the Windows SDK.

```sh
cargo build --release --locked
cargo install --path . --locked
timetoll init
```

Or run `target/release/timetoll` on macOS and `target\release\timetoll.exe` on Windows without installing it.

Configure websites:

```sh
timetoll block add site youtube.com
timetoll block add site reddit.com
timetoll earn add site doc.rust-lang.org
timetoll whitelist add 'https://www.youtube.com/watch?v=YOUR_LESSON_ID'
timetoll ratio --earn 15 --unlock 10
```

Configure apps using their bundle identifiers on macOS:

```sh
timetoll block add app com.apple.TV
timetoll earn add app com.microsoft.VSCode
```

On Windows, use executable names:

```powershell
timetoll block add app steam.exe
timetoll earn add app Code.exe
```

Run `timetoll apps` and switch to an app to find its identifier. App rules use exact, case-insensitive identifiers. They do not match process paths, window titles, child processes, or app name prefixes.

Pair the browser extension as described below, then start monitoring:

```sh
timetoll run
```

Keep this command running. Switch back to the terminal and press Ctrl+C to stop. TimeToll does not install a background service or a startup task.

## Browser extension

Chrome, Thorium on macOS, Edge, Brave, Arc, Opera, Vivaldi, and Firefox are supported through the included extension. Safari URL tracking is not included. Safari is not supported for content-only blocking. Registered browsers always use their extension for blocking, so a native overlay cannot accidentally cover browser chrome.

For Chromium browsers:

1. Open the browser's extensions page, such as `chrome://extensions` or `edge://extensions`.
2. Enable Developer mode and choose **Load unpacked**.
3. Select this project's `extension/chromium` directory.
4. Run `timetoll pair` and open the extension's options by clicking its toolbar icon.
5. Enter the printed URL and token. Set the browser's app identifier from `timetoll apps`.
6. Start `timetoll run`, then click **Save and test connection**. The status should say `Connected`.

For Thorium on macOS, set the extension's app identifier to `org.chromium.Thorium`, replacing the suggested Chrome identifier. If your configuration was created before Thorium support, add `org.chromium.Thorium` to the `browsers` list in `config.toml` and restart `timetoll run`.

For Firefox, open `about:debugging#/runtime/this-firefox`, select **Load Temporary Add-on**, and select `extension/firefox/manifest.json`. Configure its options the same way. Firefox removes temporary add-ons when it restarts. A permanent release requires Mozilla signing; this repository does not include a signed extension.

Use one browser profile per app identifier while monitoring. Multiple profiles of the same browser share an OS app identifier, so their reports can overwrite each other. Private windows need the extension enabled separately in the browser's settings. Browsers outside the configured `browsers` list are not subject to website rules. Add another browser identifier to that list in `config.toml`, then restart the monitor.

The extension reports the active tab in the browser's last focused normal window once per second. The Rust monitor uses the OS foreground app to decide whether that browser is being used. Background tabs and background apps do not earn or spend credit. Tab URLs stay in memory and are not written to the ledger or logs.

Reload the extension after updating to version 0.2.0 and allow it to run on the sites you want to block. Reload already-open pages so they receive the content script. File URLs and private windows require their separate browser permissions. Browser-internal pages, built-in PDF viewers, and sites where the browser forbids extension scripts cannot receive the overlay. The options page reports a delivery error for a blocked tab it cannot reach. TimeToll never falls back to covering browser chrome.

If a known browser has no recent report, the monitor logs that it is waiting for the extension and pauses website accounting. Existing tab overlays expire within five seconds without renewed decisions, including when the monitor stops. This lets normal shutdown restore page access. A disconnected or disabled extension therefore stops website enforcement. The background worker may take up to 30 seconds to recover after browser sleep.

When a page is locked, use the browser normally to switch tabs, enter another address, open a bookmark, or close the window. The tab bar, address bar, bookmarks bar, browser sidebars, and window controls stay usable. The page beneath the overlay cannot receive pointer or keyboard input. An allowed page removes its overlay on the next decision. A whole-browser app rule still blocks its scriptable page content, including whitelisted pages.

## Rules and credit

| Rule | Matches |
| --- | --- |
| `example.com` | The domain and all subdomains, over HTTP or HTTPS |
| `https://example.com/learn` | That exact host, scheme, port, and path; any query |
| `https://example.com/learn/*` | `/learn`, `/learn/`, and descendants; excludes `/learning` |
| `https://example.com/watch?v=lesson` | That exact path and query |

Fragments such as `#chapter` do not affect matching. URL paths are case-sensitive. Query-specific rules compare the complete query string, including parameter order. Wildcards are supported only as a trailing `/*` in a full URL. URL matching follows URL parsing rules; it does not infer server-side aliases or redirects.

Whitelist rules override website blocks. They do not override an app block. A whitelisted page can also belong to the earning group. When a target matches both blocked and earning rules without a whitelist exception, blocking wins.

Credit is awarded in completed earning intervals. With the default ratio, 14 minutes earns no credit yet; the next minute awards 10 minutes. Partial earning progress carries across earning targets and restarts. Unused credit does not expire. Only active use of a blocked target spends the shared balance, so switching to a neutral app pauses spending. Multiple earning rules never multiply the reward.

Usage stops counting after 60 seconds without keyboard or mouse input. Reading without input counts until that threshold. Adjust `idle_seconds` in the configuration if needed. Monitor gaps longer than two seconds, including sleep and suspension, earn and spend nothing. Foreground polling runs every 250 milliseconds, and intervals spanning activity changes are discarded. This slightly undercounts rapid switching.

`timetoll ratio --earn 30 --unlock 10` changes the ratio in minutes. Changing the ratio preserves banked access but resets partial earning progress when the monitor next loads it. For intervals shorter than a minute, edit `ratio.earn_seconds` and `ratio.unlock_seconds` in the configuration. Each must be between one second and 365 days.

## Commands and storage

```sh
timetoll status
timetoll status --json
timetoll config
timetoll block remove site reddit.com
timetoll earn remove app Code.exe
timetoll whitelist remove 'https://example.com/learn/*'
timetoll doctor
timetoll preview
```

`doctor` checks configuration, state, and foreground app detection. `preview` covers the content of the current foreground desktop app window for five seconds without changing credit. Use `preview --seconds 20` for a longer check. `config` redacts the token; `pair` intentionally displays it.

`init` prints the configuration path. Use `--data-dir PATH` with any command to override it. Keep `config.toml` and `state.json` together. The token is private to that configuration; do not publish it or commit it to source control.

The monitor reloads rules and the ratio within one second. Changes to the bridge port, token, or browser identifiers require a restart. Title-bar inset changes reload with the other rules. Invalid edits keep the last valid rules and print an error. Writes use atomic replacement, and the monitor checkpoints progress every second, on rewards, at balance exhaustion, and on normal shutdown. Abrupt termination can lose or restore up to roughly one second of usage. A process lock prevents two monitors from using the same ledger. Corrupt state causes an error instead of silently discarding credit.

The browser listener binds only to `127.0.0.1`. It requires a pairing token, validates browser identifiers, rejects ordinary website origins, and grants no CORS access. The extension sends no data to an external server. This is a local focus tool; another process running as your user can still read its files or interfere with its operation.

## What window blocking can do

Browser blocking uses a modal dialog in the document's top rendering layer. It fits the page viewport automatically during resizing, page zoom, and fullscreen transitions. It does not need to measure browser toolbars, bookmarks, vertical tabs, or docked developer tools. These areas belong to browser chrome and remain outside the overlay. The overlay is isolated from page styles in a closed shadow root. Repeated decisions update its text without repeatedly taking focus.

Desktop apps use a native topmost overlay. On Windows, TimeToll maps the client rectangle into screen coordinates and excludes a visible native caption area if a custom frame extends into it. Per-monitor DPI awareness keeps this aligned across differently scaled displays. On macOS, foreign window metadata provides the outer frame but not an arbitrary app's content layout. TimeToll reserves the standard AppKit title-bar height without requesting Accessibility access. It retains this strip in fullscreen so hidden title-bar controls can be revealed.

For a desktop app that draws a taller custom title bar, set its total top inset from the outer window edge in `config.toml`. Values are logical pixels on Windows and points on macOS; Windows scales them for the target display. This is needed when the app's controls fall outside the standard system title-bar area. For example:

```toml
[titlebar_insets]
"com.microsoft.VSCode" = 38
"custom-editor.exe" = 48
```

Choose the actual title-bar height for the app and its theme. These values are examples, not measurements of every app version. Insets apply to native desktop app overlays, not browser overlays. The default is the system title-bar/client-area calculation; custom macOS title bars cannot be measured exactly without Accessibility access or an explicit inset.

Native overlays track the same window ID while focused and refresh position and size every 250 milliseconds. Moving, resizing, maximizing, and changing displays update their content bounds. Closing, hiding, or minimizing the target removes the overlay after the OS window animation. Missing or empty content bounds hide it; there is no whole-window or whole-screen fallback. The overlay takes focus when first shown, then leaves focus alone when you use the exposed title bar. Clicking the overlay focuses it again. Window controls remain visible and usable, including close, minimize, and maximize/restore.

A regular desktop window cannot enforce a tamper-proof restriction. You can quit TimeToll, edit its files, disable its extension, use an unconfigured browser, or use OS escape controls. Windows may refuse foreground activation; clicking the overlay gives it keyboard focus. Secure desktops, elevated applications, exclusive fullscreen games, and other topmost windows can defeat or cover an overlay. Polling also means a newly selected page may appear briefly before the overlay. Background audio, downloads, and network requests continue.

These are limits of the permission-free window approach. TimeToll does not claim to bypass OS security or stop network access. See [Microsoft's foreground window rules](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos), [Apple's window ordering API](https://developer.apple.com/documentation/appkit/nswindow/orderfrontregardless%28%29), and [Chrome's tab permissions](https://developer.chrome.com/docs/extensions/reference/api/tabs).

## Development and verification

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
node --test extension/background.test.cjs
npm ci
npx playwright install chromium
npm run test:browser
```

The GitHub Actions workflow checks macOS, Windows, and Linux, then uploads native macOS and Windows binaries with the extension directories. Linux can run the policy tests and configuration commands; desktop monitoring supports macOS and Windows only.

Edit the extension implementation in `extension/chromium`, then run `sh extension/sync.sh` to copy the shared files into `extension/firefox`. The test suite checks that the copies agree.

For a desktop acceptance test, use a separate `--data-dir`, set a short ratio such as 5 earning seconds to 3 unlock seconds, and configure a harmless earning app and blocked app. Verify earning, spending, idle behavior, focus switching, monitor sleep, and moving the target between displays. Repeat with a blocked domain and a whitelisted path. Check tabs, the address bar, bookmarks, browser sidebars, title-bar controls, an extension disconnect, restart persistence, and rule reload. Windows desktop behavior needs testing on a Windows machine; a cross-target build check cannot verify focus or rendering.

The macOS geometry regression test creates its own temporary windows and checks title-bar exclusion, focus preservation, custom insets, movement, resizing, fullscreen transitions, minimizing, closing, and coordinate conversion for displays around the primary screen. Run it in a logged-in desktop session:

```sh
mkdir -p target
cc -fobjc-arc -Wno-deprecated-declarations native/macos_test.m -framework AppKit -framework ApplicationServices -o target/macos-window-tests
./target/macos-window-tests --fullscreen
```
