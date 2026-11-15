//! Detects supported video links in arbitrary text (e.g. clipboard contents)
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
}

impl Platform {
    pub fn label(self) -> &'static str {
        match self {
            Platform::Twitter => "X",
            Platform::Instagram => "Instagram",
            Platform::Tiktok => "TikTok",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Platform::Twitter => "x",
            Platform::Instagram => "instagram",
            Platform::Tiktok => "tiktok",
        }
    }
}

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
            Regex::new(r"(?i)https?://(?:www\.|m\.)?tiktok\.com/@([^/\s?#]+)/video/(\d+)").unwrap(),
            |c| format!("https://www.tiktok.com/@{}/video/{}", &c[1], &c[2]),
        ),
        (
            Platform::Tiktok,
            Regex::new(r"(?i)https?://(?:vm|vt)\.tiktok\.com/([a-z0-9]+)").unwrap(),
            |c| format!("https://vm.tiktok.com/{}/", &c[1]),
        ),
        (
            Platform::Tiktok,
            Regex::new(r"(?i)https?://(?:www\.)?tiktok\.com/t/([a-z0-9]+)").unwrap(),
            |c| format!("https://www.tiktok.com/t/{}/", &c[1]),
        ),
    ]
});

/// Returns every supported link in `text`, in order of appearance, without duplicates.
pub fn extract(text: &str) -> Vec<Link> {
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
    fn ignores_unsupported_and_dedupes() {
        assert!(extract("https://x.com/someone").is_empty());
        assert!(extract("https://youtube.com/watch?v=abc").is_empty());
        assert!(extract("just some copied text").is_empty());

        let links = extract(
            "look https://x.com/a/status/1 and https://twitter.com/b/status/1 then https://vm.tiktok.com/Z1/",
        );
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].platform, Platform::Twitter);
        assert_eq!(links[1].platform, Platform::Tiktok);
    }
}
