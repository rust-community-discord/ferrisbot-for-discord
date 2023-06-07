use poise::serenity_prelude::CacheHttp;

use crate::{Context, Error, serenity};

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
on_error = "crate::acknowledge_fail",
)]
pub async fn cleanup(
    ctx: Context<'_>,
    #[description = "Number of messages to delete"] num_messages: Option<usize>,
) -> Result<(), Error> {
    let num_messages = num_messages.unwrap_or(1);

    let messages_to_delete = ctx
        .channel_id()
        .messages(ctx.http(), serenity::GetMessages::new().limit(20))
        .await?
        .into_iter()
        .filter(|msg| {
            if msg.author.id != ctx.data().bot_user_id {
                return false;
            }
            if (*ctx.created_at() - *msg.timestamp).num_hours() >= 24 {
                return false;
            }
            true
        })
        .take(num_messages);

    ctx.channel_id()
        .delete_messages(ctx.http(), messages_to_delete)
        .await?;

    crate::acknowledge_success(ctx, "rustOk", '👌').await
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
on_error = "crate::acknowledge_fail",
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
        crate::custom_emoji_code(ctx, "ferrisBanne", '🔨').await
    ))
        .await?;
    Ok(())
}

/// Move a discussion to another channel
///
/// Move a discussion to a specified channel, optionally pinging a list of users in the new channel.
#[poise::command(
slash_command,
rename = "move",
aliases("migrate"),
category = "Moderation"
)]
pub async fn move_(
    ctx: Context<'_>,
    #[description = "Where to move the discussion"] target_channel: serenity::GuildChannel,
    #[description = "Participants of the discussion who will be pinged in the new channel"]
    users_to_ping: Vec<serenity::Member>,
) -> Result<(), Error> {
    use serenity::Mentionable as _;

    if Some(target_channel.guild_id) != ctx.guild_id() {
        return Err("Can't move discussion across servers".into());
    }

    // DON'T use GuildChannel::permissions_for_user - it requires member to be cached
    let guild_id = ctx.guild_id().ok_or("Guild not in cache")?;
    let member = guild_id.member(ctx.discord(), ctx.author().id).await?;
    let permissions_in_target_channel = guild_id
        .to_partial_guild(ctx.discord())
        .await?
        .user_permissions_in(&target_channel, &member)?;
    if !permissions_in_target_channel.send_messages() {
        return Err(format!(
            "You don't have permission to post in {}",
            target_channel.mention(),
        )
            .into());
    }

    let source_msg_link = match ctx {
        Context::Prefix(ctx) => ctx.msg.link_ensured(ctx.discord).await,
        _ => latest_message_link(ctx).await,
    };

    let mut comefrom_message = format!(
        "**Discussion moved here from {}**\n{}",
        ctx.channel_id().mention(),
        source_msg_link
    );
    if let Context::Prefix(ctx) = ctx {
        if let Some(referenced_message) = &ctx.msg.referenced_message {
            comefrom_message += "\n> ";
            comefrom_message += &referenced_message.content;
        }
    }

    {
        let mut users_to_ping = users_to_ping.iter();
        if let Some(user_to_ping) = users_to_ping.next() {
            comefrom_message += &format!("\n{}", user_to_ping.mention());
            for user_to_ping in users_to_ping {
                comefrom_message += &format!(", {}", user_to_ping.mention());
            }
        }
    }

    // let comefrom_message = target_channel.say(ctx.discord, comefrom_message).await?;
    let comefrom_message = target_channel
        .send_message(
            ctx.discord(),
            serenity::CreateMessage::new()
                .content(comefrom_message)
                .allowed_mentions(serenity::CreateAllowedMentions::new().users(users_to_ping)),
        )
        .await?;

    ctx.say(format!(
        "**{} suggested to move this discussion to {}**\n{}",
        &ctx.author().tag(),
        target_channel.mention(),
        comefrom_message.link_ensured(ctx.discord()).await
    ))
        .await?;

    Ok(())
}

async fn check_is_moderator(ctx: Context<'_>) -> Result<bool, Error> {
    // Retrieve via HTTP to make sure it's up-to-date
    let guild = ctx.guild_id().0.ok_or("This command only works inside guilds")?;

    let author = ctx
        .http()
        .get_member(
            guild,
            ctx.author().id.0,
        )
        .await?;

    let user_has_moderator_role = author.roles.contains(&ctx.data().mod_role_id);
    if user_has_moderator_role {
        Ok(true)
    } else {
        ctx.send(
            poise::CreateReply::new()
                .content("This command is only available to moderators")
                .ephemeral(true),
        )
            .await?;
        Ok(false)
    }
}
