# Stirrup 🐴: A TUI Filesystem Mount Manager

_When you need to mount up, put your foot in the stirrup_

Stirrup makes it easy to mount and unmount external filesystems on your Linux
system. It uses the same `mount` command that you are familiar with, but
provides a TUI based layer on top which allows you to save configurations for
common mounts. If you're simply plugging in a flash drive, it's likely easier to
invoke `mount` directly, but if you have multiple NFS shares or a set of
frequently used devices, Stirrup makes it easier to mount them.

The terminal user interface makes stirrup particularly useful for managing
headless systems over SSH, and integrated LUKS decryption using `cryptsetup`
makes it easy to use secure drives.

## Features

- Manage mounting and unmounting devices with a convenient, ssh compatible user
  interface.
- Save frequently used mounts like NFS shares as profiles.
- Runs the system `sudo mount` and `sudo cryptsetup` commands under the hood,
  and prompts for password via stdin. There is no need to run the program itself
  as root.
- Stirrup prints the commands for subprocesses as they are executed. Debugging a
  faulty config is far easier when you can try running the commands manually.
- Decrypts LUKS devices as part of the mounting process (using `cryptsetup`)

## Installation

Stirrup can be built from source or installed automatically via crates.io. There
are no build-time system dependencies. However, you must ensure that your system
has the usual `sudo`, `mount`, and `cryptsetup` facilities.

#### Via crates.io

```sh
cargo install stirrup
```

#### From Source

```sh
git clone https://github.com/skubalj/stirrup.git
cd stirrup
cargo build --release
cp ./target/release/stirrup ~/.local/bin/stirrup
```

## Notes

- If you want to set up a configuration for a physical device, do not use the
  "proper" device name (eg: `/dev/sda1`). Many systems assign `sda`, `sdb`, etc
  on a first-come-first-served basis, so a drive that is `/dev/sda` today could
  be `/dev/sdb` tomorrow. Instead, use the symlinks in `/dev/disk/by-id`,
  `/dev/disk/by-uuid`, or `/dev/disk/by-label`. These symlinks will point to
  where the disk is actually attached.

- Configurations are stored in `~/.config/stirrup`. If you uninstall Stirrup,
  you will probably want to remove this file too.

- While the `NO_COLOR` environment variable is respected, it will make the TUI
  unusable. Styling is used to communicate which table row is selected.

- This program requires that stdout be a tty. If you want to manage your mounts
  non-interactively, write a bash script instead. Pro tip: You can run your
  workflow interactively with Stirrup, then copy the commands it prints to your
  script.

- Stirrup currently only supports Linux systems. To determine which
  configurations are mounted, it probes the `/etc/mtab` file.

## Acknowledgements

This project is made possible thanks to the generous contributions of others!

| Dependency  | Owner / Maintainer                                    | License           |
| ----------- | ----------------------------------------------------- | ----------------- |
| anyhow      | David Tolnay                                          | MIT or Apache-2.0 |
| clap        | Kevin K. / clap-rs Admins                             | MIT or Apache-2.0 |
| crossterm   | Timon                                                 | MIT               |
| directories | soc                                                   | MIT or Apache-2.0 |
| ratatui 🐀  | Orhum Parmaksiz, Dheepak Krishnamurthy, Josh McKinney | MIT               |
| serde       | David Tolnay                                          | MIT or Apache-2.0 |
| toml        | Eric Huss, Ed Page                                    | MIT or Apache-2.0 |
| tui-input   | Arijit Basu                                           | MIT               |

## License

Copyright (C) 2026 Joseph Skubal

This program is free software: you can redistribute it and/or modify it under
the terms of the GNU General Public License as published by the Free Software
Foundation, either version 3 of the License, or any later version.

This program is distributed in the hope that it will be useful, but WITHOUT ANY
WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A
PARTICULAR PURPOSE. See the GNU General Public License for more details.

You should have received a copy of the GNU General Public License along with
this program. If not, see <https://www.gnu.org/licenses/>.
