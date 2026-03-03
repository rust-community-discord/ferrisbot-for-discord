#![warn(rust_2018_idioms, clippy::pedantic)]
#![allow(
	clippy::too_many_lines,
	clippy::missing_errors_doc,
	clippy::missing_panics_doc,
	clippy::cast_possible_wrap,
	clippy::module_name_repetitions,
	clippy::assigning_clones, // Too many false triggers
)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Error, anyhow};
use futures::StreamExt;
use poise::serenity_prelude::{self as serenity, ChannelType, Permissions};
use rand::{Rng, seq::IteratorRandom};
use tracing::{debug, info, warn};

use crate::commands::modmail::{create_modmail_thread, load_or_create_modmail_message};
use crate::types::Data;

const FAILED_CODEBLOCK: &str = "\\
Missing code block. Please use the following markdown:
`` `code here` ``
or
```ansi
`\x1b[0m`\x1b[0m`rust
code here
`\x1b[0m`\x1b[0m`
```";

pub mod checks;
pub mod commands;
pub mod helpers;
pub mod types;

pub struct SecretStore(pub HashMap<String, String>);

impl SecretStore {
	#[must_use]
	pub fn get(&self, key: &str) -> Option<String> {
		self.0.get(key).cloned()
	}

	/// Gets a secret and parses it as a Discord ID (u64).
	///
	/// # Errors
	/// Returns an error if the key is missing or the value cannot be parsed as u64.
	pub fn get_discord_id(&self, key: &str) -> Result<u64, Error> {
		self.get(key)
			.ok_or_else(|| anyhow!("Failed to get '{key}' from the secret store"))?
			.parse::<u64>()
			.map_err(|e| anyhow!("Failed to parse '{key}' as u64: {e}"))
	}
}

pub struct ShuttleSerenity(pub serenity::Client);

impl From<serenity::Client> for ShuttleSerenity {
	fn from(value: serenity::Client) -> Self {
		Self(value)
	}
}

pub async fn serenity(
	secret_store: SecretStore,
	database: Option<sqlx::SqlitePool>,
) -> Result<ShuttleSerenity, Error> {
	let token = secret_store
		.get("DISCORD_TOKEN")
		.expect("Couldn't find your DISCORD_TOKEN!");

	let enable_database = database.is_some();
	let command_list = build_command_list(enable_database);

	if enable_database {
		info!("Database enabled - registering database-dependent commands (tags, highlights)");
	} else {
		info!("Database disabled - skipping database-dependent commands");
	}

	let framework = poise::Framework::builder()
		.setup(move |ctx, ready, framework| {
			Box::pin(async move {
				let data = Data::new(&secret_store, database).await?;

				info!(
					"Registering {} commands...",
					framework.options().commands.len()
				);
				poise::builtins::register_in_guild(
					ctx,
					&framework.options().commands,
					data.discord_guild_id,
				)
				.await?;

				debug!("Setting activity text");
				ctx.set_activity(Some(serenity::ActivityData::listening("/help")));

				load_or_create_modmail_message(ctx, &data).await?;

				info!("ferrisbot-for-discord logged in as {}", ready.user.name);
				Ok(data)
			})
		})
		.options(poise::FrameworkOptions {
			commands: command_list,
			prefix_options: poise::PrefixFrameworkOptions {
				prefix: Some("?".into()),
				additional_prefixes: vec![
					poise::Prefix::Literal("🦀 "),
					poise::Prefix::Literal("🦀"),
					poise::Prefix::Literal("<:ferris:358652670585733120> "),
					poise::Prefix::Literal("<:ferris:358652670585733120>"),
					poise::Prefix::Literal("<:ferrisballSweat:678714352450142239> "),
					poise::Prefix::Literal("<:ferrisballSweat:678714352450142239>"),
					poise::Prefix::Literal("<:ferrisCat:1183779700485664820> "),
					poise::Prefix::Literal("<:ferrisCat:1183779700485664820>"),
					poise::Prefix::Literal("<:ferrisOwO:579331467000283136> "),
					poise::Prefix::Literal("<:ferrisOwO:579331467000283136>"),
					poise::Prefix::Regex(
						"(yo |hey )?(crab|ferris|fewwis),? can you (please |pwease )?"
							.parse()
							.unwrap(),
					),
				],
				edit_tracker: Some(Arc::new(poise::EditTracker::for_timespan(
					Duration::from_secs(60 * 5), // 5 minutes
				))),
				..Default::default()
			},
			// The global error handler for all error cases that may occur
			on_error: |error| {
				Box::pin(async move {
					warn!("Encountered error: {:?}", error);
					if let poise::FrameworkError::ArgumentParse { error, ctx, .. } = &error {
						let response = if error.is::<poise::CodeBlockError>() {
							FAILED_CODEBLOCK.to_owned()
						} else if let Some(multiline_help) = &ctx.command().help_text {
							format!("**{error}**\n{multiline_help}")
						} else {
							error.to_string()
						};

						try_say(ctx, response).await;
					} else if let poise::FrameworkError::Command { ctx, error, .. } = &error {
						if error.is::<poise::CodeBlockError>() {
							try_say(ctx, FAILED_CODEBLOCK).await;
						}
						try_say(ctx, error.to_string()).await;
					}
				})
			},
			// This code is run before every command
			pre_command: |ctx| {
				Box::pin(async move {
					let channel_name = &ctx
						.channel_id()
						.name(&ctx)
						.await
						.unwrap_or_else(|_| "<unknown>".to_owned());
					let author = &ctx.author().name;

					info!(
						"{} in {} used slash command '{}'",
						author,
						channel_name,
						&ctx.invoked_command_name()
					);
				})
			},
			// This code is run after a command if it was successful (returned Ok)
			post_command: |ctx| {
				Box::pin(async move {
					info!("Executed command {}!", ctx.command().qualified_name);
				})
			},
			// Every command invocation must pass this check to continue execution
			command_check: Some(|_ctx| Box::pin(async move { Ok(true) })),
			// Enforce command checks even for owners (enforced by default)
			// Set to true to bypass checks, which is useful for testing
			skip_checks_for_owners: false,
			event_handler: |ctx, event, _framework, data| {
				Box::pin(async move { event_handler(ctx, event, data).await })
			},
			// Disallow all mentions (except those to the replied user) by default
			allowed_mentions: Some(serenity::CreateAllowedMentions::new().replied_user(true)),
			..Default::default()
		})
		.build();

	// Don't include presence updates, as they consume a lot of memory and CPU.
	let intents = serenity::GatewayIntents::non_privileged()
		| serenity::GatewayIntents::GUILD_MEMBERS
		| serenity::GatewayIntents::MESSAGE_CONTENT;

	let client = serenity::ClientBuilder::new(token, intents)
		.framework(framework)
		.await
		.map_err(|e| anyhow!(e))?;

	Ok(client.into())
}

fn build_command_list(enable_database: bool) -> Vec<poise::Command<Data, Error>> {
	let mut command_list = vec![
		commands::man::man(),
		commands::crates::crate_(),
		commands::crates::doc(),
		commands::godbolt::godbolt(),
		commands::godbolt::mca(),
		commands::godbolt::llvmir(),
		commands::godbolt::targets(),
		commands::utilities::go(),
		commands::utilities::source(),
		commands::utilities::help(),
		commands::utilities::register(),
		commands::utilities::uptime(),
		commands::utilities::conradluget(),
		commands::utilities::cleanup(),
		commands::utilities::purge(),
		commands::utilities::ban(),
		commands::utilities::selftimeout(),
		commands::utilities::solved(),
		commands::utilities::edit(),
		commands::thread_pin::thread_pin(),
		commands::modmail::modmail(),
		commands::modmail::modmail_context_menu_for_message(),
		commands::modmail::modmail_context_menu_for_user(),
		commands::moving::move_messages_context_menu(),
		commands::playground::play(),
		commands::playground::playwarn(),
		commands::playground::eval(),
		commands::playground::miri(),
		commands::playground::expand(),
		commands::playground::clippy(),
		commands::playground::fmt(),
		commands::playground::microbench(),
		commands::playground::procmacro(),
	];
	if enable_database {
		command_list.extend([
			commands::highlight::highlight(),
			commands::tags::tags(),
			commands::tags::tag(),
		]);
	}
	command_list
}

/// Attempts to send a message, logging any failures.
/// This is useful for error handling paths where we don't want to fail the entire operation
/// if we can't send an error message.
async fn try_say(ctx: &poise::Context<'_, Data, Error>, message: impl Into<String>) {
	let msg = message.into();
	if let Err(e) = ctx.say(&msg).await {
		warn!(
			"Failed to send message '{}...': {}",
			&msg[..msg.len().min(50)],
			e
		);
	}
}

async fn event_handler(
	ctx: &serenity::Context,
	event: &serenity::FullEvent,
	data: &Data,
) -> Result<(), Error> {
	debug!(
		"Got an event in event handler: {:?}",
		event.snake_case_name()
	);

	match event {
		serenity::FullEvent::GuildMemberAddition { new_member } => {
			const RUSTIFICATION_DELAY: u64 = 30; // in minutes

			tokio::time::sleep(Duration::from_secs(RUSTIFICATION_DELAY * 60)).await;

			// Ignore errors because the user may have left already
			let _: Result<_, _> = ctx
				.http
				.add_member_role(
					new_member.guild_id,
					new_member.user.id,
					data.rustacean_role_id,
					Some(&format!(
						"Automatically rustified after {RUSTIFICATION_DELAY} minutes"
					)),
				)
				.await;
		}
		serenity::FullEvent::Ready { .. } => {
			let http = ctx.http.clone();
			tokio::spawn(init_server_icon_changer(http, data.discord_guild_id));
		}
		serenity::FullEvent::Message { new_message } => {
			if let Some(gid) = new_message.guild_id
				&& !new_message.author.bot
			{
				let hl = data.highlights.read().await;
				let matches = hl.find(&new_message.content);
				let message_link = new_message.link();
				let message_link = &*message_link;
				let mut stream = futures::stream::iter(matches)
					.map(|(person_id, matcher)| async move {
						if let Ok(member) = gid.member(ctx, person_id).await
							&& let Ok(p) = gid.to_partial_guild(ctx).await
							&& let Ok(Some(channel)) = if let Ok(Some(x)) = p
								.channels(ctx)
								.await
								.map(|x| x.get(&new_message.channel_id).cloned())
							{
								Ok(Some(x))
							} else {
								p.get_active_threads(ctx).await.map(|x| {
									x.threads
										.iter()
										.find(|th| th.id == new_message.channel_id)
										.cloned()
								})
							} && let perms = p.user_permissions_in(&channel, &member)
							&& perms.contains(Permissions::VIEW_CHANNEL)
							&& (channel.kind != ChannelType::PrivateThread
								|| channel
									.id
									.get_thread_member(ctx, member.user.id, true)
									.await
									.ok()
									.map(|x| x.user_id) == Some(member.user.id))
						{
							_ = person_id
								.direct_message(
									ctx,
									serenity::CreateMessage::new().content(format!(
										"your match `{matcher}` was satisfied on message {message_link}",
									)),
								)
								.await;
						}
					})
					.buffer_unordered(8);
				while let Some(()) = stream.next().await {}
			}
		}
		serenity::FullEvent::InteractionCreate {
			interaction: serenity::Interaction::Component(component),
			..
		} if component.data.custom_id == "rplcs_create_new_modmail" => {
			let message = "Created from modmail button";
			create_modmail_thread(ctx, message, data, component.user.id).await?;
		}
		_ => {}
	}

	Ok(())
}

async fn fetch_icon_paths() -> tokio::io::Result<Box<[PathBuf]>> {
	let mut icon_paths = Vec::new();
	let mut icon_path_iter = tokio::fs::read_dir("./assets/server-icons").await?;
	while let Some(entry) = icon_path_iter.next_entry().await? {
		let path = entry.path();
		if path.is_file() {
			icon_paths.push(path);
		}
	}

	Ok(icon_paths.into())
}

async fn init_server_icon_changer(
	ctx: impl serenity::CacheHttp,
	guild_id: serenity::GuildId,
) -> anyhow::Result<()> {
	let icon_paths = fetch_icon_paths()
		.await
		.map_err(|e| anyhow!("Failed to read server-icons directory: {e}"))?;

	if icon_paths.is_empty() {
		warn!("No server icons found in assets/server-icons; skipping icon rotation");
		return Ok(());
	}

	loop {
		// Attempt to find all images and select one at random
		let icon = icon_paths.iter().choose(&mut rand::rng());
		if let Some(icon_path) = icon {
			info!("Changing server icon to {:?}", icon_path);

			// Attempt to change the server icon
			let icon_change_result = async {
				let icon = serenity::CreateAttachment::path(icon_path).await?;
				let edit_guild = serenity::EditGuild::new().icon(Some(&icon));
				guild_id.edit(&ctx, edit_guild).await
			}
			.await;

			if let Err(e) = icon_change_result {
				warn!("Failed to change server icon: {}", e);
			}
		}

		// Sleep for between 24 and 48 hours
		let sleep_duration = rand::rng().random_range((60 * 60 * 24)..(60 * 60 * 48));
		tokio::time::sleep(Duration::from_secs(sleep_duration)).await;
	}
}
