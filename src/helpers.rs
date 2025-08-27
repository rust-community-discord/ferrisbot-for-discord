use anyhow::{Error, bail, Context as AnyhowContext};
use poise::serenity_prelude::{self as serenity, Mentionable};
use tracing::warn;

use crate::types::{Context, Data};

/// Used for playground stdout + stderr, or godbolt asm + stderr
/// If the return value is empty, returns " " instead, because Discord displays those better in
/// a code block than "".
#[must_use]
pub fn merge_output_and_errors<'a>(output: &'a str, errors: &'a str) -> std::borrow::Cow<'a, str> {
	match (output.trim(), errors.trim()) {
		("", "") => " ".into(),
		(output, "") => output.into(),
		("", errors) => errors.into(),
		(output, errors) => format!("{errors}\n\n{output}").into(),
	}
}

/// In prefix commands, react with a red cross emoji. In slash commands, respond with a short
/// explanation.
pub async fn acknowledge_fail(error: poise::FrameworkError<'_, Data, Error>) {
	if let poise::FrameworkError::Command { error, ctx, .. } = error {
		warn!("Reacting with red cross because of error: {}", error);

		match ctx {
			Context::Application(_) => {
				if let Err(e) = ctx.say(format!("❌ {error}")).await {
					warn!(
						"Failed to send failure acknowledgment slash command response: {}",
						e
					);
				}
			}
			Context::Prefix(prefix_context) => {
				if let Err(e) = prefix_context
					.msg
					.react(ctx, serenity::ReactionType::from('❌'))
					.await
				{
					warn!("Failed to react with red cross: {}", e);
				}
			}
		}
	} else {
		// crate::on_error(error).await;
	}
}

#[must_use]
pub fn find_custom_emoji(ctx: Context<'_>, emoji_name: &str) -> Option<serenity::Emoji> {
	ctx.guild_id()?
		.to_guild_cached(&ctx)?
		.emojis
		.values()
		.find(|emoji| emoji.name.eq_ignore_ascii_case(emoji_name))
		.cloned()
}

#[must_use]
pub fn custom_emoji_code(ctx: Context<'_>, emoji_name: &str, fallback: char) -> String {
	match find_custom_emoji(ctx, emoji_name) {
		Some(emoji) => emoji.to_string(),
		None => fallback.to_string(),
	}
}

/// In prefix commands, react with a custom emoji from the guild, or fallback to a default Unicode
/// emoji.
///
/// In slash commands, currently nothing happens.
pub async fn acknowledge_success(
	ctx: Context<'_>,
	emoji_name: &str,
	fallback: char,
) -> Result<(), Error> {
	let emoji = find_custom_emoji(ctx, emoji_name);
	match ctx {
		Context::Prefix(prefix_context) => {
			let reaction = emoji.map_or_else(
				|| serenity::ReactionType::from(fallback),
				serenity::ReactionType::from,
			);

			prefix_context.msg.react(&ctx, reaction).await?;
		}
		Context::Application(_) => {
			let msg_content = match emoji {
				Some(e) => e.to_string(),
				None => fallback.to_string(),
			};
			if let Ok(reply) = ctx.say(msg_content).await {
				tokio::time::sleep(std::time::Duration::from_secs(3)).await;
				let msg = reply.message().await?;
				// ignore errors as to not fail if ephemeral
				let _: Result<_, _> = msg.delete(&ctx).await;
			}
		}
	}
	Ok(())
}

/// Truncates the message with a given truncation message if the
/// text is too long. "Too long" means, it either goes beyond Discord's 2000 char message limit,
/// or if the `text_body` has too many lines.
///
/// Only `text_body` is truncated. `text_end` will always be appended at the end. This is useful
/// for example for large code blocks. You will want to truncate the code block contents, but the
/// finalizing triple backticks (` ` `) should always stay - that's what `text_end` is for.
#[expect(clippy::doc_markdown)] // backticks cause clippy to freak out
pub async fn trim_text(
	text_body: &str,
	text_end: &str,
	truncation_msg_future: impl std::future::Future<Output = String>,
) -> String {
	const MAX_OUTPUT_LINES: usize = 45;
	const MAX_OUTPUT_LENGTH: usize = 2000;

	let needs_truncating = text_body.len() + text_end.len() > MAX_OUTPUT_LENGTH
		|| text_body.lines().count() > MAX_OUTPUT_LINES;

	if needs_truncating {
		let truncation_msg = truncation_msg_future.await;

		// truncate for length
		let text_body: String = text_body
			.chars()
			.take(MAX_OUTPUT_LENGTH - truncation_msg.len() - text_end.len())
			.collect();

		// truncate for lines
		let text_body = text_body
			.lines()
			.take(MAX_OUTPUT_LINES)
			.collect::<Vec<_>>()
			.join("\n");

		format!("{text_body}{text_end}{truncation_msg}")
	} else {
		format!("{text_body}{text_end}")
	}
}

pub async fn reply_potentially_long_text(
	ctx: Context<'_>,
	text_body: &str,
	text_end: &str,
	truncation_msg_future: impl std::future::Future<Output = String>,
) -> Result<(), Error> {
	ctx.say(trim_text(text_body, text_end, truncation_msg_future).await)
		.await?;
	Ok(())
}

/// Send an audit log message to the modlog channel
pub async fn send_audit_log(
	ctx: Context<'_>,
	category: &str,
	executor: serenity::UserId,
	content: &str,
) -> Result<(), Error> {
	let modlog_channel_id = ctx.data().modlog_channel_id;

	let channel = modlog_channel_id
		.to_channel(&ctx)
		.await
		.context("Modlog channel not found. Please create a channel and set the MODLOG_CHANNEL_ID environment variable to its ID.")?;

	let is_text_channel = matches!(channel.guild(), Some(guild_channel) if guild_channel.kind == serenity::ChannelType::Text);

	if !is_text_channel {
		bail!("Modlog channel must be a text channel. Please set MODLOG_CHANNEL_ID to a valid text channel ID.");
	}

	let mentionable_username = executor.mention();

	let log_message = format!(
		"Log Category: {}\nExecutor: {}\n\n{}",
		category,
		mentionable_username,
		content
	);

	modlog_channel_id
		.say(&ctx, log_message)
		.await
		.context("Failed to send audit log message")?;

	Ok(())
}
