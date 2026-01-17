use crate::types::Context;

/// Returns the member's roles if available, handling both application and prefix contexts.
fn get_member_roles(ctx: Context<'_>) -> Option<&[poise::serenity_prelude::RoleId]> {
	match ctx {
		Context::Application(app_context) => app_context
			.interaction
			.member
			.as_ref()
			.map(|m| m.roles.as_slice()),
		Context::Prefix(msg_context) => msg_context.msg.member.as_ref().map(|m| m.roles.as_slice()),
	}
}

#[must_use]
pub fn is_moderator(ctx: Context<'_>) -> bool {
	let mod_role_id = ctx.data().mod_role_id;
	get_member_roles(ctx).is_some_and(|roles| roles.contains(&mod_role_id))
}

pub async fn check_is_moderator(ctx: Context<'_>) -> anyhow::Result<bool> {
	let user_has_moderator_role = is_moderator(ctx);
	if !user_has_moderator_role {
		ctx.send(
			poise::CreateReply::default()
				.content("This command is only available to moderators.")
				.ephemeral(true),
		)
		.await?;
	}

	Ok(user_has_moderator_role)
}
