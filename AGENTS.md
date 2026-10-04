# Project Transfer

Rust desktop app (egui) that mirrors project folders between computers on a LAN.
What the app does and how to use it: `README.md`.

## Constraints

- Never shell out for file transfer, discovery or networking. `std::fs`, tokio and the listed crates only. The only processes the app starts are a project's commands and the installer's platform tools (`codesign`, `iconutil`, `ditto`, `lsregister`, PowerShell for the Windows shortcut).
- No shell scripts in the repo, apart from the `install` and `install.ps1` wrappers, which only run `cargo xtask install`. Other tooling is Rust (`xtask`, run with `cargo xtask`).
- Only private addresses: 10/8, 172.16/12, 192.168/16, 169.254/16, 127/8, ::1, fc00::/7, fe80::/10.
- mDNS service type: `_projtransfer._tcp.local.`
- Data home: `Project Transfer` in the per-user local app data folder (`store::default_root`), overridden by env `PROJECT_TRANSFER_HOME`.
- Files in the data home: `instance.json`, `peers.json`, `projects.json`, `identity/cert.der`, `identity/key.der`, `logs/`. All JSON writes are temp-file-then-rename.
- Default theme is dark. Palette and type are set in `src/ui/theme.rs`.
- UI copy: sentence case, buttons name their action, errors say what happened and what to do. No ALL CAPS labels.
- Comments say why, not what (the exception being more subtle/complex areas).
- Commits: Conventional Commits, summary at most 80 chars, at most 3 bullets.
- Commit at the end of every task.
- Keep files under about 300 lines.

## Before calling anything done

Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test`.
