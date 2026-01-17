use anyhow::{Context as AnyhowContext, Error, anyhow};
use poise::serenity_prelude as serenity;
use poise::serenity_prelude::{EditThread, GuildChannel, Mentionable, UserId};
use rand::Rng;
use tracing::{debug, info};

use crate::types::{Context, Data};

/// Sends a success response after creating a modmail thread.
async fn send_modmail_success(ctx: Context<'_>, modmail: &GuildChannel) -> Result<(), Error> {
	ctx.say(format!(
		"Successfully sent your message to the moderators. Check out your modmail thread here: {}",
		modmail.mention()
	))
	.await?;
	Ok(())
}

/// Opens a modmail thread for a message. To use, right-click the message that
/// you want to report, then go to "Apps" > "Open Modmail".
#[poise::command(
	ephemeral,
	context_menu_command = "Open Modmail",
	hide_in_help,
	category = "Modmail"
)]
pub async fn modmail_context_menu_for_message(
	ctx: Context<'_>,
	#[description = "Message to automatically link when opening a modmail"]
	message: serenity::Message,
) -> Result<(), Error> {
	let message = format!(
		"Message reported: {}\n\nMessage contents:\n\n{}",
		message.id.link(ctx.channel_id(), ctx.guild_id()),
		message.content_safe(ctx)
	);
	let modmail = create_modmail_thread(ctx, message, ctx.data(), ctx.author().id).await?;
	send_modmail_success(ctx, &modmail).await?;
	Ok(())
}

/// Opens a modmail thread for a guild member. To use, right-click the member
/// that you want to report, then go to "Apps" > "Open Modmail".
#[poise::command(
	ephemeral,
	context_menu_command = "Open Modmail",
	hide_in_help,
	category = "Modmail"
)]
pub async fn modmail_context_menu_for_user(
	ctx: Context<'_>,
	#[description = "User to automatically link when opening a modmail"] user: serenity::User,
) -> Result<(), Error> {
	let message = format!(
		"User reported:\n{}\n{}\n\nPlease provide additional information about the user being reported.",
		user.id, user.name
	);
	let modmail = create_modmail_thread(ctx, message, ctx.data(), ctx.author().id).await?;
	send_modmail_success(ctx, &modmail).await?;
	Ok(())
}

/// Send a private message to the moderators of the server.
///
/// Call this command in a channel when someone might be breaking the rules, for example by being \
/// very rude, or starting discussions about divisive topics like politics and religion. Nobody \
/// will see that you invoked this command.
///
/// You can also use this command whenever you want to ask private questions to the moderator team,
/// open ban appeals, and generally anything that you need help with.
///
/// Your message, along with a link to the channel and its most recent message, will show up in a
/// dedicated modmail channel for moderators, and it allows them to deal with it much faster than if
/// you were to DM a potentially AFK moderator.
///
/// You can still always ping the Moderator role if you're comfortable doing so.
#[poise::command(prefix_command, slash_command, ephemeral, category = "Modmail")]
pub async fn modmail(
	ctx: Context<'_>,
	#[description = "What would you like to say?"] user_message: String,
) -> Result<(), Error> {
	let message = format!(
		"{}\n\nSent from {}",
		user_message,
		ctx.channel_id().mention()
	);
	let modmail = create_modmail_thread(ctx, message, ctx.data(), ctx.author().id).await?;
	send_modmail_success(ctx, &modmail).await?;
	Ok(())
}

pub async fn load_or_create_modmail_message(
	http: impl serenity::CacheHttp,
	data: &Data,
) -> Result<(), Error> {
	// Do nothing if message already exists in cache
	if data.modmail_message.read().await.clone().is_some() {
		debug!("Modmail message already exists on data cache.");
		return Ok(());
	}

	// Fetch modmail guild channel
	let modmail_guild_channel = data
		.modmail_channel_id
		.to_channel(&http)
		.await
		.context("Cannot enter modmail channel")?
		.guild()
		.ok_or_else(|| anyhow!("Modmail channel is not a guild channel"))?;

	// Fetch the report message itself
	let open_report_message = modmail_guild_channel
		.messages(&http, serenity::GetMessages::new().limit(1))
		.await?
		.first()
		.cloned();

	let message = if let Some(desired_message) = open_report_message {
		// If it exists, return it
		desired_message
	} else {
		// If it doesn't exist, create one and return it
		debug!("Creating new modmail message");
		modmail_guild_channel
			.send_message(
				&http,
				serenity::CreateMessage::new()
					.content("\
This is the Modmail channel. In here, you're able to create modmail reports to reach out to the Moderators about things such as reporting rule breaking, or asking a private question.

To open a ticket, either right click the offending message and then \"Apps > Report to Modmail\". Alternatively, click the \"Create new Modmail\" button below (soon).

When creating a rule-breaking report please give a brief description of what is happening along with relevant information, such as members involved, links to offending messages, and a summary of the situation.

The modmail will materialize itself as a private thread under this channel with a random ID. You will be pinged in the thread once the report is opened. Once the report is dealt with, it will be archived")
					.button(
						serenity::CreateButton::new("rplcs_create_new_modmail")
							.label("Create New Modmail")
							.emoji(serenity::ReactionType::Unicode("📩".to_string()))
							.style(serenity::ButtonStyle::Primary),
					),
			)
			.await?
	};

	// Cache the message in the Data struct
	store_message(data, message).await;

	Ok(())
}

/// It's important to keep this in a function because we're dealing with lifetimes and guard drops.
async fn store_message(data: &Data, message: serenity::Message) {
	info!("Storing modlog message on cache.");
	let mut rwguard = data.modmail_message.write().await;
	rwguard.get_or_insert(message);
}

pub async fn create_modmail_thread(
	http: impl serenity::CacheHttp,
	user_message: impl Into<String>,
	data: &Data,
	user_id: UserId,
) -> Result<GuildChannel, Error> {
	load_or_create_modmail_message(&http, data).await?;

	let modmail_message = data
		.modmail_message
		.read()
		.await
		.clone()
		.ok_or(anyhow!("Modmail message somehow ceased to exist"))?;

	let modmail_channel = modmail_message
		.channel(&http)
		.await
		.context("Failed to fetch modmail channel")?
		.guild()
		.ok_or_else(|| anyhow!("Modmail channel is not in a guild"))?;

	let modmail_name = format!("Modmail #{}", rand::rng().random_range(1..10000));

	let mut modmail_thread = modmail_channel
		.create_thread(
			&http,
			serenity::CreateThread::new(modmail_name).kind(serenity::ChannelType::PrivateThread),
		)
		.await?;

	// disallow users from inviting others to modmail threads
	modmail_thread
		.edit_thread(&http, EditThread::new().invitable(false))
		.await?;

	let thread_message_content = format!(
		"Hey {}, {} needs help with the following:\n> {}",
		data.mod_role_id.mention(),
		user_id.mention(),
		user_message.into()
	);

	modmail_thread
		.send_message(
			&http,
			serenity::CreateMessage::new()
				.content(thread_message_content)
				.allowed_mentions(
					serenity::CreateAllowedMentions::new()
						.users([user_id])
						.roles([data.mod_role_id]),
				),
		)
		.await?;

	Ok(modmail_thread)
}
