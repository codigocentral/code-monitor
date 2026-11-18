//! Image version and digest drift collector
//!
//! This collector compares the digest of container images currently running
//! against the remote registry's current manifest for the same tag.
//!
//! Designed to detect "digest drift" on floating tags (e.g. `latest`, `stable`)
//! without pulling images or causing significant network traffic (using HTTP HEAD).
//!
//! Pinned version tags (`26.1`, `1.27.3-alpine`) never drift, so for them the
//! collector also lists the repository's tags (`GET /v2/<repo>/tags/list`) and
//! looks for a newer tag in the same release line; see
//! [`super::version_tags`] for the comparison rules. The tag list is cached per
//! repository with its own, longer TTL.

use super::version_tags;
#[cfg(all(test, feature = "unit-doubles"))]
use crate::unit_bollard as bollard;
#[cfg(all(test, feature = "unit-doubles"))]
use crate::unit_registry_http as reqwest;
use anyhow::Result;
use bollard::Docker;
use chrono::Utc;
use shared::types::{ImageVersionInfo, VersionStatus};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

/// Default cache TTL for remote registry checks (24 hours).
const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(24 * 3600);

/// Default cache TTL for repository tag lists (7 days).
///
/// `tags/list` returns the whole list and costs more than a manifest `HEAD`;
/// new releases of a pinned line are not urgent enough to look daily.
const DEFAULT_TAG_CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

/// Tags requested per `tags/list` page.
const TAGS_PAGE_SIZE: usize = 1000;

/// Pages followed before giving up on the rest of a tag list.
const MAX_TAG_PAGES: usize = 20;

/// Cached tag list of one repository.
type TagCache = HashMap<String, (Arc<Vec<String>>, Instant)>;

/// Supported OCI and Docker manifest accept header
const MANIFEST_ACCEPT_HEADER: &str = concat!(
    "application/vnd.oci.image.index.v1+json, ",
    "application/vnd.docker.distribution.manifest.list.v2+json, ",
    "application/vnd.oci.image.manifest.v1+json, ",
    "application/vnd.docker.distribution.manifest.v2+json"
);

/// Parse an image reference into (registry, repository, tag).
pub fn parse_image_ref(image_ref: &str) -> (String, String, String) {
    let trimmed = image_ref.trim();
    if trimmed.is_empty() {
        return (
            "registry-1.docker.io".to_string(),
            "library/unknown".to_string(),
            "latest".to_string(),
        );
    }

    // Split tag or digest if present
    let (body, tag) = match trimmed.rsplit_once(':') {
        Some((b, t)) if !b.contains('/') || !t.contains('/') => (b, t),
        _ => (trimmed, "latest"),
    };

    let parts: Vec<&str> = body.split('/').collect();
    if parts[0].contains('.') || parts[0].contains(':') {
        // Explicit registry host in the first segment
        let registry = parts[0].to_string();
        let repository = parts[1..].join("/");
        (registry, repository, tag.to_string())
    } else if parts.len() == 1 {
        // Docker Hub library image: e.g. "postgres" -> "library/postgres"
        (
            "registry-1.docker.io".to_string(),
            format!("library/{}", parts[0]),
            tag.to_string(),
        )
    } else {
        // Docker Hub user image: e.g. "grafana/grafana"
        (
            "registry-1.docker.io".to_string(),
            body.to_string(),
            tag.to_string(),
        )
    }
}

/// Image version collector for tracking image drift against remote registries
#[derive(Clone)]
pub struct VersionsCollector {
    client: reqwest::Client,
    cache: Arc<RwLock<HashMap<String, (ImageVersionInfo, Instant)>>>,
    cache_ttl: Duration,
    tag_cache: Arc<RwLock<TagCache>>,
    tag_cache_ttl: Duration,
    include_prereleases: bool,
    skip_patterns: Vec<String>,
}

impl Default for VersionsCollector {
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_TTL, Vec::new())
    }
}

impl VersionsCollector {
    /// Create a new VersionsCollector with custom TTL and skip patterns
    pub fn new(cache_ttl: Duration, skip_patterns: Vec<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();

        Self {
            client,
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl,
            tag_cache: Arc::new(RwLock::new(HashMap::new())),
            tag_cache_ttl: DEFAULT_TAG_CACHE_TTL.max(cache_ttl),
            include_prereleases: false,
            skip_patterns,
        }
    }

    /// Build the collector from the `[image_versions]` configuration section.
    pub fn from_config(config: &crate::config::ImageVersionsConfig) -> Self {
        Self::new(
            hours(config.digest_cache_ttl_hours),
            config.skip_patterns.clone(),
        )
        .with_tag_policy(
            hours(config.tag_cache_ttl_hours),
            config.include_prereleases,
        )
    }

    /// Set the tag list TTL and whether pre-releases count as newer versions.
    ///
    /// The TTL never goes below the digest TTL: the per-image result is cached
    /// that long anyway, so a shorter tag TTL would only look like it works.
    pub fn with_tag_policy(mut self, tag_cache_ttl: Duration, include_prereleases: bool) -> Self {
        self.tag_cache_ttl = tag_cache_ttl.max(self.cache_ttl);
        self.include_prereleases = include_prereleases;
        self
    }

    /// Fetch an anonymous bearer auth token for the registry if applicable
    async fn fetch_token(&self, registry: &str, repo: &str) -> Option<String> {
        let url = match registry {
            "registry-1.docker.io" => format!(
                "https://auth.docker.io/token?service=registry.docker.io&scope=repository:{}:pull",
                repo
            ),
            "quay.io" => format!(
                "https://quay.io/v2/auth?service=quay.io&scope=repository:{}:pull",
                repo
            ),
            "gcr.io" => format!(
                "https://gcr.io/v2/token?service=gcr.io&scope=repository:{}:pull",
                repo
            ),
            "ghcr.io" => format!(
                "https://ghcr.io/token?service=ghcr.io&scope=repository:{}:pull",
                repo
            ),
            _ => return None,
        };

        match self
            .client
            .get(&url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let json: serde_json::Value = resp.json().await.ok()?;
                json.get("token")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
            }
            _ => None,
        }
    }

    /// Perform a HEAD request against the registry to obtain Docker-Content-Digest
    async fn fetch_remote_digest(
        &self,
        registry: &str,
        repo: &str,
        tag: &str,
    ) -> Result<String, String> {
        let url = format!("https://{}/v2/{}/manifests/{}", registry, repo, tag);
        let mut req = self
            .client
            .head(&url)
            .header("Accept", MANIFEST_ACCEPT_HEADER)
            .timeout(Duration::from_secs(15));

        if let Some(token) = self.fetch_token(registry, repo).await {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => return Err(format!("Network error: {}", e)),
        };

        if !resp.status().is_success() {
            return Err(format!("Registry returned HTTP {}", resp.status()));
        }

        if let Some(header_val) = resp.headers().get("Docker-Content-Digest") {
            header_val
                .to_str()
                .map(|s| s.to_string())
                .map_err(|e| format!("Invalid header: {}", e))
        } else {
            Err("Missing Docker-Content-Digest header in registry response".to_string())
        }
    }

    /// List every tag of a repository, following `Link` pagination.
    async fn fetch_tags(&self, registry: &str, repo: &str) -> Result<Vec<String>, String> {
        let token = self.fetch_token(registry, repo).await;
        let mut url = Some(format!(
            "https://{}/v2/{}/tags/list?n={}",
            registry, repo, TAGS_PAGE_SIZE
        ));
        let mut tags = Vec::new();
        for _ in 0..MAX_TAG_PAGES {
            let Some(page_url) = url.take() else {
                return Ok(tags);
            };
            let (page, next) = self.fetch_tag_page(&page_url, token.as_deref()).await?;
            tags.extend(page);
            url = next.and_then(|link| next_page_url(registry, &link));
        }
        if url.is_some() {
            debug!(
                "Tag list of {}/{} truncated after {} pages",
                registry, repo, MAX_TAG_PAGES
            );
        }
        Ok(tags)
    }

    /// Fetch one `tags/list` page: its tags and the raw `Link` header, if any.
    async fn fetch_tag_page(
        &self,
        url: &str,
        token: Option<&str>,
    ) -> Result<(Vec<String>, Option<String>), String> {
        let mut req = self.client.get(url).timeout(Duration::from_secs(15));
        if let Some(token) = token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!("Registry returned HTTP {}", resp.status()));
        }
        let link = resp
            .headers()
            .get("Link")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Invalid tag list: {}", e))?;
        let tags = body
            .get("tags")
            .and_then(|t| t.as_array())
            .map(|tags| {
                tags.iter()
                    .filter_map(|t| t.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        Ok((tags, link))
    }

    /// Tag list of a repository, from the tag cache when still fresh.
    async fn repository_tags(
        &self,
        registry: &str,
        repo: &str,
    ) -> Result<Arc<Vec<String>>, String> {
        let key = format!("{}/{}", registry, repo);
        if let Ok(cache) = self.tag_cache.read() {
            if let Some((tags, timestamp)) = cache.get(&key) {
                if timestamp.elapsed() < self.tag_cache_ttl {
                    return Ok(tags.clone());
                }
            }
        }
        let tags = Arc::new(self.fetch_tags(registry, repo).await?);
        if let Ok(mut cache) = self.tag_cache.write() {
            cache.insert(key, (tags.clone(), Instant::now()));
        }
        Ok(tags)
    }

    /// Newest tag in the line of a pinned `tag`, if newer than it.
    ///
    /// `Ok(None)` without any request when the tag is not a pinned version.
    async fn newer_tag(
        &self,
        registry: &str,
        repo: &str,
        tag: &str,
    ) -> Result<Option<String>, String> {
        if !version_tags::is_pinned_version(tag) {
            return Ok(None);
        }
        let tags = self
            .repository_tags(registry, repo)
            .await
            .map_err(|e| format!("Tag listing failed: {}", e))?;
        Ok(version_tags::newer_in_line(
            tag,
            tags.iter().map(String::as_str),
            self.include_prereleases,
        ))
    }

    /// Extract the local digest from Docker inspect RepoDigests
    pub fn extract_local_digest(repo_digests: Option<&[String]>, repo: &str) -> Option<String> {
        let digests = repo_digests?;
        if digests.is_empty() {
            return None;
        }

        // Try to match the specific repository name
        for item in digests {
            if let Some((r, dig)) = item.split_once('@') {
                if r.ends_with(repo) || repo.ends_with(r) {
                    return Some(dig.to_string());
                }
            }
        }

        // Otherwise return the first available digest
        digests[0].split_once('@').map(|(_, dig)| dig.to_string())
    }

    /// Compare the local digest with the registry's for the same tag.
    ///
    /// Returns `(status, remote_digest, error)`; without a local digest the
    /// image was built locally and the registry is not asked.
    async fn digest_status(
        &self,
        image_ref: &str,
        registry: &str,
        repository: &str,
        tag: &str,
        local_digest: Option<&str>,
    ) -> (VersionStatus, Option<String>, Option<String>) {
        let Some(local) = local_digest else {
            return (VersionStatus::LocalBuild, None, None);
        };
        match self.fetch_remote_digest(registry, repository, tag).await {
            Ok(remote) if remote == local => (VersionStatus::UpToDate, Some(remote), None),
            Ok(remote) => (VersionStatus::Drifted, Some(remote), None),
            Err(err) => {
                warn!("Failed to query registry for {}: {}", image_ref, err);
                (VersionStatus::Unknown, None, Some(err))
            }
        }
    }

    /// Check the version and digest status of a container image
    pub async fn check_image(&self, docker: &Docker, image_ref: &str) -> ImageVersionInfo {
        let (registry, repository, tag) = parse_image_ref(image_ref);

        // Check if image matches any skip pattern
        if self.skip_patterns.iter().any(|pat| image_ref.contains(pat)) {
            return ImageVersionInfo {
                image_ref: image_ref.to_string(),
                registry,
                repository,
                tag,
                local_digest: None,
                remote_digest: None,
                status: VersionStatus::Unknown,
                latest_tag: None,
                checked_at: Some(Utc::now()),
                error: Some("Skipped by pattern".to_string()),
            };
        }

        // Check cache first
        if let Ok(cache) = self.cache.read() {
            if let Some((cached_info, timestamp)) = cache.get(image_ref) {
                if timestamp.elapsed() < self.cache_ttl {
                    return cached_info.clone();
                }
            }
        }

        // Inspect image locally to read RepoDigests
        let local_digest = match docker.inspect_image(image_ref).await {
            Ok(inspect) => Self::extract_local_digest(inspect.repo_digests.as_deref(), &repository),
            Err(e) => {
                debug!("Failed to inspect image {} locally: {}", image_ref, e);
                None
            }
        };

        let (status, remote_digest, error) = self
            .digest_status(
                image_ref,
                &registry,
                &repository,
                &tag,
                local_digest.as_deref(),
            )
            .await;

        // The registry answered for this tag: a pinned one may still be behind
        let (status, latest_tag, error) = if remote_digest.is_some() {
            let newer = self.newer_tag(&registry, &repository, &tag).await;
            if let Err(err) = &newer {
                warn!("Failed to list tags for {}: {}", image_ref, err);
            }
            combine_newer_tag(status, newer)
        } else {
            (status, None, error)
        };

        let info = ImageVersionInfo {
            image_ref: image_ref.to_string(),
            registry,
            repository,
            tag,
            local_digest,
            remote_digest,
            status,
            latest_tag,
            checked_at: Some(Utc::now()),
            error,
        };

        // Update cache
        if let Ok(mut cache) = self.cache.write() {
            cache.insert(image_ref.to_string(), (info.clone(), Instant::now()));
        }

        info
    }
}

fn hours(value: u64) -> Duration {
    Duration::from_secs(value.saturating_mul(3600))
}

/// Merge the digest verdict with the newer-tag lookup.
///
/// Returns `(status, latest_tag, error)`. Drift wins over a newer tag, since
/// pulling the same tag again is the smaller step; the newer tag is still
/// reported. When the tag list cannot be read, an otherwise up-to-date pinned
/// image is `Unknown`: "the tag did not move" says nothing about new versions.
fn combine_newer_tag(
    digest_status: VersionStatus,
    newer: Result<Option<String>, String>,
) -> (VersionStatus, Option<String>, Option<String>) {
    match (digest_status, newer) {
        (VersionStatus::UpToDate, Ok(Some(latest))) => {
            (VersionStatus::NewerAvailable, Some(latest), None)
        }
        (VersionStatus::UpToDate, Err(err)) => (VersionStatus::Unknown, None, Some(err)),
        (status, newer) => (status, newer.ok().flatten(), None),
    }
}

/// Absolute URL of the next `tags/list` page from a `Link` header.
///
/// Registries send `</v2/<repo>/tags/list?last=x&n=y>; rel="next"`, usually
/// relative to the registry host. An absolute link to another host is refused:
/// the next request carries the registry's bearer token.
fn next_page_url(registry: &str, link: &str) -> Option<String> {
    let link = link.split(',').find(|part| part.contains("rel=\"next\""))?;
    let start = link.find('<')? + 1;
    let end = start + link[start..].find('>')?;
    let target = &link[start..end];
    let same_host = target
        .strip_prefix("https://")
        .and_then(|rest| rest.strip_prefix(registry))
        .is_some_and(|path| path.starts_with('/'));
    if same_host {
        Some(target.to_string())
    } else if target.starts_with('/') {
        Some(format!("https://{}{}", registry, target))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_combine_newer_tag() {
        assert_eq!(
            combine_newer_tag(VersionStatus::UpToDate, Ok(Some("26.4".into()))),
            (VersionStatus::NewerAvailable, Some("26.4".into()), None)
        );
        assert_eq!(
            combine_newer_tag(VersionStatus::UpToDate, Ok(None)),
            (VersionStatus::UpToDate, None, None)
        );
        assert_eq!(
            combine_newer_tag(VersionStatus::UpToDate, Err("HTTP 429".into())),
            (VersionStatus::Unknown, None, Some("HTTP 429".into()))
        );
        assert_eq!(
            combine_newer_tag(VersionStatus::Drifted, Ok(Some("26.4".into()))),
            (VersionStatus::Drifted, Some("26.4".into()), None)
        );
        assert_eq!(
            combine_newer_tag(VersionStatus::Drifted, Err("HTTP 429".into())),
            (VersionStatus::Drifted, None, None)
        );
    }

    #[test]
    fn test_next_page_url() {
        assert_eq!(
            next_page_url(
                "registry-1.docker.io",
                "</v2/library/nginx/tags/list?last=1.27&n=1000>; rel=\"next\""
            )
            .as_deref(),
            Some("https://registry-1.docker.io/v2/library/nginx/tags/list?last=1.27&n=1000")
        );
        assert_eq!(
            next_page_url(
                "ghcr.io",
                "<https://ghcr.io/v2/a/b/tags/list?last=x>; rel=\"next\""
            )
            .as_deref(),
            Some("https://ghcr.io/v2/a/b/tags/list?last=x")
        );
        assert!(next_page_url("ghcr.io", "</v2/a/tags/list?last=x>; rel=\"prev\"").is_none());
        assert!(next_page_url("ghcr.io", "<relative>; rel=\"next\"").is_none());
        for foreign in [
            "<https://elsewhere.invalid/v2/a/tags/list>; rel=\"next\"",
            "<https://ghcr.io.elsewhere.invalid/v2/a>; rel=\"next\"",
            "<http://ghcr.io/v2/a/tags/list>; rel=\"next\"",
        ] {
            assert!(next_page_url("ghcr.io", foreign).is_none(), "{foreign}");
        }
        assert!(next_page_url("ghcr.io", "garbage rel=\"next\"").is_none());
    }

    #[test]
    fn test_parse_image_ref_official_library() {
        let (reg, repo, tag) = parse_image_ref("postgres");
        assert_eq!(reg, "registry-1.docker.io");
        assert_eq!(repo, "library/postgres");
        assert_eq!(tag, "latest");

        let (reg, repo, tag) = parse_image_ref("redis:7-alpine");
        assert_eq!(reg, "registry-1.docker.io");
        assert_eq!(repo, "library/redis");
        assert_eq!(tag, "7-alpine");
    }

    #[test]
    fn test_parse_image_ref_dockerhub_user() {
        let (reg, repo, tag) = parse_image_ref("grafana/grafana:latest");
        assert_eq!(reg, "registry-1.docker.io");
        assert_eq!(repo, "grafana/grafana");
        assert_eq!(tag, "latest");
    }

    #[test]
    fn test_parse_image_ref_custom_registry() {
        let (reg, repo, tag) = parse_image_ref("quay.io/prometheus/node-exporter:v1.8.2");
        assert_eq!(reg, "quay.io");
        assert_eq!(repo, "prometheus/node-exporter");
        assert_eq!(tag, "v1.8.2");

        let (reg, repo, tag) = parse_image_ref("docker.didaticos.com:5000/empresa/app:1.0");
        assert_eq!(reg, "docker.didaticos.com:5000");
        assert_eq!(repo, "empresa/app");
        assert_eq!(tag, "1.0");
    }

    #[test]
    fn test_extract_local_digest() {
        let digests = vec![
            "library/redis@sha256:1111111111111111111111111111111111111111111111111111111111111111"
                .to_string(),
        ];
        let dig = VersionsCollector::extract_local_digest(Some(&digests), "library/redis");
        assert_eq!(
            dig.as_deref(),
            Some("sha256:1111111111111111111111111111111111111111111111111111111111111111")
        );

        let empty: Vec<String> = Vec::new();
        assert_eq!(
            VersionsCollector::extract_local_digest(Some(&empty), "redis"),
            None
        );
        assert_eq!(VersionsCollector::extract_local_digest(None, "redis"), None);
    }
}

#[cfg(test)]
#[cfg(feature = "unit-doubles")]
#[path = "../../tests/doubles/versions_units.rs"]
mod campaign_versions_units;
