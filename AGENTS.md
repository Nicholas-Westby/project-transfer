# Project Transfer

Rust desktop app (egui) that mirrors project folders between computers on a LAN.
What the app does and how to use it: `README.md`.
How to do local development: `DEVELOPMENT.md`.

## Constraints

- Never shell out for file transfer, discovery or networking. `std::fs`, tokio and the listed crates only. The only processes the app starts are a project's commands and the installer's platform tools (`codesign`, `iconutil`, `ditto`, `lsregister`, PowerShell for the Windows shortcut).
- No shell scripts in the repo, apart from the `install` and `install.ps1` wrappers and `.githooks/pre-commit`, which only run `cargo xtask`. Other tooling is Rust (`xtask`, run with `cargo xtask`).
- Every commit counts the patch version up and refreshes `words.txt` and `phrases.txt`: `.githooks/pre-commit` runs `cargo xtask pre-commit`. Turn it on once per clone with `cargo xtask hooks`. The app shows the version next to the settings cog.
- `words.txt` lists every word in the tracked text files, and `word-changes.txt` the words the latest word-changing commit added (`+`) or removed (`-`). `phrases.txt` and `phrase-changes.txt` do the same for words that mix cases, such as `ActivityKind` or `macOS` (not `TLS`); `words.txt` lists their parts, except for keys and other random-looking ones. All four are generated; never edit them by hand. Rules are in `xtask/src/words.rs` and `xtask/src/phrases.rs`.
- Only private addresses: 10/8, 172.16/12, 192.168/16, 169.254/16, 127/8, ::1, fc00::/7, fe80::/10.
- mDNS service type: `_projtransfer._tcp.local.`
- Data home: `Project Transfer` in the per-user local app data folder (`store::default_root`), overridden by env `PROJECT_TRANSFER_HOME`.
- Files in the data home: `instance.json`, `peers.json`, `projects.json`, `identity/cert.der`, `identity/key.der`, `logs/`. All JSON writes are temp-file-then-rename.
- When a change alters what the main window looks like, run `cargo xtask screenshot` and commit the updated `assets/images/project-transfer.webp`. It renders mock data only.
- Default theme is dark. Palette and type are set in `src/ui/theme.rs`.
- UI copy: sentence case, buttons name their action, errors say what happened and what to do. No ALL CAPS labels.
- Comments say why, not what (the exception being more subtle/complex areas).
- Commits: Conventional Commits, summary at most 80 chars, at most 3 bullets.
- Commit at the end of every task.
- Keep files under about 300 lines.

## Before calling anything done

Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test`.
