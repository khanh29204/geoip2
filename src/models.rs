use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct GeoIpResponse {
    pub ip: String,
    pub country_code: Option<String>,
    pub country_name: Option<String>,
    pub region_code: Option<String>,
    pub region_name: Option<String>,
    pub city: Option<String>,
    pub zip_code: Option<String>,
    pub time_zone: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}
