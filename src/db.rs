use std::{
    error::Error,
    fs, io,
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::SystemTime,
};

use maxminddb::{geoip2, Mmap, Reader};

use crate::models::GeoIpResponse;

#[derive(PartialEq, Eq)]
struct Fingerprint {
    modified: SystemTime,
    len: u64,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl Fingerprint {
    fn read(path: &Path) -> io::Result<Self> {
        let metadata = fs::metadata(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self {
                modified: metadata.modified()?,
                len: metadata.len(),
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {
                modified: metadata.modified()?,
                len: metadata.len(),
            })
        }
    }
}

struct Snapshot {
    reader: Arc<Reader<Mmap>>,
    fingerprint: Fingerprint,
}

pub struct GeoDb {
    path: PathBuf,
    snapshot: RwLock<Snapshot>,
}

impl GeoDb {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let path = path.into();
        let snapshot = Self::load(&path)?;
        Ok(Self {
            path,
            snapshot: RwLock::new(snapshot),
        })
    }

    pub fn lookup(&self, ip: IpAddr) -> Result<Option<GeoIpResponse>, maxminddb::MaxMindDbError> {
        let reader = self
            .snapshot
            .read()
            .map_err(|_| maxminddb::MaxMindDbError::invalid_database("database lock poisoned"))?
            .reader
            .clone();
        let Some(record) = reader.lookup(ip)?.decode::<geoip2::City>()? else {
            return Ok(None);
        };
        let subdivision = record.subdivisions.last();

        Ok(Some(GeoIpResponse {
            ip: ip.to_string(),
            country_code: record.country.iso_code.map(str::to_owned),
            country_name: record.country.names.english.map(str::to_owned),
            region_code: subdivision.and_then(|region| region.iso_code.map(str::to_owned)),
            region_name: subdivision.and_then(|region| region.names.english.map(str::to_owned)),
            city: record.city.names.english.map(str::to_owned),
            zip_code: record.postal.code.map(str::to_owned),
            time_zone: record.location.time_zone.map(str::to_owned),
            latitude: record.location.latitude,
            longitude: record.location.longitude,
        }))
    }

    fn load(path: &Path) -> Result<Snapshot, Box<dyn Error + Send + Sync>> {
        let before = Fingerprint::read(path)?;
        // SAFETY: database updates use atomic rename, never mutate or truncate a mapped file.
        let reader = unsafe { Reader::open_mmap(path)? };
        if !matches!(
            reader.metadata().database_type.as_str(),
            "GeoLite2-City" | "GeoIP2-City"
        ) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "unsupported database type: {}",
                    reader.metadata().database_type
                ),
            )
            .into());
        }
        let after = Fingerprint::read(path)?;
        if before != after {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "database changed while loading",
            )
            .into());
        }
        Ok(Snapshot {
            reader: Arc::new(reader),
            fingerprint: after,
        })
    }

    pub fn reload_if_changed(&self) -> Result<bool, Box<dyn Error + Send + Sync>> {
        let current = Fingerprint::read(&self.path)?;
        {
            let snapshot = self
                .snapshot
                .read()
                .map_err(|_| io::Error::other("database lock poisoned"))?;
            if current == snapshot.fingerprint {
                return Ok(false);
            }
        }
        let candidate = Self::load(&self.path)?;
        let mut snapshot = self
            .snapshot
            .write()
            .map_err(|_| io::Error::other("database lock poisoned"))?;
        if candidate.fingerprint == snapshot.fingerprint {
            return Ok(false);
        }
        *snapshot = candidate;
        Ok(true)
    }
}
