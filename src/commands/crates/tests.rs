struct MockDocsClient;

impl super::DocsClient for MockDocsClient {
	async fn page_exists(&self, url: &str) -> bool {
		match url {
			"https://doc.rust-lang.org/stable/std/char/constant.MAX.html"
			| "https://docs.rs/serde-json/latest/serde_json/fn.to_string.html"
			| "https://docs.rs/serde-json/latest/serde_json/value/index.html"
			| "https://docs.rs/serde-json/latest/serde_json/value/trait.Index.html" => true,
			"https://doc.rust-lang.org/stable/std/char/static.MAX.html"
			| "https://docs.rs/serde-json/latest/serde_json/macro.to_string.html"
			| "https://docs.rs/serde-json/latest/serde_json/to_string/index.html"
			| "https://docs.rs/serde-json/latest/serde_json/macro.value.html"
			| "https://docs.rs/serde-json/latest/serde_json/fn.value.html"
			| "https://docs.rs/serde-json/latest/serde_json/value/struct.Index.html"
			| "https://docs.rs/serde-json/latest/serde_json/value/enum.Index.html"
			| "https://docs.rs/serde-json/latest/serde_json/value/union.Index.html"
			| "https://docs.rs/serde-json/latest/serde_json/value/traitalias.Index.html"
			| "https://docs.rs/serde-json/latest/serde_json/value/type.Index.html"
			| "https://docs.rs/serde-json/latest/serde_json/value/derive.Index.html" => false,
			_ if url.starts_with("https://docs.rs/serde-json/latest/serde_json/non/existent") => {
				false
			}
			_ => panic!("unexpected query {url:?}"),
		}
	}

	async fn get_crate_docs(&self, crate_name: &str) -> anyhow::Result<String> {
		match crate_name {
			"serde-json" => Ok("https://docs.rs/serde-json".to_owned()),
			_ => panic!("unexpected query {crate_name:?}"),
		}
	}
}

#[tokio::test]
async fn path_to_doc_url_stable_std() {
	test_path_to_doc_url("std", "https://doc.rust-lang.org/stable/std/").await;
}

#[tokio::test]
async fn path_to_doc_url_self_ty() {
	test_path_to_doc_url(
		"Self",
		"https://doc.rust-lang.org/stable/std/keyword.SelfTy.html",
	)
	.await;
}

#[tokio::test]
async fn path_to_doc_url_std_primitive() {
	test_path_to_doc_url(
		"f128",
		"https://doc.rust-lang.org/nightly/std/primitive.f128.html",
	)
	.await;
}

#[tokio::test]
async fn path_to_doc_url_nightly_std() {
	test_path_to_doc_url("nightly", "https://doc.rust-lang.org/nightly/std/").await;
}

#[tokio::test]
async fn path_to_doc_url_stable_guessed_const() {
	test_path_to_doc_url(
		"std::char::MAX",
		"https://doc.rust-lang.org/stable/std/char/constant.MAX.html",
	)
	.await;
}

#[tokio::test]
async fn path_to_doc_url_crate_docs() {
	test_path_to_doc_url("serde-json", "https://docs.rs/serde-json").await;
}

#[tokio::test]
async fn path_to_doc_url_guessed_fn() {
	test_path_to_doc_url(
		"serde-json::to_string",
		"https://docs.rs/serde-json/latest/serde_json/fn.to_string.html",
	)
	.await;
}

#[tokio::test]
async fn path_to_doc_url_guessed_mod() {
	test_path_to_doc_url(
		"serde-json::value",
		"https://docs.rs/serde-json/latest/serde_json/value/index.html",
	)
	.await;
}

#[tokio::test]
async fn path_to_doc_url_guessed_trait() {
	test_path_to_doc_url(
		"serde-json::value::Index",
		"https://docs.rs/serde-json/latest/serde_json/value/trait.Index.html",
	)
	.await;
}

#[tokio::test]
async fn path_to_doc_url_search() {
	test_path_to_doc_url(
		"serde-json::non::existent::symbol",
		"https://docs.rs/serde-json?search=non::existent::symbol",
	)
	.await;
}

async fn test_path_to_doc_url(path: &str, expect: &str) {
	assert_eq!(
		super::path_to_doc_url(path, &MockDocsClient).await.unwrap(),
		expect,
		"{path} should resolve to {expect}",
	);
}
