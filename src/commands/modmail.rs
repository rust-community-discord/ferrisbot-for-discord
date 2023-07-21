use crate::types::{Context, Data};
use anyhow::{anyhow, Error};
use poise::serenity_prelude as serenity;
use poise::serenity_prelude::Mentionable;
use tracing::{debug, info};

pub async fn load_or_create_modmail_message(
	http: impl AsRef<serenity::Http>,
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
		.to_channel(http.as_ref())
		.await?
		.guild()
		.ok_or(anyhow!("This command can only be used in a guild"))?;

	// Fetch the report message itself
	let open_report_message = modmail_guild_channel
		.messages(http.as_ref(), |get_messages| get_messages.limit(1))
		.await?
		.get(0)
		.cloned();

	let message = if let Some(desired_message) = open_report_message {
		// If it exists, return it
		desired_message
	} else {
		// If it doesn't exist, create one and return it
		debug!("Creating new modmail message");
		modmail_guild_channel
				.send_message(http.as_ref(), |create_message| {
					create_message.content("\
This is the Modmail channel. In here, you're able to create modmail reports to reach out to the Moderators about things such as reporting rule breaking, or asking a private question.

To open a ticket, either right click the offending message and then \"Apps > Report to Modmail\". Alternatively, click the \"Create new Modmail\" button below (soon).

When creating a rule-breaking report please give a brief description of what is happening along with relevant information, such as members involved, links to offending messages, and a summary of the situation.

The modmail will materialize itself as a private thread under this channel with a random ID. You will be pinged in the thread once the report is opened. Once the report is dealt with, it will be archived"
					)
						.components(|create_components| {
						create_components.create_action_row(|create_action_row| {
							create_action_row.create_button(|create_button| {
								create_button
									.label("Create New Modmail")
									.style(serenity::ButtonStyle::Primary)
									.custom_id("rplcs_create_new_modmail")
							})
						})
					})
				})
				.await?
	};

	// Cache the message in the Data struct
	store_message(data, message).await;

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
#[poise::command(slash_command, ephemeral, category = "Modmail")]
pub async fn modmail(
	ctx: Context<'_>,
	#[description = "What did the user do wrong?"] reason: String,
) -> Result<(), Error> {
	load_or_create_modmail_message(ctx, ctx.data()).await?;

	let modmail_message = ctx
		.data()
		.modmail_message
		.read()
		.await
		.clone()
		.ok_or(anyhow!("Modmail message somehow ceased to exist"))?;

	let modmail_channel = modmail_message
		.channel(ctx)
		.await?
		.guild()
		.ok_or(anyhow!("Modmail channel is not in a guild!"))?;

	let modmail_name = format!("Modmail #{}", ctx.id() % 10000);

	let modmail_thread = modmail_channel
		.create_private_thread(ctx, |create_thread| create_thread.name(modmail_name))
		.await?;

	let thread_message_content = format!(
		"Hey <@&{}>, <@{}> needs help in channel {}: {}\n> {}",
		ctx.data().mod_role_id,
		ctx.author().id,
		ctx.channel_id().mention(),
		latest_message_link(ctx).await,
		reason
	);

	modmail_thread
		.send_message(ctx, |create_message| {
			create_message
				.content(thread_message_content)
				.allowed_mentions(|create_allowed_mentions| {
					create_allowed_mentions
						.users([ctx.author().id])
						.roles([ctx.data().mod_role_id])
				})
		})
		.await?;

	ctx.say(format!(
		"Successfully sent your message to the moderators. Check out the responses here: {}",
		modmail_thread.mention()
	))
	.await?;

	Ok(())
}

async fn latest_message_link(ctx: Context<'_>) -> String {
	let message = ctx
		.channel_id()
		.messages(ctx, |get_messages| get_messages.limit(1))
		.await
		.ok()
		.and_then(|messages| messages.into_iter().next());

	match message {
		Some(msg) => msg.link_ensured(ctx).await,
		None => "<couldn't retrieve latest message link>".into(),
	}
}

/// It's important to keep this in a function because we're dealing with lifetimes and guard drops.
async fn store_message(data: &Data, message: serenity::Message) {
	info!("Storing modlog message on cache.");
	let mut rwguard = data.modmail_message.write().await;
	rwguard.get_or_insert(message);
}
