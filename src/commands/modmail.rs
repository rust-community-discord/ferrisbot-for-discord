use crate::types::Context;
use anyhow::{anyhow, bail, Error};
use poise::serenity_prelude as serenity;

pub async fn fetch_or_create_modmail_message(ctx: Context<'_>) -> Result<serenity::Message, Error> {
	let guild = ctx
		.guild()
		.ok_or(anyhow!("This command can only be used in a guild"))?;

	let modmail_channel = guild
		.channels
		.get(&ctx.data().modmail_channel_id.into())
		.ok_or(anyhow!("Failed to find modmail channel."))?;

	let message = if let serenity::Channel::Guild(guild_channel) = modmail_channel {
		let open_report_message = guild_channel
			.messages(ctx, |get_messages| get_messages.limit(1))
			.await?
			.get(0)
			.cloned();

		if let Some(desired_message) = open_report_message {
			desired_message
		} else {
			guild_channel
				.send_message(ctx, |create_message| {
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
		}
	} else {
		bail!("Modmail channel ID isn't a guild channel!");
	};

	Ok(message)
}

/// Discreetly reports a user for breaking the rules
///
/// Call this command in a channel when someone might be breaking the rules, for example by being \
/// very rude, or starting discussions about divisive topics like politics and religion. Nobody \
/// will see that you invoked this command.
///
/// Your report, along with a link to the \
/// channel and its most recent message, will show up in a dedicated reports channel for \
/// moderators, and it allows them to deal with it much faster than if you were to DM a \
/// potentially AFK moderator.
///
/// You can still always ping the Moderator role if you're comfortable doing so.
#[poise::command(slash_command, ephemeral, category = "Modmail")]
pub async fn report(
	ctx: Context<'_>,
	#[description = "What did the user do wrong?"] reason: String,
) -> Result<(), Error> {
	let report_message = fetch_or_create_modmail_message(ctx).await?;

	let reports_channel = report_message.channel(ctx).await?;

	let naughty_channel = ctx
		.guild()
		.ok_or(anyhow!("This command can only be used in a guild"))?;

	let report_name = format!("Report {}", ctx.id() % 10000);

	let report_thread = match reports_channel {
		serenity::Channel::Guild(reports_guild_channel) => {
			reports_guild_channel
				.create_private_thread(ctx, |create_thread| create_thread.name(report_name))
				.await?
		}
		_ => bail!("Report thread is not in a guild!"),
	};

	let thread_message_content = format!(
		"Hey <@&{}>, <@{}> sent a report from channel {}: {}\n> {}",
		ctx.data().mod_role_id,
		ctx.author().id,
		naughty_channel.name,
		latest_message_link(ctx).await,
		reason
	);

	report_thread
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

	ctx.say("Successfully sent report. Thanks for helping to make this community a better place!")
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