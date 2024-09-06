use std::sync::LazyLock;

use anyhow::{anyhow, Error};
use poise::serenity_prelude as serenity;
use poise::serenity_prelude::Timestamp;

use crate::types::Context;

/// Evaluates Go code
#[poise::command(
	prefix_command,
	slash_command,
	category = "Utilities",
	discard_spare_arguments
)]
pub async fn go(ctx: Context<'_>) -> Result<(), Error> {
	use rand::Rng as _;
	if rand::thread_rng().gen_bool(0.01) {
		ctx.say("Yes").await?;
	} else {
		ctx.say("No").await?;
	}
	Ok(())
}

/// Links to the bot GitHub repo
#[poise::command(
	prefix_command,
	slash_command,
	category = "Utilities",
	discard_spare_arguments
)]
pub async fn source(ctx: Context<'_>) -> Result<(), Error> {
	ctx.say("https://github.com/rust-community-discord/ferrisbot-for-discord")
		.await?;
	Ok(())
}

/// Show this menu
#[poise::command(prefix_command, slash_command, category = "Utilities", track_edits)]
pub async fn help(
	ctx: Context<'_>,
	#[description = "Specific command to show help about"]
	#[autocomplete = "poise::builtins::autocomplete_command"]
	command: Option<String>,
) -> Result<(), Error> {
	let extra_text_at_bottom = "\
You can still use all commands with `?`, even if it says `/` above.
Type ?help command for more info on a command.
You can edit your message to the bot and the bot will edit its response.";

	poise::builtins::help(
		ctx,
		command.as_deref(),
		poise::builtins::HelpConfiguration {
			extra_text_at_bottom,
			ephemeral: true,
			..Default::default()
		},
	)
	.await?;
	Ok(())
}

/// Register slash commands in this guild or globally
#[poise::command(
	prefix_command,
	slash_command,
	category = "Utilities",
	hide_in_help,
	check = "crate::checks::check_is_moderator"
)]
pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
	poise::builtins::register_application_commands_buttons(ctx).await?;

	Ok(())
}

/// Tells you how long the bot has been up for
#[poise::command(prefix_command, slash_command, category = "Utilities")]
pub async fn uptime(ctx: Context<'_>) -> Result<(), Error> {
	let uptime = ctx.data().bot_start_time.elapsed();

	let div_mod = |a, b| (a / b, a % b);

	let seconds = uptime.as_secs();
	let (minutes, seconds) = div_mod(seconds, 60);
	let (hours, minutes) = div_mod(minutes, 60);
	let (days, hours) = div_mod(hours, 24);

	ctx.say(format!("Uptime: {days}d {hours}h {minutes}m {seconds}s"))
		.await?;

	Ok(())
}

/// Use this joke command to have Conrad Ludgate tell you to get something
///
/// Example: `/conradluget a better computer`
#[poise::command(
	prefix_command,
	slash_command,
	category = "Utilities",
	track_edits,
	hide_in_help
)]
pub async fn conradluget(
	ctx: Context<'_>,
	#[description = "Get what?"]
	#[rest]
	text: String,
) -> Result<(), Error> {
	static BASE_IMAGE: LazyLock<image::DynamicImage> = LazyLock::new(|| {
		image::io::Reader::with_format(
			std::io::Cursor::new(&include_bytes!("../../assets/conrad.png")[..]),
			image::ImageFormat::Png,
		)
		.decode()
		.expect("failed to load image")
	});
	static FONT: LazyLock<rusttype::Font> = LazyLock::new(|| {
		rusttype::Font::try_from_bytes(include_bytes!("../../assets/OpenSans.ttf"))
			.expect("failed to load font")
	});

	let text = format!("Get {text}");
	let image = imageproc::drawing::draw_text(
		&*BASE_IMAGE,
		image::Rgba([201, 209, 217, 255]),
		57,
		286,
		rusttype::Scale::uniform(65.0),
		&FONT,
		&text,
	);

	let mut img_bytes = Vec::with_capacity(200_000); // preallocate 200kB for the img
	image::DynamicImage::ImageRgba8(image).write_to(
		&mut std::io::Cursor::new(&mut img_bytes),
		image::ImageOutputFormat::Png,
	)?;

	let filename = text + ".png";

	let attachment = serenity::CreateAttachment::bytes(img_bytes, filename);

	ctx.channel_id()
		.send_files(ctx, vec![attachment], serenity::CreateMessage::new())
		.await?;

	Ok(())
}

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
	prefix_command,
	slash_command,
	category = "Utilities",
	on_error = "crate::helpers::acknowledge_fail"
)]
pub async fn cleanup(
	ctx: Context<'_>,
	#[description = "Number of messages to delete"] num_messages: Option<usize>,
) -> Result<(), Error> {
	let num_messages = num_messages.unwrap_or(1);

	let messages_to_delete = ctx
		.channel_id()
		.messages(&ctx, serenity::GetMessages::new().limit(20))
		.await?
		.into_iter()
		.filter(|msg| {
			(msg.author.id == ctx.data().application_id)
				&& (*ctx.created_at() - *msg.timestamp).num_hours() < 24
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
	prefix_command,
	slash_command,
	category = "Utilities",
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
		banned_user.user.name,
		crate::helpers::custom_emoji_code(ctx, "ferrisBanne", '🔨').await
	))
	.await?;
	Ok(())
}

/// Self-timeout yourself.
///
/// /selftimeout [duration_in_hours] [duration_in_minutes]
///
/// Self-timeout yourself.
/// You can specify how long you want to timeout yourself for, either in hours
/// or in minutes.
#[poise::command(
	slash_command,
	category = "Utilities",
	on_error = "crate::helpers::acknowledge_fail"
)]
pub async fn selftimeout(
	ctx: Context<'_>,
	#[description = "Duration of self-timeout in hours"] duration_in_hours: Option<u64>,
	#[description = "Duration of self-timeout in minutes"] duration_in_minutes: Option<u64>,
) -> Result<(), Error> {
	let total_seconds = match (duration_in_hours, duration_in_minutes) {
		(None, None) => 3600, // When nothing is specified, default to one hour.
		(hours, minutes) => hours.unwrap_or(0) * 3600 + minutes.unwrap_or(0) * 60,
	};

	let now = ctx.created_at().unix_timestamp();

	let then = Timestamp::from_unix_timestamp(now + total_seconds as i64)?;

	let mut member = ctx
		.author_member()
		.await
		.ok_or(anyhow!("failed to fetch member"))?
		.into_owned();

	member
		.disable_communication_until_datetime(&ctx, then)
		.await?;

	ctx.say(format!(
		"Self-timeout for {}. They'll be able to interact with the server again <t:{}:R>. \
		If this was a mistake, please contact a moderator or try to enjoy the time off.",
		ctx.author().name,
		then.unix_timestamp()
	))
	.await?;

	Ok(())
}
