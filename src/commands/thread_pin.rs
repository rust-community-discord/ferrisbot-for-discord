use anyhow::Result;
use poise::serenity_prelude as serenity;

use crate::types::Context;

enum ThreadPinError {
	NoChannel,
	NotThread,
	ThreadLocked,
	NotThreadOwner,
}

async fn can_pin_in_thread(ctx: Context<'_>) -> Result<(), ThreadPinError> {
	if crate::checks::is_moderator(ctx) {
		return Ok(());
	}

	let Some(channel) = ctx.guild_channel().await else {
		return Err(ThreadPinError::NoChannel);
	};

	let Some(thread_metadata) = channel.thread_metadata else {
		return Err(ThreadPinError::NotThread);
	};

	if thread_metadata.locked {
		return Err(ThreadPinError::ThreadLocked);
	}

	if channel.owner_id != Some(ctx.author().id) {
		return Err(ThreadPinError::NotThreadOwner);
	}

	Ok(())
}

#[poise::command(context_menu_command = "Pin Message to Thread", guild_only)]
pub async fn thread_pin(ctx: Context<'_>, message: serenity::Message) -> Result<()> {
	let reply = match can_pin_in_thread(ctx).await {
		Ok(()) => {
			message.pin(ctx.serenity_context()).await?;
			"Pinned message to your thread!"
		}
		Err(ThreadPinError::NoChannel) => "Error: Cannot fetch any information about this channel!",
		Err(ThreadPinError::NotThread) => "This channel is not a thread!",
		Err(ThreadPinError::NotThreadOwner) => {
			"You did not create this thread, so cannot pin messages to it."
		}
		Err(ThreadPinError::ThreadLocked) => {
			"This thread has been locked, so this cannot be performed."
		}
	};

	let reply = poise::CreateReply::default().content(reply).ephemeral(true);
	ctx.send(reply).await?;
	Ok(())
}
