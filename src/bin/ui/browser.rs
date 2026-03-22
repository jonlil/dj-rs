use iced::widget::{
    button, column, container, mouse_area, row, scrollable, space, text, text_input, Column,
};
use iced::{Alignment, Background, Border, Color, Element, Fill, Font};
use dj_rs::rekordbox::{Playlist, Track};
use dj_rs::gig::{Contact, CustomerType, GigStore};
use dj_rs::spotify::{SpotifyTrack, UserPlaylist};
use super::theme as t;
use super::Message;

// ── Section (icon bar tabs) ───────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Section {
    Library,
    Spotify,
    Contacts,
}

impl Section {
    pub fn icon(&self) -> &'static str {
        match self {
            Section::Library  => "♫",
            Section::Spotify  => "S",
            Section::Contacts => "◉",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Section::Library  => "LIBRARY",
            Section::Spotify  => "SPOTIFY",
            Section::Contacts => "CONTACTS",
        }
    }
}

// ── Tree node ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub id: i64,
    pub name: String,
    pub is_folder: bool,
    pub track_count: u32,
    pub children: Vec<TreeNode>,
}

pub fn build_tree(playlists: &[Playlist], parent_id: Option<i64>) -> Vec<TreeNode> {
    playlists.iter()
        .filter(|p| p.parent_id == parent_id)
        .map(|p| TreeNode {
            id: p.id,
            name: p.name.clone(),
            is_folder: p.attribute == 1,
            track_count: p.track_count,
            children: build_tree(playlists, Some(p.id)),
        })
        .collect()
}

// ── Selection ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Selection {
    All,
    Playlist(i64),
    SpotifyPlaylist(String), // spotify playlist id
    History(i64),
    None,
}

// ── Browser state ─────────────────────────────────────────────────────────────

pub struct BrowserState {
    pub section: Section,
    pub sidebar_open: bool,
    pub playlists: Vec<Playlist>,
    pub expanded: std::collections::HashSet<i64>,
    pub selection: Selection,
    pub tracks: Vec<Track>,
    pub search: String,
    pub gig_store: GigStore,
    // Spotify section
    pub spotify_playlists: Vec<UserPlaylist>,
    pub spotify_tracks: Vec<SpotifyTrackRow>,
    pub spotify_loading: bool,
    pub selected_track_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct SpotifyTrackRow {
    pub spotify: SpotifyTrack,
    pub in_library: bool,
    pub library_track_id: Option<i64>,
}

impl BrowserState {
    pub fn new(playlists: Vec<Playlist>, gig_store: GigStore) -> Self {
        Self {
            section: Section::Library,
            sidebar_open: true,
            playlists,
            expanded: std::collections::HashSet::new(),
            selection: Selection::All,
            tracks: Vec::new(),
            search: String::new(),
            gig_store,
            spotify_playlists: Vec::new(),
            spotify_tracks: Vec::new(),
            spotify_loading: false,
            selected_track_id: None,
        }
    }
}

// ── View ──────────────────────────────────────────────────────────────────────

pub fn view<'a>(state: &'a BrowserState, detail: Option<Element<'a, Message>>, active_contact_id: Option<&str>) -> Element<'a, Message> {
    let icon_bar = view_icon_bar(&state.section, state.sidebar_open);

    let tree = if state.sidebar_open {
        view_tree(state, active_contact_id)
    } else {
        column![].into()
    };

    let main = match detail {
        Some(d) => d,
        None => {
            if state.section == Section::Spotify && !state.spotify_tracks.is_empty() {
                view_spotify_main(state)
            } else if state.section == Section::Spotify && state.spotify_loading {
                container(text("Loading tracks…").size(14).color(t::TEXT_DIM))
                    .width(Fill).height(Fill)
                    .align_x(Alignment::Center).align_y(Alignment::Center)
                    .into()
            } else {
                view_main(state)
            }
        }
    };

    let body = row![icon_bar, tree, main].height(Fill);

    container(body)
        .width(Fill)
        .height(Fill)
        .style(|_| iced::widget::container::Style {
            background: Some(Background::Color(t::BG_BASE)),
            ..Default::default()
        })
        .into()
}

// ── Icon bar ──────────────────────────────────────────────────────────────────

fn view_icon_bar(active: &Section, _sidebar_open: bool) -> Element<'static, Message> {
    let sections = [Section::Library, Section::Spotify, Section::Contacts];

    let icons: Vec<Element<Message>> = sections.iter().map(|s| {
        let is_active = s == active;
        let icon_text = text(s.icon())
            .size(20)
            .color(if is_active { t::ACCENT_BLUE } else { t::TEXT_DIM })
            .font(Font::DEFAULT);

        let btn = button(
            container(icon_text)
                .width(t::ICON_BAR_W)
                .height(t::ICON_BAR_W)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
        )
        .padding(0)
        .style(move |_, _| button::Style {
            background: Some(Background::Color(if is_active {
                t::BG_HOVER
            } else {
                t::BG_ICON
            })),
            border: Border {
                color: if is_active { t::ACCENT_BLUE } else { Color::TRANSPARENT },
                width: if is_active { 2.0 } else { 0.0 },
                radius: 0.0.into(),
            },
            text_color: Color::WHITE,
            ..Default::default()
        })
        .on_press(Message::SectionClicked(s.clone()));

        btn.into()
    }).collect();

    let settings_btn = button(
        container(
            text("⚙").size(20).color(t::TEXT_DIM).font(Font::DEFAULT)
        )
        .width(t::ICON_BAR_W)
        .height(t::ICON_BAR_W)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center),
    )
    .padding(0)
    .style(|_, status| button::Style {
        background: Some(Background::Color(
            if matches!(status, button::Status::Hovered) { t::BG_HOVER } else { t::BG_ICON }
        )),
        border: Border::default(),
        text_color: Color::WHITE,
        ..Default::default()
    })
    .on_press(Message::SettingsClicked);

    container(
        column![
            Column::with_children(icons).spacing(2).padding([8, 0]),
            space::vertical(),
            container(settings_btn).padding([8, 0]),
        ]
    )
    .width(t::ICON_BAR_W)
    .height(Fill)
    .style(|_| iced::widget::container::Style {
        background: Some(Background::Color(t::BG_ICON)),
        ..Default::default()
    })
    .into()
}

// ── Tree panel ────────────────────────────────────────────────────────────────

fn view_tree<'a>(state: &'a BrowserState, active_contact_id: Option<&str>) -> Element<'a, Message> {
    let header = container(
        text(state.section.label())
            .size(11)
            .color(t::TEXT_SECONDARY)
            .font(Font::MONOSPACE),
    )
    .padding([6, 12])
    .width(Fill);

    let separator = container(column![])
        .width(Fill)
        .height(1)
        .style(|_| iced::widget::container::Style {
            background: Some(Background::Color(t::SEPARATOR)),
            ..Default::default()
        });

    let content: Element<Message> = match state.section {
        Section::Library => view_library_tree(state),
        Section::Contacts => view_contacts_list(&state.gig_store, active_contact_id),
        Section::Spotify => view_spotify_tree(state),
    };

    let panel = column![header, separator, content].height(Fill);

    container(panel)
        .width(t::TREE_PANEL_W)
        .height(Fill)
        .style(|_| iced::widget::container::Style {
            background: Some(Background::Color(t::BG_PANEL)),
            border: Border {
                color: t::SEPARATOR,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn view_library_tree(state: &BrowserState) -> Element<Message> {
    let all_selected = state.selection == Selection::All;
    let all_row = tree_row_btn("⊛  All Tracks", 0, all_selected, Message::NodeSelected(Selection::All));
    let tree = build_tree(&state.playlists, None);
    let nodes: Vec<Element<Message>> = tree.iter()
        .map(|node| render_tree_node(node, 0, &state.expanded, &state.selection))
        .collect();
    scrollable(
        column![all_row, Column::with_children(nodes).spacing(0)].spacing(0)
    ).height(Fill).into()
}

fn view_spotify_tree(state: &BrowserState) -> Element<Message> {
    if state.spotify_playlists.is_empty() {
        return container(
            column![
                text("No playlists loaded").size(13).color(t::TEXT_DIM),
                text("Connecting to Spotify…").size(11).color(t::TEXT_DIM),
            ].spacing(4).align_x(Alignment::Center),
        )
        .width(Fill).height(Fill)
        .align_x(Alignment::Center).align_y(Alignment::Center)
        .into();
    }

    let rows: Vec<Element<Message>> = state.spotify_playlists.iter().map(|pl| {
        let is_selected = state.selection == Selection::SpotifyPlaylist(pl.id.clone());
        let pl_id = pl.id.clone();
        let count = if pl.track_count > 0 {
            text(format!("{}", pl.track_count)).size(11).color(t::TEXT_DIM)
        } else {
            text("").size(11).color(Color::TRANSPARENT)
        };

        let label_row = row![
            container(text("")).width(12.0),
            container(
                text(&pl.name).size(13).color(if is_selected { Color::WHITE } else { t::TEXT_PRIMARY })
            ).width(Fill).clip(true),
            count,
        ]
        .align_y(Alignment::Center)
        .padding([0, 8]);

        button(
            container(label_row)
                .width(Fill)
                .height(t::TREE_ROW_H)
                .align_y(Alignment::Center)
        )
            .width(Fill)
            .height(t::TREE_ROW_H)
            .padding(0)
            .style(move |_, status| {
                let bg = if is_selected {
                    t::BG_ACTIVE
                } else if matches!(status, button::Status::Hovered) {
                    t::BG_HOVER
                } else {
                    Color::TRANSPARENT
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border::default(),
                    text_color: Color::WHITE,
                    ..Default::default()
                }
            })
            .on_press(Message::SpotifyPlaylistSelected(pl_id))
            .into()
    }).collect();

    scrollable(Column::with_children(rows).spacing(0))
        .height(Fill)
        .into()
}

fn view_spotify_main(state: &BrowserState) -> Element<Message> {
    // Column headers
    let headers = container(
        row![
            text("").width(24),
            text("TITLE").size(11).color(t::TEXT_DIM).width(Fill),
            text("ARTIST").size(11).color(t::TEXT_DIM).width(160),
            text("TIME").size(11).color(t::TEXT_DIM).width(46),
            text("STATUS").size(11).color(t::TEXT_DIM).width(80),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .padding([0, 8])
    .height(t::HEADER_H)
    .width(Fill)
    .style(|_| iced::widget::container::Style {
        background: Some(Background::Color(t::BG_PANEL)),
        ..Default::default()
    });

    let rows: Vec<Element<Message>> = state.spotify_tracks.iter().enumerate().map(|(i, tr)| {
        let bg = if i % 2 == 0 { t::BG_BASE } else { t::BG_ROW };
        let dur_secs = tr.spotify.duration_ms / 1000;
        let dur_str = format!("{}:{:02}", dur_secs / 60, dur_secs % 60);

        let (status_text, status_color) = if tr.in_library {
            ("✓ In library", t::ACCENT_GREEN)
        } else {
            ("Missing", Color::from_rgb(0.9, 0.7, 0.2))
        };

        let indicator_color = if tr.in_library { t::ACCENT_GREEN } else { Color::from_rgb(0.9, 0.7, 0.2) };
        let indicator = container(
            text("●").size(8).color(indicator_color)
        )
        .width(24)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);

        let row_content = row![
            indicator,
            container(text(&tr.spotify.title).size(14).color(t::TEXT_PRIMARY)).width(Fill).clip(true),
            container(text(&tr.spotify.artist).size(13).color(t::TEXT_SECONDARY)).width(160).clip(true),
            text(dur_str).size(13).color(t::TEXT_DIM).width(46),
            text(status_text).size(12).color(status_color).width(80),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .padding([0, 8]);

        container(row_content)
            .width(Fill)
            .height(t::TRACK_ROW_H)
            .align_y(Alignment::Center)
            .style(move |_| iced::widget::container::Style {
                background: Some(Background::Color(bg)),
                ..Default::default()
            })
            .into()
    }).collect();

    let track_list = scrollable(Column::with_children(rows).spacing(0))
        .height(Fill);

    // Summary bar
    let in_lib = state.spotify_tracks.iter().filter(|t| t.in_library).count();
    let missing = state.spotify_tracks.len() - in_lib;
    let summary = container(
        row![
            text(format!("{} tracks", state.spotify_tracks.len())).size(12).color(t::TEXT_SECONDARY),
            space::horizontal(),
            text(format!("✓ {} in library", in_lib)).size(12).color(t::ACCENT_GREEN),
            text(format!("  ·  {} missing", missing)).size(12).color(Color::from_rgb(0.9, 0.7, 0.2)),
        ]
        .align_y(Alignment::Center),
    )
    .padding([4, 8])
    .width(Fill)
    .style(|_| iced::widget::container::Style {
        background: Some(Background::Color(t::BG_PANEL)),
        ..Default::default()
    });

    container(column![headers, track_list, summary].height(Fill).width(Fill))
        .height(Fill)
        .width(Fill)
        .clip(true)
        .into()
}

fn view_contacts_list<'a>(store: &GigStore, active_contact_id: Option<&str>) -> Element<'a, Message> {
    let mut rows: Vec<Element<Message>> = Vec::new();

    // "+ New Contact" button at top
    let add_btn = button(
        row![
            container(text("")).width(20.0),
            text("+ New Contact").size(13).color(t::ACCENT_BLUE),
        ]
        .align_y(Alignment::Center)
        .padding([0, 8]),
    )
    .width(Fill)
    .height(t::TREE_ROW_H)
    .padding(0)
    .style(|_, status| button::Style {
        background: Some(Background::Color(
            if matches!(status, button::Status::Hovered) { t::BG_HOVER } else { Color::TRANSPARENT }
        )),
        border: Border::default(),
        text_color: Color::WHITE,
        ..Default::default()
    })
    .on_press(Message::ContactAdd);
    rows.push(add_btn.into());

    let types = [
        (CustomerType::Private,   "PRIVATE"),
        (CustomerType::Corporate, "CORPORATE"),
        (CustomerType::Venue,     "VENUES"),
    ];

    for (ct, header_label) in &types {
        let contacts: Vec<&Contact> = store.contacts.iter()
            .filter(|c| &c.customer_type == ct)
            .collect();

        if contacts.is_empty() {
            continue;
        }

        // Group header
        let header = container(
            text(*header_label).size(10).color(t::TEXT_DIM).font(Font::MONOSPACE),
        )
        .padding([6, 12])
        .width(Fill);
        rows.push(header.into());

        for contact in contacts {
            let badge_color = match ct {
                CustomerType::Private   => Color::from_rgb(0.4, 0.6, 1.0),
                CustomerType::Corporate => Color::from_rgb(1.0, 0.7, 0.3),
                CustomerType::Venue     => Color::from_rgb(0.5, 0.9, 0.5),
            };
            let contact_id = contact.id.clone();
            let is_active = active_contact_id == Some(contact.id.as_str());
            let name = contact.name.clone();
            let gig_count = store.gigs.iter().filter(|g| g.contact_id == contact_id).count();

            let count_label = if gig_count > 0 {
                text(format!("{}", gig_count)).size(11).color(t::TEXT_DIM)
            } else {
                text("").size(11).color(Color::TRANSPARENT)
            };

            let row_content = row![
                container(text("●").size(8).color(badge_color))
                    .width(20)
                    .align_y(Alignment::Center),
                container(
                    text(name).size(13).color(if is_active { Color::WHITE } else { t::TEXT_PRIMARY })
                ).width(Fill).clip(true),
                count_label,
            ]
            .align_y(Alignment::Center)
            .padding([0, 8]);

            let btn = button(row_content)
                .width(Fill)
                .height(t::TREE_ROW_H)
                .padding(0)
                .style(move |_, status| button::Style {
                    background: Some(Background::Color(
                        if is_active { t::BG_ACTIVE }
                        else if matches!(status, button::Status::Hovered) { t::BG_HOVER }
                        else { Color::TRANSPARENT }
                    )),
                    border: Border::default(),
                    text_color: Color::WHITE,
                    ..Default::default()
                })
                .on_press(Message::ContactOpened(contact_id));
            rows.push(btn.into());
        }
    }

    scrollable(Column::with_children(rows).spacing(0))
        .height(Fill)
        .into()
}

fn render_tree_node(
    node: &TreeNode,
    depth: usize,
    expanded: &std::collections::HashSet<i64>,
    selection: &Selection,
) -> Element<'static, Message> {
    let indent = (depth as f32) * 14.0 + 12.0;
    let is_expanded = expanded.contains(&node.id);
    let is_selected = *selection == Selection::Playlist(node.id);

    let prefix = if node.is_folder {
        if is_expanded { "▾ " } else { "▸ " }
    } else {
        "  "
    };

    let label = format!("{}{}", prefix, node.name);

    let count_text = if !node.is_folder && node.track_count > 0 {
        text(format!("{}", node.track_count))
            .size(11)
            .color(t::TEXT_DIM)
    } else {
        text("").size(11).color(Color::TRANSPARENT)
    };

    let label_row = row![
        container(text("")).width(indent),
        container(
            text(label).size(14).color(if is_selected { Color::WHITE } else { t::TEXT_PRIMARY })
        ).width(Fill).clip(true),
        count_text,
    ]
    .align_y(Alignment::Center)
    .padding([0, 8]);

    let msg = if node.is_folder {
        Message::NodeToggled(node.id)
    } else {
        Message::NodeSelected(Selection::Playlist(node.id))
    };

    let row_btn = button(
        container(label_row)
            .width(Fill)
            .height(t::TREE_ROW_H)
            .align_y(Alignment::Center)
    )
        .width(Fill)
        .height(t::TREE_ROW_H)
        .padding(0)
        .style(move |_, status| {
            let bg = if is_selected {
                t::BG_ACTIVE
            } else if matches!(status, button::Status::Hovered) {
                t::BG_HOVER
            } else {
                Color::TRANSPARENT
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border::default(),
                text_color: Color::WHITE,
                ..Default::default()
            }
        })
        .on_press(msg);

    if node.is_folder && is_expanded && !node.children.is_empty() {
        let children: Vec<Element<Message>> = node.children.iter()
            .map(|child| render_tree_node(child, depth + 1, expanded, selection))
            .collect();
        column![row_btn, Column::with_children(children)].into()
    } else {
        row_btn.into()
    }
}

fn tree_row_btn(label: &str, _depth: usize, selected: bool, msg: Message) -> Element<Message> {
    let label = label.to_string();
    button(
        container(
            row![
                container(text("")).width(12.0),
                container(
                    text(label).size(14).color(if selected { Color::WHITE } else { t::TEXT_PRIMARY })
                ).width(Fill).clip(true),
            ]
            .align_y(Alignment::Center)
            .padding([0, 8])
        )
        .width(Fill)
        .height(t::TREE_ROW_H)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .height(t::TREE_ROW_H)
    .padding(0)
    .style(move |_, status| {
        let bg = if selected {
            t::BG_ACTIVE
        } else if matches!(status, button::Status::Hovered) {
            t::BG_HOVER
        } else {
            Color::TRANSPARENT
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border::default(),
            text_color: Color::WHITE,
            ..Default::default()
        }
    })
    .on_press(msg)
    .into()
}

// ── Main content (track list) ─────────────────────────────────────────────────

fn view_main(state: &BrowserState) -> Element<Message> {
    // Search bar
    let search = container(
        row![
            text("⌕ ").size(14).color(t::TEXT_DIM),
            text_input("Search...", &state.search)
                .on_input(Message::SearchChanged)
                .size(14)
                .style(|_, _| iced::widget::text_input::Style {
                    background: Background::Color(Color::TRANSPARENT),
                    border: Border::default(),
                    icon: Color::TRANSPARENT,
                    placeholder: t::TEXT_DIM,
                    value: t::TEXT_PRIMARY,
                    selection: t::ACCENT_BLUE,
                }),
        ]
        .align_y(Alignment::Center)
        .spacing(4),
    )
    .padding([6, 12])
    .width(Fill)
    .style(|_| iced::widget::container::Style {
        background: Some(Background::Color(t::BG_PANEL)),
        ..Default::default()
    });

    // Column headers
    let headers = container(
        row![
            text("TITLE").size(11).color(t::TEXT_DIM).width(Fill),
            text("ARTIST").size(11).color(t::TEXT_DIM).width(160),
            text("BPM").size(11).color(t::TEXT_DIM).width(48),
            text("KEY").size(11).color(t::TEXT_DIM).width(40),
            text("TIME").size(11).color(t::TEXT_DIM).width(46),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .padding([0, 8])
    .height(t::HEADER_H)
    .width(Fill)
    .style(|_| iced::widget::container::Style {
        background: Some(Background::Color(t::BG_PANEL)),
        ..Default::default()
    });

    // Track rows
    let rows: Vec<Element<Message>> = state.tracks.iter().enumerate().map(|(i, track)| {
        let bg = if i % 2 == 0 { t::BG_BASE } else { t::BG_ROW };
        let is_selected = state.selected_track_id == Some(track.id);
        let bpm_str = track.bpm_display()
            .map(|b| format!("{:.1}", b))
            .unwrap_or_default();
        let key_str = track.key.as_deref().unwrap_or("").to_string();
        let dur_str = track.duration_secs
            .map(|s| format!("{:02}:{:02}", s / 60, s % 60))
            .unwrap_or_default();
        let artist_str = track.artist.clone().unwrap_or_default();

        let row_content = row![
            container(text(track.title.clone()).size(14).color(t::TEXT_PRIMARY)).width(Fill).clip(true),
            container(text(artist_str).size(13).color(t::TEXT_SECONDARY)).width(160).clip(true),
            text(bpm_str).size(13).color(t::TEXT_SECONDARY).width(48),
            text(key_str).size(13).color(t::TEXT_SECONDARY).width(40),
            text(dur_str).size(13).color(t::TEXT_DIM).width(46),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .padding([0, 8]);

        let styled_row = container(row_content)
            .width(Fill)
            .height(t::TRACK_ROW_H)
            .style(move |_| iced::widget::container::Style {
                background: Some(Background::Color(
                    if is_selected { t::BG_ACTIVE } else { bg }
                )),
                ..Default::default()
            });

        mouse_area(styled_row)
            .on_press(Message::TrackSelected(track.id))
            .on_double_click(Message::TrackClicked(track.id))
            .into()
    }).collect();

    let track_list: Element<Message> = if state.tracks.is_empty() {
        container(
            text("Select a playlist to load tracks")
                .size(14)
                .color(t::TEXT_DIM),
        )
        .width(Fill)
        .height(Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
    } else {
        scrollable(Column::with_children(rows).spacing(0))
            .height(Fill)
            .into()
    };

    container(column![search, headers, track_list].height(Fill).width(Fill))
        .height(Fill)
        .width(Fill)
        .clip(true)
        .into()
}
