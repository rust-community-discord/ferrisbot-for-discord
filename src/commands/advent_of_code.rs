use std::{fmt::Display, str::FromStr};

use poise::serenity_prelude::{
	ChannelId, ComponentInteraction, Context, CreateActionRow, CreateButton,
	CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage, CreateThread,
	EditMessage,
};
use time::{Month, OffsetDateTime, UtcOffset};

use crate::types::{Context as CommandContext, Data};
use anyhow::{Context as AnyhowContext, Error};

pub const INTERACTION_CUSTOM_ID: &str = "rplcs_open_aoc_thread";

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct AoCThreadId {
	year: u32,
	day: u8,
}

#[derive(Debug, PartialEq, Eq)]
struct AoCAnnounceMessage {
	year: u32,
	general_thread_id: ChannelId,
	days: Vec<(u8, ChannelId)>,
}

/// Creates an announcement for an Advent Of Code event in the current channel
#[poise::command(
	slash_command,
	guild_only,
	required_bot_permissions = "CREATE_PUBLIC_THREADS|READ_MESSAGE_HISTORY|SEND_MESSAGES",
	default_member_permissions = "MANAGE_CHANNELS"
)]
pub async fn create_aoc_announcement(
	ctx: CommandContext<'_>,
	#[description = "Thread for general discussions about AoC"] general_thread: ChannelId,
) -> Result<(), Error> {
	let year = OffsetDateTime::now_utc().year() as u32;

	let announcement = AoCAnnounceMessage {
		year,
		general_thread_id: general_thread,
		// TODO: Search for existing channels and include them here,
		// in case the original announcement message is deleted on accident or needs updating
		days: Vec::new(),
	};

	ctx.defer_ephemeral().await?;
	ctx.channel_id()
		.send_message(
			ctx,
			CreateMessage::new()
				.content(format!("{announcement}"))
				.components(aoc_announcement_components()),
		)
		.await?;

	ctx.reply("Announcement created!").await?;

	Ok(())
}

/// Opens a new Advent of Code thread for today and all previous missing days (if they don't exist yet)
pub async fn open_aoc_thread(
	interaction: &ComponentInteraction,
	data: &Data,
	ctx: &Context,
) -> Result<(), Error> {
	let today = OffsetDateTime::now_utc().to_offset(UtcOffset::from_hms(-5, 0, 0).unwrap());

	if today.month() != Month::December || today.day() > 24 {
		interaction
			.create_response(
				ctx,
				CreateInteractionResponse::Message(
					CreateInteractionResponseMessage::new()
						.ephemeral(true)
						.content("AoC is not taking place right now"),
				),
			)
			.await?;
		return Ok(());
	}

	let mut announcement = {
		let last_message = data.aoc_last_message.read().await;
		let current = interaction.message.as_ref();
		// Avoid a race by checking if the message stored in aoc_last_message has been edited after the message that triggered the interaction
		last_message
			.as_ref()
			.filter(|other| {
				other.id > current.id
					|| (other.id == current.id && other.edited_timestamp > current.edited_timestamp)
			})
			.unwrap_or(current)
			.content
			.parse::<AoCAnnounceMessage>()
	}
	.context("Failed to parse announcement message, please report this!")?;

	if announcement.year as i32 != today.year() {
		interaction
			.create_response(
				ctx,
				CreateInteractionResponse::Message(
					CreateInteractionResponseMessage::new()
						.ephemeral(true)
						.content("This AoC is from another year"),
				),
			)
			.await?;
		return Ok(());
	}

	let send_thread_id_response = |id| {
		interaction.create_response(
			ctx,
			CreateInteractionResponse::Message(
				CreateInteractionResponseMessage::new()
					.ephemeral(true)
					.content(format!("Today's thread thread is: <#{}>", id)),
			),
		)
	};

	// Early return if today's thread is already present in the message
	if let Some(thread_id) = announcement
		.days
		.iter()
		.find_map(|(day, id)| (*day == today.day()).then_some(*id))
	{
		send_thread_id_response(thread_id.get()).await?;
		return Ok(());
	}

	let mut last_message_lock = data.aoc_last_message.write().await;

	let today_id = AoCThreadId {
		year: announcement.year,
		day: today.day(),
	};

	// Create missing threads
	for missing in (announcement.days.last().map_or(1, |(d, _)| *d)..=today_id.day)
		.map(|day| AoCThreadId { day, ..today_id })
	{
		let thread = data
			.aoc_channel_id
			.create_thread(
				ctx,
				CreateThread::new(format!("{missing}"))
					.auto_archive_duration(poise::serenity_prelude::AutoArchiveDuration::OneWeek)
					.audit_log_reason(&format!(
						"Thread for Advent of Code day {} triggered by {}",
						missing.day, interaction.user.name
					)),
			)
			.await?;
		announcement.days.push((missing.day, thread.id));
	}

	let message = data
		.aoc_channel_id
		.edit_message(
			ctx,
			interaction.message.id,
			EditMessage::new()
				.content(format!("{announcement}"))
				.components(aoc_announcement_components()),
		)
		.await?;

	*last_message_lock = Some(message);

	Ok(())
}

fn aoc_announcement_components() -> Vec<CreateActionRow> {
	vec![CreateActionRow::Buttons(vec![CreateButton::new(
		INTERACTION_CUSTOM_ID,
	)
	.label("Open today's thread")
	.emoji('🧵')])]
}

impl Display for AoCThreadId {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "AoC {} | Day {} Discussion", self.year, self.day)
	}
}

const ANNOUNCEMENT_MESSAGE_PARTS: [&str; 4] = [
	"# Advent of Code ",
	"\nThe linked threads contain discussions that may spoiler solutions about the respective days. For general discussion about AoC without spoilers see <#",
	">\n",
	"\n-# If today's thread isn't listed here yet, click the button below.",
];

// NOTE: Any changes to this implementation must also update the FromStr implementation!
impl Display for AoCAnnounceMessage {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"{}{}{}{}{}",
			ANNOUNCEMENT_MESSAGE_PARTS[0],
			self.year,
			ANNOUNCEMENT_MESSAGE_PARTS[1],
			self.general_thread_id.get(),
			ANNOUNCEMENT_MESSAGE_PARTS[2]
		)?;

		for (day, thread_id) in &self.days {
			write!(f, "\n- Day {}: <#{}>", day, thread_id.get())?;
		}

		write!(f, "{}", ANNOUNCEMENT_MESSAGE_PARTS[3])?;

		Ok(())
	}
}

impl FromStr for AoCAnnounceMessage {
	type Err = Error;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let s = s
			.strip_prefix(ANNOUNCEMENT_MESSAGE_PARTS[0])
			.context("Invalid")?;
		let (year, s) = s
			.split_once(ANNOUNCEMENT_MESSAGE_PARTS[1])
			.context("Invalid")?;
		let year = year.parse::<u32>()?;
		let (general_thread, s) = s
			.split_once(ANNOUNCEMENT_MESSAGE_PARTS[2])
			.context("Invalid")?;
		let general_thread_id = ChannelId::new(general_thread.parse::<u64>()?);

		let days = s
			.strip_suffix(ANNOUNCEMENT_MESSAGE_PARTS[3])
			.context("missing message part")?;
		let days = days
			.lines()
			.skip(1)
			.map(|line| {
				let line = line.strip_prefix("- Day ")?;
				let (day, line) = line.split_once(": <#")?;
				let thread_id = line.strip_suffix(">")?;

				day.parse::<u8>()
					.ok()
					.zip(thread_id.parse::<u64>().ok().map(|id| ChannelId::new(id)))
			})
			.collect::<Option<Vec<(u8, ChannelId)>>>()
			.context("Failed to parse days")?;

		Ok(Self {
			year,
			general_thread_id,
			days,
		})
	}
}

#[cfg(test)]
mod tests {
	use poise::serenity_prelude::ChannelId;

	use crate::commands::advent_of_code::AoCAnnounceMessage;

	#[test]
	fn parse_aoc_announcement() {
		let announcements = [
			AoCAnnounceMessage {
				year: 2024,
				general_thread_id: ChannelId::new(123),
				days: [(1, ChannelId::new(23)), (2, ChannelId::new(56345))].into(),
			},
			AoCAnnounceMessage {
				year: 2024,
				general_thread_id: ChannelId::new(123),
				days: Default::default(),
			},
		];
		for announcement in announcements {
			assert_eq!(
				format!("{announcement}")
					.parse::<AoCAnnounceMessage>()
					.unwrap(),
				announcement
			);
		}
	}
}
