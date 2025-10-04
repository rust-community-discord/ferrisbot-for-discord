use std::sync::Arc;

use anyhow::{Error, Result, anyhow};
use poise::serenity_prelude as serenity;
use shuttle_runtime::SecretStore;
use tokio::sync::RwLock;

use crate::commands;

#[derive(Debug)]
pub struct Data {
	pub highlights: RwLock<commands::highlight::RegexHolder>,
	pub database: sqlx::PgPool,
	pub discord_guild_id: serenity::GuildId,
	pub application_id: serenity::UserId,
	pub mod_role_id: serenity::RoleId,
	pub rustacean_role_id: serenity::RoleId,
	pub modmail_channel_id: serenity::ChannelId,
	pub modlog_channel_id: serenity::ChannelId,
	pub modmail_message: Arc<tokio::sync::RwLock<Option<serenity::Message>>>,
	pub bot_start_time: std::time::Instant,
	pub http: reqwest::Client,
	pub godbolt_metadata: std::sync::Mutex<commands::godbolt::GodboltMetadata>,
}

impl Data {
	pub async fn new(secret_store: &SecretStore, database: sqlx::PgPool) -> Result<Self> {
		Ok(Self {
			highlights: RwLock::new(commands::highlight::RegexHolder::new(&database).await),
			database,
			discord_guild_id: secret_store
				.get("DISCORD_GUILD")
				.ok_or(anyhow!(
					"Failed to get 'DISCORD_GUILD' from the secret store"
				))?
				.parse::<u64>()?
				.into(),
			application_id: secret_store
				.get("APPLICATION_ID")
				.ok_or(anyhow!(
					"Failed to get 'APPLICATION_ID' from the secret store"
				))?
				.parse::<u64>()?
				.into(),
			mod_role_id: secret_store
				.get("MOD_ROLE_ID")
				.ok_or(anyhow!("Failed to get 'MOD_ROLE_ID' from the secret store"))?
				.parse::<u64>()?
				.into(),
			rustacean_role_id: secret_store
				.get("RUSTACEAN_ROLE_ID")
				.ok_or(anyhow!(
					"Failed to get 'RUSTACEAN_ROLE_ID' from the secret store"
				))?
				.parse::<u64>()?
				.into(),
			modmail_channel_id: secret_store
				.get("MODMAIL_CHANNEL_ID")
				.ok_or(anyhow!(
					"Failed to get 'MODMAIL_CHANNEL_ID' from the secret store"
				))?
				.parse::<u64>()?
				.into(),
			modlog_channel_id: secret_store
				.get("MODLOG_CHANNEL_ID")
				.ok_or(anyhow!(
					"Failed to get 'MODLOG_CHANNEL_ID' from the secret store"
				))?
				.parse::<u64>()?
				.into(),
			modmail_message: Arc::default(),
			bot_start_time: std::time::Instant::now(),
			http: reqwest::Client::new(),
			godbolt_metadata: std::sync::Mutex::new(commands::godbolt::GodboltMetadata::default()),
		})
	}
}

pub type Context<'a> = poise::Context<'a, Data, Error>;

// const EMBED_COLOR: (u8, u8, u8) = (0xf7, 0x4c, 0x00);
pub const EMBED_COLOR: (u8, u8, u8) = (0xb7, 0x47, 0x00); // slightly less saturated
