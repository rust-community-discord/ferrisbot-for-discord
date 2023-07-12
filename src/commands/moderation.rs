use anyhow::Error;
use poise::serenity_prelude as serenity;

use crate::types::Context;

/// Deletes the bot's messages for cleanup
///
/// /cleanup [limit]
///
/// By default, only the most recent bot message is deleted (limit = 1).
///
/// Deletes the bot's messages for cleanup.
/// You can specify how many messages to look for. Only the 20 most recent messages within the
/// channel from the last 24 hours can be deleted.
#[poise::command(
	slash_command,
	category = "Moderation",
	on_error = "crate::helpers::acknowledge_fail"
)]
pub async fn cleanup(
	ctx: Context<'_>,
	#[description = "Number of messages to delete"] num_messages: Option<usize>,
) -> Result<(), Error> {
	let num_messages = num_messages.unwrap_or(1);

	let messages_to_delete = ctx
		.channel_id()
		.messages(&ctx, |get_messages| get_messages.limit(20))
		.await?
		.into_iter()
		.filter(|msg| {
			if msg.author.id != ctx.data().application_id {
				return false;
			}
			if (*ctx.created_at() - *msg.timestamp).num_hours() >= 24 {
				return false;
			}
			true
		})
		.take(num_messages);

	ctx.channel_id()
		.delete_messages(&ctx, messages_to_delete)
		.await?;

	crate::helpers::acknowledge_success(ctx, "rustOk", '👌').await
}

/// Bans another person
///
/// /ban <member> [reason]
///
/// Bans another person
#[poise::command(
	slash_command,
	aliases("banne"),
	category = "Moderation",
	on_error = "crate::helpers::acknowledge_fail"
)]
pub async fn ban(
	ctx: Context<'_>,
	#[description = "Banned user"] banned_user: serenity::Member,
	#[description = "Ban reason"]
	#[rest]
	_reason: Option<String>,
) -> Result<(), Error> {
	ctx.say(format!(
		"Banned user {}  {}",
		banned_user.user.tag(),
		crate::helpers::custom_emoji_code(ctx, "ferrisBanne", '🔨').await
	))
	.await?;
	Ok(())
}
