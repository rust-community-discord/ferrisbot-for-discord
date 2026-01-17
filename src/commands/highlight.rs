use std::collections::HashMap;

use crate::types::Context;
use anyhow::{Error, Result};
use poise::{
	CreateReply,
	serenity_prelude::{CreateEmbed, UserId},
};
use regex::{Regex, RegexBuilder};
use sqlx::{Pool, Row, Sqlite};

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
	sqlx::query(
		"
	insert into highlights (id, highlight)
	    values (?1, ?2)
	on conflict (id, highlight) do nothing",
	)
	.bind(c.author().id.get() as i64)
	.bind(&regex)
	.execute(db)
	.await?;
	c.say("hl added!").await?;
	RegexHolder::update(c.data()).await;
	Ok(())
}

#[poise::command(prefix_command, slash_command)]
/// Removes a highlight.
pub async fn remove(c: Context<'_>, regex: String) -> Result<()> {
	let db = require_database!(c);
	if let Err(e) = Regex::new(&regex) {
		c.say(format!("```\n{e}```")).await?;
		return Ok(());
	}
	let u = c.author().id.get() as i64;
	c.say(
		if sqlx::query_scalar::<Sqlite, i64>(
			"select 1 from highlights where id = ?1 and highlight = ?2",
		)
		.bind(u)
		.bind(&regex)
		.fetch_optional(db)
		.await?
		.is_some()
		{
			sqlx::query("delete from highlights where id = ?1 and highlight = ?2")
				.bind(u)
				.bind(&regex)
				.execute(db)
				.await?;
			"hl removed!"
		} else {
			"hl not found."
		},
	)
	.await?;
	RegexHolder::update(c.data()).await;
	Ok(())
}

async fn get(id: UserId, db: Option<&Pool<Sqlite>>) -> Result<Vec<String>> {
	let Some(db) = db else {
		return Ok(Vec::new());
	};
	let rows = sqlx::query("select highlight from highlights where id = ?1")
		.bind(id.get() as i64)
		.fetch_all(db)
		.await?;

	let mut highlights = Vec::new();
	for row in rows {
		let highlight: String = row.try_get("highlight")?;
		highlights.push(highlight);
	}

	Ok(highlights)
}

#[poise::command(prefix_command, slash_command)]
/// Lists your current highlights
pub async fn list(c: Context<'_>) -> Result<()> {
	let db = require_database!(c);
	let x = get(c.author().id, Some(db)).await?;
	poise::send_reply(
		c,
		CreateReply::default().embed(
			CreateEmbed::new()
				.color((0xFC, 0xCA, 0x4C))
				.title("you're tracking these patterns")
				.description(itertools::intersperse(x, "\n".to_string()).collect::<String>()),
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
	for pattern in patterns {
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
		let rows = match sqlx::query("select id, highlight from highlights")
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
			let id: i64 = match row.try_get("id") {
				Ok(id) => id,
				Err(e) => {
					warn!("Failed to get 'id' from highlight row: {e}");
					continue;
				}
			};
			let highlight: String = match row.try_get("highlight") {
				Ok(highlight) => highlight,
				Err(e) => {
					warn!("Failed to get 'highlight' from row for user {id}: {e}");
					continue;
				}
			};
			match Regex::new(&highlight) {
				Ok(regex) => entries.push((UserId::new(id as u64), regex)),
				Err(e) => warn!("Invalid regex pattern '{highlight}' for user {id}: {e}"),
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
