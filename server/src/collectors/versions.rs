//! Image version and digest drift collector
//!
//! This collector compares the digest of container images currently running
//! against the remote registry's current manifest for the same tag.
//!
//! Designed to detect "digest drift" on floating tags (e.g. `latest`, `stable`)
//! without pulling images or causing significant network traffic (using HTTP HEAD).

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
            skip_patterns,
        }
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

    /// Check the version and digest status of a container image
    pub async fn check_image(
        &self,
        docker: &Docker,
        image_ref: &str,
    ) -> ImageVersionInfo {
        let (registry, repository, tag) = parse_image_ref(image_ref);

        // Check if image matches any skip pattern
        if self
            .skip_patterns
            .iter()
            .any(|pat| image_ref.contains(pat))
        {
            return ImageVersionInfo {
                image_ref: image_ref.to_string(),
                registry,
                repository,
                tag,
                local_digest: None,
                remote_digest: None,
                status: VersionStatus::Unknown,
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
            Ok(inspect) => {
                Self::extract_local_digest(inspect.repo_digests.as_deref(), &repository)
            }
            Err(e) => {
                debug!("Failed to inspect image {} locally: {}", image_ref, e);
                None
            }
        };

        // If local digest is absent, it's a locally built image
        let (status, remote_digest, error) = match &local_digest {
            None => (VersionStatus::LocalBuild, None, None),
            Some(loc) => match self.fetch_remote_digest(&registry, &repository, &tag).await {
                Ok(rem) => {
                    if *loc == rem {
                        (VersionStatus::UpToDate, Some(rem), None)
                    } else {
                        (VersionStatus::Drifted, Some(rem), None)
                    }
                }
                Err(err) => {
                    warn!("Failed to query registry for {}: {}", image_ref, err);
                    (VersionStatus::Unknown, None, Some(err))
                }
            },
        };

        let info = ImageVersionInfo {
            image_ref: image_ref.to_string(),
            registry,
            repository,
            tag,
            local_digest,
            remote_digest,
            status,
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

#[cfg(test)]
mod tests {
    use super::*;

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
            "library/redis@sha256:1111111111111111111111111111111111111111111111111111111111111111".to_string(),
        ];
        let dig = VersionsCollector::extract_local_digest(Some(&digests), "library/redis");
        assert_eq!(
            dig.as_deref(),
            Some("sha256:1111111111111111111111111111111111111111111111111111111111111111")
        );

        let empty: Vec<String> = Vec::new();
        assert_eq!(VersionsCollector::extract_local_digest(Some(&empty), "redis"), None);
        assert_eq!(VersionsCollector::extract_local_digest(None, "redis"), None);
    }
}
