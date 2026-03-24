//! Weather WASM Tool for IronClaw.
//!
//! Interact with Open-Meteo (no API key required):
//! - Get current weather conditions for any city
//! - Get 5-day daily weather forecast
//! - Get air quality / pollution data by coordinates
//!
//! Geocoding: https://geocoding-api.open-meteo.com/v1/search
//! Weather:   https://api.open-meteo.com/v1/forecast
//! Air quality: https://air-quality-api.open-meteo.com/v1/air-quality

wit_bindgen::generate!({
    world: "sandboxed-tool",
    path: "../../wit/tool.wit",
});

use serde::{Deserialize, Serialize};

struct WeatherTool;

impl exports::near::agent::tool::Guest for WeatherTool {
    fn execute(req: exports::near::agent::tool::Request) -> exports::near::agent::tool::Response {
        match execute_inner(&req.params) {
            Ok(result) => exports::near::agent::tool::Response {
                output: Some(result),
                error: None,
            },
            Err(e) => exports::near::agent::tool::Response {
                output: None,
                error: Some(e),
            },
        }
    }

    fn schema() -> String {
        SCHEMA.to_string()
    }

    fn description() -> String {
        "Get weather information using Open-Meteo (no API key required). Supports three actions: \
         'get_current' returns current weather conditions for a city (temperature, humidity, wind, \
         description); 'get_forecast' returns a 5-day daily forecast; \
         'get_air_quality' returns air pollution data for given coordinates."
            .to_string()
    }
}

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Action {
    GetCurrent(WeatherParams),
    GetForecast(WeatherParams),
    GetAirQuality(AirQualityParams),
}

#[derive(Debug, Deserialize)]
struct WeatherParams {
    city: String,
    #[serde(default)]
    country_code: Option<String>,
    #[serde(default)]
    units: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AirQualityParams {
    lat: f64,
    lon: f64,
}

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct CurrentWeatherOutput {
    city: String,
    country: String,
    latitude: f64,
    longitude: f64,
    temperature: f64,
    feels_like: f64,
    humidity: u32,
    pressure: f64,
    description: String,
    wind_speed: f64,
    wind_direction: u32,
    clouds_percent: u32,
    units: String,
}

#[derive(Debug, Serialize)]
struct ForecastOutput {
    city: String,
    country: String,
    units: String,
    entries: Vec<ForecastEntry>,
}

#[derive(Debug, Serialize)]
struct ForecastEntry {
    date: String,
    temp_max: f64,
    temp_min: f64,
    description: String,
    wind_speed_max: f64,
    precipitation_probability_max: u32,
}

#[derive(Debug, Serialize)]
struct AirQualityOutput {
    lat: f64,
    lon: f64,
    european_aqi: u32,
    aqi_label: String,
    pm2_5: f64,
    pm10: f64,
    nitrogen_dioxide: f64,
    ozone: f64,
    carbon_monoxide: f64,
}

// ---------------------------------------------------------------------------
// Geocoding
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GeoResult {
    latitude: f64,
    longitude: f64,
    name: String,
    country: String,
}

fn geocode(city: &str, country_code: Option<&str>) -> Result<GeoResult, String> {
    let mut url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
        url_encode(city)
    );
    if let Some(cc) = country_code {
        if !cc.is_empty() {
            url.push_str(&format!("&countryCode={}", url_encode(cc)));
        }
    }

    near::agent::host::log(
        near::agent::host::LogLevel::Info,
        &format!("Geocoding city: {city}"),
    );

    let resp = api_get(&url)?;
    let data: serde_json::Value =
        serde_json::from_str(&resp).map_err(|e| format!("Failed to parse geocoding response: {e}"))?;

    let results = data["results"]
        .as_array()
        .ok_or_else(|| format!("City not found: {city}"))?;

    if results.is_empty() {
        return Err(format!("City not found: {city}"));
    }

    let r = &results[0];
    Ok(GeoResult {
        latitude: r["latitude"].as_f64().unwrap_or(0.0),
        longitude: r["longitude"].as_f64().unwrap_or(0.0),
        name: r["name"].as_str().unwrap_or(city).to_string(),
        country: r["country"].as_str().unwrap_or("").to_string(),
    })
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

fn execute_inner(params: &str) -> Result<String, String> {
    let action: Action =
        serde_json::from_str(params).map_err(|e| format!("Invalid parameters: {e}"))?;

    match action {
        Action::GetCurrent(p) => get_current(p),
        Action::GetForecast(p) => get_forecast(p),
        Action::GetAirQuality(p) => get_air_quality(p),
    }
}

// ---------------------------------------------------------------------------
// Action implementations
// ---------------------------------------------------------------------------

fn get_current(params: WeatherParams) -> Result<String, String> {
    if params.city.is_empty() {
        return Err("'city' must not be empty".into());
    }

    let geo = geocode(&params.city, params.country_code.as_deref())?;
    let units = params.units.as_deref().unwrap_or("metric");
    let temp_unit = if units == "imperial" { "fahrenheit" } else { "celsius" };
    let wind_unit = if units == "imperial" { "mph" } else { "ms" };

    let url = format!(
        "https://api.open-meteo.com/v1/forecast\
         ?latitude={}&longitude={}\
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,\
         weather_code,wind_speed_10m,wind_direction_10m,cloud_cover,surface_pressure\
         &temperature_unit={}&wind_speed_unit={}",
        geo.latitude, geo.longitude, temp_unit, wind_unit
    );

    near::agent::host::log(
        near::agent::host::LogLevel::Info,
        &format!("Fetching current weather for: {} ({}, {})", geo.name, geo.latitude, geo.longitude),
    );

    let resp = api_get(&url)?;
    let data: serde_json::Value =
        serde_json::from_str(&resp).map_err(|e| format!("Failed to parse response: {e}"))?;

    let current = &data["current"];
    let output = CurrentWeatherOutput {
        city: geo.name,
        country: geo.country,
        latitude: geo.latitude,
        longitude: geo.longitude,
        temperature: current["temperature_2m"].as_f64().unwrap_or(0.0),
        feels_like: current["apparent_temperature"].as_f64().unwrap_or(0.0),
        humidity: current["relative_humidity_2m"].as_u64().unwrap_or(0) as u32,
        pressure: current["surface_pressure"].as_f64().unwrap_or(0.0),
        description: wmo_description(current["weather_code"].as_u64().unwrap_or(0) as u32),
        wind_speed: current["wind_speed_10m"].as_f64().unwrap_or(0.0),
        wind_direction: current["wind_direction_10m"].as_u64().unwrap_or(0) as u32,
        clouds_percent: current["cloud_cover"].as_u64().unwrap_or(0) as u32,
        units: units.to_string(),
    };

    serde_json::to_string(&output).map_err(|e| format!("Failed to serialize output: {e}"))
}

fn get_forecast(params: WeatherParams) -> Result<String, String> {
    if params.city.is_empty() {
        return Err("'city' must not be empty".into());
    }

    let geo = geocode(&params.city, params.country_code.as_deref())?;
    let units = params.units.as_deref().unwrap_or("metric");
    let temp_unit = if units == "imperial" { "fahrenheit" } else { "celsius" };
    let wind_unit = if units == "imperial" { "mph" } else { "ms" };

    let url = format!(
        "https://api.open-meteo.com/v1/forecast\
         ?latitude={}&longitude={}\
         &daily=temperature_2m_max,temperature_2m_min,weather_code,\
         wind_speed_10m_max,precipitation_probability_max\
         &temperature_unit={}&wind_speed_unit={}&forecast_days=5",
        geo.latitude, geo.longitude, temp_unit, wind_unit
    );

    near::agent::host::log(
        near::agent::host::LogLevel::Info,
        &format!("Fetching 5-day forecast for: {}", geo.name),
    );

    let resp = api_get(&url)?;
    let data: serde_json::Value =
        serde_json::from_str(&resp).map_err(|e| format!("Failed to parse response: {e}"))?;

    let daily = &data["daily"];
    let times = daily["time"].as_array().cloned().unwrap_or_default();
    let temp_max = daily["temperature_2m_max"].as_array().cloned().unwrap_or_default();
    let temp_min = daily["temperature_2m_min"].as_array().cloned().unwrap_or_default();
    let codes = daily["weather_code"].as_array().cloned().unwrap_or_default();
    let wind = daily["wind_speed_10m_max"].as_array().cloned().unwrap_or_default();
    let precip = daily["precipitation_probability_max"].as_array().cloned().unwrap_or_default();

    let entries: Vec<ForecastEntry> = times
        .iter()
        .enumerate()
        .map(|(i, t)| ForecastEntry {
            date: t.as_str().unwrap_or("").to_string(),
            temp_max: temp_max.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0),
            temp_min: temp_min.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0),
            description: wmo_description(
                codes.get(i).and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            ),
            wind_speed_max: wind.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0),
            precipitation_probability_max: precip
                .get(i)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        })
        .collect();

    let output = ForecastOutput {
        city: geo.name,
        country: geo.country,
        units: units.to_string(),
        entries,
    };

    serde_json::to_string(&output).map_err(|e| format!("Failed to serialize output: {e}"))
}

fn get_air_quality(params: AirQualityParams) -> Result<String, String> {
    if params.lat < -90.0 || params.lat > 90.0 {
        return Err(format!("'lat' must be between -90 and 90, got {}", params.lat));
    }
    if params.lon < -180.0 || params.lon > 180.0 {
        return Err(format!("'lon' must be between -180 and 180, got {}", params.lon));
    }

    let url = format!(
        "https://air-quality-api.open-meteo.com/v1/air-quality\
         ?latitude={}&longitude={}\
         &current=pm10,pm2_5,carbon_monoxide,nitrogen_dioxide,ozone,european_aqi",
        params.lat, params.lon
    );

    near::agent::host::log(
        near::agent::host::LogLevel::Info,
        &format!("Fetching air quality for lat={}, lon={}", params.lat, params.lon),
    );

    let resp = api_get(&url)?;
    let data: serde_json::Value =
        serde_json::from_str(&resp).map_err(|e| format!("Failed to parse response: {e}"))?;

    let current = &data["current"];
    let aqi = current["european_aqi"].as_u64().unwrap_or(0) as u32;

    let output = AirQualityOutput {
        lat: params.lat,
        lon: params.lon,
        european_aqi: aqi,
        aqi_label: european_aqi_label(aqi),
        pm2_5: current["pm2_5"].as_f64().unwrap_or(0.0),
        pm10: current["pm10"].as_f64().unwrap_or(0.0),
        nitrogen_dioxide: current["nitrogen_dioxide"].as_f64().unwrap_or(0.0),
        ozone: current["ozone"].as_f64().unwrap_or(0.0),
        carbon_monoxide: current["carbon_monoxide"].as_f64().unwrap_or(0.0),
    };

    serde_json::to_string(&output).map_err(|e| format!("Failed to serialize output: {e}"))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn api_get(url: &str) -> Result<String, String> {
    let headers = serde_json::json!({
        "Accept": "application/json",
        "User-Agent": "IronClaw-Weather-Tool/0.1"
    })
    .to_string();

    let resp = near::agent::host::http_request("GET", url, &headers, None, None)
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    if resp.status < 200 || resp.status >= 300 {
        let body = String::from_utf8_lossy(&resp.body);
        return Err(format!("API error (HTTP {}): {}", resp.status, body));
    }

    String::from_utf8(resp.body).map_err(|e| format!("Invalid UTF-8 response: {e}"))
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            _ => {
                out.push('%');
                out.push(char::from(b"0123456789ABCDEF"[(b >> 4) as usize]));
                out.push(char::from(b"0123456789ABCDEF"[(b & 0xf) as usize]));
            }
        }
    }
    out
}

/// Map WMO weather interpretation codes to human-readable descriptions.
fn wmo_description(code: u32) -> String {
    match code {
        0 => "Clear sky",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 => "Fog",
        48 => "Depositing rime fog",
        51 => "Light drizzle",
        53 => "Moderate drizzle",
        55 => "Dense drizzle",
        61 => "Slight rain",
        63 => "Moderate rain",
        65 => "Heavy rain",
        71 => "Slight snow",
        73 => "Moderate snow",
        75 => "Heavy snow",
        77 => "Snow grains",
        80 => "Slight rain showers",
        81 => "Moderate rain showers",
        82 => "Violent rain showers",
        85 => "Slight snow showers",
        86 => "Heavy snow showers",
        95 => "Thunderstorm",
        96 => "Thunderstorm with slight hail",
        99 => "Thunderstorm with heavy hail",
        _ => "Unknown",
    }
    .to_string()
}

/// European AQI scale (0–500+).
fn european_aqi_label(aqi: u32) -> String {
    match aqi {
        0..=20 => "Good",
        21..=40 => "Fair",
        41..=60 => "Moderate",
        61..=80 => "Poor",
        81..=100 => "Very Poor",
        _ => "Extremely Poor",
    }
    .to_string()
}

// ---------------------------------------------------------------------------
// JSON Schema
// ---------------------------------------------------------------------------

const SCHEMA: &str = r#"{
    "oneOf": [
        {
            "type": "object",
            "description": "Get current weather conditions for a city",
            "properties": {
                "action": { "type": "string", "const": "get_current" },
                "city": { "type": "string", "description": "City name (e.g. 'London', 'New York', 'Tokyo')" },
                "country_code": { "type": "string", "description": "Optional ISO 3166-1 alpha-2 country code (e.g. 'US', 'GB') to disambiguate city names" },
                "units": { "type": "string", "enum": ["metric", "imperial"], "description": "Units: 'metric' (Celsius, default) or 'imperial' (Fahrenheit)" }
            },
            "required": ["action", "city"],
            "additionalProperties": false
        },
        {
            "type": "object",
            "description": "Get a 5-day daily weather forecast for a city",
            "properties": {
                "action": { "type": "string", "const": "get_forecast" },
                "city": { "type": "string", "description": "City name (e.g. 'London', 'New York', 'Tokyo')" },
                "country_code": { "type": "string", "description": "Optional ISO 3166-1 alpha-2 country code (e.g. 'US', 'GB') to disambiguate city names" },
                "units": { "type": "string", "enum": ["metric", "imperial"], "description": "Units: 'metric' (Celsius, default) or 'imperial' (Fahrenheit)" }
            },
            "required": ["action", "city"],
            "additionalProperties": false
        },
        {
            "type": "object",
            "description": "Get air quality / pollution data for a location by coordinates",
            "properties": {
                "action": { "type": "string", "const": "get_air_quality" },
                "lat": { "type": "number", "description": "Latitude coordinate (-90 to 90)" },
                "lon": { "type": "number", "description": "Longitude coordinate (-180 to 180)" }
            },
            "required": ["action", "lat", "lon"],
            "additionalProperties": false
        }
    ]
}"#;

export!(WeatherTool);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_encode() {
        assert_eq!(url_encode("hello world"), "hello%20world");
        assert_eq!(url_encode("New York"), "New%20York");
        assert_eq!(url_encode("simple"), "simple");
    }

    #[test]
    fn test_wmo_description_known() {
        assert_eq!(wmo_description(0), "Clear sky");
        assert_eq!(wmo_description(3), "Overcast");
        assert_eq!(wmo_description(61), "Slight rain");
        assert_eq!(wmo_description(95), "Thunderstorm");
    }

    #[test]
    fn test_wmo_description_unknown() {
        assert_eq!(wmo_description(42), "Unknown");
    }

    #[test]
    fn test_european_aqi_label() {
        assert_eq!(european_aqi_label(10), "Good");
        assert_eq!(european_aqi_label(30), "Fair");
        assert_eq!(european_aqi_label(50), "Moderate");
        assert_eq!(european_aqi_label(70), "Poor");
        assert_eq!(european_aqi_label(90), "Very Poor");
        assert_eq!(european_aqi_label(150), "Extremely Poor");
    }

    #[test]
    fn test_parse_get_current() {
        let params = r#"{"action": "get_current", "city": "London"}"#;
        let action: Action = serde_json::from_str(params).unwrap();
        match action {
            Action::GetCurrent(p) => assert_eq!(p.city, "London"),
            _ => panic!("Expected GetCurrent"),
        }
    }

    #[test]
    fn test_parse_get_air_quality() {
        let params = r#"{"action": "get_air_quality", "lat": -34.6, "lon": -58.4}"#;
        let action: Action = serde_json::from_str(params).unwrap();
        match action {
            Action::GetAirQuality(p) => {
                assert!((p.lat - (-34.6)).abs() < f64::EPSILON);
                assert!((p.lon - (-58.4)).abs() < f64::EPSILON);
            }
            _ => panic!("Expected GetAirQuality"),
        }
    }
}
