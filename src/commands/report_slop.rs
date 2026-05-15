use anyhow::{Error, Result};
use futures::StreamExt;
use poise::serenity_prelude as serenity;
use poise::serenity_prelude::{ChannelType, EditThread, Mentionable};
use tracing::{error, warn};

use crate::checks::get_member_roles;
use crate::commands::modmail::create_modmail_thread;
use crate::types::Context;

const ACTION_UNLOCK: &str = "unlock";
const ACTION_KEEP: &str = "keep";
const ACTION_TICKET: &str = "ticket";

fn button_id(nonce: u64, action: &str) -> String {
	format!("report_slop:{nonce}:{action}")
}

fn parse_action(custom_id: &str, nonce: u64) -> Option<&'static str> {
	let expected_prefix = format!("report_slop:{nonce}:");
	let suffix = custom_id.strip_prefix(&expected_prefix)?;
	match suffix {
		ACTION_UNLOCK => Some(ACTION_UNLOCK),
		ACTION_KEEP => Some(ACTION_KEEP),
		ACTION_TICKET => Some(ACTION_TICKET),
		_ => None,
	}
}

/// Lock the current forum post and alert the moderators.
///
/// Intended as a "panic button" for non-moderator members when a forum post is
/// getting out of hand. The post auto-unlocks after a configured delay if no
/// moderator reacts via the alert buttons.
#[poise::command(
	prefix_command,
	slash_command,
	guild_only,
	category = "Moderation"
)]
pub async fn report_slop(
	ctx: Context<'_>,
	#[description = "Optional reason for reporting this post"]
	#[rest]
	reason: Option<String>,
) -> Result<(), Error> {
	let data = ctx.data();

	let member_has_banned_role = get_member_roles(ctx)
		.is_some_and(|roles| roles.contains(&data.banned_from_reporting_role_id));
	if member_has_banned_role {
		ctx.send(
			poise::CreateReply::default()
				.content("You are not allowed to use this command.")
				.ephemeral(true),
		)
		.await?;
		return Ok(());
	}

	let Some(thread) = ctx.guild_channel().await else {
		ctx.send(
			poise::CreateReply::default()
				.content("Could not fetch this channel.")
				.ephemeral(true),
		)
		.await?;
		return Ok(());
	};

	let Some(thread_metadata) = thread.thread_metadata else {
		ctx.send(
			poise::CreateReply::default()
				.content("This command can only be used inside a forum post.")
				.ephemeral(true),
		)
		.await?;
		return Ok(());
	};

	if thread_metadata.locked {
		ctx.send(
			poise::CreateReply::default()
				.content("This post is already locked.")
				.ephemeral(true),
		)
		.await?;
		return Ok(());
	}

	let parent_is_forum = match thread.parent_id {
		Some(parent_id) => match parent_id.to_channel(&ctx).await {
			Ok(parent) => parent
				.guild()
				.is_some_and(|c| c.kind == ChannelType::Forum),
			Err(err) => {
				warn!("Failed to fetch parent channel of thread {}: {err}", thread.id);
				false
			}
		},
		None => false,
	};
	if !parent_is_forum {
		ctx.send(
			poise::CreateReply::default()
				.content("This command can only be used inside a forum post.")
				.ephemeral(true),
		)
		.await?;
		return Ok(());
	}

	let mut thread = thread;
	if let Err(err) = thread
		.edit_thread(&ctx, EditThread::new().locked(true))
		.await
	{
		error!("Failed to lock thread {}: {err}", thread.id);
		ctx.send(
			poise::CreateReply::default()
				.content("Failed to lock this post. Are my permissions configured correctly?")
				.ephemeral(true),
		)
		.await?;
		return Ok(());
	}

	let nonce = ctx.id();
	let reason_line = match reason.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
		Some(r) => format!("\n> {r}"),
		None => String::new(),
	};
	let alert_content = format!(
		"⚠️ {} reported this post and locked it. {}{reason_line}",
		ctx.author().mention(),
		data.mod_role_id.mention(),
	);

	let buttons = serenity::CreateActionRow::Buttons(vec![
		serenity::CreateButton::new(button_id(nonce, ACTION_UNLOCK))
			.label("Unlock now")
			.style(serenity::ButtonStyle::Success),
		serenity::CreateButton::new(button_id(nonce, ACTION_KEEP))
			.label("Keep locked")
			.style(serenity::ButtonStyle::Danger),
		serenity::CreateButton::new(button_id(nonce, ACTION_TICKET))
			.label("Convert to mod ticket")
			.style(serenity::ButtonStyle::Primary),
	]);

	let alert_message = thread
		.send_message(
			&ctx,
			serenity::CreateMessage::new()
				.content(&alert_content)
				.components(vec![buttons])
				.allowed_mentions(
					serenity::CreateAllowedMentions::new()
						.users([ctx.author().id])
						.roles([data.mod_role_id]),
				),
		)
		.await?;

	ctx.send(
		poise::CreateReply::default()
			.content("This post has been locked and the moderators have been alerted.")
			.ephemeral(true),
	)
	.await?;

	let duration = data.report_slop_lock_duration;
	let mut stream = serenity::ComponentInteractionCollector::new(ctx.serenity_context())
		.message_id(alert_message.id)
		.timeout(duration)
		.stream();

	let resolution = loop {
		match stream.next().await {
			Some(press) => {
				let Some(action) = parse_action(&press.data.custom_id, nonce) else {
					continue;
				};

				let presser_is_mod = press
					.member
					.as_ref()
					.is_some_and(|m| m.roles.contains(&data.mod_role_id));
				if !presser_is_mod {
					let _ = press
						.create_response(
							ctx.serenity_context(),
							serenity::CreateInteractionResponse::Message(
								serenity::CreateInteractionResponseMessage::new()
									.content("Only moderators can use these buttons.")
									.ephemeral(true),
							),
						)
						.await;
					continue;
				}

				if let Err(err) = press.defer(ctx.serenity_context()).await {
					warn!("Failed to defer report_slop button press: {err}");
				}

				break Some((press, action));
			}
			None => break None,
		}
	};

	let resolution_text = match resolution {
		Some((press, ACTION_UNLOCK)) => {
			if let Err(err) = thread
				.edit_thread(&ctx, EditThread::new().locked(false))
				.await
			{
				error!("Failed to unlock thread {}: {err}", thread.id);
			}
			format!("✅ Unlocked by {}.", press.user.mention())
		}
		Some((press, ACTION_KEEP)) => {
			format!(
				"🔒 Kept locked by {} — moderators will follow up manually.",
				press.user.mention()
			)
		}
		Some((press, ACTION_TICKET)) => {
			let ticket_message = format!(
				"Reported post: {}{reason_line}",
				thread.id.mention(),
			);
			let modmail_outcome =
				create_modmail_thread(&ctx, ticket_message, data, ctx.author().id).await;
			match modmail_outcome {
				Ok(modmail) => format!(
					"🎫 Converted to modmail by {}: {}",
					press.user.mention(),
					modmail.mention()
				),
				Err(err) => {
					error!("Failed to create modmail from report_slop: {err}");
					format!(
						"🎫 {} tried to convert this to a modmail, but the thread could not be created. Post remains locked.",
						press.user.mention()
					)
				}
			}
		}
		Some((_, _)) => unreachable!("parse_action only returns known actions"),
		None => {
			if let Err(err) = thread
				.edit_thread(&ctx, EditThread::new().locked(false))
				.await
			{
				error!(
					"Failed to auto-unlock thread {} after timeout: {err}",
					thread.id
				);
			}
			let minutes = duration.as_secs() / 60;
			let notice = format!(
				"⏰ Auto-unlocked after {minutes} minutes — no moderator reviewed this report."
			);
			if let Err(err) = thread
				.send_message(&ctx, serenity::CreateMessage::new().content(&notice))
				.await
			{
				warn!(
					"Failed to post auto-unlock notice in thread {}: {err}",
					thread.id
				);
			}
			notice
		}
	};

	let final_alert = format!("{alert_content}\n\n{resolution_text}");
	if let Err(err) = alert_message
		.channel_id
		.edit_message(
			&ctx,
			alert_message.id,
			serenity::EditMessage::new()
				.content(final_alert)
				.components(vec![]),
		)
		.await
	{
		warn!(
			"Failed to update report_slop alert message {}: {err}",
			alert_message.id
		);
	}

	Ok(())
}
