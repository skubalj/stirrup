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

use crate::{interface::MountTui, mount::ConfigFile};
use anyhow::bail;
use clap::Parser;
use crossterm::tty::IsTty;
use std::{fs, io::stdout, path::PathBuf};

mod interface;
mod mount;

macro_rules! println_colored {
    ($($arg:tt)*) => {{
        use crossterm::style::Stylize;
        println!("{}", crossterm::style::style(format!($($arg)*)).with(crossterm::style::Color::Blue));
    }};
}

/// Stirrup is a filesystem mount manager with a convenient terminal user interface
#[derive(Parser)]
#[command(version, author)]
#[command(after_long_help = "Copyright (C) 2026 Joseph Skubal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.")]
struct Args {
    /// The path to a non-standard config file
    #[arg(long, short)]
    config_file: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    if !stdout().is_tty() {
        bail!("stirrup must be run interactively");
    }

    let config_path = args.config_file.unwrap_or_else(config_file_path);
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
                    if x.is_luks_encrypted {
                        println_colored!("Decrypting {name}:");
                        if let Err(e) = x.decrypt() {
                            eprintln!("Error: Failed to decrypt {name}: {e}");
                            continue;
                        }
                    }

                    println_colored!("Mounting {name}:");
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
                    println_colored!("Unmounting {name}:");
                    if let Err(e) = x.unmount() {
                        eprintln!("Error: Failed to unmount {name}: {e}");
                    }

                    if x.is_luks_encrypted {
                        println_colored!("Closing decrypted {name}:");
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
    directories::ProjectDirs::from("", "", "stirrup")
        .expect("unable to get config directory")
        .config_local_dir()
        .join("config.toml")
}
