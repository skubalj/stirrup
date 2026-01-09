/*
stirrup: A TUI Mount Manager
Copyright (C) 2026 Joseph Skubal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

use std::{fs, path::PathBuf};

use crate::{interface::MountTui, mount::ConfigFile};

mod interface;
mod mount;

fn main() -> anyhow::Result<()> {
    let config_path = config_file_path();
    if let Some(p) = config_path.parent() {
        fs::create_dir_all(p)?;
    }

    let mut cfg = ConfigFile::read_from_file(&config_path)?;

    if let Some(result) = MountTui::run(&cfg)? {
        // First, update the config file
        cfg = ConfigFile::new(result.configurations);
        cfg.write_to_file(&config_path)?;

        // Then, we can mount / unmount our devices
        for name in result.to_mount {
            match cfg.get_config(&name) {
                Some(x) => {
                    if x.luks_decrypt_name.is_some() {
                        eprintln!("Decrypting {name}");
                        if let Err(e) = x.decrypt() {
                            eprintln!("Error: Failed to decrypt {name}: {e}");
                        }
                    }

                    eprintln!("Mounting {name} to {}", x.mount_point.to_string_lossy());
                    if let Err(e) = x.mount() {
                        eprintln!("Error: Failed to mount {name}: {e}");
                    }
                }
                None => eprintln!("Unable to find configuration with name '{name}'"),
            }
        }

        for name in result.to_unmount {
            match cfg.get_config(&name) {
                Some(x) => {
                    eprintln!("Unmounting {name} from {}", x.mount_point.to_string_lossy());
                    if let Err(e) = x.unmount() {
                        eprintln!("Error: Failed to unmount {name}: {e}");
                    }

                    if x.luks_decrypt_name.is_some() {
                        eprintln!("Closing decrypted {name}");
                        if let Err(e) = x.encrypt() {
                            eprintln!("Error: Failed to close decrypted {name}: {e}");
                        }
                    }
                }
                None => eprintln!("Unable to find configuration with name '{name}'"),
            }
        }
    }

    Ok(())
}

/// Get the path where the config file should be located
fn config_file_path() -> PathBuf {
    directories::ProjectDirs::from("", "", "saddle-up")
        .expect("unable to get config directory")
        .config_local_dir()
        .join("config.toml")
}
