use std::collections::HashMap;

use crate::{require_database, types::Context};
use anyhow::{Error, Result};
use poise::{
	CreateReply,
	serenity_prelude::{CreateEmbed, UserId},
};
use regex::{Regex, RegexBuilder};
use sqlx::{Pool, Sqlite};

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

#[poise::command(prefix_command, slash_command, ephemeral)]
/// Adds a highlight. When a highlight is matched, you will receive a DM.
pub async fn add(c: Context<'_>, regex: String) -> Result<()> {
	let db = require_database!(c);
	c.defer().await?;

	if let Err(e) = RegexBuilder::new(&regex).size_limit(1 << 10).build() {
		c.say(format!("```\n{e}```")).await?;
		return Ok(());
	}

	database::highlight_add(db, c.author().id, &regex).await?;

	RegexHolder::update(c.data()).await;
	c.say(format!("added `{regex}` to your highlights")).await?;

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
			Regex::new(&pattern)
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

		let entries = rows
			.into_iter()
			.filter_map(|(member_id, highlight)| match Regex::new(&highlight) {
				Ok(regex) => Some((UserId::new(member_id.cast_unsigned()), regex)),
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
		self.0
			.iter()
			.filter(|&(_user_id, regex)| regex.is_match(haystack))
			.map(|(user_id, regex)| (*user_id, regex.as_str().to_string()))
			.collect()
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
