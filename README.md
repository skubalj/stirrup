# Stirrup 🐴: A TUI Filesystem Mount Manager

_When you need to mount up, put your foot in the stirrup_

Stirrup makes it easy to mount and unmount external filesystems on your Linux
system. It uses the same `sudo mount` command that you are familiar with, but
provides a TUI based layer on top which allows you to save configurations for
common mounts. This might not be that useful if you're just plugging in a flash
drive, but it's great for remembering your NFS shares.

## Features

- Manage mounting and unmounting devices with a convenient, ssh compatible user
  interface.
- Save frequently used mounts like NFS shares as profiles.
- Runs the system `sudo mount` command under the hood, and prompts for password
  via stdin. There is no need to run the program itself as root.

## Acknowledgements

This project is made possible thanks to the generous contributions of others!

| Dependency  | Owner / Maintainer                                    | License           |
| ----------- | ----------------------------------------------------- | ----------------- |
| anyhow      | David Tolnay                                          | MIT or Apache-2.0 |
| crossterm   | Timon                                                 | MIT               |
| directories | soc                                                   | MIT or Apache-2.0 |
| ratatui     | Orhum Parmaksiz, Dheepak Krishnamurthy, Josh McKinney | MIT               |
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
