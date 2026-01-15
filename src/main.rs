use std::collections::HashMap;
use std::fs::write;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, exit};
use std::thread;
use std::time::{Duration, Instant};

use crate::admin_api::Client;
use crate::admin_api::types::{
    AllowBucketKeyRequest, ApiBucketKeyPerm, ApplyClusterLayoutRequest, BucketKeyPermChangeRequest,
    CreateBucketRequest, GetClusterStatusResponse, ImportKeyRequest, NodeRoleChange,
    UpdateBucketRequestBody, UpdateBucketWebsiteAccess, UpdateClusterLayoutRequest,
};
use crate::config::{BucketPolicy, Config};
use crate::random::random_hex;
use anyhow::{Context, Result};
use reqwest::header;
use reqwest::header::HeaderMap;
use thiserror::Error;
use tokio::process::{Child, Command};
use toml_edit::{DocumentMut, value};
use tracing::{error, info, warn};

pub mod admin_api;
pub mod config;
pub mod random;

const GARAGE_CONFIG_PATH: &str = "/etc/garage.toml";
const GARAGE_ADMIN_URL: &str = "http://127.0.0.1:3903";
const GARAGE_START_TIMEOUT: Duration = Duration::from_secs(20);
const GARAGE_START_POLL_INTERVAL: Duration = Duration::from_millis(100);
const GARAGE_START_LOG_INTERVAL: Duration = Duration::from_secs(1);

pub struct Garage {
    pub process: Child,
    pub config_path: PathBuf,
    pub api: Client,
    pub node_id: NodeId,
}

pub struct NodeId(String);

#[derive(Debug, Error)]
pub enum StartError {
    #[error("failed to spawn garage process")]
    Spawn(#[source] std::io::Error),
    #[error("garage exited before becoming available with status {0}")]
    Exited(ExitStatus),
    #[error("timed out waiting for garage to become available after {timeout:?}")]
    Timeout { timeout: Duration },
    #[error("failed to check garage availability")]
    AvailabilityCheck(#[source] std::io::Error),
    #[error("invalid garage admin address {addr}")]
    InvalidAdminAddr {
        addr: String,
        #[source]
        source: std::net::AddrParseError,
    },
    #[error("unexpected number of nodes in status: {0}")]
    UnexpectedNumberOfNodes(usize),
    #[error("invalid garage cluster status {0:?}")]
    InvalidClusterStatus(GetClusterStatusResponse),
}

pub fn delete_keys() -> Result<()> {
    let db_path = Path::new("/var/lib/garage/meta/db.sqlite");
    if db_path
        .try_exists()
        .context("Could not check existance of DB file")?
    {
        info!("Deleting all access keys...");
        let conn = rusqlite::Connection::open("/var/lib/garage/meta/db.sqlite").unwrap();
        let count = conn
            .execute("DELETE FROM tree_key_COLON_table;", [])
            .context("Could not delete keys in DB")?;
        info!("All access keys removed: {}", count);
    } else {
        info!("db.sqlite does not exist. Skipping key deletion.")
    }
    Ok(())
}

pub fn create_config(config: &Config) -> Result<()> {
    let mut doc = include_str!("garage.toml")
        .parse::<DocumentMut>()
        .expect("Bundled garage.toml is invalid");
    doc["rpc_secret"] = value(random_hex(32));
    doc["admin"]["admin_token"] = value(config.admin_token.clone());
    doc["admin"]["metrics_token"] = value(config.metrics_token.clone());
    write(GARAGE_CONFIG_PATH, doc.to_string())?;
    Ok(())
}

async fn wait_for_garage(child: &mut Child, admin_api: &Client) -> Result<NodeId, StartError> {
    let start = Instant::now();
    let mut next_log = GARAGE_START_LOG_INTERVAL;
    loop {
        if let Some(status) = child.try_wait().map_err(StartError::AvailabilityCheck)? {
            error!("Garage exited after {:.1}s", start.elapsed().as_secs_f64());
            return Err(StartError::Exited(status));
        }
        match admin_api.get_cluster_status().await {
            Ok(status) => {
                if status.nodes.len() != 1 {
                    return Err(StartError::UnexpectedNumberOfNodes(status.nodes.len()));
                }
                if !status.nodes[0].is_up {
                    if start.elapsed() > next_log {
                        next_log += GARAGE_START_LOG_INTERVAL;
                        info!(
                            "Waiting for garage... ({:.1}s)",
                            start.elapsed().as_secs_f64()
                        );
                    }
                } else {
                    info!("Garage ready after {:.1}s", start.elapsed().as_secs_f64());
                    return Ok(NodeId(status.nodes[0].id.clone()));
                }
            }
            Err(_) => {
                if start.elapsed() > next_log {
                    next_log += GARAGE_START_LOG_INTERVAL;
                    info!(
                        "Waiting for garage... ({:.1}s)",
                        start.elapsed().as_secs_f64()
                    );
                }
            }
        };
        if start.elapsed() >= GARAGE_START_TIMEOUT {
            error!(
                "Garage not ready after {:.1}s",
                start.elapsed().as_secs_f64()
            );
            return Err(StartError::Timeout {
                timeout: GARAGE_START_TIMEOUT,
            });
        }
        thread::sleep(GARAGE_START_POLL_INTERVAL);
    }
}

pub async fn run_garage(config: &Config) -> Result<Garage, StartError> {
    info!("Starting garage...");
    let config_path = PathBuf::from(GARAGE_CONFIG_PATH);
    let mut child = Command::new("/garage")
        .arg("-c")
        .arg(&config_path)
        .arg("server")
        .spawn()
        .map_err(StartError::Spawn)?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {}", config.admin_token).parse().unwrap(),
    );
    let client = admin_api::Client::new_with_client(
        GARAGE_ADMIN_URL,
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(1))
            .default_headers(headers)
            .build()
            .unwrap(),
    );
    let node_id = wait_for_garage(&mut child, &client).await?;
    Ok(Garage {
        process: child,
        config_path,
        api: client,
        node_id,
    })
}

async fn ensure_layout(garage: &Garage) -> Result<(), progenitor_client::Error> {
    let layout = garage.api.get_cluster_layout().await?;
    if layout.version > 0 {
        info!("Layout version > 0, skipping initialization");
        return Ok(());
    }
    info!("No layout found. Updating cluster...");
    let layout = garage
        .api
        .update_cluster_layout(&UpdateClusterLayoutRequest {
            parameters: None,
            roles: vec![NodeRoleChange::Variant1 {
                capacity: Some(i64::MAX),
                tags: vec![],
                zone: "dc1".into(),
                id: garage.node_id.0.clone(),
            }],
        })
        .await?;
    info!("Layout updated. Applying layout...");
    garage
        .api
        .apply_cluster_layout(&ApplyClusterLayoutRequest {
            version: layout.version + 1,
        })
        .await?;
    info!("Layout applied.");
    Ok(())
}

async fn ensure_key(garage: &Garage, config: &Config) -> Result<(), progenitor_client::Error> {
    garage
        .api
        .import_key(&ImportKeyRequest {
            name: None,
            access_key_id: config.access_key_id.clone(),
            secret_access_key: config.secret_access_key.clone(),
        })
        .await?;
    Ok(())
}

async fn ensure_buckets(garage: &Garage, config: &Config) -> Result<(), progenitor_client::Error> {
    let mut garage_bucket_map = HashMap::<String, String>::new();
    for bucket in &garage.api.list_buckets().await?.0 {
        if bucket.global_aliases.is_empty() {
            warn!("Ignoring bucket without a global alias: {:?}", bucket);
            continue;
        }
        if bucket.global_aliases.len() > 1 {
            warn!(
                "Ignoring bucket with more than one global alias: {:?}",
                bucket
            );
            continue;
        }
        garage_bucket_map.insert(bucket.global_aliases[0].clone(), bucket.id.clone());
    }
    for bucket_config in &config.buckets {
        let bucket_id = match garage_bucket_map.get(&bucket_config.name) {
            None => {
                info!("Creating bucket {:?}...", bucket_config.name);
                let bucket = garage
                    .api
                    .create_bucket(&CreateBucketRequest {
                        global_alias: Some(bucket_config.name.clone()),
                        local_alias: None,
                    })
                    .await?;
                info!("Bucket {:?} created", bucket_config.name);
                bucket.id.clone()
            }
            Some(bucket_id) => {
                info!(
                    "Bucket {:?} found with id {:?}",
                    bucket_config.name, bucket_id
                );
                bucket_id.clone()
            }
        };
        info!("Updating bucket {:?}", bucket_config.name);
        garage
            .api
            .update_bucket(
                &bucket_id,
                &UpdateBucketRequestBody {
                    quotas: None,
                    website_access: Some(match bucket_config.policy {
                        BucketPolicy::Private => UpdateBucketWebsiteAccess {
                            enabled: false,
                            error_document: None,
                            index_document: None,
                        },
                        BucketPolicy::Public => UpdateBucketWebsiteAccess {
                            enabled: true,
                            error_document: None,
                            index_document: Some("index.html".into()),
                        },
                    }),
                },
            )
            .await?;
        info!("Granting access to bucket {:?}", bucket_config.name);
        garage
            .api
            .allow_bucket_key(&AllowBucketKeyRequest(BucketKeyPermChangeRequest {
                access_key_id: config.access_key_id.clone(),
                bucket_id: bucket_id,
                permissions: ApiBucketKeyPerm {
                    owner: Some(true),
                    read: Some(true),
                    write: Some(true),
                },
            }))
            .await?;
    }
    Ok(())
}

#[tokio::main]
pub async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let config = Config::from_env().context("Could not load config")?;
    delete_keys()?;
    create_config(&config)?;
    let mut garage = run_garage(&config).await?;
    ensure_layout(&garage).await?;
    ensure_key(&garage, &config).await?;
    ensure_buckets(&garage, &config).await?;
    info!("Bootstrapping complete.");
    let exit_status = garage.process.wait().await?;
    if !exit_status.success() {
        exit(exit_status.code().unwrap_or(1));
    }
    Ok(())
}
