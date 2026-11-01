use serde::Serialize;

#[derive(Debug, Default, Serialize)]
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
    pub asn: Option<u32>,
    pub asn_organization: Option<String>,
}
