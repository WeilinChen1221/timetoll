# TimeToll

A Rust CLI for macOS and Windows that trades time in selected apps and websites for access to distracting ones.

By default, 15 minutes in the earning group earns 10 minutes of access to the blocked group. Credit accumulates in one shared balance. TimeToll covers the desktop with native topmost windows when a blocked target is active and the balance is empty.

It runs in your desktop session without administrator access, Accessibility permission, Screen Recording permission, a VPN, or changes to the hosts file. Website tracking uses the included browser extension and its tab URL permission. App tracking works without the extension.

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

Chrome, Edge, Brave, Arc, Opera, Vivaldi, and Firefox are supported through the included extension. Safari URL tracking is not included. Safari can still be blocked as a whole app.

For Chromium browsers:

1. Open the browser's extensions page, such as `chrome://extensions` or `edge://extensions`.
2. Enable Developer mode and choose **Load unpacked**.
3. Select this project's `extension/chromium` directory.
4. Run `timetoll pair` and open the extension's options by clicking its toolbar icon.
5. Enter the printed URL and token. Set the browser's app identifier from `timetoll apps`.
6. Start `timetoll run`, then click **Save and test connection**. The status should say `Connected`.

For Firefox, open `about:debugging#/runtime/this-firefox`, select **Load Temporary Add-on**, and select `extension/firefox/manifest.json`. Configure its options the same way. Firefox removes temporary add-ons when it restarts. A permanent release requires Mozilla signing; this repository does not include a signed extension.

Use one browser profile per app identifier while monitoring. Multiple profiles of the same browser share an OS app identifier, so their reports can overwrite each other. Private windows need the extension enabled separately in the browser's settings. Browsers outside the configured `browsers` list are not subject to website rules. Add another browser identifier to that list in `config.toml`, then restart the monitor.

The extension reports the active tab in the browser's last focused normal window once per second. The Rust monitor uses the OS foreground app to decide whether that browser is being used. Background tabs and background apps do not earn or spend credit. Tab URLs stay in memory and are not written to the ledger or logs.

If website blocking is configured and a known browser has no report newer than four seconds, TimeToll displays a connection overlay. This includes an unpaired browser, unsupported Safari, or a disconnected extension. The overlay stays until fresh reports arrive or you switch apps. It does not spend credit while the URL is unknown. Extension recovery after browser sleep or worker shutdown can take up to 30 seconds.

When a page is locked, click **Open a new browser tab** on the overlay to navigate to an earning or whitelisted page. The extension opens a tab in that browser window. You can also switch to another app with Cmd+Tab or Alt+Tab. The new-tab button requires a connected extension and cannot override a whole-browser app block.

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

`doctor` checks configuration, state, and foreground app detection. `preview` shows the native blocking window for five seconds without changing credit. `config` redacts the token; `pair` intentionally displays it.

`init` prints the configuration path. Use `--data-dir PATH` with any command to override it. Keep `config.toml` and `state.json` together. The token is private to that configuration; do not publish it or commit it to source control.

The monitor reloads rules and the ratio within one second. Changes to the bridge port, token, or browser identifiers require a restart. Invalid edits keep the last valid rules and print an error. Writes use atomic replacement, and the monitor checkpoints progress every second, on rewards, at balance exhaustion, and on normal shutdown. Abrupt termination can lose or restore up to roughly one second of usage. A process lock prevents two monitors from using the same ledger. Corrupt state causes an error instead of silently discarding credit.

The browser listener binds only to `127.0.0.1`. It requires a pairing token, validates browser identifiers, rejects ordinary website origins, and grants no CORS access. The extension sends no data to an external server. This is a local focus tool; another process running as your user can still read its files or interfere with its operation.

## What window blocking can do

TimeToll creates an opaque window on each display. On macOS it uses AppKit windows above normal apps and across Spaces, and requests keyboard focus. On Windows it uses topmost Win32 windows and requests foreground activation. The monitor hides them when you switch to an allowed app or page. They absorb pointer input over covered content. They absorb keyboard input when the OS gives them focus.

A regular desktop window cannot enforce a tamper-proof restriction. You can quit TimeToll, edit its files, disable its extension, use an unconfigured browser, or use OS escape controls. Windows may refuse foreground activation; clicking the overlay gives it keyboard focus. Secure desktops, elevated applications, exclusive fullscreen games, and other topmost windows can defeat or cover an overlay. Polling also means a newly selected page may appear briefly before the overlay. Background audio, downloads, and network requests continue.

These are limits of the permission-free window approach. TimeToll does not claim to bypass OS security or stop network access. See [Microsoft's foreground window rules](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos), [Apple's window ordering API](https://developer.apple.com/documentation/appkit/nswindow/orderfrontregardless%28%29), and [Chrome's tab permissions](https://developer.chrome.com/docs/extensions/reference/api/tabs).

## Development and verification

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
node --test extension/background.test.cjs
```

The GitHub Actions workflow checks macOS, Windows, and Linux, then uploads native macOS and Windows binaries with the extension directories. Linux can run the policy tests and configuration commands; desktop monitoring supports macOS and Windows only.

Edit the extension implementation in `extension/chromium`, then run `sh extension/sync.sh` to copy the shared files into `extension/firefox`. The test suite checks that the copies agree.

For a desktop acceptance test, use a separate `--data-dir`, set a short ratio such as 5 earning seconds to 3 unlock seconds, and configure a harmless earning app and blocked app. Verify earning, spending, idle behavior, focus switching, monitor sleep, and multi-display overlays. Repeat with a blocked domain and a whitelisted path. Check the new-tab button, an extension disconnect, restart persistence, and rule reload. Windows desktop behavior needs testing on a Windows machine; a cross-target build check cannot verify focus or rendering.
