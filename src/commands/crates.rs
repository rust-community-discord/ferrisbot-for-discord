use anyhow::Result;
use anyhow::{anyhow, bail};
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use reqwest::header;
use serde::Deserialize;
use tracing::info;

use crate::serenity;
use crate::types::Context;

#[cfg(test)]
mod tests;

const USER_AGENT: &str = "kangalioo/rustbot";

#[derive(Debug, Deserialize)]
struct Crates {
	crates: Vec<Crate>,
}

#[derive(Debug, Deserialize)]
struct Crate {
	name: String,
	// newest_version: String, // https://github.com/kangalioo/rustbot/issues/23
	max_version: Option<String>,
	max_stable_version: Option<String>,
	// sometimes null empirically
	updated_at: String,
	downloads: u64,
	description: Option<String>,
	documentation: Option<String>,
	exact_match: bool,
}

/// Queries the crates.io crates list for a specific crate
async fn get_crate(http: &reqwest::Client, query: &str) -> Result<Crate> {
	info!("searching for crate `{}`", query);

	let crate_list = http
		.get("https://crates.io/api/v1/crates")
		.header(header::USER_AGENT, USER_AGENT)
		.query(&[("q", query)])
		.send()
		.await?
		.json::<Crates>()
		.await
		.map_err(|e| anyhow!("Cannot parse crates.io JSON response (`{e}`)"))?;

	let crate_ = crate_list
		.crates
		.into_iter()
		.next()
		.ok_or_else(|| anyhow!("Crate `{query}` not found"))?;

	if crate_.exact_match {
		Ok(crate_)
	} else {
		bail!(
			"Crate `{}` not found. Did you mean `{}`?",
			query,
			crate_.name
		)
	}
}

fn get_documentation(crate_: &Crate) -> String {
	match &crate_.documentation {
		Some(doc) => doc.to_owned(),
		None => format!("https://docs.rs/{}", crate_.name),
	}
}

/// 6051423 -> "6 051 423"
fn format_number(mut n: u64) -> String {
	let mut output = String::new();
	while n >= 1000 {
		output.insert_str(0, &format!(" {:03}", n % 1000));
		n /= 1000;
	}
	output.insert_str(0, &format!("{n}"));
	output
}

async fn autocomplete_crate(ctx: Context<'_>, partial: &str) -> impl Iterator<Item = String> {
	let http = &ctx.data().http;

	let response = http
		.get("https://crates.io/api/v1/crates")
		.header(header::USER_AGENT, USER_AGENT)
		.query(&[("q", partial), ("per_page", "25"), ("sort", "downloads")])
		.send()
		.await;

	let crate_list = match response {
		Ok(response) => response.json::<Crates>().await.ok(),
		Err(_) => None,
	};

	crate_list
		.map_or(Vec::new(), |list| list.crates)
		.into_iter()
		.map(|crate_| crate_.name)
}

/// Lookup crates on crates.io
///
/// Search for a crate on crates.io
/// ```
/// ?crate crate_name
/// ```
#[poise::command(
	prefix_command,
	slash_command,
	rename = "crate",
	broadcast_typing,
	category = "Crates"
)]
pub async fn crate_(
	ctx: Context<'_>,
	#[description = "Name of the searched crate"]
	#[autocomplete = "autocomplete_crate"]
	crate_name: String,
) -> Result<()> {
	if let Some(url) = rustc_crate_link(&crate_name) {
		ctx.say(url).await?;
		return Ok(());
	}

	let crate_ = get_crate(&ctx.data().http, &crate_name).await?;

	ctx.send(
		poise::CreateReply::default().embed(
			serenity::CreateEmbed::new()
				.title(&crate_.name)
				.url(get_documentation(&crate_))
				.description(
					crate_
						.description
						.as_deref()
						.unwrap_or("_<no description available>_"),
				)
				.field(
					"Version",
					crate_
						.max_stable_version
						.or(crate_.max_version)
						.unwrap_or_else(|| "<unknown version>".into()),
					true,
				)
				.field("Downloads", format_number(crate_.downloads), true)
				.timestamp(
					crate_
						.updated_at
						.parse::<serenity::Timestamp>()
						.unwrap_or(serenity::Timestamp::now()),
				)
				.color(crate::types::EMBED_COLOR),
		),
	)
	.await?;

	Ok(())
}

/// Returns whether the given type name is the one of a primitive.
#[rustfmt::skip]
fn is_in_std(name: &str) -> IsInStd<'_> {
	match name {
		"f32" | "f64"
			| "i8" | "i16" | "i32" | "i64" | "i128" | "isize"
			| "u8" | "u16" | "u32" | "u64" | "u128" | "usize"
			| "char" | "str"
			| "pointer" | "reference" | "fn"
			| "bool" | "slice" | "tuple" | "unit" | "array"
		=> IsInStd::Primitive,
		"f16" | "f128" | "never" => IsInStd::PrimitiveNightly,
		"Self" => IsInStd::Keyword("SelfTy"), // special case: SelfTy is not a real keyword
		"SelfTy"
			| "as" | "async" | "await" | "break" | "const" | "continue" | "crate" | "dyn" | "else" | "enum" | "extern" | "false"
			| "for" | "if" | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move" | "mut" | "pub" | "ref" | "return"
			| "self" | "static" | "struct" | "super" | "trait" | "true" | "type" | "union" | "unsafe" | "use" | "where" | "while"
			// omitted "fn" due to duplicate
		=> IsInStd::Keyword(name),
		name if name.chars().next().is_some_and(char::is_uppercase) => IsInStd::PossibleType,
		_ => IsInStd::False
	}
}

#[derive(Debug)]
enum IsInStd<'a> {
	PossibleType,
	Primitive,
	PrimitiveNightly,
	Keyword(&'a str),
	False,
}

/// Provide the documentation link to an official Rust crate (e.g. std, alloc, nightly)
fn rustc_crate_link(crate_name: &str) -> Option<&'static str> {
	match crate_name.to_ascii_lowercase().as_str() {
		"std" => Some("https://doc.rust-lang.org/stable/std/"),
		"core" => Some("https://doc.rust-lang.org/stable/core/"),
		"alloc" => Some("https://doc.rust-lang.org/stable/alloc/"),
		"proc_macro" => Some("https://doc.rust-lang.org/stable/proc_macro/"),
		"beta" => Some("https://doc.rust-lang.org/beta/std/"),
		"nightly" => Some("https://doc.rust-lang.org/nightly/std/"),
		"rustc" => Some("https://doc.rust-lang.org/nightly/nightly-rustc/"),
		"test" => Some("https://doc.rust-lang.org/stable/test"),
		_ => None,
	}
}

/// Lookup documentation
///
/// Retrieve documentation for a given crate
/// ```
/// ?docs crate_name::module::item
/// ```
#[poise::command(
	prefix_command,
	aliases("docs"),
	broadcast_typing,
	track_edits,
	slash_command,
	category = "Crates"
)]
pub async fn doc(
	ctx: Context<'_>,
	#[description = "Path of the crate and item to lookup"] query: String,
) -> Result<()> {
	ctx.say(path_to_doc_url(&query, &ctx.data().http).await?)
		.await?;

	Ok(())
}

async fn path_to_doc_url(query: &str, client: &impl DocsClient) -> Result<String> {
	use std::fmt::Write;

	let mut path = split_qualified_path(query);

	if path.ident.is_none() {
		// no `::`, possible ident from std
		match is_in_std(path.crate_) {
			IsInStd::Primitive => {
				path = QualifiedPath {
					kind: Some("primitive"),
					crate_: "std",
					ident: Some(path.crate_),
					mods: "",
				};
			}
			IsInStd::PrimitiveNightly => {
				path = QualifiedPath {
					kind: Some("primitive"),
					crate_: "nightly",
					ident: Some(path.crate_),
					mods: "",
				};
			}
			IsInStd::Keyword(ident) => {
				path = QualifiedPath {
					kind: Some("keyword"),
					crate_: "std",
					ident: Some(ident),
					mods: "",
				};
			}
			IsInStd::PossibleType => {
				path = QualifiedPath {
					kind: path.kind,
					crate_: "std",
					ident: Some(path.crate_),
					mods: "",
				};
			}
			IsInStd::False => {}
		}
	}

	let (is_rustc_crate, mut doc_url, root_len) =
		if let Some(prefix) = rustc_crate_link(path.crate_) {
			(true, prefix.to_owned(), prefix.len())
		} else {
			let mut prefix = client.get_crate_docs(path.crate_).await?;
			let root_len = prefix.len();

			if !prefix.ends_with('/') {
				prefix += "/";
			}
			write!(prefix, "latest/{}/", path.crate_.replace('-', "_")).unwrap();
			(false, prefix, root_len)
		};

	if let Some(ident) = path.ident {
		for segment in path.mods.split("::") {
			if !segment.is_empty() {
				doc_url += segment;
				doc_url += "/";
			}
		}

		let kind = if let Some(kind) = path.kind {
			Some(kind)
		} else {
			guess_kind(client, &doc_url, is_rustc_crate, ident).await
		};

		match kind {
			Some("" | "mod") => write!(doc_url, "{ident}/index.html").unwrap(),
			Some(kind) => write!(doc_url, "{kind}.{ident}.html").unwrap(),
			None => {
				doc_url.truncate(root_len);
				doc_url += "?search=";
				if !path.mods.is_empty() {
					doc_url += path.mods;
					doc_url += "::";
				}
				doc_url += ident;
			}
		}
	} else {
		doc_url.truncate(root_len);
	}

	Ok(doc_url)
}

fn split_qualified_path(input: &str) -> QualifiedPath<'_> {
	let (kind, path) = match input.split_once('@') {
		Some((kind, rest)) => (Some(kind), rest),
		None => (None, input),
	};

	let Some((crate_, rest)) = path.split_once("::") else {
		return QualifiedPath {
			kind,
			crate_: path,
			ident: None,
			mods: "",
		};
	};
	match rest.rsplit_once("::") {
		Some((mods, ident)) => QualifiedPath {
			kind,
			crate_,
			ident: Some(ident),
			mods,
		},
		None => QualifiedPath {
			kind,
			crate_,
			ident: Some(rest),
			mods: "",
		},
	}
}

#[derive(Debug)]
struct QualifiedPath<'a> {
	kind: Option<&'a str>,
	crate_: &'a str,
	ident: Option<&'a str>,
	mods: &'a str,
}

// Reference rust/src/tools/rust-analyzer/crates/ide/src/doc_links.rs for an exhaustive list
const SNAKE_CASE_KINDS: &[&str] = &["fn", "macro", "mod"];
const UPPER_CAMEL_CASE_KINDS: &[&str] = &[
	"struct",
	"enum",
	"union",
	"trait",
	"traitalias",
	"type",
	"derive",
];
const SCREAMING_SNAKE_CASE_KINDS: &[&str] = &["constant", "static"];
const RUSTC_CRATE_ONLY_KINDS: &[&str] = &["keyword", "primitive"];

async fn guess_kind(
	client: &impl DocsClient,
	prefix: &str,
	is_rustc_crate: bool,
	ident: &str,
) -> Option<&'static str> {
	let mut attempt_order: Vec<&[&'static str]> =
		if ident.chars().next().is_some_and(char::is_lowercase) {
			vec![
				SNAKE_CASE_KINDS,
				UPPER_CAMEL_CASE_KINDS,
				SCREAMING_SNAKE_CASE_KINDS,
			]
		} else if ident.chars().all(char::is_uppercase) {
			vec![
				SCREAMING_SNAKE_CASE_KINDS,
				UPPER_CAMEL_CASE_KINDS,
				SNAKE_CASE_KINDS,
			]
		} else {
			vec![
				UPPER_CAMEL_CASE_KINDS,
				SCREAMING_SNAKE_CASE_KINDS,
				SNAKE_CASE_KINDS,
			]
		};
	if is_rustc_crate {
		attempt_order.insert(1, RUSTC_CRATE_ONLY_KINDS);
	}

	for class in attempt_order {
		let results = class
			.iter()
			.map(|&kind| async move {
				let url = if kind == "mod" {
					format!("{prefix}{ident}/index.html")
				} else {
					format!("{prefix}{kind}.{ident}.html")
				};
				client.page_exists(&url).await.then_some(kind)
			})
			.collect::<FuturesUnordered<_>>()
			.filter_map(|result| async move { result });
		futures::pin_mut!(results);
		if let Some(kind) = results.next().await {
			return Some(kind);
		}
	}

	None
}

trait DocsClient {
	async fn get_crate_docs(&self, crate_name: &str) -> Result<String>;
	async fn page_exists(&self, url: &str) -> bool;
}

impl DocsClient for reqwest::Client {
	async fn get_crate_docs(&self, crate_name: &str) -> Result<String> {
		get_crate(self, crate_name)
			.await
			.map(|crate_| get_documentation(&crate_))
	}

	async fn page_exists(&self, url: &str) -> bool {
		self.head(url)
			.header(header::USER_AGENT, USER_AGENT)
			.send()
			.await
			.is_ok_and(|resp| resp.status() == reqwest::StatusCode::OK)
	}
}
