use anyhow::Error;
use poise::{serenity_prelude as serenity, Event};
use shuttle_poise::ShuttlePoise;
use shuttle_secrets::SecretStore;
use tracing::{debug, info, warn};

pub mod checks;
pub mod commands;
pub mod helpers;
pub mod types;

#[shuttle_runtime::main]
async fn poise(
	#[shuttle_secrets::Secrets] secret_store: SecretStore,
) -> ShuttlePoise<types::Data, Error> {
	let framework = poise::Framework::builder()
		.token(secret_store.get("DISCORD_TOKEN").unwrap())
		.setup(move |ctx, ready, framework| {
			Box::pin(async move {
				let modmail_message = commands::modmail::setup_modmail(ctx, 0).await?;

				let data = types::Data::new(&secret_store, modmail_message);

				info!("rustbot logged in as {}", ready.user.name);

				debug!("Registering commands...");
				poise::builtins::register_in_guild(
					ctx,
					&framework.options().commands,
					data.discord_guild_id,
				)
				.await?;

				debug!("Setting activity text");
				ctx.set_activity(serenity::Activity::listening("/help"))
					.await;

				Ok(data)
			})
		})
		.options(poise::FrameworkOptions {
			commands: vec![
				commands::playground::play(),
				commands::playground::playwarn(),
				commands::playground::eval(),
				commands::playground::miri(),
				commands::playground::expand(),
				commands::playground::clippy(),
				commands::playground::fmt(),
				commands::playground::microbench(),
				commands::playground::procmacro(),
				commands::godbolt::godbolt(),
				commands::godbolt::mca(),
				commands::godbolt::llvmir(),
				commands::godbolt::targets(),
				commands::crates::crate_(),
				commands::crates::doc(),
				commands::moderation::cleanup(),
				commands::moderation::ban(),
				commands::misc::go(),
				commands::misc::source(),
				commands::misc::help(),
				commands::misc::register(),
				commands::misc::uptime(),
				commands::misc::conradluget(),
			],
			prefix_options: poise::PrefixFrameworkOptions {
				prefix: Some("?".into()),
				additional_prefixes: vec![
					poise::Prefix::Literal("🦀 "),
					poise::Prefix::Literal("🦀"),
					poise::Prefix::Literal("<:ferris:358652670585733120> "),
					poise::Prefix::Literal("<:ferris:358652670585733120>"),
					poise::Prefix::Regex(
						"(yo|hey) (crab|ferris|fewwis),? can you (please |pwease )?"
							.parse()
							.unwrap(),
					),
				],
				edit_tracker: Some(poise::EditTracker::for_timespan(
					std::time::Duration::from_secs(60 * 5), // 5 minutes
				)),
				..Default::default()
			},
			/// The global error handler for all error cases that may occur
			on_error: |error| {
				Box::pin(async move {
					warn!("Encountered error: {:?}", error);
					if let poise::FrameworkError::ArgumentParse { error, ctx, .. } = error {
						let response = if error.is::<poise::CodeBlockError>() {
							"\
Missing code block. Please use the following markdown:
\\`code here\\`
or
\\`\\`\\`rust
code here
\\`\\`\\`"
								.to_owned()
						} else if let Some(multiline_help) = ctx.command().help_text {
							format!("**{}**\n{}", error, multiline_help())
						} else {
							error.to_string()
						};

						if let Err(e) = ctx.say(response).await {
							warn!("{}", e)
						}
					} else if let poise::FrameworkError::Command { ctx, error } = error {
						if let Err(e) = ctx.say(error.to_string()).await {
							warn!("{}", e)
						}
					}
				})
			},
			/// This code is run before every command
			pre_command: |ctx| {
				Box::pin(async move {
					let channel_name = &ctx
						.channel_id()
						.name(&ctx)
						.await
						.unwrap_or_else(|| "<unknown>".to_owned());
					let author = &ctx.author().name;

					info!(
						"{} in {} used slash command '{}'",
						author,
						channel_name,
						&ctx.invoked_command_name()
					);
				})
			},
			/// This code is run after a command if it was successful (returned Ok)
			post_command: |ctx| {
				Box::pin(async move {
					println!("Executed command {}!", ctx.command().qualified_name);
				})
			},
			/// Every command invocation must pass this check to continue execution
			command_check: Some(|_ctx| Box::pin(async move { Ok(true) })),
			/// Enforce command checks even for owners (enforced by default)
			/// Set to true to bypass checks, which is useful for testing
			skip_checks_for_owners: false,
			event_handler: |ctx, event, _framework, data| {
				Box::pin(async move {
					debug!("Got an event in event handler: {:?}", event.name());

					match event {
						Event::GuildMemberAddition { new_member } => {
							const RUSTIFICATION_DELAY: u64 = 30; // in minutes

							tokio::time::sleep(std::time::Duration::from_secs(
								RUSTIFICATION_DELAY * 60,
							))
							.await;

							// Ignore errors because the user may have left already
							let _: Result<_, _> = ctx
								.http
								.add_member_role(
									new_member.guild_id.0,
									new_member.user.id.0,
									data.rustacean_role.0,
									Some(&format!(
										"Automatically rustified after {} minutes",
										RUSTIFICATION_DELAY
									)),
								)
								.await;
						}
						Event::MessageUpdate {
							old_if_available,
							new,
							event,
						} => {}
						_ => {}
					}

					Ok(())
				})
			},
			..Default::default()
		})
		.intents(serenity::GatewayIntents::all())
		.build()
		.await
		.map_err(shuttle_runtime::CustomError::new)?;

	Ok(framework.into())
}
