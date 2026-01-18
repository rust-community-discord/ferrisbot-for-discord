use std::{
	collections::{HashMap, HashSet},
	ops::Not as _,
};

use anyhow::{Result, anyhow};
use futures::StreamExt as _;
use itertools::Itertools;
use poise::{
	ChoiceParameter, CreateReply, ReplyHandle, modal::execute_modal_on_component_interaction,
	serenity_prelude::*,
};

use crate::types::Context;

#[derive(Copy, Clone, Default, PartialEq, Eq, poise::ChoiceParameter)]
enum MoveDestinationOption {
	#[default]
	Channel,
	#[name = "New Thread"]
	NewThread,
	#[name = "Existing Thread"]
	ExistingThread,
	#[name = "New Forum Post"]
	NewForumPost,
}

impl MoveDestinationOption {
	fn components(self) -> Vec<MoveOptionComponent> {
		match self {
			MoveDestinationOption::Channel => ChannelComponent::base_variants(),
			MoveDestinationOption::NewThread => NewThreadComponent::base_variants(),
			MoveDestinationOption::ExistingThread => ExistingThreadComponent::base_variants(),
			MoveDestinationOption::NewForumPost => NewForumPostComponent::base_variants(),
		}
	}

	fn needs_to_be_set(self) -> HashSet<MoveOptionComponent> {
		match self {
			MoveDestinationOption::Channel => ChannelComponent::needs_to_be_set(),
			MoveDestinationOption::NewThread => NewThreadComponent::needs_to_be_set(),
			MoveDestinationOption::ExistingThread => ExistingThreadComponent::needs_to_be_set(),
			MoveDestinationOption::NewForumPost => NewForumPostComponent::needs_to_be_set(),
		}
	}
}

enum MoveOptions {
	NewThread {
		channel_id: ChannelId,
		thread_name: String,
	},
	ExistingThread {
		channel_id: ChannelId,
		thread_id: ChannelId,
	},
	Channel {
		id: ChannelId,
	},
	NewForumPost {
		forum_id: ChannelId,
		post_name: String,
	},
}

#[subenum::subenum(
	NewThreadComponent,
	ExistingThreadComponent,
	ChannelComponent,
	NewForumPostComponent
)]
#[derive(
	Copy,
	Clone,
	Debug,
	PartialEq,
	Eq,
	Hash,
	strum::IntoStaticStr,
	strum::VariantArray,
	strum::EnumString,
)]
enum MoveOptionComponent {
	#[subenum(
		NewThreadComponent,
		ExistingThreadComponent,
		ChannelComponent,
		NewForumPostComponent
	)]
	SelectUsers,
	#[subenum(
		NewThreadComponent,
		ExistingThreadComponent,
		ChannelComponent,
		NewForumPostComponent
	)]
	Destination,
	#[subenum(NewForumPostComponent)]
	Forum,
	#[subenum(ExistingThreadComponent)]
	Thread,
	#[subenum(NewThreadComponent, ChannelComponent)]
	Channel,
	#[subenum(
		NewThreadComponent,
		ExistingThreadComponent,
		ChannelComponent,
		NewForumPostComponent
	)]
	ExecuteButton,
	#[subenum(NewThreadComponent, NewForumPostComponent)]
	ChangeNameButton,
}

impl MoveOptionComponent {
	const fn needs_to_be_set(self) -> bool {
		matches!(self, Self::Forum | Self::Thread | Self::Channel)
	}

	fn can_defer(self) -> bool {
		matches!(self, Self::ChangeNameButton).not()
	}
}

trait Component {
	fn base_variants() -> Vec<MoveOptionComponent>;
	fn needs_to_be_set() -> HashSet<MoveOptionComponent>;
}

impl<T> Component for T
where
	T: Copy + strum::VariantArray,
	MoveOptionComponent: From<T>,
{
	fn base_variants() -> Vec<MoveOptionComponent> {
		T::VARIANTS
			.iter()
			.copied()
			.map(MoveOptionComponent::from)
			.collect()
	}

	fn needs_to_be_set() -> HashSet<MoveOptionComponent> {
		T::VARIANTS
			.iter()
			.copied()
			.map(MoveOptionComponent::from)
			.filter(|&v| v.needs_to_be_set())
			.collect()
	}
}

#[derive(Copy, Clone)]
enum MoveDestination {
	Channel(ChannelId),
	Thread {
		channel: ChannelId,
		thread: ChannelId,
		created_from_first_message: bool,
		delete_on_fail: bool,
	},
}

impl MoveDestination {
	const fn skip_first_message(self) -> bool {
		match self {
			Self::Channel(..) => false,
			Self::Thread {
				created_from_first_message,
				..
			} => created_from_first_message,
		}
	}

	const fn channel(self) -> ChannelId {
		match self {
			Self::Thread { channel, .. } | Self::Channel(channel) => channel,
		}
	}

	const fn thread(self) -> Option<ChannelId> {
		match self {
			Self::Channel(..) => None,
			Self::Thread { thread, .. } => Some(thread),
		}
	}
}

impl MoveOptions {
	async fn get_or_create_channel(
		&self,
		ctx: Context<'_>,
		start_msg: Message,
	) -> Result<MoveDestination> {
		match self {
			Self::Channel { id } => Ok(MoveDestination::Channel(*id)),
			Self::ExistingThread {
				thread_id,
				channel_id,
			} => Ok(MoveDestination::Thread {
				channel: *channel_id,
				thread: *thread_id,
				created_from_first_message: false,
				delete_on_fail: false,
			}),

			Self::NewThread {
				channel_id,
				thread_name,
			} => {
				let create_from_first_message = *channel_id == ctx.channel_id();

				let thread = if create_from_first_message {
					channel_id
						.create_thread_from_message(
							&ctx,
							start_msg.id,
							CreateThread::new(thread_name)
								.kind(ChannelType::PublicThread)
								.audit_log_reason("moved conversation"),
						)
						.await?
				} else {
					channel_id
						.create_thread(
							&ctx,
							CreateThread::new(thread_name)
								.kind(ChannelType::PublicThread)
								.audit_log_reason("moved conversation"),
						)
						.await?
				};

				Ok(MoveDestination::Thread {
					channel: *channel_id,
					thread: thread.id,
					created_from_first_message: create_from_first_message,
					delete_on_fail: true,
				})
			}

			Self::NewForumPost {
				forum_id,
				post_name,
			} => {
				let post = forum_id
					.create_forum_post(
						&ctx,
						CreateForumPost::new(
							post_name,
							CreateMessage::new()
								.add_embeds(start_msg.embeds.into_iter().map(Into::into).collect())
								.add_files({
									let mut attachments = Vec::new();

									for attachment in start_msg.attachments {
										attachments.push(
											CreateAttachment::url(&ctx, &attachment.url).await?,
										);
									}

									attachments
								})
								.add_sticker_ids(
									start_msg
										.sticker_items
										.into_iter()
										.map(|s| s.id)
										.collect_vec(),
								)
								.content(start_msg.content)
								.flags(start_msg.flags.unwrap_or_default()),
						),
					)
					.await?;

				Ok(MoveDestination::Thread {
					channel: *forum_id,
					thread: post.id,
					created_from_first_message: true,
					delete_on_fail: true,
				})
			}
		}
	}
}

#[poise::command(
	context_menu_command = "Move Messages",
	guild_only,
	default_member_permissions = "MANAGE_MESSAGES | SEND_MESSAGES_IN_THREADS | CREATE_PUBLIC_THREADS",
	required_bot_permissions = "MANAGE_MESSAGES | MANAGE_WEBHOOKS | MANAGE_THREADS | SEND_MESSAGES_IN_THREADS"
)]
pub async fn move_messages_context_menu(ctx: Context<'_>, msg: Message) -> Result<()> {
	Box::pin(move_messages(ctx, msg)).await
}

struct CreatedMoveOptionsDialog<'a> {
	handle: ReplyHandle<'a>,
	dialog: MoveOptionsDialog,
}

struct MoveOptionsDialog {
	initial_msg: Message,
	destination: MoveDestinationOption,

	users: Vec<UserId>,
	thread_name: String,
	selected_users: Vec<UserId>,
	selected_forum: Option<ChannelId>,
	selected_thread: Option<ChannelId>,
	selected_channel: Option<ChannelId>,

	needs_to_be_set: HashSet<MoveOptionComponent>,
}

impl MoveOptionsDialog {
	async fn create(
		ctx: Context<'_>,
		initial_msg: Message,
		users: Vec<UserId>,
	) -> Result<CreatedMoveOptionsDialog<'_>> {
		let selected_forum = initial_msg.guild(ctx.cache()).and_then(|g| {
			g.channels
				.values()
				.filter(|c| c.kind == ChannelType::Forum)
				.map(|c| c.id)
				.at_most_one()
				.ok()
				.flatten()
		});

		let mut dialog = Self {
			initial_msg,
			thread_name: String::from("Moved conversation"),
			destination: MoveDestinationOption::default(),
			selected_users: users.clone(),
			users,
			selected_forum,
			selected_thread: None,
			selected_channel: None,
			needs_to_be_set: HashSet::default(),
		};

		let components = dialog.switch_destination(dialog.destination);

		let handle = ctx
			.send(
				CreateReply::default()
					.components(components.collect())
					.ephemeral(true),
			)
			.await?;

		Ok(CreatedMoveOptionsDialog { handle, dialog })
	}

	async fn interaction_received(
		&mut self,
		ctx: Context<'_>,
		interaction: ComponentInteraction,
	) -> Result<Option<MoveOptions>> {
		#[derive(Debug, poise::Modal)]
		#[name = "Thread settings"]
		struct ThreadNameModal {
			#[name = "Name"]
			#[placeholder = "Input thread name here"]
			#[min_length = 1]
			#[max_length = 100]
			thread_name: String,
		}

		let component: MoveOptionComponent = match interaction.data.custom_id.parse() {
			Ok(c) => c,
			Err(e) => {
				tracing::warn!(err = %e, id = interaction.data.custom_id, "unknown component ID");
				return Ok(None);
			}
		};

		if component.can_defer() {
			interaction.defer(&ctx).await?;
		}

		match component {
			MoveOptionComponent::SelectUsers => {
				if let ComponentInteractionDataKind::UserSelect { values } = interaction.data.kind {
					self.users = values;
				}
			}
			MoveOptionComponent::Destination => {
				if let ComponentInteractionDataKind::StringSelect { values } =
					&interaction.data.kind
				{
					let Some(destination) = values
						.first()
						.and_then(|d| MoveDestinationOption::from_name(d))
					else {
						return Ok(None);
					};

					let components = self.switch_destination(destination);
					interaction
						.edit_response(
							&ctx,
							EditInteractionResponse::new().components(components.collect()),
						)
						.await?;
				}
			}
			MoveOptionComponent::Forum => {
				self.selected_forum = get_selected_channel(&interaction);
			}
			MoveOptionComponent::Thread => {
				self.selected_thread = get_selected_channel(&interaction);
			}
			MoveOptionComponent::Channel => {
				self.selected_channel = get_selected_channel(&interaction);
			}

			MoveOptionComponent::ChangeNameButton => {
				let thread_name_input = execute_modal_on_component_interaction(
					ctx,
					interaction,
					Some(ThreadNameModal {
						thread_name: self.thread_name.clone(),
					}),
					None,
				)
				.await?;

				if let Some(input) = thread_name_input {
					self.thread_name = input.thread_name;
				}
			}
			MoveOptionComponent::ExecuteButton => return self.build_move_options(ctx).await,
		}

		self.update_set_fields();
		Ok(None)
	}

	fn switch_destination(
		&mut self,
		destination: MoveDestinationOption,
	) -> impl Iterator<Item = CreateActionRow> + use<'_> {
		self.destination = destination;
		self.needs_to_be_set = destination.needs_to_be_set();
		self.selected_thread = None;
		self.update_set_fields();

		destination
			.components()
			.into_iter()
			.map(|c| self.create_component(c))
			// Combine adjacent button components.
			.coalesce(|a, b| match (a, b) {
				(CreateActionRow::Buttons(mut a), CreateActionRow::Buttons(mut b)) => {
					a.append(&mut b);
					Ok(CreateActionRow::Buttons(a))
				}
				other => Err(other),
			})
	}

	async fn build_move_options(&self, ctx: Context<'_>) -> Result<Option<MoveOptions>> {
		if !self.needs_to_be_set.is_empty() {
			return Ok(None);
		}

		let move_options = match self.destination {
			MoveDestinationOption::Channel => MoveOptions::Channel {
				id: self
					.selected_channel
					.ok_or_else(|| anyhow!("No channel specified"))?,
			},
			MoveDestinationOption::NewThread => MoveOptions::NewThread {
				channel_id: self
					.selected_channel
					.ok_or_else(|| anyhow!("No channel specified"))?,
				thread_name: self.thread_name.clone(),
			},
			MoveDestinationOption::ExistingThread => {
				let thread_id = self
					.selected_thread
					.ok_or_else(|| anyhow!("No thread specified"))?;

				let Channel::Guild(thread_channel) = thread_id.to_channel(&ctx).await? else {
					tracing::error!("command is marked guild_only yet returned a private channel.");
					return Err(anyhow!("failed to get thread channel"));
				};

				let Some(parent_id) = thread_channel.parent_id else {
					return Err(anyhow!("thread channel has no parent"));
				};

				MoveOptions::ExistingThread {
					channel_id: parent_id,
					thread_id,
				}
			}
			MoveDestinationOption::NewForumPost => MoveOptions::NewForumPost {
				forum_id: self
					.selected_forum
					.ok_or_else(|| anyhow!("No forum specified"))?,
				post_name: self.thread_name.clone(),
			},
		};

		Ok(Some(move_options))
	}

	fn update_set_fields(&mut self) {
		self.needs_to_be_set.retain(|c| match c {
			MoveOptionComponent::Forum => self.selected_forum.is_some(),
			MoveOptionComponent::Thread => self.selected_thread.is_some(),
			MoveOptionComponent::Channel => self.selected_channel.is_some(),
			_ => false,
		});
	}

	fn create_component(&self, component: MoveOptionComponent) -> CreateActionRow {
		let custom_id = Into::<&'static str>::into(component);
		match component {
			MoveOptionComponent::SelectUsers => CreateActionRow::SelectMenu(
				#[expect(
					clippy::cast_possible_truncation,
					reason = "more than 255 users is crazy"
				)]
				CreateSelectMenu::new(
					custom_id,
					CreateSelectMenuKind::User {
						default_users: Some(self.selected_users.clone()),
					},
				)
				.placeholder("Which users should have their messages moved?")
				.max_values(self.users.len() as _),
			),
			MoveOptionComponent::Destination => CreateActionRow::SelectMenu(
				CreateSelectMenu::new(
					custom_id,
					CreateSelectMenuKind::String {
						options: MoveDestinationOption::list()
							.into_iter()
							.map(|opt| {
								CreateSelectMenuOption::new(&opt.name, &opt.name)
									.default_selection(opt.name.as_str() == self.destination.name())
							})
							.collect(),
					},
				)
				.placeholder("Where should messages be moved to?")
				.min_values(1)
				.max_values(1),
			),
			MoveOptionComponent::Forum => CreateActionRow::SelectMenu(
				CreateSelectMenu::new(
					custom_id,
					CreateSelectMenuKind::Channel {
						channel_types: Some(vec![ChannelType::Forum]),
						default_channels: self.selected_forum.map(|id| vec![id]),
					},
				)
				.min_values(1)
				.max_values(1)
				.placeholder("Which forum should post be created in?"),
			),
			MoveOptionComponent::Thread => CreateActionRow::SelectMenu(
				CreateSelectMenu::new(
					custom_id,
					CreateSelectMenuKind::Channel {
						channel_types: Some(vec![ChannelType::PublicThread]),
						default_channels: self.selected_thread.map(|c| vec![c]),
					},
				)
				.min_values(1)
				.max_values(1)
				.placeholder("Which thread should messages be moved to?"),
			),
			MoveOptionComponent::Channel => CreateActionRow::SelectMenu(
				CreateSelectMenu::new(
					custom_id,
					CreateSelectMenuKind::Channel {
						channel_types: Some(vec![ChannelType::Text]),
						default_channels: self.selected_channel.map(|c| vec![c]),
					},
				)
				.min_values(1)
				.max_values(1)
				.placeholder("Which channel should messages be moved to?"),
			),
			MoveOptionComponent::ExecuteButton => CreateActionRow::Buttons(vec![
				CreateButton::new(custom_id)
					.style(ButtonStyle::Danger)
					.label("Move"),
			]),
			MoveOptionComponent::ChangeNameButton => {
				let label = if self.destination == MoveDestinationOption::NewForumPost {
					"Change forum post name"
				} else {
					"Change thread name"
				};
				CreateActionRow::Buttons(vec![
					CreateButton::new(custom_id)
						.style(ButtonStyle::Secondary)
						.label(label),
				])
			}
		}
	}
}

async fn move_messages(ctx: Context<'_>, start_msg: Message) -> Result<()> {
	ctx.defer_ephemeral().await?;

	let mut all_messages = start_msg
		.channel_id
		.messages(&ctx, GetMessages::new().after(start_msg.id))
		.await?;
	all_messages.push(start_msg.clone());
	all_messages.reverse();

	if all_messages.is_empty() {
		ctx.say("No messages found").await?;
		return Ok(());
	}

	let message_count_per_user: HashMap<&User, usize> =
		all_messages.iter().map(|m| &m.author).counts();
	let users_by_message_count = message_count_per_user
		.keys()
		.sorted_by_key(|&&u| message_count_per_user[u])
		.map(|u| u.id)
		.collect_vec();

	let mut options = MoveOptionsDialog::create(ctx, start_msg, users_by_message_count).await?;

	let options_handle = &options.handle;
	let options_msg = options_handle.message().await?;

	let mut interaction_stream = options_msg.await_component_interactions(ctx).stream();

	let move_options = loop {
		let Some(component_interaction) = interaction_stream.next().await else {
			break None;
		};

		if let Some(move_options) = options
			.dialog
			.interaction_received(ctx, component_interaction)
			.await?
		{
			break Some(move_options);
		}
	};

	options_handle.delete(ctx).await?;

	let Some(move_options) = move_options else {
		return Ok(());
	};

	let destination = move_options
		.get_or_create_channel(ctx, options.dialog.initial_msg.clone())
		.await?;

	let webhook = destination
		.channel()
		.create_webhook(
			&ctx,
			CreateWebhook::new(format!(
				"move conversation {}",
				options.dialog.initial_msg.id
			)),
		)
		.await?;

	let offset = usize::from(destination.skip_first_message());

	let filtered_messages = all_messages
		.into_iter()
		.filter(|m| options.dialog.selected_users.contains(&m.author.id))
		.skip(offset);

	let mut relayed_messages = Vec::new();
	let mut abort_relaying = false;

	// Send messages to destination via webhook.
	for message in filtered_messages.clone() {
		let mut builder = ExecuteWebhook::new()
			.allowed_mentions(CreateAllowedMentions::new())
			.username(
				message
					.author_nick(&ctx)
					.await
					.unwrap_or(message.author.display_name().to_owned()),
			)
			.content(message.content)
			.embeds(message.embeds.into_iter().map(Into::into).collect())
			.files({
				let mut attachments = Vec::new();

				for attachment in message.attachments {
					match CreateAttachment::url(&ctx, &attachment.url).await {
						Ok(attachment) => attachments.push(attachment),
						Err(e) => {
							tracing::warn!(err = %e, ?attachment, "failed to create attachment on relayed message");
						}
					}
				}

				attachments
			});

		if let Some(avatar) = message.author.avatar_url() {
			builder = builder.avatar_url(avatar);
		}

		if let MoveDestination::Thread { thread, .. } = destination {
			builder = builder.in_thread(thread);
		}

		match webhook.execute(&ctx, true, builder).await {
			Ok(Some(msg)) => {
				relayed_messages.push(msg);
			}
			Ok(None) => {
				tracing::error!(
					"failed to wait for message, which shouldn't happen because we tell it to wait"
				);
				abort_relaying = true;
				break;
			}
			Err(e) => {
				tracing::warn!(err = %e, "failed to create relayed message");
				abort_relaying = true;
				break;
			}
		}
	}

	// Rollback relayed messages or new thread/forum post if anything failed.
	if abort_relaying {
		if let MoveDestination::Thread {
			thread,
			delete_on_fail,
			..
		} = destination
			&& delete_on_fail
		{
			match thread.delete(&ctx).await {
				Ok(_) => return Err(anyhow!("failed to move messages")),
				Err(e) => {
					tracing::warn!(err = %e, "failed to delete thread, deleting messages");
				}
			}
		}

		for msg in relayed_messages {
			if let Err(e) = msg.delete(&ctx).await {
				tracing::warn!(err = %e, "failed to delete relayed message");
			}
		}

		return Err(anyhow!("failed to move messages"));
	}

	// Delete the original messages.
	for msg in filtered_messages {
		if let Err(e) = msg.delete(&ctx).await {
			tracing::warn!(err = %e, "failed to delete original message");
			return Err(e.into());
		}
	}

	ctx.say(format!(
		"Conversation moved from {} to {}.",
		Mention::from(ctx.channel_id()),
		Mention::from(destination.thread().unwrap_or(destination.channel()))
	))
	.await?;

	Ok(())
}

fn get_selected_channel(interaction: &ComponentInteraction) -> Option<ChannelId> {
	if let ComponentInteractionDataKind::ChannelSelect { values } = &interaction.data.kind {
		values.first().copied()
	} else {
		None
	}
}
