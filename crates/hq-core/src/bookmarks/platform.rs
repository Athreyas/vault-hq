//! A badge for where a link points, read from the URL alone (no fetch).

use crate::types::Platform;

/// Hosts per platform; a host matches itself and any subdomain (`m.youtube.com`).
const HOSTS: &[(Platform, &[&str])] = &[
    (
        Platform::Youtube,
        &["youtube.com", "youtu.be", "youtube-nocookie.com"],
    ),
    (
        Platform::Github,
        &["github.com", "github.io", "githubusercontent.com"],
    ),
    (Platform::X, &["x.com", "twitter.com"]),
    (Platform::Reddit, &["reddit.com", "redd.it"]),
    (Platform::Hackernews, &["news.ycombinator.com"]),
    (Platform::Instagram, &["instagram.com"]),
    (Platform::Tiktok, &["tiktok.com"]),
];

pub fn detect(url: &str) -> Platform {
    let Ok(u) = url::Url::parse(url) else {
        return Platform::Article;
    };
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    for (p, hosts) in HOSTS {
        if hosts
            .iter()
            .any(|h| host == *h || host.ends_with(&format!(".{h}")))
        {
            return *p;
        }
    }
    if u.path().to_ascii_lowercase().ends_with(".pdf") {
        return Platform::Pdf;
    }
    Platform::Article
}

/// The value written to a note's `platform:` key and used in filters.
pub fn name(p: Platform) -> &'static str {
    match p {
        Platform::Youtube => "youtube",
        Platform::Github => "github",
        Platform::X => "x",
        Platform::Reddit => "reddit",
        Platform::Hackernews => "hackernews",
        Platform::Instagram => "instagram",
        Platform::Tiktok => "tiktok",
        Platform::Pdf => "pdf",
        Platform::Article => "article",
    }
}

pub fn from_name(s: &str) -> Option<Platform> {
    [
        Platform::Youtube,
        Platform::Github,
        Platform::X,
        Platform::Reddit,
        Platform::Hackernews,
        Platform::Instagram,
        Platform::Tiktok,
        Platform::Pdf,
        Platform::Article,
    ]
    .into_iter()
    .find(|p| name(*p) == s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badges() {
        for (url, p) in [
            ("https://www.youtube.com/watch?v=1", Platform::Youtube),
            ("https://youtu.be/1", Platform::Youtube),
            ("https://m.youtube.com/watch?v=1", Platform::Youtube),
            ("https://github.com/a/b", Platform::Github),
            ("https://gist.github.com/a", Platform::Github),
            ("https://twitter.com/a/status/1", Platform::X),
            ("https://x.com/a", Platform::X),
            ("https://old.reddit.com/r/rust", Platform::Reddit),
            (
                "https://news.ycombinator.com/item?id=1",
                Platform::Hackernews,
            ),
            ("https://www.instagram.com/p/1", Platform::Instagram),
            ("https://www.tiktok.com/@a/video/1", Platform::Tiktok),
            ("https://example.org/Paper.PDF", Platform::Pdf),
            ("https://example.org/post", Platform::Article),
            ("https://notgithub.com/", Platform::Article),
            ("https://ycombinator.com/", Platform::Article),
            ("not a url", Platform::Article),
        ] {
            assert_eq!(detect(url), p, "{url}");
        }
    }

    #[test]
    fn names_round_trip() {
        for p in [Platform::Hackernews, Platform::X, Platform::Pdf] {
            assert_eq!(from_name(name(p)), Some(p));
        }
        assert_eq!(from_name("myspace"), None);
    }
}
