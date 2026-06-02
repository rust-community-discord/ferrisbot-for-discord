use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use crate::{require_database, types::Context};
use anyhow::{Error, Result};
use poise::{
	CreateReply,
	serenity_prelude::{ChannelId, CreateEmbed, UserId},
};
use regex::{Regex, RegexBuilder};
use serde::Deserialize;
use sqlx::{Pool, Sqlite};

#[cfg(test)]
mod tests;

static CUSTOM_EMOJI: LazyLock<Regex> =
	LazyLock::new(|| Regex::new(r"<a?:\w+:\d+>").expect("valid custom-emoji regex"));

/// Strips Discord custom-emoji markup so patterns don't match emoji names.
fn sanitize_content(content: &str) -> String {
	CUSTOM_EMOJI.replace_all(content, " ").into_owned()
}

/// Compiles a pattern; case-insensitive unless the inline `(?-i)` flag is set.
fn compile_pattern(pattern: &str) -> Result<Regex, regex::Error> {
	RegexBuilder::new(pattern).case_insensitive(true).build()
}

#[allow(clippy::unused_async)]
#[poise::command(
	prefix_command,
	slash_command,
	subcommands("add", "remove", "list", "mat"),
	subcommand_required
)]
pub async fn highlight(_: Context<'_>) -> Result<(), Error> {
	Ok(())
}

#[poise::command(prefix_command, slash_command)]
/// Adds a highlight. When a highlight is matched, you will receive a DM.
pub async fn add(c: Context<'_>, regex: String) -> Result<()> {
	let db = require_database!(c);

	if let Err(e) = RegexBuilder::new(&regex)
		.size_limit(1 << 10)
		.case_insensitive(true)
		.build()
	{
		c.say(format!("```\n{e}```")).await?;
		return Ok(());
	}

	database::highlight_add(db, c.author().id, &regex).await?;

	RegexHolder::update(c.data()).await;
	c.say("hl added!").await?;

	Ok(())
}

#[poise::command(prefix_command, slash_command)]
/// Removes a highlight by ID.
pub async fn remove(c: Context<'_>, id: i64) -> Result<()> {
	let db = require_database!(c);

	let removed = database::highlight_remove(db, c.author().id, id).await?;

	c.say({
		if removed {
			"hl removed!"
		} else {
			"hl not found."
		}
	})
	.await?;

	RegexHolder::update(c.data()).await;

	Ok(())
}

#[poise::command(prefix_command, slash_command)]
/// Lists your current highlights
pub async fn list(c: Context<'_>) -> Result<()> {
	let db = require_database!(c);
	let highlights = database::highlight_get(db, c.author().id).await?;
	let description = highlights
		.iter()
		.map(|(id, highlight)| format!("**[{id}]** {highlight}"))
		.collect::<Vec<_>>()
		.join("\n");
	poise::send_reply(
		c,
		CreateReply::default().embed(
			CreateEmbed::new()
				.color((0xFC, 0xCA, 0x4C))
				.title("you're tracking these patterns")
				.description(description),
		),
	)
	.await?;
	Ok(())
}

pub async fn matches(author: UserId, haystack: &str, db: &Pool<Sqlite>) -> Result<Vec<String>> {
	let patterns = database::highlight_get(db, author).await?;
	Ok(patterns
		.into_iter()
		.filter_map(|(_id, pattern)| {
			compile_pattern(&pattern)
				.ok()
				.filter(|regex| regex.is_match(haystack))
				.map(|_| pattern)
		})
		.collect())
}

#[poise::command(prefix_command, slash_command, rename = "match")]
/// Tests if your highlights match a given string
pub async fn mat(c: Context<'_>, haystack: String) -> Result<()> {
	let db = require_database!(c);
	let x = matches(c.author().id, &haystack, db).await?;

	poise::send_reply(
		c,
		CreateReply::default().ephemeral(true).embed(
			CreateEmbed::new()
				.color((0xFC, 0xCA, 0x4C))
				.title("these patterns match your haystack")
				.description(itertools::intersperse(x, "\n".to_string()).collect::<String>()),
		),
	)
	.await?;

	Ok(())
}

#[derive(Debug)]
pub struct RegexHolder(Vec<(UserId, Regex)>);
impl RegexHolder {
	pub async fn new(db: Option<&Pool<Sqlite>>) -> Self {
		use tracing::warn;

		let Some(db) = db else {
			return Self(Vec::new());
		};
		let rows = match database::highlight_get_all(db).await {
			Ok(rows) => rows,
			Err(e) => {
				warn!("Failed to load highlights from database: {e}");
				return Self(Vec::new());
			}
		};

		Self::from_patterns(
			rows.into_iter()
				.map(|(member_id, highlight)| (UserId::new(member_id.cast_unsigned()), highlight)),
		)
	}

	/// Compiles `(user, pattern)` pairs into a holder, skipping invalid patterns.
	fn from_patterns(patterns: impl IntoIterator<Item = (UserId, String)>) -> Self {
		use tracing::warn;

		let entries = patterns
			.into_iter()
			.filter_map(|(member_id, highlight)| match compile_pattern(&highlight) {
				Ok(regex) => Some((member_id, regex)),
				Err(e) => {
					warn!("Invalid regex pattern '{highlight}' for member {member_id}: {e}");
					None
				}
			})
			.collect();

		Self(entries)
	}

	async fn update(data: &crate::types::Data) {
		let new = Self::new(data.database.as_ref()).await;
		*data.highlights.write().await = new;
	}

	#[must_use]
	pub fn find(&self, haystack: &str) -> HashMap<UserId, String> {
		let haystack = sanitize_content(haystack);
		self.0
			.iter()
			.filter(|&(_user_id, regex)| regex.is_match(&haystack))
			.map(|(user_id, regex)| (*user_id, regex.as_str().to_string()))
			.collect()
	}
}

#[derive(Deserialize, Debug, Clone, Copy)]
pub struct HighlightConfig {
	#[serde(with = "humantime_serde")]
	cooldown: std::time::Duration,
}

/// Per-`(user, channel)` cooldown expiry instants. Expired entries are swept
/// lazily (at most once per window) by `mark_active`, so `try_notify` needn't prune.
#[derive(Debug)]
pub struct HighlightCooldowns {
	window: Duration,
	expiries: HashMap<(UserId, ChannelId), Instant>,
	next_prune: Instant,
}

impl HighlightCooldowns {
	#[must_use]
	pub fn new(config: HighlightConfig) -> Self {
		Self {
			window: config.cooldown,
			expiries: HashMap::new(),
			next_prune: Instant::now(),
		}
	}

	/// Refreshes the cooldown unconditionally (the user posted, so don't ping them).
	pub fn mark_active(&mut self, user: UserId, channel: ChannelId, now: Instant) {
		self.expiries.insert((user, channel), now + self.window);
		self.prune_amortised(now);
	}

	/// Starts a cooldown unless one is active; returns whether to send a DM.
	pub fn try_notify(&mut self, user: UserId, channel: ChannelId, now: Instant) -> bool {
		match self.expiries.entry((user, channel)) {
			Entry::Occupied(entry) if *entry.get() > now => false,
			Entry::Occupied(mut entry) => {
				entry.insert(now + self.window);
				true
			}
			Entry::Vacant(entry) => {
				entry.insert(now + self.window);
				true
			}
		}
	}

	fn prune_amortised(&mut self, now: Instant) {
		if now < self.next_prune {
			return;
		}
		self.expiries.retain(|_, expiry| *expiry > now);
		self.next_prune = now + self.window;
	}
}

mod database {
	use anyhow::{Context, Error};
	use poise::serenity_prelude::UserId;
	use sqlx::{Pool, Sqlite};

	/// Adds a highlight for a user.
	pub async fn highlight_add(
		pool: &Pool<Sqlite>,
		user_id: UserId,
		regex: &str,
	) -> Result<(), Error> {
		let member_id = u64_to_i64(user_id.get());

		sqlx::query!(
			r#"
			insert into highlights (member_id, highlight)
				values (?1, ?2)
				on conflict (member_id, highlight) do nothing
			"#,
			member_id,
			regex
		)
		.execute(pool)
		.await
		.context("Failed to add highlight to database")?;

		Ok(())
	}

	/// Removes a highlight by ID for a specific user.
	pub async fn highlight_remove(
		pool: &Pool<Sqlite>,
		user_id: UserId,
		id: i64,
	) -> Result<bool, Error> {
		let member_id = u64_to_i64(user_id.get());

		let result = sqlx::query!(
			"delete from highlights where id = ?1 and member_id = ?2",
			id,
			member_id
		)
		.execute(pool)
		.await
		.context("Failed to remove highlight from database")?;

		Ok(result.rows_affected() > 0)
	}

	/// Gets all highlights for a specific user.
	pub async fn highlight_get(
		pool: &Pool<Sqlite>,
		user_id: UserId,
	) -> Result<Vec<(i64, String)>, Error> {
		let member_id = u64_to_i64(user_id.get());

		let rows = sqlx::query!(
			"select id, highlight from highlights where member_id = ?1",
			member_id
		)
		.fetch_all(pool)
		.await
		.context("Failed to fetch highlights from database")?;

		let mut highlights = Vec::new();
		for row in rows {
			highlights.push((row.id, row.highlight));
		}

		Ok(highlights)
	}

	/// Gets all highlights from all users.
	pub async fn highlight_get_all(pool: &Pool<Sqlite>) -> Result<Vec<(i64, String)>, Error> {
		let rows = sqlx::query!("select member_id, highlight from highlights")
			.fetch_all(pool)
			.await
			.context("Failed to fetch all highlights from database")?;

		let mut highlights = Vec::new();
		for row in rows {
			highlights.push((row.member_id, row.highlight));
		}

		Ok(highlights)
	}

	fn u64_to_i64(value: u64) -> i64 {
		i64::from_le_bytes(value.to_le_bytes())
	}
}
