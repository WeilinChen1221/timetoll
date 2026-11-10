# Windows cursor lag with the status tray

The tray introduced in `caa39e1` installed a global `WH_MOUSE_LL` hook to notice clicks outside the status panel. It installed that hook on the same thread that pumps desktop events, monitors foreground usage, writes the ledger, and sleeps for 250 milliseconds between iterations.

Windows calls a low-level mouse hook on the installing thread before delivering the input. Every mouse movement passes through that hook, even though the callback only records button presses. While TimeToll slept or waited for a checkpoint write, mouse input had to wait for its message pump. The hook was active even when the status panel was closed. This explains the reported system-wide choppy cursor with the Windows tray enabled. Reducing drawing work would not remove this wait.

[Microsoft's low-level mouse hook documentation](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelmouseproc) describes the installing-thread dependency and recommends asynchronous raw input for observation. The 250 ms figure is the monitor's configured sleep, not a measured cursor-latency result.

## Fix

The Windows tray now registers for `WM_INPUT` mouse notifications with `RIDEV_INPUTSINK` only while the panel is open. Those messages are queued asynchronously. Mouse movement and foreground clicks do not wait for TimeToll to read them. The registration does not capture the mouse or suppress legacy input to other applications.

Only left/right button-down records trigger outside-click handling. Queued clicks use their message position, including signed coordinates for secondary displays. Closing the panel or dropping the tray removes the registration. If registration fails, the panel still opens and the tray icon can close it; the terminal reports the failure.

The monitor's accounting interval and checkpoint behavior are unchanged. A delayed monitor tick can delay dismissal of the status panel, but cannot delay system mouse delivery through a TimeToll hook.

## macOS

The macOS tray uses AppKit global and local event monitors with a button-down-only mask. It does not observe mouse movement or install a Core Graphics event tap. The global monitor cannot modify events or prevent their delivery to other apps; the local monitor returns the original event and only handles TimeToll's own events. There is no equivalent system-wide synchronous mouse hook to remove. See [Apple's event-monitor documentation](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/EventOverview/MonitoringEvents/MonitoringEvents.html).

## Verification

`cargo test --locked --test tray_input` fails on the original implementation and passes after the fix. This is an architectural guard against synchronous tray input hooks, not a cursor-latency benchmark.

Windows unit tests exercise mouse-record filtering and register/unregister the actual raw-input subscription using a hidden OS window. They check that observation uses only `RIDEV_INPUTSINK`, targets the intended window, and leaves no subscription after repeated close/reopen cycles. They run as part of `cargo test --locked` on Windows and require no pointer movement or injected clicks.

Local checks on macOS include the Rust suite, Windows cross-target Clippy, and the native tray test for live updates, focus preservation, and cleanup. A Windows desktop was not available locally for direct cursor-latency measurement.

For desktop acceptance, run `timetoll run` on Windows and move the pointer continuously with the panel closed, open, and after repeated open/close cycles. Compare against `run --no-tray`. Verify outside-click dismissal, icon toggling, Escape, and Ctrl+C cleanup. Check a high-polling-rate mouse and a secondary display with negative coordinates if available. Ledger checkpoints should continue while the panel is open without cursor stalls.
