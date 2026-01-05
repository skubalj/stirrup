use std::path::PathBuf;

use anyhow::Context;

use crate::{interface::MountTui, mount::ConfigFile};

mod interface;
mod mount;

fn main() -> anyhow::Result<()> {
    let config_path = config_file_path();
    let mut cfg = ConfigFile::read_from_file(&config_path)?;

    let result = MountTui::run(&cfg)?;

    // First, we'll update the config file
    for (name, config) in result.to_create {
        cfg.add_config(name, config);
    }
    for name in result.to_remove {
        cfg.remove_config(&name);
    }

    // cfg.write_to_file(&config_path)?;

    // Then, we can mount / unmount our devices
    for name in result.to_mount {
        match cfg.get_config(&name) {
            Some(x) => {
                eprintln!("Mounting {name} to {}", x.mount_point.to_string_lossy());
                x.mount()
                    .with_context(|| format!("failed to mount {name}"))?
            }
            None => eprintln!("Unable to find configuration with name '{name}'"),
        }
    }
    for name in result.to_unmount {
        match cfg.get_config(&name) {
            Some(x) => {
                eprintln!("Unmounting {name} from {}", x.mount_point.to_string_lossy());
                x.unmount()
                    .with_context(|| format!("failed to unmount {name}"))?
            }
            None => eprintln!("Unable to find configuration with name '{name}'"),
        }
    }

    Ok(())
}

/// Get the path where the config file should be located
fn config_file_path() -> PathBuf {
    PathBuf::from("./config.cfg")
    // directories::ProjectDirs::from("", "", "saddleup")
    //     .expect("unable to get config directory")
    //     .config_local_dir()
    //     .join("config.toml")
}
