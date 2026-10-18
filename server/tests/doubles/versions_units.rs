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
