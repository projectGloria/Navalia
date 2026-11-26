# Navalia Code Review

## Overview

The codebase is in good shape overall. Formatting, `clippy` with `-D warnings`, every `cargo test`, the 28 frontend tests, `tsc`, and the Vite build all pass. Risky native code (`end_task`, autostart, accessory and shell worker processes, wallpaper downloads) is careful and well tested.

**Scope:** Most files were read line by line. For large rendering-heavy modules (`menubar`, `mascot`, `flight`, `gfx`, `scene`, and the views), the review focused on structure, timers, process launches, and `unsafe` code rather than every drawing line. No edits were made.

**Main trouble spots:**
* Release automation and configuration drift
* Crash/error handling edge cases
* Documentation and comments out of sync with behavior

---

## High Severity

### 1. `release.yml` diverges from the project release process
* **Expected process:** `AGENTS.md` and `.brain/github-actions.md` specify a copy of `GloriaSetup/templates/release.yml`, triggered manually with a `dry_run` input.
* **Actual workflow:** It is a custom pipeline triggered on `v*` tag pushes that only creates a draft release.
* **Impact on `/gloria-github-release`:** Manual runs (`gh workflow run release.yml`) only test-sign, so the skill cannot ship a release here and would wait indefinitely for a release that never publishes.
* **Missing config & unpinned actions:** `gloria.toml` is absent, and actions use tags like `@v4` instead of pinned commit hashes from the template.
* **Design required:** Copying the template alone is not enough—Tauri updater artifacts (`latest.json` and `.sig` files) and SignPath signing must be designed into the workflow before migrating.

### 2. Settings window traps user when saves fail (`App.tsx:164`)
* Canceling window close whenever a queued save fails creates a deadlock when config writes are rejected.
* `apps/settings/src-tauri/src/main.rs:26` quietly displays default settings when the config file is corrupt, while `crates/navalia-config/src/lib.rs:505` refuses to overwrite a corrupt file.
* **Result:** With a corrupt config, any edit leaves the Settings window stuck open until the process is killed.

### 3. Config writes lack crash/power-loss durability (`lib.rs:565`)
* Temp files are renamed into place without flushing to disk first, risking an empty or truncated `config.json` on power loss.
* The dock falls back to defaults silently because `eprintln!` goes nowhere in a windowed app, and later pin changes also fail silently.
* The wallpaper cache already uses `sync_all` safely; config writes should match that pattern and maintain a `.bak` backup.

---

## Medium Severity

### 1. Tray host message handling and broadcast issues (`tray.rs`)
* **Over-broad `TaskbarCreated`:** `crates/navalia-shell/src/tray.rs:595` broadcasts `TaskbarCreated` to every window in every other process—including Navalia's own guard, Settings, and worker processes. The dock reclaims overlays and resets its taskbar timer, while other apps may recreate tray icons and flicker.
* **Filtered window messages:** The window procedure only forwards `WM_COPYDATA`, `WM_COMMAND`, and `WM_USER`–`WM_APP` to Explorer. Registered messages, `WM_HOTKEY`, and other `Shell_TrayWnd` messages never reach Explorer.
* **Additional edge cases:**
  * Rejected icon additions are still retained.
  * Version queries for unknown icons are dropped.
  * `crates/navalia-shell/src/tray.rs:578` relies on `IsWindow`, which can collide when Windows reuses handles.
  * A hung Explorer can stall tray-icon calls for up to 3 s (`crates/navalia-shell/src/tray.rs:97`).

### 2. Network-access claims conflict with actual behavior
* **Contradictory statements:** `App.tsx:146` states update checks are "the only network access in Navalia," and `lifecycle.rs:472` / `main.rs:152` state "the dock never touches the network."
* **Usage worker:** The dock's `--usage-worker` process reads saved credentials for Claude, Codex, OpenCode, z.ai, and Gemini and calls their APIs (`usage.rs:562+`).
* **Wallpaper downloads:** Settings downloads wallpapers from `raw.githubusercontent.com/.../main` while checksums are baked into each build—changing an asset on `main` breaks downloads for all released builds.

### 3. Unbounded usage worker execution (`usage.rs:481`)
* `.output()` blocks until the child exits.
* Antigravity discovery alone can take roughly 34 s (8 files × 2 schemes × 3 methods × 700 ms).
* Unlike the accessory worker, it lacks a job object and deadline, keeping `running` true and halting refreshes until it exits.

### 4. Autostart checks can return false results (`autostart.rs`)
* `crates/navalia-shell/src/autostart.rs:152` returns early if *any* `Run` entry exists—even one pointing to a different executable—skipping scheduled-task checks.
* `crates/navalia-shell/src/autostart.rs:548` uses a fixed 1024-character buffer, treating longer entries as missing.
* Legacy task naming depends on the `USERNAME` environment variable.

### 5. Drive-root quoting bug (`launcher.rs:92`, `launcher.rs:420`)
* `format!("\"{file}\"")` turns `C:\` into `"C:\"`, where Windows command-line parsing treats `\"` as an escaped quote.

### 6. Update mode race with newly opened Settings window (`main.rs:299`)
* `--update` checks for an open window before downloading, so opening Settings during the download still gets closed by the installer.

---

## Low Severity

| Area | Findings |
| :--- | :--- |
| **Taskbar state** | `crates/navalia-shell/src/taskbar.rs:185` strips layered and transparent window styles without saving originals. A corrupt state file at `crates/navalia-shell/src/taskbar.rs:100` disables Windows taskbar auto-hide. |
| **Guard** | `apps/dock/src/guard.rs:75` times out after 1 s (launching twice during startup shows an error). `guard.rs:271` matches by window class (a stray window with that class counts as a missed check). `guard.rs:296` re-enables transitions on all windows, including untouched ones. |
| **Build** | `build.rs:59` only warns if resource embedding fails, silently dropping the manifest (DPI awareness, modern controls). Failing the build in release mode would catch this. |
| **Config** | 80 ms lock timeout. Type mismatches in patches raise a `Parse` error blaming the config path. Unset `APPDATA` or `LOCALAPPDATA` falls back to relative paths. The "Speed of the genie" doc comment (`lib.rs:171`) sits on the wrong item. |
| **Fixed path buffers** | `SHGetPathFromIDListW` uses a 260-char buffer in `jumplist.rs:758`; 1024-char buffers appear in `tracker`, `pins`, and `end_task`. Long paths truncate or get skipped. |
| **Swallowed errors** | `eprintln!` in windowed binaries (such as `lifecycle.rs:497` and Settings `main`) goes nowhere; use `navalia_shell::log::line`. |
| **Missing doc** | CI errors reference `FIRMA-DE-CODIGO.md`, which is gitignored and unavailable to others. |
| **Repo hygiene** | `scripts/__pycache__/make_icon.cpython-313.pyc` is committed without `__pycache__` in `.gitignore`. `navalia-config` declares `serde` without `workspace = true`. `apps/settings/package.json` lists `0.1.0` (harmless at runtime since Tauri uses Cargo's version, but misleading). Error messages, logs, and comments in `.gitignore` and `build.rs` are in Spanish while code and UI are English-first. |
| **Muelle leftovers** | `CHANGELOG` history and issue numbers (`#65`, `#73`) come from Muelle. Keep the MIT copyright line in `LICENSE`, and consider adding a Navalia line. |
| **Docs** | `AGENTS.md` documents `pnpm` commands, but `pnpm` was not on `PATH` on this machine; checks ran via `node`, `tsc`, and `vite` directly. |
| **Maintainability** | `menubar.rs` is 6,233 lines with a ~2,600-line `impl` block. Active polling every 80 ms (edge watcher) and every 100 ms (menu-bar pointer) can impact battery life. |

---

## Strengths (Done Well)

* **Process isolation:** Shell actions, usage lookups, and accessory queries each run in separate processes; accessory workers use a job object and deadline, and shell actions enforce a 30 s timeout.
* **Safe termination:** `end_task.rs` performs careful checks before ending a process.
* **Wallpaper verification:** Wallpapers are restricted to catalog IDs and validated by size and SHA-256 over HTTPS with redirects disabled.
* **Release-note links:** Links are restricted to this repository's release pages.
* **Crash recovery:** Watchdog, device-loss, and stable-uptime checks are in place.