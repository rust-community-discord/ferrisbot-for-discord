use std::{collections::HashMap, mem::take};

use anyhow::{Error, anyhow};
use poise::{CodeBlockError, KeyValueArgs};
use syn::spanned::Spanned;
use tracing::warn;

use crate::types::Context;

mod targets;
pub use targets::*;

const LLVM_MCA_TOOL_ID: &str = "llvm-mcatrunk";

/// Returns the tools JSON array for Godbolt requests.
/// If `run_llvm_mca` is true, includes the llvm-mca tool; otherwise returns an empty array.
fn make_tools_json(run_llvm_mca: bool) -> serde_json::Value {
	if run_llvm_mca {
		serde_json::json!([{"id": LLVM_MCA_TOOL_ID}])
	} else {
		serde_json::json!([])
	}
}

struct Compilation {
	output: String,
	stderr: String,
}

#[derive(Debug, serde::Deserialize)]
struct GodboltOutputSegment {
	text: String,
}

#[derive(Debug, serde::Deserialize)]
struct GodboltOutput(Vec<GodboltOutputSegment>);

impl GodboltOutput {
	pub fn concatenate(&self) -> String {
		let mut complete_text = String::new();
		for segment in &self.0 {
			complete_text.push_str(&segment.text);
			complete_text.push('\n');
		}
		complete_text
	}
}

#[derive(Debug, serde::Deserialize)]
struct GodboltResponse {
	// stdout: GodboltOutput,
	stderr: GodboltOutput,
	asm: GodboltOutput,
	tools: Vec<GodboltTool>,
}

#[derive(Debug, serde::Deserialize)]
struct GodboltTool {
	id: String,
	// code: u8,
	stdout: GodboltOutput,
	// stderr: GodboltOutput,
}

struct GodboltRequest<'a> {
	source_code: &'a str,
	rustc: &'a str,
	flags: &'a str,
	run_llvm_mca: bool,
}

/// Compile a given Rust source code file on Godbolt using the latest nightly compiler with
/// full optimizations (-O3)
/// Returns a multiline string with the pretty printed assembly
async fn compile_rust_source(
	http: &reqwest::Client,
	request: &GodboltRequest<'_>,
) -> Result<Compilation, Error> {
	let tools = make_tools_json(request.run_llvm_mca);

	let http_request = http
		.post(format!(
			"https://godbolt.org/api/compiler/{}/compile",
			request.rustc
		))
		.header(reqwest::header::ACCEPT, "application/json") // to make godbolt respond in JSON
		.json(&serde_json::json! { {
            "source": request.source_code,
            "options": {
                "userArguments": request.flags,
                "tools": tools,
                // "libraries": [{"id": "itoa", "version": "102"}],
            },
        } })
		.build()?;

	let response: GodboltResponse = http.execute(http_request).await?.json().await?;

	// TODO: use the extract_relevant_lines utility to strip stderr nicely
	Ok(Compilation {
		output: if request.run_llvm_mca {
			let text = response
				.tools
				.iter()
				.find(|tool| tool.id == LLVM_MCA_TOOL_ID)
				.map(|llvm_mca| llvm_mca.stdout.concatenate())
				.ok_or(anyhow!("No llvm-mca result was sent by Godbolt"))?;
			// Strip junk
			text[..text.find("Instruction Info").unwrap_or(text.len())]
				.trim()
				.to_string()
		} else {
			response.asm.concatenate()
		},
		stderr: response.stderr.concatenate(),
	})
}

async fn save_to_shortlink(http: &reqwest::Client, req: &GodboltRequest<'_>) -> String {
	#[derive(serde::Deserialize)]
	struct GodboltShortenerResponse {
		url: String,
	}

	let tools = make_tools_json(req.run_llvm_mca);

	let request = http
		.post("https://godbolt.org/api/shortener")
		.json(&serde_json::json! { {
			"sessions": [{
				"language": "rust",
				"source": req.source_code,
				"compilers": [{
					"id": req.rustc,
					"options": req.flags,
					"tools": tools,
				}],
			}]
		} });

	// Try block substitute
	let url = async move {
		Ok::<_, crate::Error>(
			request
				.send()
				.await?
				.json::<GodboltShortenerResponse>()
				.await?
				.url,
		)
	};
	url.await.unwrap_or_else(|e| {
		warn!("failed to generate godbolt shortlink: {}", e);
		"failed to retrieve".to_owned()
	})
}

#[derive(PartialEq, Clone, Copy)]
#[allow(unused)]
enum GodboltMode {
	Asm,
	LlvmIr,
	Mca,
}

fn note(no_mangle_added: bool) -> &'static str {
	if no_mangle_added {
		""
	} else {
		"Note: only `pub fn` at file scope are shown"
	}
}

fn add_no_mangle(code: &mut String) -> bool {
	let mut no_mangle_added = false;
	if let Ok(file) = syn::parse_str::<syn::File>(code) {
		let mut spans = vec![];
		for item in &file.items {
			let syn::Item::Fn(function) = item else {
				continue;
			};
			let syn::Visibility::Public(_) = function.vis else {
				continue;
			};

			// could check for existing `#[unsafe(no_mangle)]` attributes before adding it here
			spans.push(function.span());
			no_mangle_added = true;
		}

		// iterate in reverse so that the indices dont get messed up
		for span in spans.iter().rev() {
			let range = span.byte_range();
			code.insert_str(range.start, "#[unsafe(no_mangle)] ");
		}
	}
	no_mangle_added
}

async fn respond_codeblocks(
	ctx: Context<'_>,
	godbolt_result: Compilation,
	godbolt_request: GodboltRequest<'_>,
	lang: &'static str,
	note: &str,
) -> Result<(), Error> {
	match (godbolt_result.output.trim(), godbolt_result.stderr.trim()) {
		("", "") => respond_codeblock(ctx, "", " ", note, &godbolt_request).await?,
		(output, "") => respond_codeblock(ctx, lang, output, note, &godbolt_request).await?,
		("<Compilation failed>", errors) => {
			respond_codeblock(ctx, "ansi", errors, "Compilation failed.", &godbolt_request).await?;
		}
		("", warnings) => respond_codeblock(ctx, "ansi", warnings, note, &godbolt_request).await?,
		(output, errors) => {
			ctx.say(
				crate::helpers::trim_text(
					&format!("```{lang}\n{output}``````ansi\n{errors}"),
					&format!("\n```{note}"),
					async {
						format!(
							"Output too large. Godbolt link: <{}>",
							save_to_shortlink(&ctx.data().http, &godbolt_request).await,
						)
					},
				)
				.await,
			)
			.await?;
		}
	}
	Ok(())
}

async fn respond_codeblock(
	ctx: Context<'_>,
	codeblock_lang: &str,
	text: &str,
	note: &str,
	godbolt_request: &GodboltRequest<'_>,
) -> Result<(), Error> {
	ctx.say(
		crate::helpers::trim_text(
			&format!("```{codeblock_lang}\n{text}"),
			&format!("\n```{note}"),
			async {
				format!(
					"Output too large. Godbolt link: <{}>",
					save_to_shortlink(&ctx.data().http, godbolt_request).await,
				)
			},
		)
		.await,
	)
	.await?;
	Ok(())
}

fn parse(args: &str) -> Result<(KeyValueArgs, String), CodeBlockError> {
	let mut map = HashMap::new();
	let mut key = String::new();
	let mut value = String::new();
	// flag for in a key
	let mut k = true;
	let mut args = args.chars();
	let mut tick_count = 0;
	// note: you cant put a backtick in an argument
	for ch in args.by_ref() {
		match ch {
			'`' => {
				tick_count += 1;
				break;
			}
			' ' | '\n' => {
				map.insert(take(&mut key), take(&mut value));
				k = true;
			}
			'=' if k => k = false,
			c if k => key.push(c),
			c => value.push(c),
		}
	}

	// note: language can be parsed, but is discarded here
	let mut parsed_lang = false;
	let mut code = String::new();
	for ch in args {
		match ch {
			// ```
			'`' if tick_count == 3 && !parsed_lang => return Err(CodeBlockError::default()),
			// closing
			'`' if tick_count == 3 && parsed_lang => break,
			'`' => tick_count += 1,
			// ```rust
			//    ^^^^
			'\n' if tick_count == 3 && !parsed_lang => parsed_lang = true,
			_ if tick_count == 3 && !parsed_lang => {}

			c => code.push(c),
		}
	}
	Ok((KeyValueArgs(map), code))
}

/// View assembly using Godbolt
///
/// Compile Rust code using <https://rust.godbolt.org>. Full optimizations are applied unless \
/// overriden.
/// ```
/// ?godbolt $($flags )* rustc={} ``​`
/// pub fn your_function() {
///     // Code
/// }
/// ``​`
/// ```
/// Optional arguments:
/// - `flags*`: flags to pass to rustc invocation. Defaults to ["-Copt-level=3", "--edition=2024"]
/// - `rustc`: compiler version to invoke. Defaults to `nightly`. Possible values: `nightly`, `beta` or full version like `1.45.2`
#[expect(
	clippy::doc_link_with_quotes,
	reason = "not markdown, shown to end user"
)]
#[poise::command(prefix_command, category = "Godbolt", broadcast_typing, track_edits)]
pub async fn godbolt(ctx: Context<'_>, #[rest] arguments: String) -> Result<(), Error> {
	let (params, mut code) = parse(&arguments)?;
	let no_mangle_added = add_no_mangle(&mut code);
	let hl = params
		.get("--emit")
		.map(|emit| match emit {
			"llvmir" => "llvm",
			"dep-info" | "link" | "metadata" | "obj" | "llvm-bc" => "",
			"mir" => "rust",
			_ => "x86asm",
		})
		.or(params
			.get("--target")
			.map(|target| match target.split('-').next() {
				Some("aarch64") => "arm",
				Some(x) if x.starts_with("arm") => "arm",
				Some(x) if x.starts_with("mips") || x.starts_with("riscv") => "mips",
				Some("wasm32" | "wasm64") => "wasm",
				Some("x86_64" | _) => "x86asm",
				None => "", // ??? (0 valid targets here)
			}))
		.unwrap_or("x86asm");
	let (rustc, flags) = rustc_id_and_flags(ctx.data(), &params).await?;
	let godbolt_request = GodboltRequest {
		source_code: &code,
		rustc: &rustc,
		flags: &flags,
		run_llvm_mca: false,
	};
	let godbolt_result = compile_rust_source(&ctx.data().http, &godbolt_request).await?;

	let note = note(no_mangle_added);
	respond_codeblocks(ctx, godbolt_result, godbolt_request, hl, note).await
}

/// Run performance analysis using llvm-mca
///
/// Run the performance analysis tool llvm-mca using <https://rust.godbolt.org>. Full optimizations \
/// are applied unless overriden.
/// ```
/// ?mca $($flags )* rustc={} ``​`
/// pub fn your_function() {
///     // Code
/// }
/// ``​`
/// ```
/// Optional arguments:
/// - `flags*`: flags to pass to rustc invocation. Defaults to ["-Copt-level=3", "--edition=2024"]
/// - `rustc`: compiler version to invoke. Defaults to `nightly`. Possible values: `nightly`, `beta` or full version like `1.45.2`
#[expect(
	clippy::doc_link_with_quotes,
	reason = "not markdown, shown to end user"
)]
#[poise::command(prefix_command, category = "Godbolt", broadcast_typing, track_edits)]
pub async fn mca(ctx: Context<'_>, #[rest] arguments: String) -> Result<(), Error> {
	let (params, mut code) = parse(&arguments)?;
	let no_mangle_added = add_no_mangle(&mut code);
	let (rustc, flags) = rustc_id_and_flags(ctx.data(), &params).await?;
	let godbolt_request = GodboltRequest {
		source_code: &code,
		rustc: &rustc,
		flags: &flags,
		run_llvm_mca: true,
	};

	let godbolt_result = compile_rust_source(&ctx.data().http, &godbolt_request).await?;

	let note = note(no_mangle_added);
	respond_codeblocks(ctx, godbolt_result, godbolt_request, "rust", note).await
}

/// View LLVM IR using Godbolt
///
/// Compile Rust code using <https://rust.godbolt.org> and emits LLVM IR. Full optimizations \
/// are applied unless overriden.
///
/// Equivalent to ?godbolt but with extra flags `--emit=llvm-ir -Cdebuginfo=0`.
/// ```
/// ?llvmir $($flags )* rustc={} ``​`
/// pub fn your_function() {
///     // Code
/// }
/// ``​`
/// ```
/// Optional arguments:
/// - `flags*`: flags to pass to rustc invocation. Defaults to ["-Copt-level=3", "--edition=2024"]
/// - `rustc`: compiler version to invoke. Defaults to `nightly`. Possible values: `nightly`, `beta` or full version like `1.45.2`
#[expect(
	clippy::doc_link_with_quotes,
	reason = "not markdown, shown to end user"
)]
#[poise::command(prefix_command, category = "Godbolt", broadcast_typing, track_edits)]
pub async fn llvmir(ctx: Context<'_>, #[rest] arguments: String) -> Result<(), Error> {
	let (params, mut code) = parse(&arguments)?;
	let no_mangle_added = add_no_mangle(&mut code);
	let (rustc, flags) = rustc_id_and_flags(ctx.data(), &params).await?;
	let godbolt_request = GodboltRequest {
		source_code: &code,
		rustc: &rustc,
		flags: &(flags + " --emit=llvm-ir -Cdebuginfo=0"),
		run_llvm_mca: false,
	};
	let godbolt_result = compile_rust_source(&ctx.data().http, &godbolt_request).await?;

	let note = note(no_mangle_added);
	respond_codeblocks(ctx, godbolt_result, godbolt_request, "llvm", note).await
}
