# AGENTS.md — Navalia

Lightweight macOS-inspired dock and optional menu bar for Windows 10/11 (Rust Win32/Direct2D dock + Tauri 2 React/TypeScript Settings app).

## Commands
- Run dock: `cargo run -p navalia` (exiting restores the taskbar; manual fallback: `cargo run -p navalia -- --restore`)
- Run Settings (dev): `pnpm --dir apps/settings tauri dev` (keep dock running; frontend hot-reloads, Rust changes need restart)
- Test (all): `cargo test --workspace --locked` and `pnpm --dir apps/settings test`
- Test (single): `cargo test -p CRATE TEST_NAME` or `node --test apps/settings/tests/FILE.test.mjs`
- Lint / Format / Typecheck: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `pnpm --dir apps/settings build`
- PowerShell checks: `./scripts/Test-WingetAutomation.ps1`, `./scripts/Test-CodeSigning.ps1`, `./scripts/Test-ReleaseAntivirus.ps1`
- Build installer: `./scripts/build-installer.ps1` (stages `navalia.exe` sidecar into `apps/settings/src-tauri/binaries/` and bundles)

## Architecture
- `apps/dock/src/main.rs` — Native Windows dock entry point (`--quit`, `--restore`, `--autostart`, `--guard`, `--usage-worker`, `--shell-worker`), Direct2D/DirectComposition rendering, taskbar hiding, and watchdog recovery.
- `apps/settings/src-tauri/src/main.rs` — Separate Tauri 2 Settings process (single-instance, frameless acrylic window, `--update` headless check); mutates config and calls `dock_link::notify_config_changed()`.
- `apps/settings/src/` — React 19 + Vite 7 + TypeScript Settings UI; compiled into `apps/settings/dist` and embedded into `navalia-settings` at compile time.
- `crates/navalia-core/` — Platform-independent dock layout and animation math (must stay free of Win32/OS dependencies).
- `crates/navalia-shell/` — Windows shell integration (taskbar control, autostart, app launching, `dock_link` IPC).
- `crates/navalia-config/` — Shared settings schema, defaults, and persistence used by both `navalia` and `navalia-settings`.
- **Version Manifests:** `Cargo.toml` (`[workspace.package]`, inherited via `version.workspace = true` by `apps/dock`, `apps/settings/src-tauri`, and `crates/*`), `gloria.toml`, and `apps/settings/package.json`.

## Personal Rules (Brain)
- **Git Branching:** Never create new branches or git worktrees. Always work directly on `main` until explicitly told otherwise.
- **Versioning:** Never bump versions during normal work. The user picks the version at release time and `/gloria-github-release` sets it in every version file at once; keep version files in sync with each other otherwise.
- **Commit Format:** Write one plain sentence stating what changed and why; do not use `feat:`/`fix:`/`chore:` conventional-commit prefixes.
- **Commit & Push Policy:** Never run `git commit`, `git tag`, or `git push` unless explicitly asked; never force-push to `main`. When the user says "commit and push", verify the build and tests pass, commit on `main`, and push to `origin main`. Pushing never releases anything.
- **Releases:** Releases happen only through `/gloria-github-release`, which bumps the version, pushes, runs `gh workflow run release.yml`, and watches it until the GitHub Release is live. Never create release tags or releases by hand.
- **Path Privacy:** Always use `C:\Users\you` in documentation, comments, and examples—never hardcode the local machine's real username.
- **Release Workflow:** `.github/workflows/release.yml` must be a copy of `GloriaSetup/templates/release.yml` (shared `check`/`release` jobs from `projectGloria/GloriaSetup/.github/workflows/app-{check,release}.yml@main`, plus this repo's own `build` job). Don't hand-write release workflows or pin `gloria-setup`; see `.brain/github-actions.md`.
- **Window Chrome:** Disable native OS titlebars; build an integrated frameless titlebar with drag region, double-click maximize, and custom min/max/close controls.
- **Desktop Shell Lock:** Lock the app shell to `100vw x 100vh` (`overflow: hidden`) with internal scroll panes and `user-select: none` on UI chrome.
- **Scrollbars:** Ban raw OS scrollbars; use thin (4–6px) themed scrollbars with transparent tracks and zero layout shift.
- **Interactive Controls:** Custom-style all form controls; define explicit `hover`, `active`, `focus-visible`, and `disabled` states with snappy 120–180ms transitions.

## Playbooks (Read Before Task)
- **Packaging & Installers:** Read `.brain/building-the-project.md` before packaging, compiling release binaries, or building an installer.
- **GitHub Actions & Releases:** Read `.brain/github-actions.md` before creating/modifying `.github/workflows/*.yml`, setting up release automation, or debugging a failed release.

## Repo Guardrails & Gotchas
- **Settings Frontend Build Order:** `apps/settings/src-tauri` embeds `apps/settings/dist` via `custom-protocol` by default; run `pnpm --dir apps/settings build` before workspace `cargo clippy` or `cargo test` if `dist/` is missing or stale.
- **Workspace Clippy Policy:** `Cargo.toml` denies `clippy::perf` and warns on `unwrap_used` and `expect_used`; CI runs Clippy with `-D warnings`, so avoid new `unwrap()`/`expect()` calls.
- **Release Panic Mode:** `[profile.release]` sets `panic = "abort"` so a crash exits immediately and the `--guard` watchdog restores the Windows taskbar.

## Done Criteria
- Run `pnpm --dir apps/settings test && pnpm --dir apps/settings build`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` (plus relevant `scripts/Test-*.ps1` when touching signing/winget/antivirus scripts).
- Confirm all Version Manifests agree (currently `Cargo.toml` `[workspace.package]`).

<!-- brain-sync: dbdb119dacdc -->
