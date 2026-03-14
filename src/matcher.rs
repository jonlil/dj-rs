use crate::rekordbox::Track;
use crate::spotify::SpotifyTrack;

const DURATION_TOLERANCE_SECS: i32 = 20;
const MATCH_THRESHOLD: f64 = 0.82;

#[derive(Debug)]
pub struct MatchResult {
    pub spotify: SpotifyTrack,
    /// Some if a library track matched, None if missing
    pub matched: Option<Track>,
}

pub fn match_tracks(spotify_tracks: &[SpotifyTrack], library: &[Track]) -> Vec<MatchResult> {
    spotify_tracks.iter().map(|st| {
        let norm_title  = normalize_title(&st.title);
        let norm_artist = normalize_artist(&st.artist);
        let spotify_secs = (st.duration_ms / 1000) as i32;

        let best = library.iter()
            .filter(|t| {
                // Cheap duration pre-filter
                t.duration_secs
                    .map(|d| (d - spotify_secs).abs() <= DURATION_TOLERANCE_SECS)
                    .unwrap_or(true)
            })
            .filter_map(|t| {
                let t_title  = normalize_title(&t.title);
                let t_artist = t.artist.as_deref()
                    .map(normalize_artist)
                    .unwrap_or_default();

                let title_score  = strsim::jaro_winkler(&norm_title,  &t_title);
                let artist_score = strsim::jaro_winkler(&norm_artist, &t_artist);
                // Title matters more than artist
                let combined = title_score * 0.65 + artist_score * 0.35;

                if combined >= MATCH_THRESHOLD {
                    Some((combined, t))
                } else {
                    None
                }
            })
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, t)| t.clone());

        MatchResult { spotify: st.clone(), matched: best }
    }).collect()
}

// ── Normalisation ─────────────────────────────────────────────────────────────

/// Strip common parenthesized version suffixes and lowercase.
pub fn normalize_title(s: &str) -> String {
    let s = s.to_lowercase();

    // Strip anything in parentheses/brackets that looks like a version tag
    let s = strip_version_parens(&s);

    // Strip trailing "- remastered", "- radio edit" etc. after a dash
    let s = strip_trailing_dash_suffix(&s);

    // Strip feat. from title if it crept in
    let s = strip_feat(&s);

    s.trim().to_string()
}

/// Lowercase and take only the primary artist (before feat./ft./& /,).
pub fn normalize_artist(s: &str) -> String {
    let s = s.to_lowercase();
    // Split on common collaboration separators and take first part
    let primary = s
        .split(" feat")
        .next()
        .unwrap_or(&s)
        .split(" ft.")
        .next()
        .unwrap_or(&s)
        .split(" featuring")
        .next()
        .unwrap_or(&s)
        .split(" & ")
        .next()
        .unwrap_or(&s)
        .split(", ")
        .next()
        .unwrap_or(&s);
    primary.trim().to_string()
}

fn strip_version_parens(s: &str) -> String {
    const VERSION_KEYWORDS: &[&str] = &[
        "remastered", "remaster", "radio edit", "original mix", "original version",
        "single version", "album version", "extended mix", "extended version",
        "club mix", "club version", "12\" version", "7\" version",
        "feat.", "ft.", "featuring",
    ];

    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '(' || c == '[' {
            // Collect the content inside the bracket
            let close = if c == '(' { ')' } else { ']' };
            let mut inner = String::new();
            let mut closed = false;
            for ic in chars.by_ref() {
                if ic == close {
                    closed = true;
                    break;
                }
                inner.push(ic);
            }
            let inner_lower = inner.to_lowercase();
            let is_version = VERSION_KEYWORDS.iter()
                .any(|kw| inner_lower.contains(kw));
            if !is_version || !closed {
                result.push(c);
                result.push_str(&inner);
                if closed { result.push(close); }
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn strip_trailing_dash_suffix(s: &str) -> String {
    const DASH_SUFFIXES: &[&str] = &[
        "remastered", "remaster", "radio edit", "original mix",
        "single edit", "album version", "extended",
    ];
    if let Some(pos) = s.rfind(" - ") {
        let suffix = s[pos + 3..].to_lowercase();
        if DASH_SUFFIXES.iter().any(|kw| suffix.starts_with(kw)) {
            return s[..pos].to_string();
        }
    }
    s.to_string()
}

fn strip_feat(s: &str) -> String {
    for marker in &[" feat.", " ft.", " featuring"] {
        if let Some(pos) = s.find(marker) {
            // Only strip if feat. goes to end of string (not mid-title)
            return s[..pos].to_string();
        }
    }
    s.to_string()
}

// ── Shopping list ─────────────────────────────────────────────────────────────

pub fn shopping_list(missing: &[&SpotifyTrack]) -> String {
    missing.iter().map(|t| {
        let query = format!("{} {}", t.artist, t.title);
        let q = urlencoding::encode(&query);
        let dur_secs = t.duration_ms / 1000;
        format!(
            "{} - {}  ({}:{:02})\n  Beatport:    https://www.beatport.com/search?q={}\n  Traxsource:  https://www.traxsource.com/search?term={}",
            t.artist, t.title,
            dur_secs / 60, dur_secs % 60,
            q, q,
        )
    }).collect::<Vec<_>>().join("\n\n")
}
