use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use latch_core::DeviceId;
use serde::{Deserialize, Serialize};

use crate::LinkError;

const IDENTITY_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub device_id: DeviceId,
    pub device_name: String,
}

#[derive(Serialize, Deserialize)]
struct StoredIdentity {
    version: u8,
    device_id: DeviceId,
}

pub fn default_device_id_path() -> Result<PathBuf, LinkError> {
    let base = dirs::data_local_dir().ok_or(LinkError::ApplicationDataUnavailable)?;
    Ok(base.join("Latch").join("device.json"))
}

pub fn load_or_create_device_id(path: &Path) -> Result<DeviceId, LinkError> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => write_new_identity(path, file),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => read_identity(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| LinkError::IdentityIo {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            match OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(file) => write_new_identity(path, file),
                Err(source) if source.kind() == io::ErrorKind::AlreadyExists => read_identity(path),
                Err(source) => Err(LinkError::IdentityIo {
                    path: path.to_path_buf(),
                    source,
                }),
            }
        }
        Err(source) => Err(LinkError::IdentityIo {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub fn load_device_credential(device_id: DeviceId) -> Result<Option<String>, LinkError> {
    let entry = keyring::Entry::new("Latch", &device_id.to_string())
        .map_err(|_| LinkError::CredentialStore)?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(LinkError::CredentialStore),
    }
}

pub fn store_device_credential(device_id: DeviceId, credential: &str) -> Result<(), LinkError> {
    keyring::Entry::new("Latch", &device_id.to_string())
        .and_then(|entry| entry.set_password(credential))
        .map_err(|_| LinkError::CredentialStore)
}

pub fn delete_device_credential(device_id: DeviceId) -> Result<(), LinkError> {
    let entry = keyring::Entry::new("Latch", &device_id.to_string())
        .map_err(|_| LinkError::CredentialStore)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(LinkError::CredentialStore),
    }
}

fn write_new_identity(path: &Path, file: File) -> Result<DeviceId, LinkError> {
    let device_id = DeviceId::new();
    let stored = StoredIdentity {
        version: IDENTITY_VERSION,
        device_id,
    };
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, &stored).map_err(|source| LinkError::InvalidIdentity {
        path: path.to_path_buf(),
        source,
    })?;
    writer.flush().map_err(|source| LinkError::IdentityIo {
        path: path.to_path_buf(),
        source,
    })?;
    writer
        .get_ref()
        .sync_all()
        .map_err(|source| LinkError::IdentityIo {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(device_id)
}

fn read_identity(path: &Path) -> Result<DeviceId, LinkError> {
    let file = File::open(path).map_err(|source| LinkError::IdentityIo {
        path: path.to_path_buf(),
        source,
    })?;
    let stored: StoredIdentity =
        serde_json::from_reader(BufReader::new(file)).map_err(|source| {
            LinkError::InvalidIdentity {
                path: path.to_path_buf(),
                source,
            }
        })?;
    if stored.version != IDENTITY_VERSION {
        return Err(LinkError::UnsupportedIdentityVersion {
            path: path.to_path_buf(),
            version: stored.version,
        });
    }
    Ok(stored.device_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_is_persisted_across_reloads() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nested").join("device.json");

        let first = load_or_create_device_id(&path).unwrap();
        let second = load_or_create_device_id(&path).unwrap();

        assert_eq!(first, second);
        assert!(path.exists());
    }
}
