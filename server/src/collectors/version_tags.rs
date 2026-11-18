//! Version-tag comparison for pinned image tags
//!
//! Digest drift only says that *the same tag* now points somewhere else, which
//! never happens to a pinned tag such as `26.1` or `1.27.3-alpine`. This module
//! answers the other question — "is there a newer tag in the line I am on?" —
//! from the plain list of tags a registry returns.
//!
//! The rules are deliberately conservative, so a suggestion is always safe to
//! follow:
//!
//! - **The line is the tag minus its last component.** `26.1` stays on `26.x`
//!   and `2.4.10` stays on `2.4.x`. Pinning a full `x.y.z` usually means a
//!   long-term-support line whose patches keep shipping after the next minor,
//!   so it is never pushed to that minor. A major is never crossed, and a
//!   major-only tag (`16`) has no line to move along: it floats, and digest
//!   drift covers it.
//! - **Same shape.** Candidates have the same number of components and the same
//!   `v` prefix, so `26.1` is compared with `26.4`, not with `26.4.2`.
//! - **Same variant.** `-alpine`, `-community`, `-management` and the like must
//!   match exactly: an Alpine image is never told to move to a Debian one.
//! - **No pre-releases** (`-rc1`, `-beta`, `-alpha`, `-dev`, `nightly`...)
//!   unless explicitly allowed.
//!
//! Anything that does not parse (`latest`, `stable`, dates, hashes) is not a
//! pinned version and keeps the digest drift behaviour.

use std::cmp::Ordering;

/// Suffix segments that mark a pre-release, optionally followed by a number
/// (`rc1`, `rc.2`, `beta`, `beta3`).
const PRERELEASE_MARKERS: &[&str] = &[
    "alpha", "beta", "rc", "dev", "nightly", "snapshot", "preview", "canary", "pre",
];

/// Longest numeric component still read as a version number. Longer runs are
/// dates or build numbers (`20240115`), which are not semantic versions.
const MAX_COMPONENT_DIGITS: usize = 6;

/// Most numeric components a version tag may have (`1.2.3.4`).
const MAX_COMPONENTS: usize = 4;

/// A tag read as a version: `v1.27.3-alpine` → `v`, `[1, 27, 3]`, `alpine`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagVersion {
    v_prefix: bool,
    numbers: Vec<u64>,
    variant: String,
    prerelease: bool,
}

impl TagVersion {
    /// Parse a tag, or `None` when it is not a version tag.
    pub fn parse(tag: &str) -> Option<Self> {
        let (v_prefix, rest) = match tag.strip_prefix(['v', 'V']) {
            Some(rest) => (true, rest),
            None => (false, tag),
        };
        let (core, suffix) = split_core(rest);
        let numbers = parse_numbers(core)?;
        let (variant, prerelease) = parse_suffix(suffix)?;
        Some(Self {
            v_prefix,
            numbers,
            variant,
            prerelease,
        })
    }

    /// Whether the tag pins a version that can move along a line.
    ///
    /// Major-only tags (`16`, `7-alpine`) float on their own and are left to
    /// digest drift.
    pub fn is_pinned(&self) -> bool {
        self.numbers.len() >= 2
    }

    pub fn is_prerelease(&self) -> bool {
        self.prerelease
    }

    /// Same release line, shape and variant as `self`: comparable at all.
    fn same_line(&self, other: &Self) -> bool {
        let line = self.numbers.len() - 1;
        self.v_prefix == other.v_prefix
            && self.variant == other.variant
            && self.numbers.len() == other.numbers.len()
            && self.numbers[..line] == other.numbers[..line]
    }

    /// Release order: numbers first, then a release above its pre-releases.
    fn release_cmp(&self, other: &Self) -> Ordering {
        self.numbers
            .cmp(&other.numbers)
            .then_with(|| other.prerelease.cmp(&self.prerelease))
    }
}

/// Split `1.27.3-alpine` into `1.27.3` and `-alpine`.
fn split_core(rest: &str) -> (&str, &str) {
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    // A trailing dot belongs to the suffix (`1.2.rc1` is not `1.2.` + `rc1`).
    let end = if rest[..end].ends_with('.') {
        end - 1
    } else {
        end
    };
    rest.split_at(end)
}

fn parse_numbers(core: &str) -> Option<Vec<u64>> {
    let numbers = core
        .split('.')
        .map(|part| {
            let valid = !part.is_empty() && part.len() <= MAX_COMPONENT_DIGITS;
            valid.then(|| part.parse::<u64>().ok()).flatten()
        })
        .collect::<Option<Vec<u64>>>()?;
    (numbers.len() <= MAX_COMPONENTS).then_some(numbers)
}

/// Read what follows the numbers: `-rc1-alpine` → (`alpine`, pre-release).
///
/// `None` when the suffix does not start with a separator and is not a
/// pre-release marker either (`1.2abc`), i.e. the tag is not a version.
fn parse_suffix(suffix: &str) -> Option<(String, bool)> {
    if suffix.is_empty() {
        return Some((String::new(), false));
    }
    let body = match suffix.strip_prefix(['-', '.', '_']) {
        Some(body) => body,
        None if is_prerelease_segment(suffix) => suffix,
        None => return None,
    };
    let mut prerelease = false;
    let mut variant = Vec::new();
    for segment in body.split('-') {
        if is_prerelease_segment(segment) {
            prerelease = true;
        } else {
            variant.push(segment);
        }
    }
    Some((variant.join("-"), prerelease))
}

/// `rc`, `rc1`, `rc.1`, `beta2`, `nightly`...
fn is_prerelease_segment(segment: &str) -> bool {
    let lower = segment.to_ascii_lowercase();
    PRERELEASE_MARKERS.iter().any(|marker| {
        lower.strip_prefix(marker).is_some_and(|tail| {
            let tail = tail.strip_prefix('.').unwrap_or(tail);
            tail.bytes().all(|b| b.is_ascii_digit())
        })
    })
}

/// The newest tag in `current`'s line, when it is newer than `current`.
///
/// `None` when `current` is not a pinned version or nothing newer exists in
/// its line. Pre-releases are candidates only with `include_prereleases`.
pub fn newer_in_line<'a, I>(
    current: &str,
    available: I,
    include_prereleases: bool,
) -> Option<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let current = TagVersion::parse(current).filter(TagVersion::is_pinned)?;
    available
        .into_iter()
        .filter_map(|tag| TagVersion::parse(tag).map(|version| (tag, version)))
        .filter(|(_, v)| include_prereleases || !v.is_prerelease())
        .filter(|(_, v)| current.same_line(v))
        .filter(|(_, v)| v.release_cmp(&current) == Ordering::Greater)
        .max_by(|(_, a), (_, b)| a.release_cmp(b))
        .map(|(tag, _)| tag.to_string())
}

/// Whether `tag` is a pinned version worth a `tags/list` lookup.
pub fn is_pinned_version(tag: &str) -> bool {
    TagVersion::parse(tag).is_some_and(|v| v.is_pinned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn newer(current: &str, available: &[&str]) -> Option<String> {
        newer_in_line(current, available.iter().copied(), false)
    }

    #[test]
    fn parses_prefix_numbers_variant_and_prerelease() {
        let v = TagVersion::parse("v1.27.3-alpine").unwrap();
        assert!(v.v_prefix);
        assert_eq!(v.numbers, [1, 27, 3]);
        assert_eq!(v.variant, "alpine");
        assert!(!v.prerelease);

        let v = TagVersion::parse("1.2.3-rc.1-alpine").unwrap();
        assert_eq!(v.variant, "alpine");
        assert!(v.prerelease);

        let v = TagVersion::parse("3.13.0rc1").unwrap();
        assert_eq!(v.numbers, [3, 13, 0]);
        assert!(v.prerelease);
        assert!(v.variant.is_empty());

        assert_eq!(
            TagVersion::parse("16.4-alpine3.20").unwrap().variant,
            "alpine3.20"
        );
        assert!(TagVersion::parse("26.1-nightly").unwrap().prerelease);
    }

    #[test]
    fn non_semver_tags_are_not_versions() {
        for tag in [
            "latest",
            "stable",
            "nightly",
            "alpine",
            "20240115",
            "2024-01-15",
            "a1b2c3d",
            "1.2abc",
            "1..2",
            ".1",
            "1.2.3.4.5",
            "",
        ] {
            assert!(!is_pinned_version(tag), "{tag}");
        }
        assert!(newer("latest", &["1.0", "2.0"]).is_none());
        assert!(newer("20240115", &["20240201"]).is_none());
    }

    #[test]
    fn major_only_tags_float_and_are_left_to_digest_drift() {
        assert!(!is_pinned_version("16"));
        assert!(!is_pinned_version("7-alpine"));
        assert!(newer("16", &["16", "17", "16.4"]).is_none());
        assert!(is_pinned_version("16.4"));
        assert!(is_pinned_version("v1.8.2"));
    }

    #[test]
    fn suggests_the_newest_minor_on_the_same_major() {
        let tags = ["26.0", "26.1", "26.2", "26.4", "26.3", "26.4.2", "27.0"];
        assert_eq!(newer("26.1", &tags).as_deref(), Some("26.4"));
    }

    #[test]
    fn never_suggests_a_new_major() {
        assert!(newer("26.4", &["26.4", "27.0", "28.1"]).is_none());
        assert!(newer("1.8.2", &["2.0.0", "1.8.2"]).is_none());
    }

    #[test]
    fn keeps_a_long_term_support_line() {
        // 2.4 is LTS: 2.4.11 ships after 2.5.0, smaller in number, newer in date
        let tags = ["2.4.10", "2.5.0", "2.5.3", "2.4.11", "3.0.0"];
        assert_eq!(newer("2.4.10", &tags).as_deref(), Some("2.4.11"));
        assert!(newer("2.4.11", &tags).is_none());
    }

    #[test]
    fn preserves_the_variant_suffix() {
        let tags = [
            "1.27.3",
            "1.27.4",
            "1.27.4-alpine",
            "1.27.5-alpine-slim",
            "1.27.5-perl",
        ];
        assert_eq!(
            newer("1.27.3-alpine", &tags).as_deref(),
            Some("1.27.4-alpine")
        );
        assert_eq!(newer("1.27.3", &tags).as_deref(), Some("1.27.4"));
        let editions = ["26.0-community", "26.3-community", "26.5-enterprise"];
        assert_eq!(
            newer("26.0-community", &editions).as_deref(),
            Some("26.3-community")
        );
    }

    #[test]
    fn ignores_prereleases_unless_opted_in() {
        let tags = [
            "1.2.3",
            "1.2.4-rc1",
            "1.2.5-beta",
            "1.2.6-alpha.2",
            "1.2.7-dev",
        ];
        assert!(newer("1.2.3", &tags).is_none());
        assert_eq!(
            newer_in_line("1.2.3", tags, true).as_deref(),
            Some("1.2.7-dev")
        );
        let nightly = ["26.1", "26.2-nightly"];
        assert!(newer("26.1", &nightly).is_none());
    }

    #[test]
    fn a_prerelease_in_use_is_told_about_its_release() {
        let tags = ["1.0.0-rc1", "1.0.0", "1.0.1-rc1"];
        assert_eq!(newer("1.0.0-rc1", &tags).as_deref(), Some("1.0.0"));
    }

    #[test]
    fn same_shape_and_prefix_only() {
        assert!(newer("26.1", &["26.4.2", "v26.4"]).is_none());
        assert_eq!(
            newer("v1.8.2", &["1.8.9", "v1.8.3"]).as_deref(),
            Some("v1.8.3")
        );
    }

    #[test]
    fn numeric_order_not_lexical() {
        assert_eq!(
            newer("1.9", &["1.10", "1.9", "1.2"]).as_deref(),
            Some("1.10")
        );
    }
}
