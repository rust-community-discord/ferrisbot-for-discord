use std::collections::HashMap;
use std::str::FromStr;

use anyhow::{Error, anyhow, bail};
use reqwest::header;
use serde::{Deserialize, Deserializer, Serialize};
use tracing::info;

use crate::types::Context;

pub struct CommandFlags {
	pub channel: Channel,
	pub mode: Mode,
	pub edition: Edition,
	pub warn: bool,
	pub run: bool,
	pub aliasing_model: AliasingModel,
}

#[derive(Debug, Serialize)]
pub struct PlaygroundRequest<'a> {
	pub channel: Channel,
	pub edition: Edition,
	pub code: &'a str,
	#[serde(rename = "crateType")]
	pub crate_type: CrateType,
	pub mode: Mode,
	pub tests: bool,
}

#[derive(Debug, Serialize)]
pub struct MiriRequest<'a> {
	pub edition: Edition,
	#[serde(rename = "aliasingModel")]
	pub aliasing_model: AliasingModel,
	pub code: &'a str,
}

#[derive(Debug, Serialize)]
pub struct MacroExpansionRequest<'a> {
	pub edition: Edition,
	pub code: &'a str,
}

#[derive(Debug, Serialize)]
pub struct ClippyRequest<'a> {
	pub edition: Edition,
	#[serde(rename = "crateType")]
	pub crate_type: CrateType,
	pub code: &'a str,
}

#[derive(Debug, Serialize)]
pub struct FormatRequest<'a> {
	pub code: &'a str,
	pub edition: Edition,
}

#[derive(Debug, Deserialize)]
pub struct FormatResponse {
	pub success: bool,
	pub code: String,
	pub stderr: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(unused)]
pub enum CompileTarget {
	Mir,
}

#[allow(unused)]
pub type CompileResponse = FormatResponse;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(unused)]
pub enum Channel {
	Stable,
	Beta,
	Nightly,
}

impl FromStr for Channel {
	type Err = Error;

	fn from_str(s: &str) -> Result<Self, Error> {
		match s {
			"stable" => Ok(Channel::Stable),
			"beta" => Ok(Channel::Beta),
			"nightly" => Ok(Channel::Nightly),
			_ => bail!("invalid release channel `{s}`"),
		}
	}
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum Edition {
	#[serde(rename = "2015")]
	E2015,
	#[serde(rename = "2018")]
	E2018,
	#[serde(rename = "2021")]
	E2021,
	#[serde(rename = "2024")]
	E2024,
}

impl FromStr for Edition {
	type Err = Error;

	fn from_str(s: &str) -> Result<Self, Error> {
		match s {
			"2015" => Ok(Edition::E2015),
			"2018" => Ok(Edition::E2018),
			"2021" => Ok(Edition::E2021),
			"2024" => Ok(Edition::E2024),
			_ => bail!("invalid edition `{s}`"),
		}
	}
}

#[derive(Debug, Clone, Copy, Serialize)]
#[allow(unused)]
pub enum CrateType {
	#[serde(rename = "bin")]
	Binary,
	#[serde(rename = "lib")]
	Library,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
	Debug,
	Release,
}

impl FromStr for Mode {
	type Err = Error;

	fn from_str(s: &str) -> Result<Self, Error> {
		match s {
			"debug" => Ok(Mode::Debug),
			"release" => Ok(Mode::Release),
			_ => bail!("invalid compilation mode `{s}`"),
		}
	}
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AliasingModel {
	Stacked,
	Tree,
}

impl FromStr for AliasingModel {
	type Err = Error;

	fn from_str(s: &str) -> Result<Self, Error> {
		Ok(match s {
			"stacked" => AliasingModel::Stacked,
			"tree" => AliasingModel::Tree,
			_ => bail!("invalid aliasing model `{s}`"),
		})
	}
}

#[derive(Debug)]
pub struct PlayResult {
	pub success: bool,
	pub stdout: String,
	pub stderr: String,
}

impl PlayResult {
	/// Inserts invisible whitespace in sequences of more than 2 backticks to prevent
	/// escaping discord code blocks
	pub fn sanitize_backticks(&mut self) {
		if self.stdout.contains("```") {
			self.stdout = self.stdout.replace("``", "``\u{200b}");
		}

		if self.stderr.contains("```") {
			self.stderr = self.stderr.replace("``", "``\u{200b}");
		}
	}
}

impl<'de> Deserialize<'de> for PlayResult {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		// The playground occasionally sends just a single "error" field, for example with
		// `loop{println!("a")}`. We put the error into the stderr field

		#[derive(Deserialize)]
		#[serde(untagged)]
		pub enum RawPlayResponse {
			Err {
				error: String,
			},
			Ok {
				success: bool,
				stdout: String,
				stderr: String,
			},
		}

		Ok(match RawPlayResponse::deserialize(deserializer)? {
			RawPlayResponse::Ok {
				success,
				stdout,
				stderr,
			} => PlayResult {
				success,
				stdout,
				stderr,
			},
			RawPlayResponse::Err { error } => PlayResult {
				success: false,
				stdout: String::new(),
				stderr: error,
			},
		})
	}
}

/// Returns a gist ID
pub async fn post_gist(ctx: Context<'_>, code: &str) -> Result<String, Error> {
	let mut payload = HashMap::new();
	payload.insert("code", code);

	let resp = ctx
		.data()
		.http
		.post("https://play.rust-lang.org/meta/gist/")
		.header(header::REFERER, "https://discord.gg/rust-lang-community")
		.json(&payload)
		.send()
		.await?;

	let mut resp: HashMap<String, String> = resp.json().await?;
	info!("gist response: {:?}", resp);

	let gist_id = resp.remove("id").ok_or(anyhow!("no gist found"))?;
	Ok(gist_id)
}

pub fn url_from_gist(flags: &CommandFlags, gist_id: &str) -> String {
	format!(
		"https://play.rust-lang.org/?version={}&mode={}&edition={}&gist={}",
		match flags.channel {
			Channel::Nightly => "nightly",
			Channel::Beta => "beta",
			Channel::Stable => "stable",
		},
		match flags.mode {
			Mode::Debug => "debug",
			Mode::Release => "release",
		},
		match flags.edition {
			Edition::E2015 => "2015",
			Edition::E2018 => "2018",
			Edition::E2021 => "2021",
			Edition::E2024 => "2024",
		},
		gist_id
	)
}

pub async fn apply_online_rustfmt(
	ctx: Context<'_>,
	code: &str,
	edition: Edition,
) -> Result<PlayResult, Error> {
	let result = ctx
		.data()
		.http
		.post("https://play.rust-lang.org/format")
		.json(&FormatRequest { code, edition })
		.send()
		.await?
		.json::<FormatResponse>()
		.await?;

	Ok(PlayResult {
		success: result.success,
		stdout: result.code,
		stderr: result.stderr,
	})
}
