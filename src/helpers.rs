use anyhow::Error;
use poise::serenity_prelude as serenity;
use tracing::warn;

use crate::types::{Context, Data};

/// Used for playground stdout + stderr, or godbolt asm + stderr
/// If the return value is empty, returns " " instead, because Discord displays those better in
/// a code block than "".
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
	if let poise::FrameworkError::Command { error, ctx } = error {
		warn!("Reacting with red cross because of error: {}", error);

		match ctx {
			Context::Application(_) => {
				if let Err(e) = ctx.say(format!("❌ {}", error)).await {
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

pub async fn find_custom_emoji(ctx: Context<'_>, emoji_name: &str) -> Option<serenity::Emoji> {
	ctx.guild_id()?
		.to_guild_cached(ctx)?
		.emojis
		.values()
		.find(|emoji| emoji.name.eq_ignore_ascii_case(emoji_name))
		.cloned()
}

pub async fn custom_emoji_code(ctx: Context<'_>, emoji_name: &str, fallback: char) -> String {
	match find_custom_emoji(ctx, emoji_name).await {
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
	let emoji = find_custom_emoji(ctx, emoji_name).await;
	match ctx {
		Context::Prefix(prefix_context) => {
			let reaction = emoji
				.map(serenity::ReactionType::from)
				.unwrap_or_else(|| serenity::ReactionType::from(fallback));

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
/// or if the text_body has too many lines.
///
/// Only `text_body` is truncated. `text_end` will always be appended at the end. This is useful
/// for example for large code blocks. You will want to truncate the code block contents, but the
/// finalizing triple backticks (` ` `) should always stay - that's what `text_end` is for.
pub async fn trim_text(
	mut text_body: &str,
	text_end: &str,
	truncation_msg_future: impl std::future::Future<Output = String>,
) -> String {
	const MAX_OUTPUT_LINES: usize = 45;

	// Err with the future inside if no truncation occurs
	let mut truncation_msg_maybe = Err(truncation_msg_future);

	// check Discord's 2000 char message limit first
	if text_body.len() + text_end.len() > 2000 {
		let truncation_msg = match truncation_msg_maybe {
			Ok(msg) => msg,
			Err(future) => future.await,
		};

		// This is how long the text body may be at max to conform to Discord's limit
		let available_space = 2000_usize
			.saturating_sub(text_end.len())
			.saturating_sub(truncation_msg.len());

		let mut cut_off_point = available_space;
		while !text_body.is_char_boundary(cut_off_point) {
			cut_off_point -= 1;
		}

		text_body = &text_body[..cut_off_point];
		truncation_msg_maybe = Ok(truncation_msg);
	}

	// check number of lines
	let text_body = if text_body.lines().count() > MAX_OUTPUT_LINES {
		truncation_msg_maybe = Ok(match truncation_msg_maybe {
			Ok(msg) => msg,
			Err(future) => future.await,
		});

		text_body
			.lines()
			.take(MAX_OUTPUT_LINES)
			.collect::<Vec<_>>()
			.join("\n")
	} else {
		text_body.to_owned()
	};

	if let Ok(truncation_msg) = truncation_msg_maybe {
		format!("{}{}{}", text_body, text_end, truncation_msg)
	} else {
		format!("{}{}", text_body, text_end)
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
