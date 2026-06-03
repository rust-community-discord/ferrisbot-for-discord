use std::time::{Duration, Instant};

use poise::serenity_prelude::{ChannelId, UserId};

use crate::commands::highlight::HighlightConfig;

use super::{HighlightCooldowns, RegexHolder, sanitize_content};

const WINDOW: Duration = Duration::from_mins(1);
const CONFIG: HighlightConfig = HighlightConfig { cooldown: WINDOW };

fn holder(patterns: &[(u64, &str)]) -> RegexHolder {
	RegexHolder::from_patterns(
		patterns
			.iter()
			.map(|&(id, pattern)| (UserId::new(id), pattern.to_owned())),
	)
}

#[test]
fn sanitize_strips_custom_emoji_markup() {
	assert!(!sanitize_content("meow <a:ferrisPet:755501265454366801>!").contains("ferris"));
	assert!(!sanitize_content("<:conradBall:1508889024344100985>").contains("conrad"));
}

#[test]
fn sanitize_keeps_plain_text_and_unicode_emoji() {
	assert_eq!(sanitize_content("plain text 🦀"), "plain text 🦀");
}

#[test]
fn matching_is_case_insensitive_by_default() {
	let hl = holder(&[(1, "rust")]);
	assert!(
		hl.find("ferrisbot is programmed in Rust")
			.contains_key(&UserId::new(1))
	);
}

#[test]
fn case_sensitivity_can_be_opted_out() {
	let hl = holder(&[(1, "(?-i)rust")]);
	assert!(hl.find("Rust").is_empty());
	assert!(hl.find("rust").contains_key(&UserId::new(1)));
}

#[test]
fn custom_emoji_names_do_not_match() {
	let hl = holder(&[(1, "ferris")]);
	assert!(hl.find("<:ferrisClueless:1180624887707074641>").is_empty());
	assert!(
		hl.find("hey, ferris is cute!")
			.contains_key(&UserId::new(1))
	);
}

#[test]
fn find_for_user_returns_only_the_authors_matches() {
	let hl = holder(&[(1, "foo"), (1, "bar"), (2, "foo")]);
	let mut m = hl.find_for_user(UserId::new(1), "foo bar baz");
	m.sort();
	assert_eq!(m, vec!["bar".to_string(), "foo".to_string()]);
	assert!(hl.find_for_user(UserId::new(3), "foo").is_empty());
}

#[test]
fn find_for_user_sanitizes_custom_emoji() {
	let hl = holder(&[(1, "ferris")]);
	assert!(
		hl.find_for_user(UserId::new(1), "<:ferrisClueless:1180624887707074641>")
			.is_empty()
	);
}

#[test]
fn invalid_patterns_are_skipped() {
	let hl = holder(&[(1, "("), (2, "valid")]);
	let recipients = hl.find("this is valid");
	assert!(recipients.contains_key(&UserId::new(2)));
	assert!(!recipients.contains_key(&UserId::new(1)));
}

#[test]
fn try_notify_respects_window() {
	let mut cooldowns = HighlightCooldowns::new(CONFIG);
	let (user, channel) = (UserId::new(1), ChannelId::new(2));
	let now = Instant::now();

	assert!(cooldowns.try_notify(user, channel, now));
	assert!(!cooldowns.try_notify(user, channel, now));

	let after = now + WINDOW + Duration::from_millis(1);
	assert!(cooldowns.try_notify(user, channel, after));
}

#[test]
fn activity_suppresses_notification() {
	let mut cooldowns = HighlightCooldowns::new(CONFIG);
	let (user, channel) = (UserId::new(1), ChannelId::new(2));
	let now = Instant::now();

	cooldowns.mark_active(user, channel, now);
	assert!(!cooldowns.try_notify(user, channel, now));
}

#[test]
fn expired_entries_are_pruned_when_sweep_fires() {
	let mut cooldowns = HighlightCooldowns::new(CONFIG);
	let now = Instant::now();
	cooldowns.mark_active(UserId::new(1), ChannelId::new(1), now);
	assert_eq!(cooldowns.expiries.len(), 1);

	let later = now + WINDOW + Duration::from_secs(1);
	cooldowns.mark_active(UserId::new(2), ChannelId::new(2), later);

	assert_eq!(cooldowns.expiries.len(), 1);
	assert!(
		cooldowns
			.expiries
			.contains_key(&(UserId::new(2), ChannelId::new(2)))
	);
}

#[test]
fn pruning_is_amortised_within_window() {
	let mut cooldowns = HighlightCooldowns::new(CONFIG);
	let now = Instant::now();
	cooldowns.mark_active(UserId::new(1), ChannelId::new(1), now);

	// Already-expired entry must survive a sweep-free (amortised) call.
	cooldowns
		.expiries
		.insert((UserId::new(9), ChannelId::new(9)), now);
	cooldowns.mark_active(UserId::new(2), ChannelId::new(2), now);

	assert!(
		cooldowns
			.expiries
			.contains_key(&(UserId::new(9), ChannelId::new(9)))
	);
}
