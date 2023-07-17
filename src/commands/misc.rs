use anyhow::Error;

use crate::types::Context;

/// Evaluates Go code
#[poise::command(prefix_command, category = "Miscellaneous", discard_spare_arguments)]
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
#[poise::command(slash_command, category = "Miscellaneous", discard_spare_arguments)]
pub async fn source(ctx: Context<'_>) -> Result<(), Error> {
	ctx.say("https://github.com/rust-community-discord/rustbot")
		.await?;
	Ok(())
}

/// Show this menu
#[poise::command(slash_command, category = "Miscellaneous", track_edits)]
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
	category = "Miscellaneous",
	hide_in_help,
	check = "crate::checks::check_is_moderator"
)]
pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
	poise::builtins::register_application_commands_buttons(ctx).await?;

	Ok(())
}

/// Tells you how long the bot has been up for
#[poise::command(slash_command, category = "Miscellaneous")]
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
/// Example: `?conradluget a better computer`
#[poise::command(slash_command, category = "Miscellaneous", track_edits, hide_in_help)]
pub async fn conradluget(
	_ctx: Context<'_>,
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

	// TODO: fix the below command
	// ctx.send(
	// 	poise::CreateReply::new()
	// 		.attachment(serenity::AttachmentType::from(serenity::CreateEmbed(img_bytes, text + ".png"))),
	// )
	// 	.await?;

	Ok(())
}
