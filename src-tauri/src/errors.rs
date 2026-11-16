//! Turns yt-dlp's error messages into one plain sentence and, where there is one, the fix to offer.

use serde::{Deserialize, Serialize};

use crate::links::Platform;

/// The one-click fix offered next to a failed download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Fix {
    /// Open Settings at "Use browser sign-in".
    SignIn,
    /// Update the downloader engine (yt-dlp), then try again.
    Update,
}

/// Why a download failed, ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// One plain sentence.
    pub message: String,
    pub fix: Option<Fix>,
    /// yt-dlp's own message, kept for the tooltip when `message` rewords it.
    pub detail: Option<String>,
}

/// Errors ClipDrop writes itself are already plain sentences.
impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self { message, fix: None, detail: None }
    }
}

fn browser_name(browser: &str) -> &'static str {
    match browser {
        "firefox" => "Firefox",
        "chrome" => "Chrome",
        "edge" => "Edge",
        "brave" => "Brave",
        _ => "your browser",
    }
}

/// Explains one of yt-dlp's `ERROR:` messages. `browser` is the browser sign-in setting ("" for none).
pub fn explain(raw: &str, platform: Platform, browser: &str) -> Failure {
    let e = raw.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| e.contains(n));
    let site = platform.label();
    let signed_in = !browser.is_empty();
    let say = |message: String, fix: Option<Fix>| Failure { message, fix, detail: Some(raw.to_string()) };

    // Checked in order: yt-dlp messages often mention several things ("Private video. Sign in if…").
    if signed_in && e.contains("cookie") && has(&["could not copy", "failed to decrypt", "could not find", "permission denied", "locked", "dpapi"]) {
        let name = browser_name(browser);
        return say(
            format!("Couldn't read your sign-in from {name}. Close {name} and try again, or switch to Firefox."),
            Some(Fix::SignIn),
        );
    }
    if has(&["confirm your age", "age-restricted", "age restricted", "inappropriate for some users"]) {
        return say("This video is age-restricted. Sign in with your browser to save it.".into(), Some(Fix::SignIn));
    }
    if has(&["private video", "account is private", "this video is private", "post is private"]) {
        return say("This post is private. Sign in with an account that can see it.".into(), Some(Fix::SignIn));
    }
    if e.contains("not a bot") {
        return say(
            "YouTube wants to check you're not a bot. Sign in with your browser, or wait a while and try again.".into(),
            Some(Fix::SignIn),
        );
    }
    if has(&["login required", "log in", "login", "sign in", "logged in", "authentication", "rate-limit", "empty media response"]) {
        let message = if signed_in {
            format!("{site} still wants you signed in. Check you're logged in to {site} in {}.", browser_name(browser))
        } else {
            format!("{site} wants you signed in to save this. Choose the browser you're logged in with.")
        };
        return say(message, Some(Fix::SignIn));
    }
    if has(&["your country", "your region", "geo restrict", "geo-restrict", "georestrict"]) {
        return say("This isn't available in your country.".into(), None);
    }
    if has(&[
        "video unavailable",
        "has been removed",
        "been deleted",
        "no longer available",
        "does not exist",
        "http error 404",
        "post is unavailable",
        "content isn't available",
        "this page isn't available",
    ]) {
        return say("This post was deleted or isn't available anymore.".into(), None);
    }
    if has(&["http error 403", "forbidden", "unable to extract", "nsig extraction", "please report this issue", "unsupported url"]) {
        return say(
            format!("{site} turned the download away. Updating the downloader usually fixes this."),
            Some(Fix::Update),
        );
    }
    if has(&["no space left", "disk full", "not enough space"]) {
        return say("Your drive is full. Free up some space or choose another save location.".into(), None);
    }
    if has(&["timed out", "getaddrinfo", "name resolution", "network is unreachable", "connection", "unable to download webpage"]) {
        return say(format!("Couldn't reach {site}. Check your connection and try again."), None);
    }
    // Unknown: show yt-dlp's message without its "[extractor] id: " prefix.
    Failure::from(strip_prefix(raw).to_string())
}

/// `[youtube] abc123: Something went wrong` -> `Something went wrong`.
fn strip_prefix(raw: &str) -> &str {
    let Some(rest) = raw.strip_prefix('[') else { return raw };
    let Some((_, rest)) = rest.split_once("] ") else { return raw };
    match rest.split_once(": ") {
        Some((id, msg)) if !id.contains(' ') && !msg.is_empty() => msg,
        _ => rest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix(raw: &str, platform: Platform, browser: &str) -> (Option<Fix>, String) {
        let f = explain(raw, platform, browser);
        (f.fix, f.message)
    }

    #[test]
    fn explains_common_yt_dlp_errors() {
        let ig = Platform::Instagram;
        let yt = Platform::Youtube;
        // Real messages from yt-dlp.
        let (f, m) = fix("[Instagram] C9xQ: Requested content is not available, rate-limit reached or login required. Use --cookies, --cookies-from-browser, --username and --password, --netrc-cmd, or --netrc (instagram) to provide account credentials", ig, "");
        assert_eq!(f, Some(Fix::SignIn));
        assert!(m.starts_with("Instagram wants you signed in"), "{m}");
        let (f, m) = fix("[Instagram] C9xQ: Requested content is not available, rate-limit reached or login required", ig, "firefox");
        assert_eq!(f, Some(Fix::SignIn));
        assert!(m.contains("Firefox"), "{m}");

        assert_eq!(fix("[youtube] abc: Private video. Sign in if you've been granted access to this video", yt, "").1, "This post is private. Sign in with an account that can see it.");
        assert_eq!(fix("[youtube] abc: Sign in to confirm your age. This video may be inappropriate for some users.", yt, "").0, Some(Fix::SignIn));
        assert!(fix("[youtube] abc: Sign in to confirm you're not a bot. Use --cookies-from-browser", yt, "").1.contains("not a bot"));
        assert_eq!(fix("[youtube] abc: Video unavailable. The uploader has not made this video available in your country", yt, "").1, "This isn't available in your country.");
        assert_eq!(fix("[youtube] abc: Video unavailable. This video has been removed by the uploader", yt, "").1, "This post was deleted or isn't available anymore.");
        assert_eq!(fix("[twitter] 123: NotFound: No status found with that ID. (HTTP Error 404: Not Found)", Platform::Twitter, "").0, None);
        assert_eq!(fix("unable to download video data: HTTP Error 403: Forbidden", yt, "").0, Some(Fix::Update));
        assert!(fix("[TikTok] 1: Unable to download webpage: <urlopen error [Errno 11001] getaddrinfo failed>", Platform::Tiktok, "").1.starts_with("Couldn't reach TikTok"));
        let (f, m) = fix("Could not copy Chrome cookie database. See https://github.com/yt-dlp/yt-dlp/issues/7271 for more info", ig, "chrome");
        assert_eq!(f, Some(Fix::SignIn));
        assert!(m.contains("Close Chrome"), "{m}");

        // The original stays available for the tooltip.
        assert!(explain("[youtube] abc: Private video", yt, "").detail.unwrap().contains("Private video"));
    }

    #[test]
    fn unknown_errors_lose_only_their_prefix() {
        let f = explain("[generic] xyz: Something odd happened", Platform::Twitter, "");
        assert_eq!((f.message.as_str(), f.fix), ("Something odd happened", None));
        assert_eq!(strip_prefix("Plain message"), "Plain message");
        assert_eq!(strip_prefix("[download] Got error: x"), "Got error: x");
    }
}
