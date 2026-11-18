//! Real digest comparison/cache/auth behavior against Docker and HTTP SDK doubles.
use super::*;
use crate::{unit_bollard as sdk, unit_registry_http as http};
use std::sync::Mutex;
fn docker() -> (Docker, sdk::SharedState) {
    let state = Arc::new(Mutex::new(sdk::State::default()));
    (Docker(state.clone()), state)
}
fn image(state: &sdk::SharedState, name: &str, digest: Option<&str>) {
    state.lock().unwrap().images.push_back((
        name.into(),
        Ok(sdk::models::ImageInspect {
            repo_digests: digest.map(|d| vec![format!("unit@{d}")]),
            ..Default::default()
        }),
    ));
}
fn digest(value: &[u8]) -> http::Response {
    let mut response = http::Response::new(200, "");
    response.headers.insert(
        "Docker-Content-Digest",
        http::header::HeaderValue::from_bytes(value).unwrap(),
    );
    response
}
#[tokio::test]
async fn auth_url_construction_status_json_and_missing_token() {
    http::reset();
    let collector = VersionsCollector::default();
    for (registry,url) in [
 ("registry-1.docker.io","https://auth.docker.io/token?service=registry.docker.io&scope=repository:unit/app:pull"),
 ("quay.io","https://quay.io/v2/auth?service=quay.io&scope=repository:unit/app:pull"),
 ("gcr.io","https://gcr.io/v2/token?service=gcr.io&scope=repository:unit/app:pull"),
 ("ghcr.io","https://ghcr.io/token?service=ghcr.io&scope=repository:unit/app:pull")]{
  http::reply(Ok(http::Response::new(200,"{\"token\":\"unit-token\"}")));
  assert_eq!(collector.fetch_token(registry,"unit/app").await.as_deref(),Some("unit-token"));
  http::STATE.with(|s|{let s=s.borrow();let call=s.calls.last().unwrap();assert_eq!(call.method,"GET");assert_eq!(call.url,url);assert_eq!(call.timeout,Duration::from_secs(10));});
 }
    for response in [
        Ok(http::Response::new(403, "")),
        Err(std::io::Error::other("unit send refused")),
        Ok(http::Response::new(200, "invalid JSON")),
        Ok(http::Response::new(200, "{}")),
        Ok(http::Response::new(200, "{\"token\":7}")),
    ] {
        http::reply(response);
        assert!(collector.fetch_token("ghcr.io", "unit/app").await.is_none());
    }
    assert!(collector
        .fetch_token("fixture.invalid", "unit")
        .await
        .is_none());
}
#[tokio::test]
async fn manifest_requests_preserve_accept_auth_timeout_and_all_failure_contexts() {
    http::reset();
    let collector = VersionsCollector::default();
    http::reply(Ok(http::Response::new(200, "{\"token\":\"unit-token\"}")));
    http::reply(Ok(digest(b"sha256:unit")));
    assert_eq!(
        collector
            .fetch_remote_digest("ghcr.io", "unit/app", "stable")
            .await
            .unwrap(),
        "sha256:unit"
    );
    http::STATE.with(|s| {
        let s = s.borrow();
        let call = s.calls.last().unwrap();
        assert_eq!(call.url, "https://ghcr.io/v2/unit/app/manifests/stable");
        assert_eq!(call.method, "HEAD");
        assert_eq!(call.timeout, Duration::from_secs(15));
        assert_eq!(
            call.headers,
            [
                ("Accept".into(), MANIFEST_ACCEPT_HEADER.into()),
                ("Authorization".into(), "Bearer unit-token".into())
            ]
        );
    });
    for (reply, prefix) in [
        (
            Err(std::io::Error::other("unit offline")),
            "Network error: unit offline",
        ),
        (
            Ok(http::Response::new(403, "")),
            "Registry returned HTTP 403",
        ),
        (
            Ok(http::Response::new(200, "")),
            "Missing Docker-Content-Digest",
        ),
        (Ok(digest(b"\xff")), "Invalid header:"),
    ] {
        http::reply(reply);
        assert!(collector
            .fetch_remote_digest("fixture.invalid", "unit", "tag")
            .await
            .unwrap_err()
            .starts_with(prefix));
    }
}
#[tokio::test]
async fn comparison_skip_cache_expiry_and_poisoned_locks() {
    http::reset();
    let (docker, state) = docker();
    let skip = VersionsCollector::new(Duration::from_secs(1), vec!["skip".into()]);
    assert_eq!(
        skip.check_image(&docker, "skip").await.status,
        VersionStatus::Unknown
    );
    assert!(state.lock().unwrap().calls.is_empty());
    let collector = VersionsCollector::default();
    for (name, local, remote, status) in [
        (
            "fixture.invalid/unit:same",
            Some("sha256:1"),
            Some("sha256:1"),
            VersionStatus::UpToDate,
        ),
        (
            "fixture.invalid/unit:drift",
            Some("sha256:1"),
            Some("sha256:2"),
            VersionStatus::Drifted,
        ),
        (
            "fixture.invalid/unit:local",
            None,
            None,
            VersionStatus::LocalBuild,
        ),
        (
            "fixture.invalid/unit:error",
            Some("sha256:1"),
            None,
            VersionStatus::Unknown,
        ),
    ] {
        image(&state, name, local);
        if let Some(remote) = remote {
            http::reply(Ok(digest(remote.as_bytes())));
        } else if local.is_some() {
            http::reply(Err(std::io::Error::other("unit registry offline")));
        }
        let result = collector.check_image(&docker, name).await;
        assert_eq!(result.status, status);
        assert_eq!(result.local_digest.as_deref(), local);
        assert_eq!(result.remote_digest.as_deref(), remote);
        assert!(result.checked_at.is_some());
        assert_eq!(result.error.is_some(), status == VersionStatus::Unknown);
        assert_eq!(
            collector.check_image(&docker, name).await.status,
            status,
            "fresh result must be cached"
        );
    }
    let expired = VersionsCollector::new(Duration::ZERO, vec![]);
    for _ in 0..2 {
        image(&state, "fixture.invalid/unit:expired", None);
        assert_eq!(
            expired
                .check_image(&docker, "fixture.invalid/unit:expired")
                .await
                .status,
            VersionStatus::LocalBuild
        );
    }
    let poisoned = VersionsCollector::default();
    let lock = poisoned.cache.clone();
    assert!(std::thread::spawn(move || {
        let _guard = lock.write().unwrap();
        panic!("unit cache poisoned");
    })
    .join()
    .is_err());
    assert_eq!(
        poisoned
            .check_image(&docker, "unit:inspect-error")
            .await
            .status,
        VersionStatus::LocalBuild
    );
    http::STATE.with(|s| assert!(s.borrow().responses.is_empty()));
}
#[test]
fn parser_and_digest_fallbacks_cover_empty_references_and_nonmatching_repositories() {
    assert_eq!(
        parse_image_ref(" "),
        (
            "registry-1.docker.io".into(),
            "library/unknown".into(),
            "latest".into()
        )
    );
    assert_eq!(
        parse_image_ref("registry.invalid/unit"),
        ("registry.invalid".into(), "unit".into(), "latest".into())
    );
    let entries = vec!["other@sha256:1".into(), "repo@sha256:2".into()];
    assert_eq!(
        VersionsCollector::extract_local_digest(Some(&entries), "unit"),
        Some("sha256:1".into())
    );
    assert_eq!(
        VersionsCollector::extract_local_digest(Some(&["malformed".into()]), "unit"),
        None
    );
    let _ = VersionsCollector::extract_local_digest(
        Some(&["malformed".into(), "repo@sha256:2".into()]),
        "repo",
    );
    http::reset();
    http::STATE.with(|s| s.borrow_mut().build_error = true);
    let _ = VersionsCollector::default();
    http::STATE.with(|s| assert_eq!(s.borrow().builder_timeouts, [Duration::from_secs(15)]));
    http::reset();
}
fn tags(list: &[&str]) -> http::Response {
    let body = serde_json::json!({ "name": "unit", "tags": list }).to_string();
    http::Response::new(200, &body)
}
fn tags_page(list: &[&str], next: &str) -> http::Response {
    let mut response = tags(list);
    response.headers.insert(
        "Link",
        http::header::HeaderValue::from_str(&format!("<{next}>; rel=\"next\"")).unwrap(),
    );
    response
}
/// Pinned image whose digest matches: the registry is asked for its tags.
async fn pinned(
    collector: &VersionsCollector,
    docker: &Docker,
    state: &sdk::SharedState,
    name: &str,
    listing: io_result::Reply,
) -> ImageVersionInfo {
    image(state, name, Some("sha256:1"));
    http::reply(Ok(digest(b"sha256:1")));
    http::reply(listing);
    collector.check_image(docker, name).await
}
mod io_result {
    pub type Reply = std::io::Result<super::http::Response>;
}
#[tokio::test]
async fn pinned_tags_report_the_newest_tag_in_their_line() {
    http::reset();
    let (docker, state) = docker();
    let collector = VersionsCollector::default();
    let line = [
        "26.0", "26.1", "26.3", "26.4", "26.5-rc1", "27.0", "latest", "26.4.2",
    ];
    let info = pinned(
        &collector,
        &docker,
        &state,
        "fixture.invalid/sso:26.1",
        Ok(tags(&line)),
    )
    .await;
    assert_eq!(info.status, VersionStatus::NewerAvailable);
    assert_eq!(info.latest_tag.as_deref(), Some("26.4"));
    assert_eq!(info.remote_digest.as_deref(), Some("sha256:1"));
    assert!(info.error.is_none());
    http::STATE.with(|s| {
        let s = s.borrow();
        let call = s.calls.last().unwrap();
        assert_eq!(call.method, "GET");
        assert_eq!(call.url, "https://fixture.invalid/v2/sso/tags/list?n=1000");
        assert_eq!(call.timeout, Duration::from_secs(15));
    });

    // Same repository, another pinned tag: the tag list comes from its cache
    image(&state, "fixture.invalid/sso:26.4", Some("sha256:1"));
    http::reply(Ok(digest(b"sha256:1")));
    let info = collector
        .check_image(&docker, "fixture.invalid/sso:26.4")
        .await;
    assert_eq!(info.status, VersionStatus::UpToDate, "27.0 is a new major");
    assert!(info.latest_tag.is_none());

    // Variant suffix and pre-releases
    let web = [
        "1.27.3-alpine",
        "1.27.4",
        "1.27.5-rc1-alpine",
        "1.27.4-alpine",
    ];
    let info = pinned(
        &collector,
        &docker,
        &state,
        "fixture.invalid/web:1.27.3-alpine",
        Ok(tags(&web)),
    )
    .await;
    assert_eq!(info.latest_tag.as_deref(), Some("1.27.4-alpine"));
    http::STATE.with(|s| assert!(s.borrow().responses.is_empty()));
}
#[tokio::test]
async fn floating_and_non_semver_tags_never_list_tags() {
    http::reset();
    let (docker, state) = docker();
    let collector = VersionsCollector::default();
    for (name, remote, status) in [
        (
            "fixture.invalid/unit:latest",
            "sha256:2",
            VersionStatus::Drifted,
        ),
        (
            "fixture.invalid/unit:16",
            "sha256:1",
            VersionStatus::UpToDate,
        ),
        (
            "fixture.invalid/unit:2024-01-15",
            "sha256:1",
            VersionStatus::UpToDate,
        ),
        (
            "fixture.invalid/unit:stable",
            "sha256:1",
            VersionStatus::UpToDate,
        ),
    ] {
        image(&state, name, Some("sha256:1"));
        http::reply(Ok(digest(remote.as_bytes())));
        let info = collector.check_image(&docker, name).await;
        assert_eq!(info.status, status, "{name}");
        assert!(info.latest_tag.is_none());
    }
    http::STATE.with(|s| {
        let s = s.borrow();
        assert!(s.calls.iter().all(|c| c.method == "HEAD"));
        assert!(s.responses.is_empty());
    });
}
#[tokio::test]
async fn tag_listing_failures_are_unknown_and_drift_still_wins() {
    http::reset();
    let (docker, state) = docker();
    let collector = VersionsCollector::default();
    for (name, reply, prefix) in [
        (
            "fixture.invalid/a:1.2",
            Ok(http::Response::new(429, "")),
            "Tag listing failed: Registry returned HTTP 429",
        ),
        (
            "fixture.invalid/b:1.2",
            Err(std::io::Error::other("unit offline")),
            "Tag listing failed: Network error: unit offline",
        ),
        (
            "fixture.invalid/c:1.2",
            Ok(http::Response::new(200, "not json")),
            "Tag listing failed: Invalid tag list",
        ),
    ] {
        let info = pinned(&collector, &docker, &state, name, reply).await;
        assert_eq!(info.status, VersionStatus::Unknown, "{name}");
        assert!(info.error.as_deref().unwrap().starts_with(prefix), "{name}");
        assert_eq!(info.remote_digest.as_deref(), Some("sha256:1"));
    }
    // A failed listing is not cached: the next check asks again
    let expired = VersionsCollector::new(Duration::ZERO, vec![]);
    for reply in [Ok(http::Response::new(429, "")), Ok(tags(&["1.3"]))] {
        let info = pinned(&expired, &docker, &state, "fixture.invalid/a:1.2", reply).await;
        assert!(matches!(
            info.status,
            VersionStatus::Unknown | VersionStatus::NewerAvailable
        ));
    }
    // Missing "tags" is an empty list, not an error
    let info = pinned(
        &collector,
        &docker,
        &state,
        "fixture.invalid/d:1.2",
        Ok(http::Response::new(200, "{\"tags\":null}")),
    )
    .await;
    assert_eq!(info.status, VersionStatus::UpToDate);

    // Drift and a newer tag at once: drift wins, the newer tag is kept
    image(&state, "fixture.invalid/e:1.2", Some("sha256:1"));
    http::reply(Ok(digest(b"sha256:2")));
    http::reply(Err(std::io::Error::other("unit offline")));
    let info = collector
        .check_image(&docker, "fixture.invalid/e:1.2")
        .await;
    assert_eq!(info.status, VersionStatus::Drifted);
    assert!(info.error.is_none());
    image(&state, "fixture.invalid/f:1.2", Some("sha256:1"));
    http::reply(Ok(digest(b"sha256:2")));
    http::reply(Ok(tags(&["1.2", "1.5"])));
    let info = collector
        .check_image(&docker, "fixture.invalid/f:1.2")
        .await;
    assert_eq!(info.status, VersionStatus::Drifted);
    assert_eq!(info.latest_tag.as_deref(), Some("1.5"));
    http::STATE.with(|s| assert!(s.borrow().responses.is_empty()));
}
#[tokio::test]
async fn tag_listing_follows_pages_with_the_same_token() {
    http::reset();
    let collector = VersionsCollector::default();
    http::reply(Ok(http::Response::new(200, "{\"token\":\"unit-token\"}")));
    http::reply(Ok(tags_page(
        &["1.0", "1.1"],
        "/v2/unit/app/tags/list?last=1.1&n=1000",
    )));
    http::reply(Ok(tags(&["1.2"])));
    assert_eq!(
        collector.fetch_tags("ghcr.io", "unit/app").await.unwrap(),
        ["1.0", "1.1", "1.2"]
    );
    http::STATE.with(|s| {
        let s = s.borrow();
        let urls: Vec<_> = s.calls.iter().map(|c| c.url.as_str()).collect();
        assert_eq!(
            urls[1..],
            [
                "https://ghcr.io/v2/unit/app/tags/list?n=1000",
                "https://ghcr.io/v2/unit/app/tags/list?last=1.1&n=1000",
            ]
        );
        for call in &s.calls[1..] {
            assert_eq!(
                call.headers,
                [("Authorization".into(), "Bearer unit-token".into())]
            );
        }
    });
    // A registry that keeps paginating is cut off, keeping what it sent
    http::reset();
    for _ in 0..MAX_TAG_PAGES {
        http::reply(Ok(tags_page(&["9.9"], "/v2/unit/tags/list?last=9.9")));
    }
    let listed = collector
        .fetch_tags("fixture.invalid", "unit")
        .await
        .unwrap();
    assert_eq!(listed.len(), MAX_TAG_PAGES);
    http::STATE.with(|s| assert!(s.borrow().responses.is_empty()));
}
#[tokio::test]
async fn configuration_sets_ttls_prereleases_and_skip_patterns() {
    http::reset();
    let config = crate::config::ImageVersionsConfig {
        digest_cache_ttl_hours: 48,
        tag_cache_ttl_hours: 24,
        include_prereleases: true,
        skip_patterns: vec!["fixture.invalid/frozen".into()],
    };
    let collector = VersionsCollector::from_config(&config);
    assert_eq!(collector.cache_ttl, Duration::from_secs(48 * 3600));
    assert_eq!(
        collector.tag_cache_ttl,
        Duration::from_secs(48 * 3600),
        "never below the digest TTL"
    );
    assert!(collector.include_prereleases);
    let default = VersionsCollector::default();
    assert_eq!(default.tag_cache_ttl, DEFAULT_TAG_CACHE_TTL);
    assert!(!default.include_prereleases);

    let (docker, state) = docker();
    let skipped = collector
        .check_image(&docker, "fixture.invalid/frozen:1.2")
        .await;
    assert_eq!(skipped.status, VersionStatus::Unknown);
    assert!(skipped.latest_tag.is_none());
    assert!(state.lock().unwrap().calls.is_empty());

    let info = pinned(
        &collector,
        &docker,
        &state,
        "fixture.invalid/app:1.2",
        Ok(tags(&["1.2", "1.3-rc1"])),
    )
    .await;
    assert_eq!(info.latest_tag.as_deref(), Some("1.3-rc1"));

    // Poisoned tag cache: lookups still work, uncached
    let poisoned = VersionsCollector::new(Duration::ZERO, vec![]);
    let lock = poisoned.tag_cache.clone();
    assert!(std::thread::spawn(move || {
        let _guard = lock.write().unwrap();
        panic!("unit tag cache poisoned");
    })
    .join()
    .is_err());
    let info = pinned(
        &poisoned,
        &docker,
        &state,
        "fixture.invalid/app:1.2",
        Ok(tags(&["1.4"])),
    )
    .await;
    assert_eq!(info.latest_tag.as_deref(), Some("1.4"));
    http::STATE.with(|s| assert!(s.borrow().responses.is_empty()));
}
