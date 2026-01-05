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

    pub fn add_config(&mut self, name: String, config: MountConfiguration) {
        self.mounts.insert(name, config);
    }

    pub fn remove_config(&mut self, name: &str) {
        self.mounts.remove(name);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &MountConfiguration)> {
        self.mounts.iter()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountConfiguration {
    pub device: String,
    pub mount_point: PathBuf,
    pub filesystem: Option<String>,
}

impl MountConfiguration {
    /// Attempt to mount this configuration
    pub fn mount(&self) -> io::Result<()> {
        let mut type_arg = Vec::new();
        if let Some(ref t) = self.filesystem {
            type_arg = vec!["-t", t];
        }

        let status = Command::new("sudo")
            .arg("mount")
            .args(type_arg)
            .arg(&self.device)
            .arg(&self.mount_point)
            .status()?;

        if status.success() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "mount command did not exit successfully",
            ))
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
            Err(io::Error::new(
                io::ErrorKind::Other,
                "umount command did not exit successfully",
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
