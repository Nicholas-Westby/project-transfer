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
(`-word`). A word that mixes cases, such as `ActivityKind` or `macOS` (not
`TLS`), is a phrase: `phrases.txt` lists it as written, `words.txt` lists
its parts (`activity` and `kind`), and `phrase-changes.txt` shows the
phrases a commit added or removed. A key or other random-looking phrase
adds no parts. A word that is also a phrase in lower case, such as
`coolthings` next to `CoolThings`, is listed only as the phrase. The
pre-commit hook refreshes the lists, reading what is staged, so a commit's
diff shows its new and dropped terms in two short files. Run
`cargo xtask words` to refresh them by hand. Binary files, `Cargo.lock` and
the lists themselves are left out.
