use anyhow::bail;
use anyhow::Result;
use reqwest::header;

use crate::serenity;
use crate::types::Context;

const USER_AGENT: &str = "kangalioo/rustbot";

#[poise::command(prefix_command, slash_command, category = "Utilities")]
pub async fn man(
	ctx: Context<'_>,
	#[description = "Section of the man page"] section: Option<String>,
	#[description = "Name of the man page"] man_page: String,
) -> Result<()> {
	let section = section.unwrap_or_else(|| "1".to_owned());

	let mut url = format!("https://manpages.debian.org/{section}/{man_page}");

	if let Ok(response) = ctx
		.data()
		.http
		.get(&url)
		.header(header::USER_AGENT, USER_AGENT)
		.send()
		.await
	{
		if response.status() == 404 {
			bail!("Man page not found");
		}
	} else {
		bail!("Failed to fetch man page");
	}

	url.push_str(".html");

	ctx.send(
		poise::CreateReply::default().embed(
			serenity::CreateEmbed::new()
				.title(format!("man {section} {man_page}"))
				.url(&url)
				.color(crate::types::EMBED_COLOR),
		),
	)
	.await?;

	Ok(())
}
