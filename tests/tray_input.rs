// Architectural regression guard, not a cursor-latency benchmark. The tray
// shares the monitor thread, which waits and performs blocking file I/O.
// System input must never wait for that thread to run a synchronous hook.
#[test]
fn tray_input_observation_cannot_block_system_input() {
    let windows = include_str!("../src/tray/windows.rs");
    assert!(
        !windows.contains("SetWindowsHookEx"),
        "Windows tray installs a synchronous input hook on the monitor thread"
    );
    let macos = include_str!("../native/macos_tray.m");
    assert!(
        !macos.contains("CGEventTapCreate"),
        "macOS tray must use passive AppKit observation, not a system input event tap"
    );
}
