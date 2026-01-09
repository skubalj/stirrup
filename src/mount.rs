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
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ConfigFile {
    mounts: BTreeMap<String, MountConfiguration>,
}

impl ConfigFile {
    pub fn new(mounts: BTreeMap<String, MountConfiguration>) -> Self {
        Self { mounts }
    }

    pub fn read_from_file(p: &Path) -> anyhow::Result<ConfigFile> {
        let data = match fs::read(p) {
            Ok(x) => x,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).context("unable to open config file"),
        };

        toml::from_slice(&data).context("unable to deserialize config file")
    }

    pub fn write_to_file(&self, p: &Path) -> anyhow::Result<()> {
        let payload = toml::to_string_pretty(&self)?;
        fs::write(p, payload).context("unable to write to config file")
    }

    pub fn get_config(&self, name: &str) -> Option<&MountConfiguration> {
        self.mounts.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &MountConfiguration)> {
        self.mounts.iter()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountConfiguration {
    pub device: String,
    pub luks_decrypt_name: Option<String>,
    pub mount_point: PathBuf,
    pub filesystem: Option<String>,
}

impl MountConfiguration {
    /// Return the device that should be mounted for this configuration. This is either the given
    /// device, or the mapped device if this is a decyprted volume.
    fn mount_device(&self) -> PathBuf {
        match &self.luks_decrypt_name {
            Some(x) => Path::new("/dev/mapper").join(x),
            None => PathBuf::from(&self.device),
        }
    }

    /// Attempt to mount this configuration
    pub fn mount(&self) -> io::Result<()> {
        let type_arg = if let Some(ref t) = self.filesystem {
            vec!["-t", t]
        } else {
            Vec::new()
        };

        println!("{:?}", self.mount_device());

        let status = Command::new("sudo")
            .arg("mount")
            .args(type_arg)
            .arg(self.mount_device())
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
                self.luks_decrypt_name.as_ref().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "no luks decrypt name specified",
                    )
                })?,
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
            .args([
                "cryptsetup",
                "luksClose",
                self.luks_decrypt_name.as_ref().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "no luks decrypt name specified",
                    )
                })?,
            ])
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
            device: missing_data_msg(fields.next(), "no device found in mtab record")?.to_owned(),
            mount_point: missing_data_msg(fields.next(), "no mount point found in mtab record")?
                .into(),
            luks_decrypt_name: None,
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
