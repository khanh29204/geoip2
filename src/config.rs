use std::{env, error::Error, ffi::OsString, io, path::PathBuf};

const DEFAULT_DB_PATH: &str = "/data/GeoLite2-City.mmdb";
const DEFAULT_PORT: u16 = 8080;

#[derive(Debug)]
pub struct Config {
    pub db_path: PathBuf,
    pub port: u16,
}

impl Config {
    pub fn from_env() -> Result<Self, Box<dyn Error + Send + Sync>> {
        Self::parse(env::var_os("DB_PATH"), env::var_os("PORT"))
    }

    pub fn parse(
        db_path: Option<OsString>,
        port: Option<OsString>,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let db_path = match db_path {
            Some(path) if path.is_empty() => {
                return Err(
                    io::Error::new(io::ErrorKind::InvalidInput, "DB_PATH cannot be empty").into(),
                );
            }
            Some(path) => PathBuf::from(path),
            None => PathBuf::from(DEFAULT_DB_PATH),
        };
        let port = match port {
            Some(port) => port
                .into_string()
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "PORT must be valid Unicode")
                })?
                .parse::<u16>()?,
            None => DEFAULT_PORT,
        };

        Ok(Self { db_path, port })
    }
}
