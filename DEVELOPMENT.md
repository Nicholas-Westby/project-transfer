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
