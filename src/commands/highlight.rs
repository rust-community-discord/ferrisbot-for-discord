use std::collections::HashMap;

use crate::types::Context;
use anyhow::{Error, Result};
use poise::{
	CreateReply,
	serenity_prelude::{CreateEmbed, UserId},
};
use regex::{Regex, RegexBuilder};
use sqlx::{Pool, Sqlite};

const DATABASE_DISABLED_MSG: &str = "Database is disabled; highlights are unavailable.";

fn database_pool<'a>(c: &'a Context<'_>) -> Option<&'a Pool<Sqlite>> {
	c.data().database.as_ref()
}

/// Helper macro to get the database pool or return early with an error message.
/// This reduces repetitive boilerplate in highlight commands.
macro_rules! require_database {
	($ctx:expr) => {
		match database_pool(&$ctx) {
			Some(db) => db,
			None => {
				$ctx.say(DATABASE_DISABLED_MSG).await?;
				return Ok(());
			}
		}
	};
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

	if let Err(e) = RegexBuilder::new(&regex).size_limit(1 << 10).build() {
		c.say(format!("```\n{e}```")).await?;
		return Ok(());
	}

	let author_id = c.author().id.get() as i64;

	sqlx::query!(
		r#"
		insert into highlights (member_id, highlight)
			values (?1, ?2)
			on conflict (member_id, highlight) do nothing
		"#,
		author_id,
		regex
	)
	.execute(db)
	.await?;

	RegexHolder::update(c.data()).await;
	c.say("hl added!").await?;

	Ok(())
}

#[poise::command(prefix_command, slash_command)]
/// Removes a highlight by ID.
pub async fn remove(c: Context<'_>, id: i64) -> Result<()> {
	let db = require_database!(c);

	let author = c.author().id.get() as i64;

	let result = sqlx::query!(
		"delete from highlights where id = ?1 and member_id = ?2",
		id,
		author
	)
	.execute(db)
	.await?;

	c.say({
		if result.rows_affected() > 0 {
			"hl removed!"
		} else {
			"hl not found."
		}
	})
	.await?;

	RegexHolder::update(c.data()).await;

	Ok(())
}

async fn get(id: UserId, db: Option<&Pool<Sqlite>>) -> Result<Vec<(i64, String)>> {
	let Some(db) = db else {
		return Ok(Vec::new());
	};
	let member_id = id.get() as i64;
	let rows = sqlx::query!(
		"select id, highlight from highlights where member_id = ?1",
		member_id
	)
	.fetch_all(db)
	.await?;

	let mut highlights = Vec::new();
	for row in rows {
		highlights.push((row.id, row.highlight));
	}

	Ok(highlights)
}

#[poise::command(prefix_command, slash_command)]
/// Lists your current highlights
pub async fn list(c: Context<'_>) -> Result<()> {
	let db = require_database!(c);
	let highlights = get(c.author().id, Some(db)).await?;
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

pub async fn matches(
	author: UserId,
	haystack: &str,
	db: Option<&Pool<Sqlite>>,
) -> Result<Vec<String>> {
	let patterns = get(author, db).await?;
	let mut matched = Vec::new();
	for (_id, pattern) in patterns {
		if let Ok(regex) = Regex::new(&pattern)
			&& regex.is_match(haystack)
		{
			matched.push(pattern);
		}
	}
	Ok(matched)
}

#[poise::command(prefix_command, slash_command, rename = "match")]
/// Tests if your highlights match a given string
pub async fn mat(c: Context<'_>, haystack: String) -> Result<()> {
	let db = require_database!(c);
	let x = matches(c.author().id, &haystack, Some(db)).await?;

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
		let rows = match sqlx::query!("select member_id, highlight from highlights")
			.fetch_all(db)
			.await
		{
			Ok(rows) => rows,
			Err(e) => {
				warn!("Failed to load highlights from database: {e}");
				return Self(Vec::new());
			}
		};

		let mut entries = Vec::new();
		for row in rows {
			let member_id = row.member_id;
			let highlight = row.highlight;
			match Regex::new(&highlight) {
				Ok(regex) => entries.push((UserId::new(member_id.cast_unsigned()), regex)),
				Err(e) => warn!("Invalid regex pattern '{highlight}' for member {member_id}: {e}"),
			}
		}

		Self(entries)
	}

	async fn update(data: &crate::types::Data) {
		let new = Self::new(data.database.as_ref()).await;
		*data.highlights.write().await = new;
	}

	#[must_use]
	pub fn find(&self, haystack: &str) -> Vec<(UserId, String)> {
		self.0
			.iter()
			.filter(|(_, regex)| regex.is_match(haystack))
			.map(|(user_id, regex)| (*user_id, regex.as_str().to_string()))
			.collect::<HashMap<_, _>>()
			.into_iter()
			.collect()
	}
}
