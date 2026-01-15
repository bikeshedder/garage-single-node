use std::env;
use std::str::FromStr;

use serde::Deserialize;
use strum::EnumString;
use thiserror::Error;

use crate::random::random_base64;

pub struct Config {
    pub admin_token: String,
    pub metrics_token: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub buckets: Vec<BucketConfig>,
}

pub struct BucketConfig {
    pub name: String,
    pub policy: BucketPolicy,
}

#[derive(Debug, Copy, Clone, Deserialize, EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum BucketPolicy {
    Private,
    Public,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing environment variable {name}")]
    MissingVar { name: &'static str },
    #[error("environment variable {name} is empty")]
    EmptyVar { name: &'static str },
    #[error("environment variable {name} is not valid unicode")]
    InvalidUnicode { name: &'static str },
    #[error("invalid bucket entry {entry}")]
    InvalidBucketEntry { entry: String },
    #[error("invalid bucket name {name}")]
    InvalidBucketName { name: String },
    #[error("invalid bucket policy {value} for bucket {bucket}")]
    InvalidBucketPolicy { bucket: String, value: String },
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let garage_admin_token = read_env_default("GARAGE_ADMIN_TOKEN", || random_base64(32))?;
        let garage_metrics_token = read_env_default("GARAGE_METRICS_TOKEN", || random_base64(32))?;
        let garage_access_key_id = read_env("GARAGE_ACCESS_KEY_ID")?;
        let garage_secret_access_key = read_env("GARAGE_SECRET_ACCESS_KEY")?;
        let garage_buckets_raw = read_env("GARAGE_BUCKETS")?;

        let mut garage_buckets = Vec::new();
        for raw_entry in garage_buckets_raw.split(',') {
            let entry = raw_entry.trim();
            if entry.is_empty() {
                return Err(ConfigError::InvalidBucketEntry {
                    entry: raw_entry.to_string(),
                });
            }
            let mut parts = entry.splitn(2, ':');
            let name = parts.next().unwrap().trim();
            if name.is_empty() || !is_valid_bucket_name(name) {
                return Err(ConfigError::InvalidBucketName {
                    name: name.to_string(),
                });
            }

            let policy = match parts.next() {
                Some(value) => {
                    BucketPolicy::from_str(value).map_err(|_| ConfigError::InvalidBucketPolicy {
                        bucket: name.to_string(),
                        value: value.to_string(),
                    })?
                }
                None => BucketPolicy::Private,
            };

            garage_buckets.push(BucketConfig {
                name: name.to_string(),
                policy,
            });
        }

        Ok(Self {
            admin_token: garage_admin_token,
            metrics_token: garage_metrics_token,
            access_key_id: garage_access_key_id,
            secret_access_key: garage_secret_access_key,
            buckets: garage_buckets,
        })
    }
}

fn read_env(name: &'static str) -> Result<String, ConfigError> {
    match env::var(name) {
        Ok(value) => {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() {
                Err(ConfigError::EmptyVar { name })
            } else {
                Ok(trimmed)
            }
        }
        Err(env::VarError::NotPresent) => Err(ConfigError::MissingVar { name }),
        Err(env::VarError::NotUnicode(_)) => Err(ConfigError::InvalidUnicode { name }),
    }
}

fn read_env_default(name: &'static str, default: fn() -> String) -> Result<String, ConfigError> {
    match read_env(name) {
        Err(ConfigError::MissingVar { .. }) => Ok(default()),
        Err(ConfigError::EmptyVar { .. }) => Ok(default()),
        x => x,
    }
}

fn is_valid_bucket_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => (),
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric())
}
