use anyhow::Error;
use poise::serenity_prelude as serenity;

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
#[poise::command(slash_command, category = "Utilities", discard_spare_arguments)]
pub async fn source(ctx: Context<'_>) -> Result<(), Error> {
	ctx.say("https://github.com/rust-community-discord/rustbot")
		.await?;
	Ok(())
}

/// Show this menu
#[poise::command(slash_command, category = "Utilities", track_edits)]
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
#[poise::command(slash_command, category = "Utilities")]
pub async fn uptime(ctx: Context<'_>) -> Result<(), Error> {
	let uptime = std::time::Instant::now() - ctx.data().bot_start_time;

	let div_mod = |a, b| (a / b, a % b);

	let seconds = uptime.as_secs();
	let (minutes, seconds) = div_mod(seconds, 60);
	let (hours, minutes) = div_mod(minutes, 60);
	let (days, hours) = div_mod(hours, 24);

	ctx.say(format!(
		"Uptime: {}d {}h {}m {}s",
		days, hours, minutes, seconds
	))
	.await?;

	Ok(())
}

/// Use this joke command to have Conrad Ludgate tell you to get something
///
/// Example: `/conradluget a better computer`
#[poise::command(slash_command, category = "Utilities", track_edits, hide_in_help)]
pub async fn conradluget(
	ctx: Context<'_>,
	#[description = "Get what?"]
	#[rest]
	text: String,
) -> Result<(), Error> {
	use once_cell::sync::Lazy;
	static BASE_IMAGE: Lazy<image::DynamicImage> = Lazy::new(|| {
		image::io::Reader::with_format(
			std::io::Cursor::new(&include_bytes!("../../assets/conrad.png")[..]),
			image::ImageFormat::Png,
		)
		.decode()
		.expect("failed to load image")
	});
	static FONT: Lazy<rusttype::Font> = Lazy::new(|| {
		rusttype::Font::try_from_bytes(include_bytes!("../../assets/OpenSans.ttf"))
			.expect("failed to load font")
	});

	let text = format!("Get {}", text);
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

	let attachment: serenity::AttachmentType = (&img_bytes[..], filename.as_ref()).into();

	ctx.channel_id()
		.send_files(ctx, vec![attachment], |message| message)
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
