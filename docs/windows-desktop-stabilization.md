# Windows desktop stabilization

V0.5.1 makes `LatchDesktop.exe` the normal Windows product shell and keeps `latch.exe` as the worker/CLI underneath it.

Normal user behavior:

- Start menu → **Latch** opens/focuses the tray application.
- `latch start` opens/focuses the tray application; the desktop app ensures the worker is running.
- Closing the desktop window hides it to the system tray.
- **Restart connection** restarts only the worker.
- **Quit Latch** stops the worker and exits the tray application.
- `latch stop` stops the worker and closes the tray application.

Windows process probes and termination helpers use hidden child processes so background status checks do not flash console windows. The worker supervisor also backs off after repeated rapid failures instead of respawning indefinitely every few seconds.

The desktop UI intentionally uses a flatter Windows utility layout: native title bar, compact sidebar, separators instead of soft-depth cards, and restrained status indicators.
