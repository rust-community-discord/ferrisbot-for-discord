use crate::{require_database, types::Context};
use anyhow::Error;
use poise::serenity_prelude::{CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter};
use poise::{CreateReply, serenity_prelude as serenity};
use serde::Serialize;
use sqlx::{FromRow, Pool, Sqlite};

/*
SQLite schema for tags:

CREATE TABLE tags
(
	name                TEXT PRIMARY KEY NOT NULL,
	content             TEXT NOT NULL,
	creator_user_id     INTEGER NOT NULL,
	last_editor_user_id INTEGER,
	creation_date       TEXT DEFAULT CURRENT_TIMESTAMP NOT NULL,
	last_edit_date      TEXT,
	times_used          INTEGER DEFAULT 0 NOT NULL,
	restricted          INTEGER DEFAULT 0 NOT NULL
);

CREATE TABLE tag_aliases
(
	alias    TEXT PRIMARY KEY NOT NULL,
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
	times_used: i64,
	restricted: i64,
}

struct TagStatsServer {
	total_number_of_tags: i64,
	total_tag_uses: i64,
	top_3_tags_used: Vec<Tag>,
	top_3_tag_creators: Vec<(i64, i64)>,
	top_3_tag_creators_by_uses: Vec<(i64, i64)>,
}

struct TagStatsMember {
	owned_tags: i64,
	owned_tags_uses: i64,
	top_3_tags: Vec<Tag>,
}

/// Display a tag.
#[poise::command(slash_command, category = "Tags")]
pub async fn tag(ctx: Context<'_>, name: String) -> Result<(), Error> {
	let db = require_database!(ctx);
	let tag = database::tag_get(db, &name).await?;

	ctx.say(tag.content).await?;

	database::tag_increase_use_count(db, &name).await?;

	Ok(())
}

/// Command that allows users to create, delete, and manage tags.
#[allow(clippy::unused_async)]
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
	let db = require_database!(ctx);
	let tag = database::tag_get(db, &name).await;

	let tag_already_exists = tag.is_ok();
	if tag_already_exists {
		ctx.say("Tag already exists").await?;
		return Ok(());
	}

	let user_id = ctx.author().id.get();
	database::tag_create(db, &name, &content, user_id).await?;

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
	let db = require_database!(ctx);
	database::tag_delete(db, &name).await?;

	ctx.say("Tag deleted").await?;
	Ok(())
}

/// Creates an alias for an already existing tag so you can call it with either of the names.
#[poise::command(rename = "alias", slash_command, ephemeral, category = "Tags")]
pub async fn tags_alias(ctx: Context<'_>, existing: String, new: String) -> Result<(), Error> {
	let db = require_database!(ctx);
	let tag_already_exists = database::tag_get(db, &new).await.is_ok();
	if tag_already_exists {
		ctx.say("Tag already exists").await?;
		return Ok(());
	}

	let tag = database::tag_get(db, &existing).await?;
	database::tag_create_alias(db, &tag.name, &new).await?;

	ctx.say("Alias created").await?;
	Ok(())
}

/// Edits the content of an already existing tag.
#[poise::command(rename = "edit", slash_command, ephemeral, category = "Tags")]
pub async fn tags_edit(ctx: Context<'_>, name: String, content: String) -> Result<(), Error> {
	let db = require_database!(ctx);
	let tag = database::tag_get(db, &name).await?;
	let editor_is_staff = ctx
		.author()
		.has_role(ctx, ctx.data().discord_guild_id, ctx.data().mod_role_id)
		.await?;

	if tag.restricted != 0 && !editor_is_staff {
		ctx.say("Tag is restricted").await?;
		return Ok(());
	}

	database::tag_edit(db, &name, &content, ctx.author().id.get()).await?;

	ctx.say("Tag edited").await?;
	Ok(())
}

/// This will make the bot post the content in the bot-channel and ping the author upon being used.
#[poise::command(rename = "restrict", slash_command, ephemeral, category = "Tags")]
pub async fn tags_restrict(ctx: Context<'_>, name: String) -> Result<(), Error> {
	let db = require_database!(ctx);
	database::tag_restrict(db, &name).await?;

	ctx.say("Tag restricted").await?;
	Ok(())
}

/// Shows information about the server tags. If you mention someone, it will show their tags instead.
#[poise::command(rename = "stats", slash_command, ephemeral, category = "Tags")]
pub async fn tags_stats(ctx: Context<'_>, member: Option<serenity::UserId>) -> Result<(), Error> {
	let db = require_database!(ctx);
	let embed = if let Some(member) = member {
		tags_member_stats(&ctx, db, member).await?
	} else {
		tags_server_stats(&ctx, db).await?
	};

	ctx.send(CreateReply::default().embed(embed)).await?;

	Ok(())
}

async fn tags_server_stats(_ctx: &Context<'_>, db: &Pool<Sqlite>) -> Result<CreateEmbed, Error> {
	let stats = database::tag_get_server_stats(db).await?;

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
				.map(|(user_id, tags_created)| format!("<@{user_id}> ({tags_created} tags)"))
				.collect::<Vec<_>>()
				.join("\n"),
			true,
		)
		.field(
			"Top Tag Creators by Uses",
			stats
				.top_3_tag_creators_by_uses
				.iter()
				.map(|(user_id, tags_uses)| format!("<@{user_id}> ({tags_uses} uses)"))
				.collect::<Vec<_>>()
				.join("\n"),
			true,
		);
	Ok(embed)
}

async fn tags_member_stats(
	ctx: &Context<'_>,
	db: &Pool<Sqlite>,
	member: serenity::UserId,
) -> Result<CreateEmbed, Error> {
	let stats = database::tag_get_member_stats(db, member.get()).await?;

	let user = ctx.http().get_user(member).await?;

	let embed = CreateEmbed::new()
		.author(CreateEmbedAuthor::new(&user.name).icon_url(user.face()))
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
	let db = require_database!(ctx);
	let tag = database::tag_get(db, &name).await?;

	let creator_id = database::i64_to_u64(tag.creator_user_id);
	let rank = database::tag_get_rank(db, &name).await?;

	let mut embed = CreateEmbed::new()
		.author(
			CreateEmbedAuthor::new(&tag.name)
				.icon_url(ctx.http().get_user(creator_id.into()).await?.face()),
		)
		.title(&tag.name)
		.field("Created by", format!("<@{creator_id}>"), true)
		.field("Uses", tag.times_used.to_string(), true)
		.field("Rank", format!("{}/{}", rank.0, rank.1), true)
		.field("Restricted", (tag.restricted != 0).to_string(), true)
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
	let db = require_database!(ctx);
	let tags = if let Some(member) = member {
		let member = member.get();
		database::tag_get_by_member(db, member).await?
	} else {
		database::tag_list(db).await?
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
	use anyhow::{Context, Error};
	use sqlx::{Pool, Sqlite};

	/// Fetches a tag from the database, including aliases. Returns an error if the tag or alias is
	/// not found.
	pub async fn tag_get(pool: &Pool<Sqlite>, name: &str) -> Result<Tag, Error> {
		let query = sqlx::query_as!(
			Tag,
			"
			SELECT name as \"name!\", content as \"content!\", creator_user_id as \"creator_user_id!\", last_editor_user_id, creation_date as \"creation_date!\", last_edit_date, times_used as \"times_used!\", restricted as \"restricted!\"
			FROM tags
			WHERE name = ?1
			OR name IN (
				SELECT tag_name
				FROM tag_aliases
				WHERE alias = ?1
			)
			",
			name
		)
		.fetch_one(pool)
		.await;

		if let Err(sqlx::Error::RowNotFound) = query {
			return Err(anyhow::Error::msg("Tag not found"));
		}

		query.context("Failed to fetch tag from database")
	}

	/// Increases the use count of a tag by 1.
	pub async fn tag_increase_use_count(pool: &Pool<Sqlite>, name: &str) -> Result<(), Error> {
		sqlx::query!(
			"
			UPDATE tags
			SET times_used = times_used + 1
			WHERE name = ?1
			",
			name
		)
		.execute(pool)
		.await
		.context("Failed to increment tag use count")?;

		Ok(())
	}

	/// Creates a new tag in the database.
	pub async fn tag_create(
		pool: &Pool<Sqlite>,
		name: &str,
		content: &str,
		creator_user_id: u64,
	) -> Result<(), Error> {
		let creator_user_id = u64_to_i64(creator_user_id);

		sqlx::query!(
			"
			INSERT INTO tags (name, content, creator_user_id)
			VALUES (?1, ?2, ?3)
			",
			name,
			content,
			creator_user_id
		)
		.execute(pool)
		.await
		.context("Failed to create tag")?;

		Ok(())
	}

	/// Deletes a tag from the `tag_aliases` table, and if that doesn't delete anything, then it deletes
	/// on the tags table. If that doesn't delete anything then the tag didn't exist, and it returns
	/// an error.
	pub async fn tag_delete(pool: &Pool<Sqlite>, name: &str) -> Result<(), Error> {
		let deleted_aliases = sqlx::query!("DELETE FROM tag_aliases WHERE alias = ?1", name)
			.execute(pool)
			.await
			.context("Failed to delete tag alias")?;

		let alias_deleted = deleted_aliases.rows_affected() == 1;
		if alias_deleted {
			return Ok(());
		}

		let deleted_tags = sqlx::query!("DELETE FROM tags WHERE name = ?1", name)
			.execute(pool)
			.await
			.context("Failed to delete tag")?;

		let tag_deleted = deleted_tags.rows_affected() == 1;
		if !tag_deleted {
			return Err(anyhow::Error::msg("Tag not found"));
		}

		Ok(())
	}

	/// Creates an alias for a tag.
	pub async fn tag_create_alias(
		pool: &Pool<Sqlite>,
		tag_name: &str,
		alias: &str,
	) -> Result<(), Error> {
		sqlx::query!(
			"
			INSERT INTO tag_aliases (alias, tag_name)
			VALUES (?1, ?2)
			",
			alias,
			tag_name
		)
		.execute(pool)
		.await
		.context("Failed to create tag alias")?;

		Ok(())
	}

	/// Edits the content of a tag or alias. If the tag doesn't exist, it returns an error.
	pub async fn tag_edit(
		pool: &Pool<Sqlite>,
		name: &str,
		content: &str,
		editor_id: u64,
	) -> Result<(), Error> {
		let editor_id = u64_to_i64(editor_id);

		sqlx::query!(
			"
			UPDATE tags
			SET content = ?2,
			    last_editor_user_id = ?3,
			    last_edit_date = CURRENT_TIMESTAMP
			WHERE name = ?1
			",
			name,
			content,
			editor_id
		)
		.execute(pool)
		.await
		.context("Failed to edit tag")?;

		Ok(())
	}

	/// Restricts a tag by tag name or alias.
	pub async fn tag_restrict(pool: &Pool<Sqlite>, name: &str) -> Result<(), Error> {
		sqlx::query!(
			"
			UPDATE tags
			SET restricted = 1
			WHERE name = ?1
			OR name IN (
				SELECT tag_name
				FROM tag_aliases
				WHERE alias = ?1
			)
			",
			name
		)
		.execute(pool)
		.await
		.context("Failed to restrict tag")?;

		Ok(())
	}

	/// Gets general stats about the tags in the server.
	pub async fn tag_get_server_stats(pool: &Pool<Sqlite>) -> Result<TagStatsServer, Error> {
		let total_tag_uses_row = sqlx::query!("SELECT SUM(times_used) as total FROM tags")
			.fetch_one(pool)
			.await
			.context("Failed to fetch total tag uses")?;
		let total_tag_uses = total_tag_uses_row.total.unwrap_or(0);

		let total_number_of_tags_row = sqlx::query!("SELECT COUNT(*) as count FROM tags")
			.fetch_one(pool)
			.await
			.context("Failed to fetch total number of tags")?;
		let total_number_of_tags = total_number_of_tags_row.count;

		let top_3_tags_used = sqlx::query_as!(
			Tag,
			"SELECT name as \"name!\", content as \"content!\", creator_user_id as \"creator_user_id!\", last_editor_user_id, creation_date as \"creation_date!\", last_edit_date, times_used as \"times_used!\", restricted as \"restricted!\" FROM tags ORDER BY times_used DESC LIMIT 3"
		)
		.fetch_all(pool)
		.await
		.context("Failed to fetch top tags by usage")?;

		let top_3_tag_creators_rows = sqlx::query!(
			"SELECT creator_user_id, COUNT(*) as count FROM tags GROUP BY creator_user_id ORDER BY count DESC LIMIT 3"
		)
		.fetch_all(pool)
		.await
		.context("Failed to fetch top tag creators")?;
		let top_3_tag_creators = top_3_tag_creators_rows
			.into_iter()
			.map(|row| (row.creator_user_id, row.count))
			.collect();

		let top_3_tag_creators_by_uses_rows = sqlx::query!(
			"SELECT creator_user_id, SUM(times_used) as total FROM tags GROUP BY creator_user_id ORDER BY total DESC LIMIT 3"
		)
		.fetch_all(pool)
		.await
		.context("Failed to fetch top tag creators by uses")?;
		let top_3_tag_creators_by_uses = top_3_tag_creators_by_uses_rows
			.into_iter()
			.map(|row| (row.creator_user_id, row.total))
			.collect();

		Ok(TagStatsServer {
			total_number_of_tags,
			total_tag_uses,
			top_3_tags_used,
			top_3_tag_creators,
			top_3_tag_creators_by_uses,
		})
	}

	pub async fn tag_get_member_stats(
		pool: &Pool<Sqlite>,
		member: u64,
	) -> Result<TagStatsMember, Error> {
		let member = u64_to_i64(member);

		let total_number_of_tags_row = sqlx::query!(
			"SELECT COUNT(*) as count FROM tags WHERE creator_user_id = ?1",
			member
		)
		.fetch_one(pool)
		.await
		.context("Failed to fetch member's tag count")?;
		let total_number_of_tags = total_number_of_tags_row.count;

		let total_tag_uses_row = sqlx::query!(
			"SELECT SUM(times_used) as total FROM tags WHERE creator_user_id = ?1",
			member
		)
		.fetch_one(pool)
		.await
		.context("Failed to fetch member's tag usage count")?;
		let total_tag_uses = total_tag_uses_row.total.unwrap_or_default();

		let top_3_tags = sqlx::query_as!(
			Tag,
			"SELECT name as \"name!\", content as \"content!\", creator_user_id as \"creator_user_id!\", last_editor_user_id, creation_date as \"creation_date!\", last_edit_date, times_used as \"times_used!\", restricted as \"restricted!\" FROM tags WHERE creator_user_id = ?1 ORDER BY times_used DESC LIMIT 3",
			member
		)
		.fetch_all(pool)
		.await
		.context("Failed to fetch member's top tags")?;

		Ok(TagStatsMember {
			owned_tags: total_number_of_tags,
			owned_tags_uses: total_tag_uses,
			top_3_tags,
		})
	}

	/// Returns the times the tag has been used and the total amount of tags.
	pub async fn tag_get_rank(pool: &Pool<Sqlite>, name: &str) -> Result<(i64, i64), Error> {
		let times_used_row = sqlx::query!("SELECT times_used FROM tags WHERE name = ?1", name)
			.fetch_one(pool)
			.await
			.context("Failed to fetch tag usage count")?;
		let times_used = times_used_row.times_used;

		let total_tags_row = sqlx::query!("SELECT COUNT(*) as count FROM tags")
			.fetch_one(pool)
			.await
			.context("Failed to fetch total tag count")?;
		let total_tags = total_tags_row.count;

		Ok((times_used, total_tags))
	}

	/// Lists all tags owned by a user.
	pub async fn tag_get_by_member(pool: &Pool<Sqlite>, user_id: u64) -> Result<Vec<Tag>, Error> {
		let user_id = u64_to_i64(user_id);

		let tags = sqlx::query_as!(
			Tag,
			"SELECT name as \"name!\", content as \"content!\", creator_user_id as \"creator_user_id!\", last_editor_user_id, creation_date as \"creation_date!\", last_edit_date, times_used as \"times_used!\", restricted as \"restricted!\" FROM tags WHERE creator_user_id = ?1",
			user_id
		)
		.fetch_all(pool)
		.await
		.context("Failed to fetch member's tags")?;

		Ok(tags)
	}

	/// Lists all tags.
	pub async fn tag_list(pool: &Pool<Sqlite>) -> Result<Vec<Tag>, Error> {
		let tags = sqlx::query_as!(
			Tag,
			"SELECT name as \"name!\", content as \"content!\", creator_user_id as \"creator_user_id!\", last_editor_user_id, creation_date as \"creation_date!\", last_edit_date, times_used as \"times_used!\", restricted as \"restricted!\" FROM tags"
		)
		.fetch_all(pool)
		.await
		.context("Failed to fetch tag list")?;

		Ok(tags)
	}

	pub fn i64_to_u64(value: i64) -> u64 {
		u64::from_le_bytes(value.to_le_bytes())
	}

	pub fn u64_to_i64(value: u64) -> i64 {
		i64::from_le_bytes(value.to_le_bytes())
	}
}
