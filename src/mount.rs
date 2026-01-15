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

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ConfigFile {
    mounts: Vec<MountConfiguration>,
}

impl ConfigFile {
    pub fn new(mut mounts: Vec<MountConfiguration>) -> Self {
        mounts.sort_by(|a, b| a.name.cmp(&b.name));
        Self { mounts }
    }

    pub fn read_from_file(p: &Path) -> anyhow::Result<ConfigFile> {
        let mut file: ConfigFile = match fs::read(p) {
            Ok(data) => toml::from_slice(&data).context("unable to deserialize config file")?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).context("unable to open config file"),
        };

        file.mounts.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(file)
    }

    pub fn write_to_file(&self, p: &Path) -> anyhow::Result<()> {
        let payload = toml::to_string_pretty(&self)?;
        fs::write(p, payload).context("unable to write to config file")
    }

    pub fn get_config(&self, name: &str) -> Option<&MountConfiguration> {
        self.mounts
            .binary_search_by(|probe| probe.name.as_str().cmp(name))
            .ok()
            .and_then(|idx| self.mounts.get(idx))
    }

    pub fn iter(&self) -> impl Iterator<Item = &MountConfiguration> {
        self.mounts.iter()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountConfiguration {
    pub name: String,
    pub device: String,
    pub is_luks_encrypted: bool,
    pub mount_point: PathBuf,
    pub filesystem: Option<String>,
}

impl MountConfiguration {
    /// The name that will be used when creating a cryptsetup mapping
    fn cryptsetup_mapping(&self) -> String {
        let mut prefix = String::from("stirrup-");
        prefix.extend(
            self.name
                .chars()
                .map(|x| if x.is_ascii_alphanumeric() { x } else { '_' }),
        );

        prefix
    }

    /// Attempt to mount this configuration
    pub fn mount(&self) -> io::Result<()> {
        let type_arg = if let Some(ref t) = self.filesystem {
            vec!["-t", t]
        } else {
            Vec::new()
        };

        let mount_device = if self.is_luks_encrypted {
            Path::new("/dev/mapper").join(self.cryptsetup_mapping())
        } else {
            PathBuf::from(&self.device)
        };

        let status = Command::new("sudo")
            .arg("mount")
            .args(type_arg)
            .arg(mount_device)
            .arg(&self.mount_point)
            .status()?;

        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other("mount command did not exit successfully"))
        }
    }

    /// Attempt to unmount this configuration
    pub fn unmount(&self) -> io::Result<()> {
        let status = Command::new("sudo")
            .arg("umount")
            .arg(&self.mount_point)
            .status()?;

        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other("umount command did not exit successfully"))
        }
    }

    pub fn decrypt(&self) -> io::Result<()> {
        let status = Command::new("sudo")
            .args([
                "cryptsetup",
                "luksOpen",
                self.device.as_str(),
                &self.cryptsetup_mapping(),
            ])
            .status()?;

        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(
                "cryptsetup luksOpen ommand did not exit successfully",
            ))
        }
    }

    pub fn encrypt(&self) -> io::Result<()> {
        let status = Command::new("sudo")
            .args(["cryptsetup", "luksClose", &self.cryptsetup_mapping()])
            .status()?;

        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(
                "cryptsetup luksClose command did not exit successfully",
            ))
        }
    }
}

/// Probe `/etc/mtab` and return the records as configurations
pub fn probe_mtab() -> io::Result<Vec<MountConfiguration>> {
    let data = fs::read_to_string("/etc/mtab")?;

    let mut configs = Vec::new();
    for record in data.lines() {
        let mut fields = record.split_ascii_whitespace().take(3);
        configs.push(MountConfiguration {
            name: String::new(),
            device: missing_data_msg(fields.next(), "no device found in mtab record")?.to_owned(),
            mount_point: missing_data_msg(fields.next(), "no mount point found in mtab record")?
                .into(),
            is_luks_encrypted: false,
            filesystem: Some(
                missing_data_msg(fields.next(), "no filesystem found in mtab record")?.to_owned(),
            ),
        })
    }

    Ok(configs)
}

fn missing_data_msg<'a>(data: Option<&'a str>, msg: &str) -> io::Result<&'a str> {
    data.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, msg))
}
