use anyhow::Result;
use poise::CreateReply;

use crate::types::Context;

/// ShortHand for GitHub URLs
#[poise::command(slash_command, prefix_command, rename = "gh")]
pub async fn gh_bang(
	ctx: Context<'_>,
	#[description = "Prefix with `https://github.com/`"] bang: String,
) -> Result<()> {
	ctx.send(CreateReply::default().content(format!("https://github.com/{}", bang)))
		.await?;
	Ok(())
}

/// ShortHand for Codeberg URLs
#[poise::command(slash_command, prefix_command, rename = "cb")]
pub async fn codeberg_bang(
	ctx: Context<'_>,
	#[description = "Prefix with `https://codeberg.org/`"] bang: String,
) -> Result<()> {
	ctx.send(CreateReply::default().content(format!("https://codeberg.org/{}", bang)))
		.await?;
	Ok(())
}

/// ShortHand for DuckDuckGo Search URLs
#[poise::command(slash_command, prefix_command, rename = "ddg")]
pub async fn duckduckgo_bang(
	ctx: Context<'_>,
	#[description = "Get a search URL to duckduckgo"] bang: String,
) -> Result<()> {
	ctx.send(CreateReply::default().content(format!(
		"https://duckduckgo.com/search?q={}",
		bang.replace(" ", "%20")
	)))
	.await?;
	Ok(())
}

/// ShortHand for Google Search URLs
#[poise::command(slash_command, prefix_command, rename = "gle")]
pub async fn google_bang(
	ctx: Context<'_>,
	#[description = "Get a search URL to google"] bang: String,
) -> Result<()> {
	ctx.send(CreateReply::default().content(format!(
		"https://google.com/search?q={}",
		bang.replace(" ", "%20")
	)))
	.await?;
	Ok(())
}

/// ShortHand for Wikipedia URLs
#[poise::command(slash_command, prefix_command, rename = "wiki")]
pub async fn wikipedia_bang(
	ctx: Context<'_>,
	#[description = "Get a search URL to wikipedia"] bang: String,
) -> Result<()> {
	ctx.send(CreateReply::default().content(format!(
		"https://en.wikipedia.org/wiki/{}",
		bang.replace(" ", "%20")
	)))
	.await?;
	Ok(())
}
