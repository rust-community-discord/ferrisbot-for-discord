use std::{collections::HashMap, fs, panic, path::PathBuf, str::FromStr, sync::LazyLock};

use ferrisbot_for_discord::SecretStore;
use figment::{
	Figment,
	providers::{Env, Format as _, Serialized, Toml},
};
use serde::Deserialize;
use serde_json::json;
use snafu::{Report, ResultExt as _, Snafu};
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use tokio::runtime::Runtime;
use tracing::{debug, error, info, level_filters::LevelFilter, warn};
use tracing_appender::rolling::{self, RollingFileAppender, Rotation};
use tracing_log::AsLog as _;
use tracing_subscriber::{EnvFilter, Registry, fmt, layer::SubscriberExt as _};

#[derive(Deserialize, Debug)]
struct LogConfig {
	filter: String,
	rotation: LogRotation,
	dir: PathBuf,
	prefix: Option<String>,
	suffix: Option<String>,
	max_files: u32,
}

#[derive(Deserialize, Debug)]
struct DatabaseConfig {
	url: String,
}

#[derive(Deserialize, Debug)]
struct Config {
	log: LogConfig,
	database: DatabaseConfig,
	secrets: HashMap<String, String>,
}

/// All of these values can be overridden in the ferris.toml file.
static DEFAULT_CONFIG: LazyLock<serde_json::Value> = LazyLock::new(|| {
	json!({
		"log": {
			"filter": "",
			"rotation": "daily",
			"dir": "logs",
			"prefix": "ferris",
			"suffix": "log",
			"max_files": 10,
		},
		"database": {
			"url": "sqlite://database/ferris.sqlite3"
		},
		"secrets": {}
	})
});

#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
enum LogRotation {
	Minutely,
	Hourly,
	Daily,
	Weekly,
	Never,
}

fn app(config: &Config) -> Result<(), AppError> {
	let rt = Runtime::new().context(TokioRuntimeSnafu)?;
	rt.block_on(async {
		let disable_database = std::env::var("FERRIS_DISABLE_DATABASE")
			.map(|value| {
				matches!(
					value.to_ascii_lowercase().as_str(),
					"1" | "true" | "yes" | "on"
				)
			})
			.unwrap_or(false);

		let pool = if disable_database {
			warn!("database disabled via FERRIS_DISABLE_DATABASE");
			None
		} else {
			info!("connecting to SQLite database...");

			let opts = SqliteConnectOptions::from_str(&config.database.url)
				.context(DatabaseSnafu {
					url: config.database.url.to_owned(),
				})?
				.create_if_missing(true);

			let pool = SqlitePool::connect_with(opts)
				.await
				.context(DatabaseSnafu {
					url: config.database.url.to_owned(),
				})?;

			sqlx::migrate!()
				.run(&pool)
				.await
				.expect("Failed to run migrations");

			Some(pool)
		};

		info!("initializing serenity...");

		let secret_store = SecretStore(
			config
				.secrets
				.iter()
				.map(|(k, v)| (k.clone(), v.clone()))
				.collect(),
		);

		let mut client = ferrisbot_for_discord::serenity(secret_store, pool)
			.await
			.context(SerenityInitSnafu)?;

		info!("starting serenity...");

		client.0.start_autosharded().await.context(SerenitySnafu)?;

		info!("serenity stopped");

		Ok(())
	})
}

impl LogConfig {
	fn build_appender(&self) -> Result<RollingFileAppender, AppError> {
		let mut appender = RollingFileAppender::builder()
			.rotation(self.rotation.into())
			.max_log_files(self.max_files as _);

		if let Some(prefix) = &self.prefix {
			appender = appender.filename_prefix(prefix);
		}

		if let Some(suffix) = &self.suffix {
			appender = appender.filename_suffix(suffix);
		}

		appender.build(&self.dir).context(AppenderSnafu)
	}
}

impl From<LogRotation> for Rotation {
	fn from(value: LogRotation) -> Self {
		match value {
			LogRotation::Minutely => Self::MINUTELY,
			LogRotation::Hourly => Self::HOURLY,
			LogRotation::Daily => Self::DAILY,
			LogRotation::Weekly => Self::WEEKLY,
			LogRotation::Never => Self::NEVER,
		}
	}
}

fn main() {
	// Here we setup telemetry (including panic logging), load the config, and handle app panics.

	// Start basic telemetry to the terminal.
	let basic_subscriber = tracing_subscriber::fmt();
	let guard = tracing::subscriber::set_default(basic_subscriber.finish());

	std::panic::set_hook(Box::new(tracing_panic::panic_hook));

	// Manually setup the log compatibility to have max debug level.
	tracing_log::LogTracer::builder()
		.with_max_level(tracing::level_filters::LevelFilter::DEBUG.as_log())
		.init()
		.expect("should set default log handler");

	info!("ferris starting...");

	// Load config.
	let config_file = "ferris.toml";
	let config_secrets_file = "ferris.secrets.toml";
	check_for_config_files(config_file, config_secrets_file);
	let config: Config = match Report::capture_into_result(|| {
		Figment::new()
			.merge(Serialized::defaults(&*DEFAULT_CONFIG))
			.merge(Toml::file(config_file))
			.merge(Toml::file(config_secrets_file))
			.merge(Env::prefixed("FERRIS_").split("_"))
			.extract()
			.context(ConfigSnafu)
	}) {
		Ok(config) => config,
		Err(err) => {
			error!(%err, "failed to load config");
			return;
		}
	};

	// Start full telemetry.
	let file_appender = match Report::capture_into_result(|| config.log.build_appender()) {
		Ok(appender) => appender,
		Err(err) => {
			error!(%err, "failed to create log file appender");
			return;
		}
	};
	let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
	let subscriber = match Report::capture_into_result(|| {
		Ok::<_, AppError>(
			Registry::default()
				.with(
					fmt::Layer::default()
						.with_ansi(false)
						.with_writer(non_blocking),
				)
				.with(fmt::Layer::default().with_writer(std::io::stderr))
				.with(
					EnvFilter::builder()
						.with_default_directive(LevelFilter::INFO.into())
						.parse(&config.log.filter)
						.context(TelemetryFilterSnafu {
							filter: config.log.filter.to_owned(),
						})?,
				),
		)
	}) {
		Ok(subscriber) => subscriber,
		Err(err) => {
			error!(%err, "failed to create telemetry subscriber");
			return;
		}
	};

	info!("switching to rolling file + stderr logger...");
	tracing::subscriber::set_global_default(subscriber).expect("should set default subscriber");
	drop(guard);

	// Log the loaded config so we can get it if needed.
	debug!(?config);

	// Run the actual application code now that the environment is setup.
	match panic::catch_unwind(|| Report::capture_into_result(|| app(&config))) {
		Ok(Ok(())) => {
			info!("clean shutdown");
		}
		Ok(Err(err)) => {
			error!(%err, "shutdown because of error");
		}
		Err(_err) => {
			// The panic handler should have already captured the panic
			// message so we don't do anything here.
			warn!("shutdown because of panic")
		}
	}
}

fn check_for_config_files(main: &str, secrets: &str) {
	if !fs::exists(main).is_ok_and(|x| x) {
		warn!(file = main, "config file missing");
	}

	if !fs::exists(secrets).is_ok_and(|x| x) {
		warn!(file = secrets, "secrets config file missing");
	}
}

#[derive(Snafu, Debug)]
enum AppError {
	#[snafu(display("problem building log appender"))]
	Appender { source: rolling::InitError },
	#[snafu(display("unable to parse config"))]
	Config {
		#[snafu(source(from(figment::Error, Box::new)))]
		source: Box<figment::Error>, // The figment error is large.
	},
	#[snafu(display("issue creating tokio async runtime"))]
	TokioRuntime { source: std::io::Error },
	#[snafu(display("unable to parse log filter: {filter}"))]
	TelemetryFilter {
		source: tracing_subscriber::filter::ParseError,
		filter: String,
	},
	#[snafu(display("unable to connect to database at: {url}"))]
	Database { source: sqlx::Error, url: String },
	#[snafu(display("failed to initialize serenity client"))]
	SerenityInit { source: anyhow::Error },
	#[snafu(display("serenity client failed"))]
	Serenity {
		#[snafu(source(from(poise::serenity_prelude::Error, Box::new)))]
		source: Box<poise::serenity_prelude::Error>,
	},
}
