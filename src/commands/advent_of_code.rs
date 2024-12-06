use std::{fmt::Display, str::FromStr};

use poise::serenity_prelude::{
	ChannelId, ComponentInteraction, Context, CreateActionRow, CreateButton,
	CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage, CreateThread,
	EditMessage, GuildChannel,
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

	// Get existing threads for AoC days in case this command is being used to re-create
	// the announcement after being (accidentally) deleted.
	let mut days = get_existing_aoc_threads(year, &ctx)
		.await?
		.filter_map(|(id, channel)| {
			channel
				.owner_id
				.is_some_and(|owner| owner == ctx.framework().bot_id)
				.then_some((id.day, channel.id))
		})
		.collect::<Vec<_>>();
	days.sort_by_key(|(day, _)| *day);
	days.dedup_by_key(|(day, _)| *day);

	let announcement = AoCAnnounceMessage {
		year,
		general_thread_id: general_thread,
		days,
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
	let reply = |reply| {
		interaction.create_response(
			ctx,
			CreateInteractionResponse::Message(
				CreateInteractionResponseMessage::new()
					.ephemeral(true)
					.content(reply),
			),
		)
	};

	if today.month() != Month::December || today.day() > 24 {
		reply("AoC is not taking place right now".to_string()).await?;
		return Ok(());
	}

	let mut announcement = {
		let last_message = data.aoc_last_message.read().await;
		let current = interaction.message.as_ref();
		// Avoid a race by checking if the message stored in aoc_last_message has been edited
		// after the message that triggered the interaction.
		// Make sure there are no .await points between here and the write lock!
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
	.context("Failed to parse announcement message")?;

	if announcement.year as i32 != today.year() {
		reply("This AoC is from another year".to_string()).await?;
		return Ok(());
	}

	let today_id = AoCThreadId {
		year: announcement.year,
		day: today.day(),
	};

	// Early return if today's thread is already present in the message
	if let Some(thread_id) = announcement
		.days
		.iter()
		.find_map(|(day, id)| (*day == today_id.day).then_some(*id))
	{
		reply(format!("Today's thread thread is: <#{}>", thread_id.get())).await?;

		return Ok(());
	}

	let mut last_message_lock = data.aoc_last_message.write().await;

	// Create missing threads
	for missing in (announcement
		.days
		.last()
		.map_or(1, |(day, _)| *day + 1)..=today_id.day)
		.map(|day| AoCThreadId { day, ..today_id })
	{
		let thread = interaction
			.channel_id
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

		if missing.day == today_id.day {
			reply(format!(
				"Created thread for today's challenge: <#{}>",
				thread.id
			))
			.await?;
		}
	}

	let mut message = *interaction.message.to_owned();
	message
		.edit(
			ctx,
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

async fn get_existing_aoc_threads<'ctx>(
	year: u32,
	ctx: &'ctx CommandContext<'_>,
) -> Result<impl Iterator<Item = (AoCThreadId, GuildChannel)> + 'ctx, Error> {
	// Get all active threads in the current channel
	let from_active_threads = ctx
		.guild_id()
		.context("Must be run inside a guild")?
		.get_active_threads(ctx)
		.await?
		.threads
		.into_iter()
		.filter(|channel| {
			channel
				.parent_id
				.is_some_and(|parent_id| parent_id == ctx.channel_id())
		});
	// Get all archived threads in the current channel
	let from_archived_threads = ctx
		.channel_id()
		.get_archived_public_threads(ctx, None, Some(100))
		.await?
		.threads;

	Ok(from_active_threads
		.chain(from_archived_threads)
		.filter_map(move |channel| {
			Some((
				channel
					.name()
					.parse::<AoCThreadId>()
					.ok()
					.filter(|id| id.year == year)?,
				channel,
			))
		}))
}

impl Display for AoCThreadId {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "AoC {} | Day {} Discussion", self.year, self.day)
	}
}

impl FromStr for AoCThreadId {
	type Err = Error;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let s = s.strip_prefix("AoC ").context("Missing 'AoC ' prefix")?;
		let (year, s) = s
			.split_once(" | Day ")
			.context("Not separated by ' | Day ")?;
		let day = s
			.strip_suffix(" Discussion")
			.context("Missing ' Discussion' suffix")?;

		Ok(Self {
			year: year.parse()?,
			day: day.parse()?,
		})
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

	use crate::commands::advent_of_code::{AoCAnnounceMessage, AoCThreadId};

	#[test]
	fn parse_aoc_id() {
		let id = AoCThreadId { year: 2024, day: 3 };

		assert_eq!(format!("{id}").parse::<AoCThreadId>().unwrap(), id);
	}

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
