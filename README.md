# Project Transfer

Project Transfer is a desktop app for macOS and Windows that copies project
folders between your computers on the same local network. Open it on two
computers, pair them once, and from then on push or pull a project with one
button. The app finds the other computer by itself, talks to it over its own
encrypted connection, and shows you every change before it writes anything.
There is nothing to set up in a terminal.

## Install

You need Rust (stable) to build it. From the repository, run the helper for
your system:

| System  | Command                         |
| ------- | ------------------------------- |
| macOS   | `./install`                     |
| Windows | `.\install.ps1` (in PowerShell) |

Both run `cargo xtask install`, which you can also run yourself. If Windows
says running scripts is disabled, run
`powershell -ExecutionPolicy Bypass -File .\install.ps1` instead.

The installer builds a release binary and installs it:

| System  | What it does                                                                                                                                                                                                                                  |
| ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| macOS   | Builds `Project Transfer.app`, signs it ad hoc and installs it in `/Applications`, or in `~/Applications` when `/Applications` is not writable. A running copy is quit first. The old copy is kept aside until the new one is in place. |
| Windows | Copies `Project Transfer.exe` to `%LOCALAPPDATA%\Programs\Project Transfer` and adds a Start menu shortcut. A running copy is closed first.                                                                                                |

To install somewhere else, pass a folder:

```
./install --dest ~/Desktop/test-install
```

The installed app does not need the source checkout. Settings and paired
computers are kept when you reinstall.

## First run and pairing

On first launch the app gives this computer a name made from the computer name
and two random words, such as `Studio Amber Otter`. You can change it in
Settings.

To pair two computers:

1. Open Project Transfer on both.
2. On one, open the computer menu in the top bar, pick the other computer and
   click **Pair**.
3. Choose what this computer allows, and what you ask the other one to allow:
   - **Push to this computer**: the other computer may overwrite this
     computer's files.
   - **Pull from this computer**: the other computer may read this computer's
     files.
4. Both computers show the same 6-digit code, new for every pairing. On the
   other computer, check the code, choose what it allows and click **Pair**
   (or **Decline**). Push starts unticked there even if it was asked for.
5. On the computer you started on, answer "Does *name* show this code?" with
   **Codes match** or **Cancel**. You can answer before or after the other
   computer does.

Nothing is saved on either computer until both people have confirmed. If
either one declines, cancels or stops waiting, neither computer keeps the
pairing and both say why.

Push is off by default on both sides, because it lets the other computer
overwrite your files. Each computer decides only what others may do to it,
and you can change that later under Computers. A computer that is not listed
can be added by address (shown in the other computer's Settings).

Pairing is only needed once. The two computers recognize each other by their
certificates from then on, and a connection from a computer with a changed
certificate is refused.

## Projects and folders

A project is a name plus one or more folders that move together. Each computer
picks its own folder for each one, with the normal folder picker, so the paths
can differ (`~/Dev/garden` on a Mac, `D:\dev\garden` on Windows).

- Project and folder names become folder names on every paired computer, so
  names with `/` or `\`, or names Windows can't hold (`CON`, `a:b`, a
  trailing dot), are refused with the reason.
- Your home folder and the top of a disk can't be project folders: a mirror
  of them would rewrite everything on them.
- The first folder you add is the project's **primary folder**. Commands run
  there. You can choose another primary folder later.
- When a project arrives that this computer has never seen, it goes into the
  **Projects folder** from Settings (`~/Dev` on macOS, `%USERPROFILE%\Dev` on
  Windows). A single-folder project lands in `<Projects folder>/<folder>`; a
  project with several folders in `<Projects folder>/<project>/<folder>`.
- If that path already belongs to another project, the app adds a number
  instead of mixing the two: `app` becomes `app 2`, then `app 3`.
- The preview always shows the path a new folder will get, and you can change
  it afterwards on that computer.
- Projects the selected computer has and this one doesn't are listed under
  **Only on** *name*, below your own projects. **Pull** next to one opens the
  normal preview and creates the project here. It is greyed out, with the
  reason, when that computer doesn't let this one pull.

Deleting a project or removing a folder only removes the setup. Files on disk
are kept.

## Push, pull and the preview

**Push** sends this computer's copy to the selected computer. **Pull** fetches
the selected computer's copy to this one. Both are mirrors: afterwards the
destination matches the source, including deletions.

Before anything is written, a preview lists every change with counts:

| Kind           | Meaning                                                    |
| -------------- | ---------------------------------------------------------- |
| Added          | Only on the source                                         |
| Changed        | Different content                                          |
| Timestamp only | Same content, different modified time                      |
| Deleted        | Only on the destination, so it is removed                  |

Things to know:

- Empty folders are created and removed like files.
- A folder that no longer exists on the source is removed from the
  destination with everything in it, ignored files included. The preview
  shows it as one line, such as "Remove folder `old-sketches` (312 files, 280 of
  them ignored)".
- Ignored files inside a folder that still exists on the source are never
  touched.
- The preview warns when files on the destination are newer than the source,
  so you do not overwrite newer work by accident.
- Paths that differ only by case are flagged and the second is skipped.
  Names Windows cannot hold are flagged and skipped when the destination is
  Windows.
- Nothing is written until you click the confirm button, which names the
  action and the count, for example "Push 42 changes to Desktop".
- Each file is written to a temporary file and renamed into place, so a
  cancelled or interrupted transfer never leaves a half-written file.

**Send everything** skips the ignore list for the next transfer only. It is
useful for a first copy, but it also sends folders such as `node_modules`.

## Commands

A project can have commands, such as "Install git hooks" or "Fetch
dependencies". Each has a label and a command line.

- A command runs only on the computer where you click it, in the project's
  primary folder. Nothing runs by itself and nothing runs on the other
  computer.
- macOS runs it with your login shell; Windows runs it with PowerShell.
- Output streams into a panel under the command, ending with the exit code.
  A running command can be stopped; Stop also ends anything it started.
- Commands travel with the project: a command added or deleted on one
  computer shows up on the other after the next push or pull, not straight
  away.
- A command that came from another computer, or that changed since it last
  ran here, shows its full text and asks before it runs for the first time.
- A command created on the other system (a Mac command on Windows, say) shows
  a warning first, since it rarely works there.

## Ignore list

Some folders are regenerated by tools and are not worth copying. By default
the app skips `node_modules`, `bin`, `obj`, `packages`, `dist`, `.build`,
`target`, `.vs`, `.idea`, `__pycache__`, `.pytest_cache`, `.gradle`, `.next`,
`.nuxt`, `.cache`, `.DS_Store` and `Thumbs.db`.

In Settings you can add and remove patterns (gitignore style) and add
**always include** patterns, which win over every ignore pattern. The list of
the computer that starts a transfer applies to both sides, and changes apply
to the next transfer.

## Local network only

The app only talks to computers on your local network. Discovery uses mDNS,
which cannot leave the network, and the app refuses any address that is not a
private one (such as `192.168.x.x`, `10.x.x.x` or `172.16.x.x` to
`172.31.x.x`). Connections are encrypted with TLS. The app listens on TCP
port 47820, or on another free port if that one is taken; if a firewall asks,
allow it on private networks.

If your network blocks discovery, use **Add by address** with the address
shown in the other computer's Settings.

## Settings and logs

Each computer keeps its settings in its app data folder:

| System  | Folder                                           |
| ------- | ------------------------------------------------ |
| macOS   | `~/Library/Application Support/Project Transfer` |
| Windows | `%LOCALAPPDATA%\Project Transfer`                |

| File or folder  | Holds                                                     |
| --------------- | --------------------------------------------------------- |
| `instance.json` | This computer's id, name, Projects folder, ignore list    |
| `peers.json`    | Paired computers, their certificates and permissions      |
| `projects.json` | Projects, folders, local paths and commands               |
| `identity/`     | This computer's certificate and private key               |
| `logs/`         | A log file per day, kept for 14 days                      |

Settings has an **Open log folder** button. The log records connections,
pairing, permission checks, transfers and command runs, but never file
contents or command output.

Set `PROJECT_TRANSFER_HOME` to a folder to keep everything there instead.
This is how two copies run side by side on one computer.

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
