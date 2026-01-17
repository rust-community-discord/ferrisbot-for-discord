use std::sync::Arc;

use anyhow::{Error, Result};
use poise::serenity_prelude as serenity;
use tokio::sync::RwLock;

use crate::{SecretStore, commands};

#[derive(Debug)]
pub struct Data {
	pub highlights: RwLock<commands::highlight::RegexHolder>,
	pub database: Option<sqlx::SqlitePool>,
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
	pub async fn new(
		secret_store: &SecretStore,
		database: Option<sqlx::SqlitePool>,
	) -> Result<Self> {
		Ok(Self {
			highlights: RwLock::new(commands::highlight::RegexHolder::new(database.as_ref()).await),
			database,
			discord_guild_id: secret_store.get_discord_id("DISCORD_GUILD")?.into(),
			application_id: secret_store.get_discord_id("APPLICATION_ID")?.into(),
			mod_role_id: secret_store.get_discord_id("MOD_ROLE_ID")?.into(),
			rustacean_role_id: secret_store.get_discord_id("RUSTACEAN_ROLE_ID")?.into(),
			modmail_channel_id: secret_store.get_discord_id("MODMAIL_CHANNEL_ID")?.into(),
			modlog_channel_id: secret_store.get_discord_id("MODLOG_CHANNEL_ID")?.into(),
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
