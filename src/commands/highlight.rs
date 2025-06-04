use crate::types::Context;
use anyhow::{Error, Result};
use implicit_fn::implicit_fn;
use poise::{
	CreateReply,
	serenity_prelude::{CreateEmbed, UserId},
};
use regex::Regex;
use sqlx::{Pool, Postgres};

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
	if let Err(e) = regex_syntax::parse(&regex) {
		c.say(format!("```\n{e}```")).await?;
		return Ok(());
	};
	sqlx::query!(
		"
	insert into highlights (id, highlight)
	    values ($1, array[$2])
	on conflict (id) do update
	    set highlight = array_append(highlights.highlight, $2)",
		c.author().id.get() as i64,
		regex
	)
	.execute(&c.data().database)
	.await?;
	c.say("hl added!").await?;
	Ok(())
}

#[poise::command(prefix_command, slash_command)]
/// Removes a highlight.
pub async fn remove(c: Context<'_>, regex: String) -> Result<()> {
	if let Err(e) = regex_syntax::parse(&regex) {
		c.say(format!("```\n{e}```")).await?;
		return Ok(());
	};
	let u = c.author().id;
	let u = u.get() as i64;
	c.say(
		if sqlx::query_scalar!(
			"select $2 = any(highlight) from highlights where id = $1",
			u,
			regex,
		)
		.fetch_optional(&c.data().database)
		.await?
		.flatten()
		.unwrap_or(false)
		{
			sqlx::query!(
				r#"
                update highlights
                set highlight = array_remove(highlight, $2)
                where id = $1
            "#,
				u,
				regex
			)
			.execute(&c.data().database)
			.await?;
			"hl removed!"
		} else {
			"hl not found."
		},
	)
	.await?;
	Ok(())
}

#[implicit_fn]
async fn get(id: UserId, db: &Pool<Postgres>) -> impl Iterator<Item = String> {
	sqlx::query!(
		"select highlight from highlights where id = $1",
		id.get() as i64
	)
	.fetch_optional(db)
	.await
	.ok()
	.flatten()
	.into_iter()
	.flat_map(_.highlight)
}

#[poise::command(prefix_command, slash_command)]
/// Lists your current highlights
pub async fn list(c: Context<'_>) -> Result<()> {
	let x = get(c.author().id, &c.data().database).await;
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

pub async fn matches<'a, 'b: 'a>(
	author: UserId,
	haystack: &'b str,
	db: &'a Pool<Postgres>,
) -> impl Iterator<Item = String> + 'a {
	get(author, db)
		.await
		.filter(|x| Regex::new(&x).unwrap().is_match(haystack))
}

#[implicit_fn]
pub async fn all_matches<'a>(
	haystack: &'a str,
	db: &Pool<Postgres>,
) -> impl Iterator<Item = (String, UserId)> + 'a {
	// could use sql regex matching if problematic
	// or maybe we have to use words like turtle
	sqlx::query!["select * from highlights"]
		.fetch_all(db)
		.await
		.ok()
		.into_iter()
		.flatten()
		.flat_map(|x| {
			x.highlight
				.into_iter()
				.filter(|x| Regex::new(&x).unwrap().is_match(haystack))
				.take(1)
				.map(move |y| (y, UserId::new(x.id as _)))
		})
}

#[poise::command(prefix_command, slash_command, rename = "match")]
#[implicit_fn::implicit_fn]
/// Tests if your highlights match a given string
pub async fn mat(c: Context<'_>, haystack: String) -> Result<()> {
	let x = matches(c.author().id, &haystack, &c.data().database).await;

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
