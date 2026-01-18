use std::{
	collections::{HashMap, HashSet},
	ops::Not as _,
	sync::Mutex,
	time::Duration,
};

use anyhow::{Result, anyhow};
use futures::StreamExt as _;
use itertools::Itertools;
use poise::{
	ChoiceParameter, CreateReply, ReplyHandle, modal::execute_modal_on_component_interaction,
	serenity_prelude::*,
};

use crate::types::Context;

const MESSAGE_LIMIT: u8 = 100;
const MAX_TIME_SPAN: Duration = Duration::from_hours(2);

#[poise::command(
	context_menu_command = "Move Messages",
	guild_only,
	default_member_permissions = "MANAGE_MESSAGES",
	required_permissions = "MANAGE_MESSAGES",
	required_bot_permissions = "MANAGE_MESSAGES | MANAGE_WEBHOOKS | MANAGE_THREADS | SEND_MESSAGES_IN_THREADS"
)]
pub async fn move_messages_context_menu(ctx: Context<'_>, msg: Message) -> Result<()> {
	Box::pin(move_messages(ctx, msg)).await
}

struct ChannelLock<'ctx> {
	mutex: &'ctx Mutex<HashSet<ChannelId>>,
	channel: ChannelId,
}

impl<'ctx> ChannelLock<'ctx> {
	fn try_lock(ctx: &Context<'ctx>, channel: ChannelId) -> Option<Self> {
		let data = ctx.data();

		// Ignore poison, as a thread that panics while holding this lock won't execute any more code.
		let (Ok(mut locked_channels) | Err(mut locked_channels)) = data
			.move_channel_locks
			.lock()
			.map_err(std::sync::PoisonError::into_inner);

		if locked_channels.contains(&channel) {
			return None;
		}

		locked_channels.insert(channel);
		Some(ChannelLock {
			mutex: &data.move_channel_locks,
			channel,
		})
	}

	async fn wait_for_lock(ctx: &Context<'ctx>, channel: ChannelId) -> Result<Self> {
		let start = std::time::Instant::now();

		while start.elapsed() < Duration::from_secs(120) {
			if let Some(lock) = Self::try_lock(ctx, channel) {
				return Ok(lock);
			}

			tokio::time::sleep(Duration::from_millis(100)).await;
		}

		Err(anyhow!(
			"channel has been locked for over two minutes, giving up"
		))
	}
}

impl Drop for ChannelLock<'_> {
	fn drop(&mut self) {
		let (Ok(mut locked_channels) | Err(mut locked_channels)) = self
			.mutex
			.lock()
			.map_err(std::sync::PoisonError::into_inner);

		// Remove lock from channel.
		locked_channels.remove(&self.channel);
	}
}

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
	#[subenum(
		NewThreadComponent,
		ExistingThreadComponent,
		ChannelComponent,
		NewForumPostComponent
	)]
	SetLastMessageButton,
	#[subenum(NewThreadComponent, NewForumPostComponent)]
	ChangeNameButton,
}

impl MoveOptionComponent {
	const fn needs_to_be_set(self) -> bool {
		matches!(self, Self::Forum | Self::Thread | Self::Channel)
	}

	fn can_defer(self) -> bool {
		matches!(self, Self::ChangeNameButton | Self::SetLastMessageButton).not()
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
		delete_on_fail: bool,
	},
}

impl MoveDestination {
	const fn id(self) -> ChannelId {
		match self {
			Self::Channel(channel) => channel,
			Self::Thread { thread, .. } => thread,
		}
	}

	const fn channel(self) -> ChannelId {
		match self {
			Self::Thread { channel, .. } | Self::Channel(channel) => channel,
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
			Self::Channel { id } | Self::ExistingThread { thread_id: id, .. }
				if *id == start_msg.channel_id =>
			{
				Err(anyhow!("source and destination cannot be the same"))
			}

			Self::Channel { id } => Ok(MoveDestination::Channel(*id)),
			Self::ExistingThread {
				thread_id,
				channel_id,
			} => Ok(MoveDestination::Thread {
				channel: *channel_id,
				thread: *thread_id,
				delete_on_fail: false,
			}),

			Self::NewThread {
				channel_id,
				thread_name,
			} => {
				let thread = channel_id
					.create_thread(
						&ctx,
						CreateThread::new(thread_name)
							.kind(ChannelType::PublicThread)
							.audit_log_reason("moved conversation"),
					)
					.await?;

				Ok(MoveDestination::Thread {
					channel: *channel_id,
					thread: thread.id,
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
							CreateMessage::new().content("Moved conversation"),
						),
					)
					.await?;

				Ok(MoveDestination::Thread {
					channel: *forum_id,
					thread: post.id,
					delete_on_fail: true,
				})
			}
		}
	}
}

struct CreatedMoveOptionsDialog<'a> {
	handle: ReplyHandle<'a>,
	dialog: MoveOptionsDialog,
}

struct MoveOptionsDialog {
	initial_msg: Message,
	destination: MoveDestinationOption,
	involved_users: Vec<UserId>,

	thread_name: String,
	last_message_id: Option<MessageId>,

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
		// Select forum immediately if there's only one.
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
			last_message_id: None,
			destination: MoveDestinationOption::default(),
			involved_users: users.clone(),
			selected_users: users,
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
		#[derive(Debug, poise::Modal)]
		#[name = "Set last message"]
		struct LastMessageModal {
			#[name = "Last message ID"]
			#[placeholder = "Input ID here"]
			#[min_length = 18]
			#[max_length = 20]
			message_id: Option<String>,
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
					self.selected_users = values;
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
				let selected_thread = get_selected_channel(&interaction);

				// Prevent us from selecting the thread we're already in.
				if self.destination == MoveDestinationOption::ExistingThread {
					if selected_thread.is_none_or(|c| c != self.initial_msg.channel_id) {
						self.selected_thread = selected_thread;
					}
				} else {
					self.selected_thread = selected_thread;
				}
			}
			MoveOptionComponent::Channel => {
				let selected_channel = get_selected_channel(&interaction);

				// Prevent us from selecting the channel we're already in.
				if self.destination == MoveDestinationOption::Channel {
					if selected_channel.is_none_or(|c| c != self.initial_msg.channel_id) {
						self.selected_channel = selected_channel;
					}
				} else {
					self.selected_channel = selected_channel;
				}
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
			MoveOptionComponent::SetLastMessageButton => {
				let last_message_input = execute_modal_on_component_interaction(
					ctx,
					interaction,
					self.last_message_id.map(|id| LastMessageModal {
						message_id: Some(id.to_string()),
					}),
					None,
				)
				.await?;

				if let Some(input) = last_message_input {
					if let Some(id) = input.message_id
						&& let Ok(id) = id.parse::<u64>()
					{
						self.last_message_id = Some(MessageId::new(id));
					} else {
						self.last_message_id = None;
					}
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
						default_users: Some(self.involved_users.clone()),
					},
				)
				.placeholder("Which users should have their messages moved?")
				.max_values(self.involved_users.len() as _),
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
			MoveOptionComponent::SetLastMessageButton => CreateActionRow::Buttons(vec![
				CreateButton::new(custom_id)
					.style(ButtonStyle::Secondary)
					.label("Set last message"),
			]),
		}
	}
}

/// Returns the messages in the order they were posted.
async fn get_messages_after_and_including_msg(
	ctx: &Context<'_>,
	start_msg: &Message,
) -> Result<Vec<Message>> {
	let mut messages = start_msg
		.channel_id
		.messages(
			&ctx,
			GetMessages::new().after(start_msg.id).limit(MESSAGE_LIMIT),
		)
		.await?;

	messages.push(start_msg.clone());
	messages.reverse();

	Ok(messages)
}

async fn move_messages(ctx: Context<'_>, start_msg: Message) -> Result<()> {
	let _source_lock = ChannelLock::try_lock(&ctx, start_msg.channel_id)
		.ok_or_else(|| anyhow!("channel is already used by another move operation"))?;

	ctx.defer_ephemeral().await?;

	let messages = get_messages_after_and_including_msg(&ctx, &start_msg).await?;

	if messages.is_empty() {
		ctx.say("No messages found").await?;
		return Ok(());
	}

	let users_by_message_count = {
		let message_count_per_user: HashMap<&User, usize> =
			messages.iter().map(|m| &m.author).counts();

		message_count_per_user
			.keys()
			.sorted_by_key(|&&u| message_count_per_user[u])
			.map(|u| u.id)
			.collect_vec()
	};

	let mut options =
		MoveOptionsDialog::create(ctx, start_msg.clone(), users_by_message_count).await?;

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

	let destination_lock = ChannelLock::wait_for_lock(&ctx, destination.id()).await?;

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

	let message_posted_within_max_timespan = |m: &Message| {
		let time_span = (*m.timestamp - *start_msg.timestamp)
			.to_std()
			.unwrap_or(Duration::ZERO);

		time_span < MAX_TIME_SPAN
	};

	// If we haven't already gathered enough messages to reach the max timespan, fetch
	// messages again, because more could have been posted after we started this interaction.
	let should_refetch_messages = match messages.last() {
		Some(last) => message_posted_within_max_timespan(last),
		None => true,
	};

	let messages = if should_refetch_messages {
		get_messages_after_and_including_msg(&ctx, &start_msg).await?
	} else {
		messages
	};

	// If the user has set a message ID to stop at, but the message doesn't exist,
	// use the creation date from the ID to figure out a stop time.
	let (stop_at_id, stop_at_time) = if let Some(last_msg_id) = options.dialog.last_message_id {
		if messages.iter().any(|m| m.id == last_msg_id) {
			(Some(last_msg_id), None)
		} else {
			let last_msg_ts = last_msg_id.created_at();
			(None, Some(last_msg_ts))
		}
	} else {
		(None, None)
	};

	let filtered_messages = messages
		.into_iter()
		.filter(|m| options.dialog.selected_users.contains(&m.author.id))
		.filter(message_posted_within_max_timespan)
		.take(MESSAGE_LIMIT as _)
		.take_while_inclusive(|m| {
			if let Some(stop_id) = stop_at_id {
				m.id != stop_id
			} else if let Some(stop_time) = stop_at_time {
				m.id.created_at() < stop_time
			} else {
				true
			}
		});

	let mut relayed_messages = Vec::new();
	let mut original_users = HashMap::new();
	let mut relay_error = None;

	// Send messages to destination via webhook.
	for message in filtered_messages.clone() {
		// Prevent us from trying to send empty messages.
		let text = if message.content.is_empty() {
			String::from("_ _")
		} else {
			message.content.clone()
		};

		let mut builder = ExecuteWebhook::new()
			.allowed_mentions(CreateAllowedMentions::new())
			.username(
				message
					.author_nick(&ctx)
					.await
					.unwrap_or(message.author.display_name().to_owned()),
			)
			.content(text)
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
				original_users.insert(msg.id, message.author.id);
				relayed_messages.push(msg);
			}
			Ok(None) => {
				tracing::error!(
					"failed to wait for message, which shouldn't happen because we tell it to wait"
				);
				relay_error = Some(anyhow!("failed to wait for webhook message"));
				break;
			}
			Err(e) => {
				tracing::warn!(err = %e, "failed to create relayed message");
				relay_error = Some(e.into());
				break;
			}
		}
	}

	// Rollback relayed messages or new thread/forum post if anything failed.
	if let Some(err) = relay_error {
		// Try to delete webhook.
		if let Err(e) = webhook.delete(&ctx).await {
			tracing::warn!(err = %e, "failed to delete webhook used for relaying messages");
		}

		if let MoveDestination::Thread {
			thread,
			delete_on_fail,
			..
		} = destination
			&& delete_on_fail
		{
			match thread.delete(&ctx).await {
				Ok(_) => return Err(anyhow!("failed to move messages: {err}")),
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

		return Err(anyhow!("failed to move messages: {err}"));
	}

	// Post notice in destination.
	let notice_res = destination
		.id()
		.say(
			&ctx,
			format!(
				"{} moved the conversation from {} to here.\nParticipants: {}",
				Mention::from(ctx.author().id),
				Mention::from(ctx.channel_id()),
				options
					.dialog
					.selected_users
					.iter()
					.copied()
					.map(Mention::from)
					.join(""),
			),
		)
		.await;

	if let Err(e) = notice_res {
		tracing::warn!(err = %e, "failed to send notice to move destination");
	}

	drop(destination_lock);

	// Start collector to keep track of reactions to relayed messages.
	let mut collector = ReactionCollector::new(ctx)
		.channel_id(destination.id())
		.timeout(Duration::from_hours(4));

	if let Some(guild_id) = ctx.guild_id() {
		collector = collector.guild_id(guild_id);
	}

	tokio::spawn({
		let ctx = ctx.serenity_context().clone();
		listen_for_reactions(ctx, collector, webhook, relayed_messages, original_users)
	});

	// Delete the original messages.
	for msg in filtered_messages {
		if let Err(e) = msg.delete(&ctx).await {
			tracing::warn!(err = %e, "failed to delete original message");
			return Err(e.into());
		}
	}

	ctx.say(format!(
		"{} moved a conversation from here to {}.",
		Mention::from(ctx.author().id),
		Mention::from(destination.id())
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

async fn listen_for_reactions(
	ctx: poise::serenity_prelude::Context,
	collector: ReactionCollector,
	webhook: Webhook,
	mut relayed_messages: Vec<Message>,
	mut original_users: HashMap<MessageId, UserId>,
) {
	let (allowed_messages, allowed_users): (Vec<_>, Vec<_>) =
		original_users.iter().map(|(m, u)| (*m, *u)).unzip();

	// Cheap filter to reduce load.
	let filter = move |reaction: &Reaction| {
		if !allowed_messages.contains(&reaction.message_id) {
			return false;
		}
		// Only allow reactions from the user who originally posted the message before it was relayed.
		if reaction.user_id.is_none_or(|u| !allowed_users.contains(&u)) {
			return false;
		}

		true
	};

	let mut collector = collector.filter(filter).stream();

	while let Some(reaction) = collector.next().await {
		let Some(&user_id) = original_users.get(&reaction.message_id) else {
			continue;
		};

		let Some(message) = relayed_messages
			.iter()
			.find(|m| m.id == reaction.message_id)
		else {
			tracing::warn!("message exists in `original_users` but not in `relayed_messages`");
			continue;
		};

		// Only allow reactions from the original poster of the message.
		if Some(user_id) != reaction.user_id {
			continue;
		}

		let ReactionType::Unicode(emoji) = &reaction.emoji else {
			continue;
		};

		match emoji.as_str() {
			// Delete message.
			"❌" => {
				if let Err(e) = message.delete(&ctx).await {
					tracing::warn!(err = %e, "failed to delete relayed message");
				}
				original_users.remove(&message.id);

				if let Some(idx) = relayed_messages.iter().position(|m| m.id == message.id) {
					relayed_messages.swap_remove(idx);
				}
			}
			// Edit message.
			"📝" | "✏" => {
				tokio::spawn({
					let ctx = ctx.clone();
					let message = message.clone();
					let webhook = webhook.clone();
					prompt_user_for_edit_to_relayed_message(ctx, user_id, message, webhook)
				});

				if let Err(e) = reaction.delete(&ctx).await {
					tracing::warn!(err = %e, "failed to remove edit reaction");
				}
			}
			_ => {}
		}
	}

	// Try to delete webhook.
	if let Err(e) = webhook.delete(&ctx).await {
		tracing::warn!(err = %e, "failed to delete webhook used for relaying messages");
	}
}

async fn prompt_user_for_edit_to_relayed_message(
	ctx: poise::serenity_prelude::Context,
	user_id: UserId,
	message: Message,
	webhook: Webhook,
) {
	let dm = match user_id.create_dm_channel(&ctx).await {
		Ok(channel) => channel,
		Err(e) => {
			tracing::warn!(err = %e, "failed to DM user");
			return;
		}
	};

	let mut prompt_string = MessageBuilder::new();

	prompt_string.push_bold_line("Original message:");

	for line in message.content.lines() {
		prompt_string.push_quote_line(line);
	}

	prompt_string.push_bold_line("\nPlease respond with your edit within the next five minutes:");

	let mut prompt_string = prompt_string.build();
	prompt_string.truncate(2048);

	let prompt = match dm.say(&ctx, prompt_string).await {
		Ok(msg) => msg,
		Err(e) => {
			tracing::warn!(err = %e, "failed to DM user");
			return;
		}
	};

	let Some(reply) = MessageCollector::new(&ctx)
		.channel_id(dm.id)
		.timeout(Duration::from_mins(5))
		.next()
		.await
	else {
		if let Err(e) = prompt.delete(&ctx).await {
			tracing::warn!(err = %e, "failed to delete edit prompt");
		}
		return;
	};

	let edit_result = webhook
		.edit_message(
			&ctx,
			message.id,
			EditWebhookMessage::new().content(&reply.content),
		)
		.await;

	if let Err(e) = edit_result {
		tracing::warn!(err = %e, "failed to edit relayed message");

		let reply_result = reply
			.reply_ping(
				&ctx,
				format!("Failed to edit message, webhook has likely been deleted: {e}"),
			)
			.await;

		if let Err(e) = reply_result {
			tracing::warn!(err = %e, "failed to notify user of failure to edit");
		}
		return;
	}

	if let Err(e) = prompt.delete(&ctx).await {
		tracing::warn!(err = %e, "failed to delete edit prompt in DM");
	}
}
