use iced::widget::{
    button, column, container, row, scrollable, space, text, text_input, Column,
};
use iced::{Alignment, Background, Border, Color, Element, Fill};
use dj_rs::config::PathMapping;
use super::theme as t;
use super::Message;

// ── Settings state ──────────────────────────────────────────────────────────

pub struct SettingsState {
    pub db_path: String,
    pub path_mappings: Vec<MappingRow>,
    pub music_library_path: String,
    pub spotify_connected: bool,
    pub spotify_status: String,
    pub dirty: bool,
}

pub struct MappingRow {
    pub from: String,
    pub to: String,
}

impl SettingsState {
    pub fn from_config(config: &dj_rs::config::Config) -> Self {
        let path_mappings = config.path_mappings.iter().map(|m| MappingRow {
            from: m.from.clone(),
            to: m.to.clone(),
        }).collect();

        let spotify_connected = config.spotify_access_token.is_some();

        Self {
            db_path: config.resolved_db_path().unwrap_or_default(),
            path_mappings,
            music_library_path: config.music_library_dir().to_string_lossy().into_owned(),
            spotify_connected,
            spotify_status: if spotify_connected {
                "Connected".to_string()
            } else {
                "Not connected".to_string()
            },
            dirty: false,
        }
    }

    pub fn to_mappings(&self) -> Vec<PathMapping> {
        self.path_mappings.iter()
            .filter(|m| !m.from.is_empty())
            .map(|m| PathMapping {
                from: m.from.clone(),
                to: m.to.clone(),
            })
            .collect()
    }
}

// ── View ─────────────────────────────────────────────────────────────────────

pub fn view(state: &SettingsState) -> Element<Message> {
    // ── Header ────────────────────────────────────────────────────────────────
    let title = text("Settings").size(18).color(t::TEXT_PRIMARY);

    let saved_label = if state.dirty {
        text("● unsaved").size(11).color(Color::from_rgb(0.9, 0.7, 0.2))
    } else {
        text("✓ saved").size(11).color(t::ACCENT_GREEN)
    };

    let save_btn = flat_btn("Save", t::ACCENT_BLUE, Message::SettingsSave);
    let close_btn = flat_btn("✕", t::TEXT_SECONDARY, Message::SettingsClicked);

    let header = container(
        row![title, space::horizontal(), saved_label, save_btn, close_btn]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .padding([8, 12])
    .width(Fill);

    let sep = separator();

    // ── Database Path ──────────────────────────────────────────────────────────
    let db_header = text("Rekordbox Database").size(14).color(t::TEXT_PRIMARY);
    let db_hint = text(
        "Path to the Rekordbox master.db file (SQLCipher). Default: ~/.local/share/dj-rs/master.db"
    ).size(12).color(t::TEXT_DIM);
    let db_input = container(
        text_input("~/.local/share/dj-rs/master.db", &state.db_path)
            .on_input(Message::SettingsDbPathChanged)
            .size(13)
            .style(|_, _| iced::widget::text_input::Style {
                background: Background::Color(t::BG_ROW),
                border: Border { color: t::ACCENT_BORDER, width: 1.0, radius: 3.0.into() },
                icon: Color::TRANSPARENT,
                placeholder: t::TEXT_DIM,
                value: t::TEXT_PRIMARY,
                selection: t::ACCENT_BLUE,
            }),
    ).width(Fill);

    let db_section = column![
        db_header,
        db_hint,
        db_input,
    ]
    .spacing(8);

    // ── Path Mappings ─────────────────────────────────────────────────────────
    let mappings_header = text("Path Mappings").size(14).color(t::TEXT_PRIMARY);
    let mappings_hint = text(
        "Rewrite path prefixes stored in the database to match your local file system."
    ).size(12).color(t::TEXT_DIM);

    let mapping_rows: Vec<Element<Message>> = state.path_mappings.iter().enumerate().map(|(i, m)| {
        let idx = i;
        row![
            container(
                text_input("From prefix", &m.from)
                    .on_input(move |s| Message::SettingsMappingFromChanged(idx, s))
                    .size(13)
                    .style(|_, _| iced::widget::text_input::Style {
                        background: Background::Color(t::BG_ROW),
                        border: Border { color: t::ACCENT_BORDER, width: 1.0, radius: 3.0.into() },
                        icon: Color::TRANSPARENT,
                        placeholder: t::TEXT_DIM,
                        value: t::TEXT_PRIMARY,
                        selection: t::ACCENT_BLUE,
                    }),
            ).width(Fill),
            text("→").size(14).color(t::TEXT_DIM),
            container(
                text_input("To prefix", &m.to)
                    .on_input(move |s| Message::SettingsMappingToChanged(idx, s))
                    .size(13)
                    .style(|_, _| iced::widget::text_input::Style {
                        background: Background::Color(t::BG_ROW),
                        border: Border { color: t::ACCENT_BORDER, width: 1.0, radius: 3.0.into() },
                        icon: Color::TRANSPARENT,
                        placeholder: t::TEXT_DIM,
                        value: t::TEXT_PRIMARY,
                        selection: t::ACCENT_BLUE,
                    }),
            ).width(Fill),
            flat_btn("✕", Color::from_rgb(0.9, 0.3, 0.3), Message::SettingsMappingRemove(idx)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    }).collect();

    let add_mapping_btn = flat_btn("+ Add mapping", t::ACCENT_BLUE, Message::SettingsMappingAdd);

    let mappings_section = column![
        mappings_header,
        mappings_hint,
        Column::with_children(mapping_rows).spacing(4),
        add_mapping_btn,
    ]
    .spacing(8);

    // ── Music Library Path ──────────────────────────────────────────────────
    let lib_path_header = text("Music Library Path").size(14).color(t::TEXT_PRIMARY);
    let lib_path_hint = text(
        "Where imported files are stored. Defaults to ~/Music/"
    ).size(12).color(t::TEXT_DIM);
    let lib_path_input = container(
        text_input("~/Music", &state.music_library_path)
            .on_input(Message::SettingsMusicPathChanged)
            .size(13)
            .style(|_, _| iced::widget::text_input::Style {
                background: Background::Color(t::BG_ROW),
                border: Border { color: t::ACCENT_BORDER, width: 1.0, radius: 3.0.into() },
                icon: Color::TRANSPARENT,
                placeholder: t::TEXT_DIM,
                value: t::TEXT_PRIMARY,
                selection: t::ACCENT_BLUE,
            }),
    ).width(Fill);

    let lib_path_section = column![
        lib_path_header,
        lib_path_hint,
        lib_path_input,
    ]
    .spacing(8);

    // ── Spotify ───────────────────────────────────────────────────────────────
    let spotify_header = text("Spotify").size(14).color(t::TEXT_PRIMARY);

    let status_color = if state.spotify_connected { t::ACCENT_GREEN } else { t::TEXT_DIM };
    let status_prefix = if state.spotify_connected { "✓ " } else { "" };
    let status_label = text(format!("{}{}", status_prefix, state.spotify_status))
        .size(13).color(status_color);

    let connect_btn = flat_btn("Connect with Spotify", t::ACCENT_BLUE, Message::SpotifyConnect);

    let spotify_section = column![
        spotify_header,
        row![status_label, space::horizontal(), connect_btn]
            .spacing(8)
            .align_y(Alignment::Center),
    ]
    .spacing(8);

    // ── Assemble ──────────────────────────────────────────────────────────────
    let form = column![
        db_section,
        separator(),
        mappings_section,
        separator(),
        lib_path_section,
        separator(),
        spotify_section,
    ]
    .spacing(16)
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

// ── Helpers ──────────────────────────────────────────────────────────────────

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
