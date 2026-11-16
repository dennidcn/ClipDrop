//! Detects supported video links in arbitrary pasted text
//! and normalises them so the same post is only downloaded once.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Twitter,
    Instagram,
    Tiktok,
    Youtube,
}

impl Platform {
    pub fn label(self) -> &'static str {
        match self {
            Platform::Twitter => "X",
            Platform::Instagram => "Instagram",
            Platform::Tiktok => "TikTok",
            Platform::Youtube => "YouTube",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Platform::Twitter => "x",
            Platform::Instagram => "instagram",
            Platform::Tiktok => "tiktok",
            Platform::Youtube => "youtube",
        }
    }

    /// Strict yt-dlp `--allowed-extractors` value for this platform so shortlinks cannot pivot to arbitrary extractors.
    pub fn allowed_extractors(self) -> &'static str {
        match self {
            Platform::Twitter => "twitter",
            Platform::Instagram => "instagram",
            // Names are regexes matched against yt-dlp extractor names; short links use `vm.tiktok`.
            Platform::Tiktok => r"tiktok,vm\.tiktok",
            Platform::Youtube => "youtube",
        }
    }
}

/// Maximum bytes of input text scanned for links (protects against huge pasted payloads).
const MAX_EXTRACT_BYTES: usize = 32 * 1024;
/// Maximum distinct links extracted from a single paste.
pub const MAX_LINKS_PER_EXTRACT: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub platform: Platform,
    /// Canonical URL handed to yt-dlp and used for de-duplication.
    pub url: String,
}

type Canonicalise = fn(&regex::Captures) -> String;

static PATTERNS: LazyLock<Vec<(Platform, Regex, Canonicalise)>> = LazyLock::new(|| {
    vec![
        (
            Platform::Twitter,
            Regex::new(
                r"(?i)https?://(?:www\.|mobile\.)?(?:twitter|x|fxtwitter|vxtwitter|fixupx|fixvx)\.com/(?:[a-z0-9_]{1,15}|i(?:/web)?)/status(?:es)?/(\d+)",
            )
            .unwrap(),
            |c| format!("https://x.com/i/status/{}", &c[1]),
        ),
        (
            Platform::Instagram,
            Regex::new(
                r"(?i)https?://(?:www\.)?instagram\.com/(?:[a-z0-9_.]+/)?(p|reels?|tv)/([a-z0-9_-]+)",
            )
            .unwrap(),
            |c| {
                let kind = match c[1].to_ascii_lowercase().as_str() {
                    "reels" | "reel" => "reel",
                    "tv" => "tv",
                    _ => "p",
                };
                format!("https://www.instagram.com/{kind}/{}/", &c[2])
            },
        ),
        (
            Platform::Tiktok,
            Regex::new(r"(?i)https?://(?:www\.|m\.)?tiktok\.com/@([a-z0-9_.-]{1,64})/video/(\d+)").unwrap(),
            |c| format!("https://www.tiktok.com/@{}/video/{}", &c[1], &c[2]),
        ),
        (
            Platform::Tiktok,
            Regex::new(r"(?i)https?://(?:vm|vt)\.tiktok\.com/([a-z0-9_-]+)").unwrap(),
            |c| format!("https://vm.tiktok.com/{}/", &c[1]),
        ),
        (
            Platform::Tiktok,
            Regex::new(r"(?i)https?://(?:www\.)?tiktok\.com/t/([a-z0-9_-]+)").unwrap(),
            |c| format!("https://www.tiktok.com/t/{}/", &c[1]),
        ),
        // YouTube video IDs are exactly 11 characters; the trailing group stops longer IDs matching.
        (
            Platform::Youtube,
            Regex::new(
                r"(?i)https?://(?:www\.|m\.|music\.)?youtube\.com/watch\?(?:[^\s#]*?&)?v=([a-z0-9_-]{11})(?:[^a-z0-9_-]|$)",
            )
            .unwrap(),
            youtube_canonical,
        ),
        (
            Platform::Youtube,
            Regex::new(
                r"(?i)https?://(?:www\.|m\.)?youtube\.com/(?:shorts|live|embed)/([a-z0-9_-]{11})(?:[^a-z0-9_-]|$)",
            )
            .unwrap(),
            youtube_canonical,
        ),
        (
            Platform::Youtube,
            Regex::new(r"(?i)https?://youtu\.be/([a-z0-9_-]{11})(?:[^a-z0-9_-]|$)").unwrap(),
            youtube_canonical,
        ),
    ]
});

fn youtube_canonical(c: &regex::Captures) -> String {
    format!("https://www.youtube.com/watch?v={}", &c[1])
}

fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Returns supported links in `text` (up to `MAX_LINKS_PER_EXTRACT`), in order of appearance, without duplicates.
pub fn extract(text: &str) -> Vec<Link> {
    let text = truncate_utf8(text, MAX_EXTRACT_BYTES);
    let mut found: Vec<(usize, Link)> = Vec::new();
    for (platform, re, canon) in PATTERNS.iter() {
        for caps in re.captures_iter(text) {
            let start = caps.get(0).map_or(0, |m| m.start());
            found.push((start, Link { platform: *platform, url: canon(&caps) }));
        }
    }
    found.sort_by_key(|(start, _)| *start);

    let mut out: Vec<Link> = Vec::new();
    for (_, link) in found {
        if !out.contains(&link) {
            out.push(link);
            if out.len() >= MAX_LINKS_PER_EXTRACT {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> Link {
        let mut links = extract(text);
        assert_eq!(links.len(), 1, "expected exactly one link in {text:?}, got {links:?}");
        links.remove(0)
    }

    #[test]
    fn twitter_variants() {
        for url in [
            "https://x.com/someone/status/1790000000000000000",
            "https://twitter.com/someone/status/1790000000000000000?s=20&t=abc",
            "http://mobile.twitter.com/someone/status/1790000000000000000/video/1",
            "https://fxtwitter.com/someone/status/1790000000000000000",
            "https://x.com/i/status/1790000000000000000",
            "https://x.com/i/web/status/1790000000000000000",
        ] {
            let link = one(url);
            assert_eq!(link.platform, Platform::Twitter);
            assert_eq!(link.url, "https://x.com/i/status/1790000000000000000");
        }
    }

    #[test]
    fn instagram_variants() {
        assert_eq!(
            one("https://www.instagram.com/reel/C1a2B3c4D5e/?igsh=xyz").url,
            "https://www.instagram.com/reel/C1a2B3c4D5e/"
        );
        assert_eq!(
            one("https://instagram.com/reels/C1a2B3c4D5e").url,
            "https://www.instagram.com/reel/C1a2B3c4D5e/"
        );
        assert_eq!(
            one("https://www.instagram.com/some.user/p/C1a2B3c4D5e/").url,
            "https://www.instagram.com/p/C1a2B3c4D5e/"
        );
    }

    #[test]
    fn tiktok_variants() {
        assert_eq!(
            one("https://www.tiktok.com/@some.user/video/7300000000000000000?is_from_webapp=1").url,
            "https://www.tiktok.com/@some.user/video/7300000000000000000"
        );
        assert_eq!(one("https://vm.tiktok.com/ZMabc123/").url, "https://vm.tiktok.com/ZMabc123/");
        assert_eq!(one("https://vt.tiktok.com/ZSabc123").url, "https://vm.tiktok.com/ZSabc123/");
        assert_eq!(one("https://www.tiktok.com/t/ZTabc123/").url, "https://www.tiktok.com/t/ZTabc123/");
    }

    #[test]
    fn youtube_variants() {
        for url in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/watch?v=dQw4w9WgXcQ&t=42s",
            "https://www.youtube.com/watch?feature=share&v=dQw4w9WgXcQ&list=PL123",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ&si=abc",
            "https://youtu.be/dQw4w9WgXcQ?si=abc",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
            "https://www.youtube.com/live/dQw4w9WgXcQ?feature=share",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
        ] {
            let link = one(url);
            assert_eq!(link.platform, Platform::Youtube, "{url}");
            assert_eq!(link.url, "https://www.youtube.com/watch?v=dQw4w9WgXcQ", "{url}");
        }
        // IDs keep their case and longer IDs are rejected.
        assert_eq!(one("youtu.be is https://youtu.be/AbC-_12345z").url, "https://www.youtube.com/watch?v=AbC-_12345z");
        assert!(extract("https://youtu.be/dQw4w9WgXcQextra").is_empty());
    }

    #[test]
    fn ignores_unsupported_and_dedupes() {
        assert!(extract("https://x.com/someone").is_empty());
        assert!(extract("https://youtube.com/watch?v=abc").is_empty()); // ID too short
        assert!(extract("https://youtube.com/@somechannel").is_empty());
        assert!(extract("https://vimeo.com/123456").is_empty());
        assert!(extract("just some copied text").is_empty());

        let links = extract(
            "look https://x.com/a/status/1 and https://twitter.com/b/status/1 then https://vm.tiktok.com/Z1/",
        );
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].platform, Platform::Twitter);
        assert_eq!(links[1].platform, Platform::Tiktok);
    }

    #[test]
    fn canonical_urls_are_stable() {
        // The saved downloads list is only restored for links that come back unchanged.
        for url in [
            "https://twitter.com/someone/status/1790000000000000000?s=20",
            "https://instagram.com/reels/C1a2B3c4D5e",
            "https://www.instagram.com/some.user/p/C1a2B3c4D5e/",
            "https://www.tiktok.com/@some.user/video/7300000000000000000?is_from_webapp=1",
            "https://vt.tiktok.com/ZSabc123",
            "https://www.tiktok.com/t/ZTabc123/",
            "https://youtu.be/dQw4w9WgXcQ?si=abc",
        ] {
            let first = one(url);
            assert_eq!(one(&first.url), first, "{url}");
        }
    }

    #[test]
    fn caps_extracted_links_and_truncates_large_inputs() {
        let many: String = (1..=50)
            .map(|i| format!("https://x.com/user/status/{i} "))
            .collect();
        let links = extract(&many);
        assert_eq!(links.len(), MAX_LINKS_PER_EXTRACT);

        let padding = "a".repeat(MAX_EXTRACT_BYTES + 1024);
        let tail = format!("{padding} https://x.com/user/status/999");
        assert!(extract(&tail).is_empty());
    }
}
