# Windows monitor exit investigation

The captured October 29 run ended with `error: Access is denied. (os error 5)`.
The strongest explanation is a failed state checkpoint during atomic file
replacement. The code treated a single temporary Windows access or sharing
failure as fatal. The fix retries replacement without discarding the prepared
file, and adds operation and path context to storage errors.

## Evidence and limits

Evidence came from `Documents/handoff/20261029-091518-wrapper3036` and
`Documents/handoff/scheduler-log.png`:

- The wrapper started at 09:15:18 and detected child PID 9100 exiting at
  14:21:49, after 5 hours and 6 minutes.
- `timetoll.stderr.log` contains one bare OS error, with no panic or overlay
  error prefix. Runtime Windows overlay errors include a descriptive prefix.
- `timetoll status --watch`, PID 9432, remained alive after the monitor exited.
- The task had no execution time limit, and the scheduler screenshot shows
  action completion and task completion at 14:21:57, after the child exited.
  The wrapper found no matching application-crash or power/reboot events.
- The October 28 capture ends in heartbeat entries without an exit record.
  It does not establish why that run ended.

During normal monitoring, ledger checkpoints call `atomic_write`, which syncs
a temporary file and calls `tempfile::NamedTempFile::persist`. The locked
tempfile version uses `MoveFileExW` on Windows. Any failure previously escaped
through `save_ledger(...)?` and ended the monitor, even if the shutdown save
then succeeded. Rule-reload errors are caught and logged instead.

Windows replacement depends on the sharing permissions of open file handles.
See [Microsoft's file replacement documentation](https://learn.microsoft.com/en-us/windows/win32/fileio/moving-and-replacing-files).
[LLVM's Windows filesystem implementation](https://www.llvm.org/doxygen/Windows_2Path_8inc_source.html)
also handles access failures caused by open files and retries operations that
can conflict with filesystem scanners.

The old message does not identify the failing filesystem operation, file,
or competing process. It cannot distinguish a replacement conflict from a
temporary-file creation, write, or sync failure. It also cannot prove whether
the watcher, a scanner, or a persistent permission change caused the denial.
The watcher uses Rust's ordinary file reads, so its presence alone is not proof
that it blocked replacement. The replacement diagnosis remains an inference.

## Changes

Windows replacement now retries OS errors 5, 32, and 33 up to 20 times with
25 ms between attempts. It retains the same synced temporary file and uses
atomic replacement on every attempt. It never deletes the destination first
or truncates it in place. Permanent failures remain errors; retry delays total
at most 500 ms, excluding time spent in filesystem calls and scheduling.
Other platforms retain their previous replacement behavior.

Storage errors now name the operation and destination. A future capture can
distinguish `cannot atomically replace ...state.json` from failure to create,
write, or sync the temporary file.

The handoff diagnostic script was also corrected to retain the child's process
handle and reject a null exit code. Its old `[int]$child.ExitCode` converted
null to zero. That is a possible explanation for the captured `code=0` despite
stderr reporting an error; the capture does not preserve the uncast value.
Repository `main` exits with code 1 on a returned error.

## Verification

`cargo test --locked --lib checkpoint_survives_transient_windows_access_denied`
failed before the retry fix with `checkpoint aborted: Err(Os { code: 5, ... })`
and passed afterward. This replays the numeric Windows error at the real
temporary-file persistence boundary; the host's text for OS error 5 differs
on macOS.

Additional tests cover exhausted retries, immediate failure for unrelated
errors, preservation of the previous state, temporary-file cleanup, error
context, and concurrent checkpoint/status reads. A Windows-only test opens
the destination without delete sharing, verifies an actual OS rejection,
releases the handle, and verifies successful replacement on retry. The
existing Windows CI job runs this test with `cargo test --locked`.

The 35 Rust tests, formatting check, and Clippy check passed on macOS.
The Windows GNU cross-target check passed for all targets, including the
Windows-only test, using the installed stable Rust toolchain.
The Windows-only runtime test and PowerShell script require Windows execution;
error injection and cross-compilation do not establish that the original
five-hour scheduled scenario is fixed on the affected machine.

## Applying the change on Windows

Build the updated checkout with `cargo build --release --locked`, stop the
scheduled task, and replace the exact `timetoll.exe` used by its action with
`target\release\timetoll.exe`. Copy the updated handoff diagnostic script to
its configured Windows location, then start the task and `status --watch`.
The diagnostic script still captures a single run without auto-restarting it.
If the error recurs, the new stderr should identify the failing operation and
path.
