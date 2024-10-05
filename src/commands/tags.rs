use crate::types::Context;
use anyhow::Error;
use poise::serenity_prelude::{CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter};
use poise::{serenity_prelude as serenity, CreateReply};
use serde::Serialize;
use sqlx::FromRow;

/*
PostgreSQL schema for tags:

CREATE TABLE tags
(
	name                TEXT PRIMARY KEY,
	content             TEXT NOT NULL,
	creator_user_id     BIGINT NOT NULL,
	last_editor_user_id BIGINT,
	creation_date       TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
	last_edit_date      TIMESTAMPTZ,
	times_used          INT         DEFAULT 0,
	restricted          BOOLEAN     DEFAULT FALSE
);

CREATE TABLE tag_aliases
(
	alias    TEXT PRIMARY KEY,
	tag_name TEXT NOT NULL,
	FOREIGN KEY (tag_name) REFERENCES tags (name) ON DELETE CASCADE
);

 */

#[derive(Serialize, FromRow)]
struct Tag {
	name: String,
	content: String,
	creator_user_id: i64,
	last_editor_user_id: Option<i64>,
	creation_date: String,
	last_edit_date: Option<String>,
	times_used: i32,
	restricted: bool,
}

struct TagStatsServer {
	total_number_of_tags: i32,
	total_tag_uses: i32,
	top_3_tags_used: Vec<Tag>,
	top_3_tag_creators: Vec<(i64, i32)>,
	top_3_tag_creators_by_uses: Vec<(i64, i32)>,
}

struct TagStatsMember {
	owned_tags: i32,
	owned_tags_uses: i32,
	top_3_tags: Vec<Tag>,
}

/// Display a tag.
#[poise::command(slash_command, category = "Tags")]
pub async fn tag(ctx: Context<'_>, name: String) -> Result<(), Error> {
	let tag = database::tag_get(&ctx.data().database, &name).await?;

	ctx.say(tag.content).await?;

	database::tag_increase_use_count(&ctx.data().database, &name).await?;

	Ok(())
}

/// Command that allows users to create, delete, and manage tags.
#[poise::command(
	slash_command,
	category = "Tags",
	subcommands(
		"tags_create",
		"tags_delete",
		"tags_alias",
		"tags_edit",
		"tags_restrict",
		"tags_stats",
		"tags_info",
		"tags_list",
	)
)]
pub async fn tags(_ctx: Context<'_>) -> Result<(), Error> {
	// Can't be invoked directly
	Ok(())
}

/// Creates a tag.
#[poise::command(
	rename = "create",
	slash_command,
	ephemeral,
	category = "Tags",
	aliases("add")
)]
pub async fn tags_create(ctx: Context<'_>, name: String, content: String) -> Result<(), Error> {
	let tag = database::tag_get(&ctx.data().database, &name).await;

	let tag_already_exists = tag.is_ok();
	if tag_already_exists {
		ctx.say("Tag already exists").await?;
		return Ok(());
	}

	let user_id = ctx.author().id.get();
	database::tag_create(&ctx.data().database, &name, &content, user_id).await?;

	ctx.say("Tag created").await?;
	Ok(())
}

/// Removes a tag and all of its aliases. If an alias is removed, the original tag will not be deleted.
#[poise::command(
	rename = "delete",
	slash_command,
	ephemeral,
	category = "Tags",
	aliases("remove")
)]
pub async fn tags_delete(ctx: Context<'_>, name: String) -> Result<(), Error> {
	database::tag_delete(&ctx.data().database, &name).await?;

	ctx.say("Tag deleted").await?;
	Ok(())
}

/// Creates an alias for an already existing tag so you can call it with either of the names.
#[poise::command(rename = "alias", slash_command, ephemeral, category = "Tags")]
pub async fn tags_alias(ctx: Context<'_>, existing: String, new: String) -> Result<(), Error> {
	let tag_already_exists = database::tag_get(&ctx.data().database, &new).await.is_ok();
	if tag_already_exists {
		ctx.say("Tag already exists").await?;
		return Ok(());
	}

	let tag = database::tag_get(&ctx.data().database, &existing).await?;
	database::tag_create_alias(&ctx.data().database, &tag.name, &new).await?;

	ctx.say("Alias created").await?;
	Ok(())
}

/// Edits the content of an already existing tag.
#[poise::command(rename = "edit", slash_command, ephemeral, category = "Tags")]
pub async fn tags_edit(ctx: Context<'_>, name: String, content: String) -> Result<(), Error> {
	let tag = database::tag_get(&ctx.data().database, &name).await?;
	let editor_is_staff = ctx
		.author()
		.has_role(ctx, ctx.data().discord_guild_id, ctx.data().mod_role_id)
		.await?;

	if tag.restricted && editor_is_staff {
		ctx.say("Tag is restricted").await?;
		return Ok(());
	}

	database::tag_edit(&ctx.data().database, &name, &content, ctx.author().id.get()).await?;

	ctx.say("Tag edited").await?;
	Ok(())
}

/// This will make the bot post the content in the bot-channel and ping the author upon being used.
#[poise::command(rename = "restrict", slash_command, ephemeral, category = "Tags")]
pub async fn tags_restrict(ctx: Context<'_>, name: String) -> Result<(), Error> {
	database::tag_restrict(&ctx.data().database, &name).await?;

	ctx.say("Tag restricted").await?;
	Ok(())
}

/// Shows information about the server tags. If you mention someone, it will show their tags instead.
#[poise::command(rename = "stats", slash_command, ephemeral, category = "Tags")]
pub async fn tags_stats(ctx: Context<'_>, member: Option<serenity::UserId>) -> Result<(), Error> {
	let embed = if let Some(member) = member {
		tags_member_stats(&ctx, member).await?
	} else {
		tags_server_stats(&ctx).await?
	};

	ctx.send(CreateReply::default().embed(embed)).await?;

	Ok(())
}

async fn tags_server_stats(ctx: &Context<'_>) -> Result<CreateEmbed, Error> {
	let stats = database::tag_get_server_stats(&ctx.data().database).await?;

	let embed = CreateEmbed::new()
		.title("Tag Stats")
		.description(format!(
			"Total number of tags: {}\nTotal tag uses: {}",
			stats.total_number_of_tags, stats.total_tag_uses
		))
		.field(
			"Top Tags",
			stats
				.top_3_tags_used
				.iter()
				.map(|tag| format!("{} ({} uses)", tag.name, tag.times_used))
				.collect::<Vec<_>>()
				.join("\n"),
			true,
		)
		.field(
			"Top Tag Creators",
			stats
				.top_3_tag_creators
				.iter()
				.map(|(user_id, tags_created)| format!("<@{}> ({} tags)", user_id, tags_created))
				.collect::<Vec<_>>()
				.join("\n"),
			true,
		)
		.field(
			"Top Tag Creators by Uses",
			stats
				.top_3_tag_creators_by_uses
				.iter()
				.map(|(user_id, tags_uses)| format!("<@{}> ({} uses)", user_id, tags_uses))
				.collect::<Vec<_>>()
				.join("\n"),
			true,
		);
	Ok(embed)
}

async fn tags_member_stats(
	ctx: &Context<'_>,
	member: serenity::UserId,
) -> Result<CreateEmbed, Error> {
	let stats = database::tag_get_member_stats(&ctx.data().database, member.get()).await?;

	let user = ctx.http().get_user(member).await?;

	let embed = CreateEmbed::new()
		.author(CreateEmbedAuthor::new(&user.name).icon_url(&user.face()))
		.description(format!(
			"Total number of tags: {}\nTotal tag uses: {}",
			stats.owned_tags, stats.owned_tags_uses
		))
		.field(
			"Top Tags",
			stats
				.top_3_tags
				.iter()
				.map(|tag| format!("{} ({} uses)", tag.name, tag.times_used))
				.collect::<Vec<_>>()
				.join("\n"),
			true,
		)
		.field(
			"Owned Tags",
			format!("{} tags ({} uses)", stats.owned_tags, stats.owned_tags_uses),
			true,
		);
	Ok(embed)
}

/// Shows some stats collected about the tag.
#[poise::command(rename = "info", slash_command, ephemeral, category = "Tags")]
pub async fn tags_info(ctx: Context<'_>, name: String) -> Result<(), Error> {
	let tag = database::tag_get(&ctx.data().database, &name).await?;

	let creator_id = database::i64_to_u64(tag.creator_user_id);
	let rank = database::tag_get_rank(&ctx.data().database, &name).await?;

	let mut embed = CreateEmbed::new()
		.author(
			CreateEmbedAuthor::new(&tag.name)
				.icon_url(ctx.http().get_user(creator_id.into()).await?.face()),
		)
		.title(&tag.name)
		.field("Created by", format!("<@{}>", creator_id), true)
		.field("Uses", tag.times_used.to_string(), true)
		.field("Rank", format!("{}/{}", rank.0, rank.1), true)
		.field("Restricted", tag.restricted.to_string(), true)
		.footer(CreateEmbedFooter::new(format!(
			"Tag created at: {}",
			tag.creation_date
		)));

	if let Some(last_editor_user_id) = tag.last_editor_user_id {
		embed = embed.field(
			"Last Editor",
			format!("<@{}>", database::i64_to_u64(last_editor_user_id)),
			true,
		);
	}

	if let Some(last_edit_date) = tag.last_edit_date {
		embed = embed.field("Last Edit Date", &last_edit_date, true);
	}

	ctx.send(CreateReply::default().embed(embed)).await?;

	Ok(())
}

/// Lists all tags in the server. If you mention someone, it will show their tags instead.
#[poise::command(rename = "list", slash_command, ephemeral, category = "Tags")]
pub async fn tags_list(ctx: Context<'_>, member: Option<serenity::UserId>) -> Result<(), Error> {
	let tags = if let Some(member) = member {
		let member = member.get();
		database::tag_get_by_member(&ctx.data().database, member).await?
	} else {
		database::tag_list(&ctx.data().database).await?
	};

	let tag_pages = tags
		.into_iter()
		.map(|tag| tag.name)
		.collect::<Vec<_>>()
		.chunks(30)
		.map(|chunk| chunk.join(", "))
		.collect::<Vec<_>>();

	crate::helpers::paginate(ctx, &tag_pages).await?;

	Ok(())
}

mod database {
	use super::{Tag, TagStatsMember, TagStatsServer};
	use anyhow::{anyhow, Error};
	use sqlx::{PgPool, Row};

	/// Fetches a tag from the database, including aliases. Returns an error if the tag or alias is
	/// not found.
	pub async fn tag_get(pool: &PgPool, name: &str) -> Result<Tag, Error> {
		let sql = "
			SELECT *
			FROM tags
			WHERE name = $1
			OR name IN (
				SELECT tag_name
				FROM tag_aliases
				WHERE alias = $1
			)
		";
		let query = sqlx::query_as::<_, Tag>(sql)
			.bind(name)
			.fetch_one(pool)
			.await;

		if let Err(sqlx::Error::RowNotFound) = query {
			return Err(anyhow!("Tag not found"));
		}

		query.map_err(|e| anyhow!(e))
	}

	/// Increases the use count of a tag by 1.
	pub async fn tag_increase_use_count(pool: &PgPool, name: &str) -> Result<(), Error> {
		let sql = "
			UPDATE tags
			SET times_used = times_used + 1
			WHERE name = $1
		";
		sqlx::query(sql)
			.bind(name)
			.execute(pool)
			.await
			.map_err(|e| anyhow!(e))?;

		Ok(())
	}

	/// Creates a new tag in the database.
	pub async fn tag_create(
		pool: &PgPool,
		name: &str,
		content: &str,
		creator_user_id: u64,
	) -> Result<(), Error> {
		let creator_user_id = u64_to_i64(creator_user_id);

		let sql = "
			INSERT INTO tags (name, content, creator_user_id)
			VALUES ($1, $2, $3)
		";
		sqlx::query(sql)
			.bind(name)
			.bind(content)
			.bind(creator_user_id)
			.execute(pool)
			.await
			.map_err(|e| anyhow!(e))?;

		Ok(())
	}

	/// Deletes a tag from the tag_aliases table, and if that doesn't delete anything, then it deletes
	/// on the tags table. If that doesn't delete anything then the tag didn't exist, and it returns
	/// an error.
	pub async fn tag_delete(pool: &PgPool, name: &str) -> Result<(), Error> {
		let sql = "DELETE FROM tag_aliases WHERE alias = $1";
		let deleted_aliases = sqlx::query(sql)
			.bind(name)
			.execute(pool)
			.await
			.map_err(|e| anyhow!(e))?;

		let alias_deleted = deleted_aliases.rows_affected() == 1;
		if alias_deleted {
			return Ok(());
		}

		let sql = "DELETE FROM tags WHERE name = $1";
		let deleted_tags = sqlx::query(sql)
			.bind(name)
			.execute(pool)
			.await
			.map_err(|e| anyhow!(e))?;

		let tag_deleted = deleted_tags.rows_affected() == 1;
		if !tag_deleted {
			return Err(anyhow!("Tag not found"));
		}

		Ok(())
	}

	/// Creates an alias for a tag.
	pub async fn tag_create_alias(pool: &PgPool, tag_name: &str, alias: &str) -> Result<(), Error> {
		let sql = "
			INSERT INTO tag_aliases (alias, tag_name)
			VALUES ($1, $2)
		";
		sqlx::query(sql)
			.bind(alias)
			.bind(tag_name)
			.execute(pool)
			.await
			.map_err(|e| anyhow!(e))?;

		Ok(())
	}

	/// Edits the content of a tag or alias. If the tag doesn't exist, it returns an error.
	pub async fn tag_edit(
		pool: &PgPool,
		name: &str,
		content: &str,
		editor_id: u64,
	) -> Result<(), Error> {
		let editor_id = u64_to_i64(editor_id);

		let sql = "
			UPDATE tags
			SET content = $2,
			    last_editor_user_id = $3,
			    last_edit_date = CURRENT_TIMESTAMP
			WHERE name = $1
		";
		sqlx::query(sql)
			.bind(name)
			.bind(content)
			.bind(editor_id)
			.execute(pool)
			.await
			.map_err(|e| anyhow!(e))?;

		Ok(())
	}

	/// Restricts a tag by tag name or alias.
	pub async fn tag_restrict(pool: &PgPool, name: &str) -> Result<(), Error> {
		let sql = "
			UPDATE tags
			SET restricted = TRUE
			WHERE name = $1
			OR name IN (
				SELECT tag_name
				FROM tag_aliases
				WHERE alias = $1
			)
		";
		sqlx::query(sql)
			.bind(name)
			.execute(pool)
			.await
			.map_err(|e| anyhow!(e))?;

		Ok(())
	}

	/// Gets general stats about the tags in the server.
	pub async fn tag_get_server_stats(pool: &PgPool) -> Result<TagStatsServer, Error> {
		let total_tag_uses = sqlx::query("SELECT SUM(times_used) FROM tags")
			.fetch_one(pool)
			.await
			.map_err(|e| anyhow!(e))?
			.get(0);

		let total_number_of_tags = sqlx::query("SELECT COUNT(*) FROM tags")
			.fetch_one(pool)
			.await
			.map_err(|e| anyhow!(e))?
			.get(0);

		let top_3_tags_used = sqlx::query_as("SELECT * FROM tags ORDER BY times_used DESC LIMIT 3")
			.fetch_all(pool)
			.await
			.map_err(|e| anyhow!(e))?;

		let top_3_tag_creators = sqlx::query_as(
			"SELECT creator_user_id, COUNT(*) FROM tags GROUP BY creator_user_id ORDER BY COUNT(*) DESC LIMIT 3",
		)
		.fetch_all(pool)
		.await
		.map_err(|e| anyhow!(e))?;

		let top_3_tag_creators_by_uses = sqlx::query_as(
			"SELECT creator_user_id, SUM(times_used) FROM tags GROUP BY creator_user_id ORDER BY SUM(times_used) DESC LIMIT 3",
		)
		.fetch_all(pool)
		.await
		.map_err(|e| anyhow!(e))?;

		Ok(TagStatsServer {
			total_number_of_tags,
			total_tag_uses,
			top_3_tags_used,
			top_3_tag_creators,
			top_3_tag_creators_by_uses,
		})
	}

	pub async fn tag_get_member_stats(pool: &PgPool, member: u64) -> Result<TagStatsMember, Error> {
		let member = u64_to_i64(member);

		let total_number_of_tags =
			sqlx::query("SELECT COUNT(*) FROM tags WHERE creator_user_id = $1")
				.bind(member)
				.fetch_one(pool)
				.await
				.map_err(|e| anyhow!(e))?
				.get(0);

		let total_tag_uses =
			sqlx::query("SELECT SUM(times_used) FROM tags WHERE creator_user_id = $1")
				.bind(member)
				.fetch_one(pool)
				.await
				.map_err(|e| anyhow!(e))?
				.get(0);

		let top_3_tags = sqlx::query_as(
			"SELECT * FROM tags WHERE creator_user_id = $1 ORDER BY times_used DESC LIMIT 3",
		)
		.bind(member)
		.fetch_all(pool)
		.await
		.map_err(|e| anyhow!(e))?;

		Ok(TagStatsMember {
			owned_tags: total_number_of_tags,
			owned_tags_uses: total_tag_uses,
			top_3_tags,
		})
	}

	/// Returns the times the tag has been used and the total amount of tags.
	pub async fn tag_get_rank(pool: &PgPool, name: &str) -> Result<(i32, i32), Error> {
		let sql = "SELECT times_used FROM tags WHERE name = $1";
		let times_used = sqlx::query(sql)
			.bind(name)
			.fetch_one(pool)
			.await
			.map_err(|e| anyhow!(e))?
			.get(0);

		let sql = "SELECT COUNT(*) FROM tags";
		let total_tags = sqlx::query(sql)
			.fetch_one(pool)
			.await
			.map_err(|e| anyhow!(e))?
			.get(0);

		Ok((times_used, total_tags))
	}

	/// Lists all tags owned by a user.
	pub async fn tag_get_by_member(pool: &PgPool, user_id: u64) -> Result<Vec<Tag>, Error> {
		let user_id = u64_to_i64(user_id);

		let sql = "SELECT * FROM tags WHERE creator_user_id = $1";
		let tags = sqlx::query_as::<_, Tag>(sql)
			.bind(user_id)
			.fetch_all(pool)
			.await
			.map_err(|e| anyhow!(e))?;

		Ok(tags)
	}

	/// Lists all tags.
	pub async fn tag_list(pool: &PgPool) -> Result<Vec<Tag>, Error> {
		let sql = "SELECT * FROM tags";
		let tags = sqlx::query_as::<_, Tag>(sql)
			.fetch_all(pool)
			.await
			.map_err(|e| anyhow!(e))?;

		Ok(tags)
	}

	pub fn i64_to_u64(value: i64) -> u64 {
		u64::from_le_bytes(value.to_le_bytes())
	}

	pub fn u64_to_i64(value: u64) -> i64 {
		i64::from_le_bytes(value.to_le_bytes())
	}
}
