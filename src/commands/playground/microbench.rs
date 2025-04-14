use anyhow::Error;

use crate::types::Context;

use super::{
	api::{CrateType, Mode, PlayResult, PlaygroundRequest},
	util::{
		format_play_eval_stderr, generic_help, hoise_crate_attributes, parse_flags, send_reply,
		stub_message, GenericHelp,
	},
};

const BENCH_FUNCTION: &str = r#"
fn bench(functions: &[(&str, fn())]) {
	const CHUNK_SIZE: usize = 1000;

	// Warm up
	for (_, function) in functions.iter() {
		for _ in 0..CHUNK_SIZE {
			(function)();
		}
	}

	let mut functions_chunk_times = functions.iter().map(|_| Vec::new()).collect::<Vec<_>>();

	let start = std::time::Instant::now();
	while (start.elapsed()).as_secs() < 5 {
		for (chunk_times, (_, function)) in functions_chunk_times.iter_mut().zip(functions) {
			let start = std::time::Instant::now();
			for _ in 0..CHUNK_SIZE {
				(function)();
			}
			chunk_times.push(start.elapsed().as_secs_f64() / CHUNK_SIZE as f64);
		}
	}

	for (chunk_times, (function_name, _)) in functions_chunk_times.iter().zip(functions) {
		let mean_time: f64 = chunk_times.iter().sum::<f64>() / chunk_times.len() as f64;

		let mut sum_of_squared_deviations = 0.0;
		let mut n = 0;
		for &time in chunk_times {
			// Filter out outliers (there are some crazy outliers, I've checked)
			if time < mean_time * 3.0 {
				sum_of_squared_deviations += (time - mean_time).powi(2);
				n += 1;
			}
		}
		let standard_deviation = f64::sqrt(sum_of_squared_deviations / n as f64);

		println!(
			"{}: {:.1}ns ± {:.1}",
			function_name,
			mean_time * 1_000_000_000.0,
			standard_deviation * 1_000_000_000.0,
		);
	}
}
"#;

/// Benchmark small snippets of code
#[poise::command(
	prefix_command,
	track_edits,
	help_text_fn = "microbench_help",
	category = "Playground"
)]
pub async fn microbench(
	ctx: Context<'_>,
	flags: poise::KeyValueArgs,
	code: poise::CodeBlock,
) -> Result<(), Error> {
	ctx.say(stub_message(ctx)).await?;

	let user_code = &code.code;
	let black_box_hint = !user_code.contains("black_box");

	// insert convenience import for users
	let after_crate_attrs = "#[allow(unused_imports)] use std::hint::black_box;\n";

	let pub_fn_names: Vec<&str> = extract_pub_fn_names_from_user_code(user_code);
	match pub_fn_names.len() {
		0 => {
			ctx.say("No public functions (`pub fn`) found for benchmarking :thinking:")
				.await?;
			return Ok(());
		}
		1 => {
			ctx.say("Please include multiple functions. Times are not comparable across runs")
				.await?;
			return Ok(());
		}
		_ => {}
	};

	//

	// insert this after user code
	let mut after_code = BENCH_FUNCTION.to_owned();
	after_code += "fn main() {\nbench(&[";
	for function_name in pub_fn_names {
		after_code += "(\"";
		after_code += &function_name;
		after_code += "\", ";
		after_code += &function_name;
		after_code += "), ";
	}
	after_code += "]);\n}\n";

	// final assembled code
	let code = hoise_crate_attributes(user_code, after_crate_attrs, &after_code);

	let (flags, mut flag_parse_errors) = parse_flags(flags);
	let mut result: PlayResult = ctx
		.data()
		.http
		.post("https://play.rust-lang.org/execute")
		.json(&PlaygroundRequest {
			code: &code,
			channel: flags.channel,
			crate_type: CrateType::Binary,
			edition: flags.edition,
			mode: Mode::Release, // benchmarks on debug don't make sense
			tests: false,
		})
		.send()
		.await?
		.json()
		.await?;

	result.stderr = format_play_eval_stderr(&result.stderr, flags.warn);

	if black_box_hint {
		flag_parse_errors +=
			"Hint: use the black_box function to prevent computations from being optimized out\n";
	}
	send_reply(ctx, result, &code, &flags, &flag_parse_errors).await
}

#[must_use]
pub fn microbench_help() -> String {
	generic_help(GenericHelp {
		command: "microbench",
		desc: "\
Benchmarks small snippets of code by running them repeatedly. Public functions \
are run in blocks of 1000 repetitions in a cycle until 5 seconds have \
passed. Measurements are averaged and standard deviation is calculated for each

Use the `std::hint::black_box` function, which is already imported, to wrap results of \
computations that shouldn't be optimized out. Also wrap computation inputs in `black_box(...)` \
that should be opaque to the optimizer: `number * 2` produces optimized integer doubling assembly while \
`number * black_box(2)` produces a generic integer multiplication instruction",
		mode_and_channel: false,
		warn: true,
		run: false,
		example_code: "
pub fn add() {
    black_box(black_box(42.0) + black_box(99.0));
}
pub fn mul() {
    black_box(black_box(42.0) * black_box(99.0));
}
",
	})
}

fn extract_pub_fn_names_from_user_code(s: &str) -> Vec<&str> {
	let mut buf = vec![];
	let mut indent_level: u32 = 0;

	let mut in_string = false;
	let mut in_char = false;
	let mut in_line_comment = false;
	let mut in_block_comment = false;
	let mut escape_next = false;

	for (i, c) in s.char_indices() {
		if escape_next {
			escape_next = false;
			continue;
		}

		if (in_string || in_char) && c == '\\' {
			escape_next = true;
			continue;
		}

		if c == '"' && !in_line_comment && !in_block_comment && !in_char {
			in_string = !in_string;
			continue;
		}

		if c == '\'' && !in_line_comment && !in_block_comment && !in_string {
			in_char = !in_char;
			continue;
		}

		if !in_string && !in_char && !in_line_comment && !in_block_comment && c == '/' {
			if i + 1 < s.len() {
				if let Some(next_char) = s[i + 1..].chars().next() {
					if next_char == '/' {
						in_line_comment = true;
						continue;
					} else if next_char == '*' {
						in_block_comment = true;
						continue;
					}
				}
			}
		}

		if in_line_comment && (c == '\n' || c == '\r') {
			in_line_comment = false;
			continue;
		}

		if in_block_comment && c == '*' {
			if i + 1 < s.len() {
				if let Some(next_char) = s[i + 1..].chars().next() {
					if next_char == '/' {
						in_block_comment = false;
						continue;
					}
				}
			}
		}

		if !in_string && !in_char && !in_line_comment && !in_block_comment {
			if c == '{' {
				indent_level += 1;
			} else if c == '}' {
				indent_level = indent_level.saturating_sub(1);
			} else if c == 'p' && s[i..].starts_with("pub") && indent_level == 0 {
				let after_pub = &s[i + 3..];

				let Some(pos_of_fn) = after_pub.find("fn") else {
					continue;
				};

				let between_pub_fn = &after_pub[..pos_of_fn];
				if !between_pub_fn.chars().all(|c| c.is_whitespace()) {
					continue;
				}

				let after_fn = &after_pub[pos_of_fn + 2..];

				let name_start = after_fn.find(|c: char| c.is_alphanumeric() || c == '_');
				let Some(name_start) = name_start else {
					continue;
				};

				let name_end_offset =
					after_fn[name_start..].find(|c: char| !(c.is_alphanumeric() || c == '_'));
				let name_end = match name_end_offset {
					Some(offset) => name_start + offset,
					None => after_fn.len(),
				};

				let fn_name = &after_fn[name_start..name_end];

				let after_name = &after_fn[name_end..];
				let paren_pos = after_name.find('(');

				if let Some(paren_pos) = paren_pos {
					let between_name_paren = &after_name[..paren_pos];
					if between_name_paren.chars().all(|c| c.is_whitespace()) {
						buf.push(fn_name);
					}
				}
			}
		}
	}

	buf
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_raw_string_with_braces() {
		let code = r##"
            pub fn valid_function() {}
            let s = r#"pub fn fake_function() {}"#;
            pub fn another_valid_function() {}
        "##;
		assert_eq!(
			extract_pub_fn_names_from_user_code(code),
			vec!["valid_function", "another_valid_function"]
		);
	}

	#[test]
	fn test_nested_function_visibility() {
		let code = r#"
            pub fn outer() {
                fn inner() {}
                pub fn not_visible() {}
            }
        "#;
		assert_eq!(extract_pub_fn_names_from_user_code(code), vec!["outer"]);
	}

	#[test]
	fn test_comments_with_function_declarations() {
		let code = r#"
            // pub fn not_a_real_function() {}
            pub fn real_function() {}
            /* pub fn also_not_real() {} */
            /* multi-line
               pub fn still_not_real() {}
               comment */
            pub fn another_real_function() {}
        "#;
		assert_eq!(
			extract_pub_fn_names_from_user_code(code),
			vec!["real_function", "another_real_function"]
		);
	}

	#[test]
	fn test_string_literals_with_function_declarations() {
		let code = r#"
            let s = "pub fn fake_function() {}";
            pub fn real_function() {}
            let complex = "nested \"pub fn also_fake() {}\" string";
            pub fn another_real_function() {}
        "#;
		assert_eq!(
			extract_pub_fn_names_from_user_code(code),
			vec!["real_function", "another_real_function"]
		);
	}

	#[test]
	fn test_various_whitespace_patterns() {
		let code = r#"
            pub fn normal() {}
            pub    fn extra_spaces() {}
            pub
            fn
            newline_between
            () {}
            pub fn comment_between() {}
        "#;
		assert_eq!(
			extract_pub_fn_names_from_user_code(code),
			vec![
				"normal",
				"extra_spaces",
				"newline_between",
				"comment_between"
			]
		);
	}

	#[test]
	fn test_unicode_identifiers() {
		let code = r#"
            pub fn α_alpha() {}
            pub fn こんにちは_hello() {}
        "#;
		assert_eq!(
			extract_pub_fn_names_from_user_code(code),
			vec!["α_alpha", "こんにちは_hello"]
		);
	}

	#[test]
	fn test_underscore_patterns() {
		let code = r#"
            pub fn _leading_underscore() {}
            pub fn with_multiple_underscores_in_name() {}
            pub fn trailing_underscore_() {}
        "#;
		assert_eq!(
			extract_pub_fn_names_from_user_code(code),
			vec![
				"_leading_underscore",
				"with_multiple_underscores_in_name",
				"trailing_underscore_"
			]
		);
	}

	#[test]
	fn test_pub_in_other_contexts() {
		let code = r#"
            struct Republic;
            fn republic_function() {}
            pub fn actual_function() {}
            let republic = Republic;
            fn public_but_not_pub() {}
            pub fn another() {}
        "#;
		assert_eq!(
			extract_pub_fn_names_from_user_code(code),
			vec!["actual_function", "another"]
		);
	}

	#[test]
	fn test_escaped_quotes_and_special_chars() {
		let code = r#"
            let s = "escaped quote \"pub fn fake() {}\"";
            pub fn real_function() {}
            let t = "backslash \\ pub fn also_fake() {}";
            pub fn another_real_function() {}
        "#;
		assert_eq!(
			extract_pub_fn_names_from_user_code(code),
			vec!["real_function", "another_real_function"]
		);
	}

	#[test]
	fn test_complex_nested_structures() {
		let code = r#"
            pub fn outer() {
                { { { 
                    // deeply nested
                    pub fn inner() {} // this shouldn't be found
                } } }
                "{{{}}}" // braces in string
                /* { pub fn in_comment() {} } */
            }
            pub fn another() {
                r#"raw string with { pub fn fake() {} };
            }
        "#;
		assert_eq!(
			extract_pub_fn_names_from_user_code(code),
			vec!["outer", "another"]
		);
	}
}
