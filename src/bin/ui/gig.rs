use iced::widget::{
    button, column, container, row, scrollable, space, text, text_input, text_editor,
    Column,
};
use iced::{Alignment, Background, Border, Color, Element, Fill};
use dj_rs::gig::{Gig, PendingBuyTrack};
use dj_rs::matcher;
use dj_rs::spotify::SpotifyTrack;
use super::theme as t;
use super::Message;

// ── Match status ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum MatchStatus {
    Idle,
    Running,
    Done,
    Error(String),
}

// ── Gig editing state ────────────────────────────────────────────────────────

pub struct GigState {
    pub gig_id: String,
    pub contact_id: String,
    pub contact_name: String,
    pub name: String,
    pub date: String,
    pub start_time: String,
    pub end_time: String,
    pub location: String,
    pub notes: text_editor::Content,
    pub spotify_url: String,
    pub dirty: bool,
    // Match state
    pub match_status: MatchStatus,
    pub match_results: Vec<MatchResultEntry>,
    pub accepted_track_ids: std::collections::HashSet<i64>,
    pub pending_buy_tracks: Vec<PendingBuyTrack>,
    pub denied_spotify_ids: std::collections::HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct MatchResultEntry {
    pub spotify: SpotifyTrack,
    pub matched_track_id: Option<i64>,
    pub matched_title: Option<String>,
    pub matched_artist: Option<String>,
}

impl GigState {
    pub fn from_gig(gig: &Gig, contact_name: &str) -> Self {
        Self {
            gig_id: gig.id.clone(),
            contact_id: gig.contact_id.clone(),
            contact_name: contact_name.to_string(),
            name: gig.name.clone(),
            date: gig.date.clone().unwrap_or_default(),
            start_time: gig.start_time.clone().unwrap_or_default(),
            end_time: gig.end_time.clone().unwrap_or_default(),
            location: gig.location.clone().unwrap_or_default(),
            notes: text_editor::Content::with_text(&gig.notes),
            spotify_url: gig.spotify_playlist_url.clone().unwrap_or_default(),
            dirty: false,
            match_status: MatchStatus::Idle,
            match_results: Vec::new(),
            accepted_track_ids: gig.accepted_track_ids.iter().cloned().collect(),
            pending_buy_tracks: gig.pending_buy_tracks.clone(),
            denied_spotify_ids: gig.denied_spotify_ids.iter().cloned().collect(),
        }
    }

    pub fn notes_text(&self) -> String {
        self.notes.text()
    }
}

// ── View ─────────────────────────────────────────────────────────────────────

pub fn view(state: &GigState) -> Element<Message> {
    // ── Header bar ────────────────────────────────────────────────────────────
    let back_btn = flat_btn(
        &format!("← {}", state.contact_name),
        t::TEXT_SECONDARY,
        Message::GigClosed,
    );

    let title = text(if state.name.is_empty() { "New Gig" } else { &state.name })
        .size(18)
        .color(t::TEXT_PRIMARY);

    let saved_label = if state.dirty {
        text("● unsaved").size(11).color(Color::from_rgb(0.9, 0.7, 0.2))
    } else {
        text("✓ saved").size(11).color(t::ACCENT_GREEN)
    };

    let save_btn = flat_btn("Save", t::ACCENT_BLUE, Message::GigSave);

    let header = container(
        row![back_btn, title, space::horizontal(), saved_label, save_btn]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .padding([8, 12])
    .width(Fill);

    let sep = separator();

    // ── Info section ──────────────────────────────────────────────────────────
    let info_section = column![
        section_header("Info"),
        field_row("Name", text_field(&state.name, "Event name", |s| Message::GigNameChanged(s))),
        field_row("Date", text_field(&state.date, "YYYY-MM-DD", |s| Message::GigDateChanged(s))),
        row![
            field_row("Start", text_field(&state.start_time, "HH:MM", |s| Message::GigStartTimeChanged(s))),
            field_row("End", text_field(&state.end_time, "HH:MM", |s| Message::GigEndTimeChanged(s))),
        ].spacing(16),
        field_row("Location", text_field(&state.location, "Venue / address", |s| Message::GigLocationChanged(s))),
        text("Notes").size(12).color(t::TEXT_SECONDARY),
        container(
            text_editor(&state.notes)
                .placeholder("Music preferences, vibe notes, client wishes...")
                .on_action(Message::GigNotesAction)
                .height(100)
                .style(|_, _| iced::widget::text_editor::Style {
                    background: Background::Color(t::BG_ROW),
                    border: Border { color: t::ACCENT_BORDER, width: 1.0, radius: 3.0.into() },
                    placeholder: t::TEXT_DIM,
                    value: t::TEXT_PRIMARY,
                    selection: t::ACCENT_BLUE,
                }),
        ).width(Fill),
    ]
    .spacing(8);

    // ── Spotify match section ─────────────────────────────────────────────────
    let match_btn_label = match &state.match_status {
        MatchStatus::Idle    => "Run Match",
        MatchStatus::Running => "Matching…",
        MatchStatus::Done    => "Re-run Match",
        MatchStatus::Error(_) => "Retry Match",
    };

    let match_btn_enabled = !matches!(state.match_status, MatchStatus::Running)
        && !state.spotify_url.is_empty();

    let mut match_btn = button(
        text(match_btn_label).size(13).color(if match_btn_enabled { t::ACCENT_BLUE } else { t::TEXT_DIM }),
    )
    .padding([4, 8])
    .style(|_, status| button::Style {
        background: Some(Background::Color(
            if matches!(status, button::Status::Hovered) { t::BG_HOVER } else { t::BG_ROW }
        )),
        border: Border { color: t::ACCENT_BORDER, width: 1.0, radius: 3.0.into() },
        text_color: Color::WHITE,
        ..Default::default()
    });

    if match_btn_enabled {
        match_btn = match_btn.on_press(Message::GigRunMatch);
    }

    let status_label: Element<Message> = match &state.match_status {
        MatchStatus::Running => text("Fetching playlist & matching…").size(12).color(t::TEXT_DIM).into(),
        MatchStatus::Error(e) => text(format!("Error: {}", e)).size(12).color(Color::from_rgb(0.9, 0.3, 0.3)).into(),
        MatchStatus::Done => {
            let matched = state.match_results.iter().filter(|r| r.matched_track_id.is_some()).count();
            let missing = state.match_results.iter().filter(|r| r.matched_track_id.is_none()).count();
            text(format!("{} matched, {} missing", matched, missing))
                .size(12).color(t::TEXT_SECONDARY).into()
        }
        MatchStatus::Idle => text("").size(12).into(),
    };

    let spotify_section = column![
        separator(),
        section_header("Spotify Matching"),
        row![
            container(
                text_input("Spotify playlist URL", &state.spotify_url)
                    .on_input(Message::GigSpotifyUrlChanged)
                    .size(14)
                    .style(|_, _| iced::widget::text_input::Style {
                        background: Background::Color(t::BG_ROW),
                        border: Border { color: t::ACCENT_BORDER, width: 1.0, radius: 3.0.into() },
                        icon: Color::TRANSPARENT,
                        placeholder: t::TEXT_DIM,
                        value: t::TEXT_PRIMARY,
                        selection: t::ACCENT_BLUE,
                    }),
            ).width(Fill),
            match_btn,
        ].spacing(8).align_y(Alignment::Center),
        status_label,
    ]
    .spacing(8);

    // ── Match results ─────────────────────────────────────────────────────────
    let results_section: Element<Message> = if !state.match_results.is_empty() {
        let matched: Vec<Element<Message>> = state.match_results.iter()
            .filter(|r| r.matched_track_id.is_some())
            .map(|r| view_matched_row(r, &state.accepted_track_ids))
            .collect();

        let missing: Vec<Element<Message>> = state.match_results.iter()
            .filter(|r| r.matched_track_id.is_none())
            .filter(|r| !state.denied_spotify_ids.contains(&r.spotify.spotify_id))
            .map(|r| view_missing_row(r, &state.pending_buy_tracks))
            .collect();

        let skipped: Vec<Element<Message>> = state.match_results.iter()
            .filter(|r| r.matched_track_id.is_none())
            .filter(|r| state.denied_spotify_ids.contains(&r.spotify.spotify_id))
            .map(|r| view_skipped_row(r))
            .collect();

        let mut sections = column![].spacing(8);

        if !matched.is_empty() {
            sections = sections
                .push(text(format!("Matched ({})", matched.len())).size(12).color(t::TEXT_SECONDARY))
                .push(Column::with_children(matched).spacing(2));
        }

        if !missing.is_empty() {
            sections = sections
                .push(text(format!("Missing ({})", missing.len())).size(12).color(Color::from_rgb(0.9, 0.7, 0.2)))
                .push(Column::with_children(missing).spacing(2));
        }

        if !skipped.is_empty() {
            sections = sections
                .push(text(format!("Skipped ({})", skipped.len())).size(12).color(t::TEXT_DIM))
                .push(Column::with_children(skipped).spacing(2));
        }

        sections.into()
    } else {
        column![].into()
    };

    // ── Buy list ──────────────────────────────────────────────────────────────
    let buy_section: Element<Message> = if !state.pending_buy_tracks.is_empty() {
        let buy_rows: Vec<Element<Message>> = state.pending_buy_tracks.iter().map(|t| {
            container(
                text(format!("{} – {}", t.artist, t.title))
                    .size(13).color(t::TEXT_SECONDARY),
            )
            .padding([2, 8])
            .into()
        }).collect();

        column![
            separator(),
            row![
                text(format!("Buy List ({})", state.pending_buy_tracks.len()))
                    .size(13).color(t::TEXT_PRIMARY),
                space::horizontal(),
                flat_btn("Copy Shopping List", t::ACCENT_BLUE, Message::GigCopyShoppingList),
            ].spacing(8).align_y(Alignment::Center),
            Column::with_children(buy_rows).spacing(1),
        ]
        .spacing(8)
        .into()
    } else {
        column![].into()
    };

    // ── Assemble ──────────────────────────────────────────────────────────────
    let form = column![
        info_section,
        spotify_section,
        results_section,
        buy_section,
    ]
    .spacing(12)
    .padding([16, 20])
    .width(Fill);

    let content = scrollable(form).height(Fill);

    container(
        column![header, sep, content].height(Fill).width(Fill),
    )
    .width(Fill)
    .height(Fill)
    .style(|_| iced::widget::container::Style {
        background: Some(Background::Color(t::BG_BASE)),
        ..Default::default()
    })
    .into()
}

// ── Match result rows ────────────────────────────────────────────────────────

fn view_matched_row<'a>(entry: &MatchResultEntry, accepted: &std::collections::HashSet<i64>) -> Element<'a, Message> {
    let track_id = entry.matched_track_id.unwrap();
    let is_accepted = accepted.contains(&track_id);

    let spotify_label = format!("{} – {}", entry.spotify.artist, entry.spotify.title);
    let local_label = format!(
        "→ {} – {}",
        entry.matched_artist.as_deref().unwrap_or(""),
        entry.matched_title.as_deref().unwrap_or(""),
    );

    let accept_label = if is_accepted { "✓ Accepted" } else { "Accept" };
    let accept_color = if is_accepted { t::ACCENT_GREEN } else { t::ACCENT_BLUE };

    let accept_btn = flat_btn(accept_label, accept_color, Message::GigAcceptTrack(track_id));

    container(
        row![
            column![
                text(spotify_label).size(13).color(t::TEXT_PRIMARY),
                text(local_label).size(12).color(t::TEXT_SECONDARY),
            ].spacing(1).width(Fill),
            accept_btn,
        ]
        .align_y(Alignment::Center)
        .spacing(8),
    )
    .padding([4, 8])
    .style(|_| iced::widget::container::Style {
        background: Some(Background::Color(t::BG_ROW)),
        border: Border { radius: 3.0.into(), ..Default::default() },
        ..Default::default()
    })
    .into()
}

fn view_missing_row<'a>(entry: &MatchResultEntry, buy_list: &[PendingBuyTrack]) -> Element<'a, Message> {
    let spotify_label = format!("{} – {}", entry.spotify.artist, entry.spotify.title);
    let dur_secs = entry.spotify.duration_ms / 1000;
    let dur_str = format!("{}:{:02}", dur_secs / 60, dur_secs % 60);

    let is_buying = buy_list.iter().any(|b| b.spotify_id == entry.spotify.spotify_id);
    let sid = entry.spotify.spotify_id.clone();

    let buy_btn = if is_buying {
        flat_btn("✓ Buy", t::ACCENT_GREEN, Message::GigBuyTrack(sid.clone()))
    } else {
        flat_btn("Buy", Color::from_rgb(0.9, 0.7, 0.2), Message::GigBuyTrack(sid.clone()))
    };

    let deny_btn = flat_btn("Skip", t::TEXT_DIM, Message::GigDenyTrack(sid));

    container(
        row![
            column![
                text(spotify_label).size(13).color(t::TEXT_PRIMARY),
                text(dur_str).size(11).color(t::TEXT_DIM),
            ].spacing(1).width(Fill),
            buy_btn,
            deny_btn,
        ]
        .align_y(Alignment::Center)
        .spacing(4),
    )
    .padding([4, 8])
    .style(|_| iced::widget::container::Style {
        background: Some(Background::Color(t::BG_ROW)),
        border: Border { radius: 3.0.into(), ..Default::default() },
        ..Default::default()
    })
    .into()
}

fn view_skipped_row<'a>(entry: &MatchResultEntry) -> Element<'a, Message> {
    let spotify_label = format!("{} – {}", entry.spotify.artist, entry.spotify.title);
    let sid = entry.spotify.spotify_id.clone();

    let undo_btn = flat_btn("Undo", t::TEXT_SECONDARY, Message::GigUnskipTrack(sid));

    container(
        row![
            text(spotify_label).size(13).color(t::TEXT_DIM).width(Fill),
            undo_btn,
        ]
        .align_y(Alignment::Center)
        .spacing(4),
    )
    .padding([4, 8])
    .style(|_| iced::widget::container::Style {
        background: Some(Background::Color(t::BG_ROW)),
        border: Border { radius: 3.0.into(), ..Default::default() },
        ..Default::default()
    })
    .into()
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn section_header(label: &str) -> Element<'static, Message> {
    let label = label.to_string();
    text(label).size(14).color(t::TEXT_PRIMARY).into()
}

fn field_row<'a>(label: &'a str, widget: Element<'a, Message>) -> Element<'a, Message> {
    row![
        container(text(label).size(12).color(t::TEXT_SECONDARY))
            .width(80)
            .align_y(Alignment::Center),
        container(widget).width(Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn text_field<'a, F>(value: &str, placeholder: &str, on_input: F) -> Element<'a, Message>
where
    F: Fn(String) -> Message + 'a,
{
    text_input(placeholder, value)
        .on_input(on_input)
        .size(14)
        .style(|_, _| iced::widget::text_input::Style {
            background: Background::Color(t::BG_ROW),
            border: Border { color: t::ACCENT_BORDER, width: 1.0, radius: 3.0.into() },
            icon: Color::TRANSPARENT,
            placeholder: t::TEXT_DIM,
            value: t::TEXT_PRIMARY,
            selection: t::ACCENT_BLUE,
        })
        .into()
}

fn flat_btn(label: &str, color: Color, msg: Message) -> Element<'static, Message> {
    let label = label.to_string();
    button(
        text(label).size(13).color(color),
    )
    .padding([4, 8])
    .style(move |_, status| button::Style {
        background: Some(Background::Color(
            if matches!(status, button::Status::Hovered) { t::BG_HOVER } else { Color::TRANSPARENT }
        )),
        border: Border::default(),
        text_color: Color::WHITE,
        ..Default::default()
    })
    .on_press(msg)
    .into()
}

fn separator() -> Element<'static, Message> {
    container(column![])
        .width(Fill)
        .height(1)
        .style(|_| iced::widget::container::Style {
            background: Some(Background::Color(t::SEPARATOR)),
            ..Default::default()
        })
        .into()
}
