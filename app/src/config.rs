use std::{env, fs};

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};

const NULL_YAML: Value = Value::Null;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub trusted_proxy_hops: usize,
    #[serde(default)]
    pub bg_image_url: Option<String>,
    #[serde(default)]
    pub bg_image_blur: bool,
    #[serde(default)]
    pub services: Vec<Service>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Service {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AuthConfig {
    pub secret_key: String,
    pub session_days: i64,
    pub secure_cookie: bool,
    pub password_enabled: bool,
    pub username: Option<String>,
    pub password_hash: Option<String>,
    pub oidc_enabled: bool,
    pub oidc_provider_name: String,
    pub oidc_discovery_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret: Option<String>,
    pub oidc_scope: Vec<String>,
    pub oidc_allowed_emails: Vec<String>,
    pub login_max_attempts: usize,
    pub login_window_seconds: u64,
    pub login_lockout_seconds: u64,
    pub state_cleanup_interval_seconds: u64,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub app: AppConfig,
    pub auth: AuthConfig,
}

fn default_title() -> String {
    "Starry Cloud".to_owned()
}

fn read_yaml(path: &str) -> Result<Value, String> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_yaml::from_str(&contents)
            .map_err(|error| format!("could not parse {path}: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(Value::Mapping(Mapping::new()))
        }
        Err(error) => Err(format!("could not read {path}: {error}")),
    }
}

fn mapping_value<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.as_mapping()?.get(&Value::String(key.to_owned()))
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_owned)
}

fn bool_value(value: Option<&Value>, default: bool) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => value.eq_ignore_ascii_case("true"),
        _ => default,
    }
}

fn integer_value(value: Option<&Value>, default: i64) -> i64 {
    match value {
        Some(Value::Number(number)) => number.as_i64().unwrap_or(default),
        Some(Value::String(value)) => value.parse().unwrap_or(default),
        _ => default,
    }
}

fn string_list(value: Option<&Value>) -> Result<Vec<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Sequence(items)) => Ok(items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect()),
        _ => Err("expected a list of strings in auth configuration".to_owned()),
    }
}

fn read_app_config(value: Value) -> Result<AppConfig, String> {
    let app: AppConfig =
        serde_yaml::from_value(value).map_err(|error| format!("invalid config.yml: {error}"))?;
    Ok(app)
}

fn read_auth_config(auth_file: Value, app_file: &Value) -> Result<AuthConfig, String> {
    let auth_root = mapping_value(&auth_file, "auth").unwrap_or(&auth_file);
    let oidc = mapping_value(auth_root, "oidc").unwrap_or(&NULL_YAML);
    let login_protection = mapping_value(app_file, "login_protection").unwrap_or(&NULL_YAML);

    let secret_key = string_value(mapping_value(auth_root, "secret_key"))
        .or_else(|| env::var("SECRET_KEY").ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "authentication requires auth.secret_key in auth.yml or SECRET_KEY".to_owned()
        })?;

    let mut oidc_scope = string_value(mapping_value(oidc, "scope"))
        .unwrap_or_else(|| "openid email profile".to_owned())
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if !oidc_scope.iter().any(|scope| scope == "openid") {
        oidc_scope.insert(0, "openid".to_owned());
    }

    let allowed_emails = string_list(mapping_value(oidc, "allowed_emails"))?
        .into_iter()
        .map(|email| email.to_lowercase())
        .collect();
    let password_enabled = bool_value(mapping_value(auth_root, "password_enabled"), true);
    let oidc_enabled = bool_value(mapping_value(oidc, "enabled"), false);
    let username = string_value(mapping_value(auth_root, "username"));
    let password_hash = string_value(mapping_value(auth_root, "password_hash"));
    let oidc_discovery_url = string_value(mapping_value(oidc, "discovery_url"));
    let oidc_client_id = string_value(mapping_value(oidc, "client_id"));
    let oidc_client_secret = string_value(mapping_value(oidc, "client_secret"));

    if password_enabled && (username.is_none() || password_hash.is_none()) {
        return Err(
            "password authentication requires auth.username and auth.password_hash".to_owned(),
        );
    }
    if oidc_enabled
        && (oidc_discovery_url.is_none()
            || oidc_client_id.is_none()
            || oidc_client_secret.is_none())
    {
        return Err(
            "OIDC requires auth.oidc.discovery_url, client_id, and client_secret".to_owned(),
        );
    }
    if !password_enabled && !oidc_enabled {
        return Err("at least one authentication method must be enabled".to_owned());
    }

    let session_days = integer_value(mapping_value(auth_root, "session_days"), 30).max(1);
    let provider_name = string_value(mapping_value(oidc, "provider_name"))
        .unwrap_or_else(|| "OIDC".to_owned())
        .trim()
        .to_owned();

    Ok(AuthConfig {
        secret_key,
        session_days,
        secure_cookie: bool_value(mapping_value(auth_root, "secure_cookie"), false),
        password_enabled,
        username,
        password_hash,
        oidc_enabled,
        oidc_provider_name: if provider_name.is_empty() {
            "OIDC".to_owned()
        } else {
            provider_name
        },
        oidc_discovery_url,
        oidc_client_id,
        oidc_client_secret,
        oidc_scope,
        oidc_allowed_emails: allowed_emails,
        login_max_attempts: integer_value(mapping_value(login_protection, "login_max_attempts"), 5)
            .max(1) as usize,
        login_window_seconds: integer_value(
            mapping_value(login_protection, "login_window_seconds"),
            300,
        )
        .max(10) as u64,
        login_lockout_seconds: integer_value(
            mapping_value(login_protection, "login_lockout_seconds"),
            900,
        )
        .max(30) as u64,
        state_cleanup_interval_seconds: integer_value(
            mapping_value(login_protection, "state_cleanup_interval_seconds"),
            300,
        )
        .max(30) as u64,
    })
}

pub fn load_settings() -> Result<Settings, String> {
    let app_file = read_yaml("config.yml")?;
    let app_config = read_app_config(app_file.clone())?;
    let auth_config = read_auth_config(read_yaml("auth.yml")?, &app_file)?;
    Ok(Settings {
        app: app_config,
        auth: auth_config,
    })
}
