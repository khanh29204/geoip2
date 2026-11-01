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

struct MappedDb {
    path: PathBuf,
    expected_types: &'static [&'static str],
    snapshot: RwLock<Snapshot>,
}

impl MappedDb {
    fn open(
        path: PathBuf,
        expected_types: &'static [&'static str],
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let snapshot = Self::load(&path, expected_types)?;
        Ok(Self {
            path,
            expected_types,
            snapshot: RwLock::new(snapshot),
        })
    }

    fn reader(&self) -> Result<Arc<Reader<Mmap>>, maxminddb::MaxMindDbError> {
        Ok(self
            .snapshot
            .read()
            .map_err(|_| maxminddb::MaxMindDbError::invalid_database("database lock poisoned"))?
            .reader
            .clone())
    }

    fn load(
        path: &Path,
        expected_types: &[&str],
    ) -> Result<Snapshot, Box<dyn Error + Send + Sync>> {
        let before = Fingerprint::read(path)?;
        // SAFETY: database updates use atomic rename, never mutate or truncate a mapped file.
        let reader = unsafe { Reader::open_mmap(path)? };
        if !expected_types.contains(&reader.metadata().database_type.as_str()) {
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

    fn reload_if_changed(&self) -> Result<bool, Box<dyn Error + Send + Sync>> {
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
        let candidate = Self::load(&self.path, self.expected_types)?;
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

pub struct GeoDb {
    city: MappedDb,
    asn: MappedDb,
}

impl GeoDb {
    pub fn open(
        city_path: impl Into<PathBuf>,
        asn_path: impl Into<PathBuf>,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        Ok(Self {
            city: MappedDb::open(city_path.into(), &["GeoLite2-City", "GeoIP2-City"])?,
            asn: MappedDb::open(asn_path.into(), &["GeoLite2-ASN"])?,
        })
    }

    pub fn lookup(&self, ip: IpAddr) -> Result<Option<GeoIpResponse>, maxminddb::MaxMindDbError> {
        let city_reader = self.city.reader()?;
        let asn_reader = self.asn.reader()?;
        let city = city_reader.lookup(ip)?.decode::<geoip2::City>()?;
        let asn = asn_reader.lookup(ip)?.decode::<geoip2::Asn>()?;
        if city.is_none() && asn.is_none() {
            return Ok(None);
        }

        let mut response = GeoIpResponse {
            ip: ip.to_string(),
            ..GeoIpResponse::default()
        };
        if let Some(record) = city {
            let subdivision = record.subdivisions.last();
            response.country_code = record.country.iso_code.map(str::to_owned);
            response.country_name = record.country.names.english.map(str::to_owned);
            response.region_code =
                subdivision.and_then(|region| region.iso_code.map(str::to_owned));
            response.region_name =
                subdivision.and_then(|region| region.names.english.map(str::to_owned));
            response.city = record.city.names.english.map(str::to_owned);
            response.zip_code = record.postal.code.map(str::to_owned);
            response.time_zone = record.location.time_zone.map(str::to_owned);
            response.latitude = record.location.latitude;
            response.longitude = record.location.longitude;
        }
        if let Some(record) = asn {
            response.asn = record.autonomous_system_number;
            response.asn_organization = record.autonomous_system_organization.map(str::to_owned);
        }
        Ok(Some(response))
    }

    pub fn reload_if_changed(&self) -> Result<bool, Box<dyn Error + Send + Sync>> {
        let city = self.city.reload_if_changed();
        let asn = self.asn.reload_if_changed();
        match (city, asn) {
            (Ok(city), Ok(asn)) => Ok(city || asn),
            (Err(error), _) => Err(error),
            (_, Err(error)) => Err(error),
        }
    }
}
