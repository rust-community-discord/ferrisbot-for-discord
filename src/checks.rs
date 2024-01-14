use anyhow::{anyhow, Error};

use crate::types::Context;

pub async fn check_is_moderator(ctx: Context<'_>) -> Result<bool, Error> {
	let author = ctx
		.author_member()
		.await
		.ok_or(anyhow!("Failed to fetch server member."))?;

	let user_has_moderator_role = author.roles.contains(&ctx.data().mod_role_id);

	if !user_has_moderator_role {
		ctx.send(|create_reply| {
			create_reply
				.content("This command is only available to moderators.")
				.ephemeral(true)
		})
		.await?;
	}

	Ok(user_has_moderator_role)
}
