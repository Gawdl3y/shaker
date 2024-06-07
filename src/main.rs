#![warn(clippy::pedantic)]

use std::{
	net::SocketAddr,
	path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clap::Parser;
use dotenv::dotenv;
use secrecy::Secret;
use tokio::fs;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub mod api;
pub mod db;

/// Configuration for the server
#[derive(Debug, Parser)]
#[command(version)]
pub struct Config {
	/// Path to the SQLite database
	#[allow(clippy::doc_markdown)]
	#[arg(long, short, env("SHAKER_DB"), default_value = "shaker.db")]
	pub db: PathBuf,

	/// Address for the API to listen on
	#[arg(long, short, env("SHAKER_API"), default_value = "127.0.0.1:9001")]
	pub api: SocketAddr,

	/// Token required to make requests
	#[arg(long, short, env("SHAKER_TOKEN"))]
	pub token: Option<Secret<String>>,

	/// Path to a plain-text file to import line-separated usernames of past handshakes from
	#[arg(long, env("SHAKER_IMPORT"))]
	pub import: Option<PathBuf>,
}

impl Config {
	/// Loads configuration from the following sources, in order of precedence:
	/// - CLI arguments
	/// - `.env` file
	/// - Environment variables
	#[tracing::instrument("Loading configuration", level = "info")]
	pub fn load() -> Result<Self> {
		// Parse a .env file (if available) into true environment variables
		dotenv()
			.map(|path| {
				info!(path = %path.display(), "Processed .env file");
			})
			.or_else(|err| {
				if err.not_found() {
					info!("No .env file to load");
					Ok(())
				} else {
					Err(err)
				}
			})?;

		// Run the clap parser
		let cfg = Self::parse();
		info!(config = ?cfg, "Done loading");

		Ok(cfg)
	}
}

#[tokio::main]
async fn main() -> Result<()> {
	// Set up the tracing subscriber
	tracing_subscriber::registry()
		.with(tracing_subscriber::fmt::layer())
		.with(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
			"warn,shaker=info"
				.parse()
				.expect("Unable to parse default EnvFilter string")
		}))
		.init();

	// Load the config
	info!("Starting Shaker server");
	let cfg = Config::load()?;

	// Open the database and run pending migrations
	let db_url = format!(
		"sqlite://{}",
		cfg.db.to_str().context("Unable to convert database path to string")?
	);
	let db = db::Database::open(&db_url).await?;
	db.migrate().await?;

	// Run a legacy import if necessary
	if let Some(path) = &cfg.import {
		import(path, &db).await?;
		return Ok(());
	}

	// Run the API server
	api::run(cfg, db).await?;

	Ok(())
}

/// Imports legacy handshake data from a file
#[tracing::instrument("Importing legacy handshakes", level = "info", skip(db))]
async fn import(path: &Path, db: &db::Database) -> Result<()> {
	let content = fs::read_to_string(path).await?;

	for name in content.lines() {
		match db.create_legacy_user(name).await {
			Ok(user) => {
				if let Err(err) = db.create_legacy_handshake(user.id).await {
					error!(
						"Unable to create legacy handshake for user {name} (ID {}): {err}",
						user.id
					);
				}
			}
			Err(err) => error!("Unable to import legacy user {name}: {err}"),
		}
	}

	Ok(())
}
