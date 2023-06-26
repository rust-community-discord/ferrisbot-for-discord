use anyhow::{Context as _, Error};
use poise::serenity_prelude as serenity;
use shuttle_secrets::SecretStore;

use crate::commands;

#[derive(Debug)]
pub struct Data {
	pub discord_guild: serenity::GuildId,
	pub application_id: serenity::UserId,
	pub mod_role_id: serenity::RoleId,
	pub rustacean_role: serenity::RoleId,
	pub bot_start_time: std::time::Instant,
	pub http: reqwest::Client,
	pub godbolt_metadata: std::sync::Mutex<commands::godbolt::GodboltMetadata>,
}

impl Data {
	pub fn new(secret_store: &SecretStore) -> Self {
		Self {
			discord_guild: secret_store
				.get("DISCORD_GUILD")
				.context("Failed to get 'DISCORD_GUILD' from the secret store")
				.expect("Failed to get 'DISCORD_GUILD' from the secret store")
				.parse::<u64>()
				.context("Failed to parse 'DISCORD_GUILD' as u64")
				.expect("Failed to parse 'DISCORD_GUILD' as u64")
				.into(),
			application_id: secret_store
				.get("APPLICATION_ID")
				.context("Failed to get 'APPLICATION_ID' from the secret store")
				.expect("Failed to get 'APPLICATION_ID' from the secret store")
				.parse::<u64>()
				.context("Failed to parse 'APPLICATION_ID' as u64")
				.expect("Failed to parse 'APPLICATION_ID' as u64")
				.into(),
			mod_role_id: secret_store
				.get("MOD_ROLE_ID")
				.context("Failed to get 'MOD_ROLE_ID' from the secret store")
				.expect("Failed to get 'MOD_ROLE_ID' from the secret store")
				.parse::<u64>()
				.context("Failed to parse 'MOD_ROLE_ID' as u64")
				.expect("Failed to parse 'MOD_ROLE_ID' as u64")
				.into(),
			rustacean_role: secret_store
				.get("RUSTACEAN_ROLE_ID")
				.context("Failed to get 'RUSTACEAN_ROLE_ID' from the secret store")
				.expect("Failed to get 'RUSTACEAN_ROLE_ID' from the secret store")
				.parse::<u64>()
				.context("Failed to parse 'RUSTACEAN_ROLE_ID' as u64")
				.expect("Failed to parse 'RUSTACEAN_ROLE_ID' as u64")
				.into(),
			bot_start_time: std::time::Instant::now(),
			http: reqwest::Client::new(),
			godbolt_metadata: std::sync::Mutex::new(commands::godbolt::GodboltMetadata::default()),
		}
	}
}

pub type Context<'a> = poise::Context<'a, Data, Error>;

// const EMBED_COLOR: (u8, u8, u8) = (0xf7, 0x4c, 0x00);
pub const EMBED_COLOR: (u8, u8, u8) = (0xb7, 0x47, 0x00); // slightly less saturated
