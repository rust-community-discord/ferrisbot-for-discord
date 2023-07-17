use anyhow::{anyhow, bail, Error};
use poise::serenity_prelude as serenity;

pub async fn setup_modmail<T>(
	ctx: &serenity::Context,
	modmail_channel_id: T,
) -> Result<serenity::Message, Error>
where
	T: Into<serenity::ChannelId>,
{
	let guild = ctx.guild().ok_or(anyhow!("Couldn't find guild."))?;

	let modmail_channel = guild
		.channels
		.get(&modmail_channel_id.into())
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
					create_message.components(|create_components| {
						create_components.create_action_row(|create_action_row| {
							create_action_row.create_button(|create_button| {
								create_button
									.label("Create New Modmail")
									.style(serenity::ButtonStyle::Primary)
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

/// Register slash commands in this guild or globally
#[poise::command(
	slash_command,
	category = "Miscellaneous",
	hide_in_help,
	check = "crate::checks::check_is_moderator"
)]
pub async fn modmail(ctx: Context<'_>) -> Result<(), Error> {
	poise::builtins::register_application_commands_buttons(ctx).await?;

	Ok(())
}
