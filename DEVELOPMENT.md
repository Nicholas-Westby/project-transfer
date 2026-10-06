## Development

```
cargo fmt --check
cargo clippy --all-targets --workspace -- -D warnings
cargo test --workspace
```

Tests that need multicast are ignored by default. Run them with
`cargo test discovery -- --ignored`.

To run two copies on one computer, give each its own home and open them in
two terminals:

```
PROJECT_TRANSFER_HOME=/tmp/pt-a RUST_LOG=info cargo run
PROJECT_TRANSFER_HOME=/tmp/pt-b RUST_LOG=info cargo run
```

On Windows PowerShell, set the variable first with
`$env:PROJECT_TRANSFER_HOME = "$env:TEMP\pt-a"`. The second copy picks a free
port by itself, and the two find each other within a few seconds.

`cargo xtask icon` redraws `assets/icon.png` and `assets/icon.ico`.
`cargo xtask screenshot` redraws the README screenshot,
`assets/images/project-transfer.webp`, from the UI tests' made-up computers
and projects. Run it whenever the main window looks different, and commit the
image.

`words.txt` lists every word in the files git tracks, one per line, and
`word-changes.txt` lists the words a commit added (`+word`) or removed
(`-word`). The pre-commit hook rewrites both, reading what is staged, so a
commit's diff shows its new and dropped terms in one short file. Run
`cargo xtask words` to refresh them by hand. Binary files, `Cargo.lock` and
the two lists themselves are left out.
