use iced::widget::{button, canvas, column, container, row, scrollable, space, text, Column};
use iced::widget::canvas::{Frame, Geometry, Path, Stroke, path};
use iced::{Alignment, Background, Border, Color, Element, Fill, Font, Point, Rectangle, Renderer, Size, Theme};
use iced::mouse;
use dj_rs::rekordbox::CuePoint;
use super::theme as t;
use super::Message;

// ── Constants ─────────────────────────────────────────────────────────────────

pub const PLAYER_H: f32 = INFO_ROW_H + OVERVIEW_H + ZOOMED_H + 12.0;
const INFO_ROW_H: f32   = 36.0;
const OVERVIEW_H: f32   = 28.0;
const ZOOMED_H: f32     = 100.0; // includes 18px cue strip at top
const CUE_STRIP_H: f32  = 18.0;
const CONTROLS_W: f32   = 64.0;
const CUE_LIST_W: f32   = 180.0;
const TRANSPORT_BTN: f32 = 42.0;

const ZOOM_WINDOW: f64   = 6.0;
const PLAYHEAD_FRAC: f64 = 0.25;

const HOT_CUE_COLORS: [(f32, f32, f32); 8] = [
    (0.90, 0.20, 0.20), // A red
    (0.90, 0.50, 0.10), // B orange
    (0.85, 0.80, 0.10), // C yellow
    (0.20, 0.72, 0.20), // D green
    (0.10, 0.80, 0.80), // E cyan
    (0.20, 0.40, 0.90), // F blue
    (0.62, 0.18, 0.85), // G purple
    (0.75, 0.75, 0.75), // H grey
];

fn cue_color(kind: i32) -> Color {
    if kind == 0 {
        return t::ACCENT_BLUE; // memory cue
    }
    let (r, g, b) = HOT_CUE_COLORS[(kind as usize - 1).min(7)];
    Color::from_rgb(r, g, b)
}

fn cue_slot_label(kind: i32) -> char {
    (b'A' + ((kind as u8).saturating_sub(1)).min(7)) as char
}

// ── Player state ──────────────────────────────────────────────────────────────

pub struct PlayerState {
    pub track_id: Option<i64>,
    pub title: String,
    pub artist: String,
    pub is_playing: bool,
    pub cue_pos_secs: f64,
    pub play_pos_secs: f64,
    pub duration_secs: Option<i32>,
    pub bpm: Option<f32>,
    pub key: Option<String>,
    pub color_waveform: Option<Vec<u8>>,
    pub overview_waveform: Option<Vec<u8>>,
    pub cue_points: Vec<CuePoint>,
}

impl PlayerState {
    pub fn new() -> Self {
        Self {
            track_id: None,
            title: String::new(),
            artist: String::new(),
            is_playing: false,
            cue_pos_secs: 0.0,
            play_pos_secs: 0.0,
            duration_secs: None,
            bpm: None,
            key: None,
            color_waveform: None,
            overview_waveform: None,
            cue_points: Vec::new(),
        }
    }

    pub fn load_track(&mut self, id: i64, title: String, artist: String,
        duration_secs: Option<i32>, bpm: Option<f32>, key: Option<String>) {
        self.track_id = Some(id);
        self.title = title;
        self.artist = artist;
        self.duration_secs = duration_secs;
        self.bpm = bpm;
        self.key = key;
        self.is_playing = false;
        self.play_pos_secs = 0.0;
        self.cue_pos_secs = 0.0;
        self.color_waveform = None;
        self.overview_waveform = None;
        self.cue_points = Vec::new();
    }
}

// ── View ──────────────────────────────────────────────────────────────────────

pub fn view(state: &PlayerState) -> Element<Message> {
    let loaded = state.track_id.is_some();
    let dur = state.duration_secs.unwrap_or(0) as f64;
    let pos = state.play_pos_secs;

    // ── Left: transport buttons ───────────────────────────────────────────────
    // Pioneer-style: CUE = yellow/amber, PLAY = vivid green
    const CUE_COLOR:  Color = Color { r: 0.88, g: 0.68, b: 0.0,  a: 1.0 }; // #E0AD00
    const PLAY_COLOR: Color = Color { r: 0.10, g: 0.80, b: 0.22, a: 1.0 }; // #1ACD38

    let cue_btn  = transport_btn("CUE", CUE_COLOR,  loaded, Message::CuePressed);
    let play_btn = transport_btn(
        if state.is_playing { "■" } else { "▶" },
        PLAY_COLOR, loaded, Message::PlayPressed,
    );
    let controls = container(
        column![cue_btn, play_btn].spacing(10).align_x(Alignment::Center)
    )
    .width(CONTROLS_W)
    .height(Fill)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center);

    // ── Center: info row + waveforms ──────────────────────────────────────────
    let remaining = (dur - pos).max(0.0);
    let rem_str = format!("-{:02}:{:04.1}", remaining as u32 / 60, remaining % 60.0);
    let dur_str = state.duration_secs
        .map(|s| format!("{:02}:{:02}", s / 60, s % 60))
        .unwrap_or_else(|| "--:--".to_string());
    let bpm_str = state.bpm
        .map(|b| format!("{:.2}", b))
        .unwrap_or_else(|| "---.--".to_string());
    let key_str = state.key.clone().unwrap_or_else(|| "---".to_string());

    let info_row = container(row![
        container(
            column![
                container(
                    text(if loaded { state.title.clone() } else { "No track loaded".to_string() })
                        .size(14).color(if loaded { t::TEXT_PRIMARY } else { t::TEXT_DIM })
                ).width(Fill).clip(true),
                container(
                    text(if loaded { state.artist.clone() } else { String::new() })
                        .size(13).color(t::TEXT_SECONDARY)
                ).width(Fill).clip(true),
            ].spacing(2)
        ).width(Fill),
        space::horizontal(),
        row![
            text(rem_str).size(14).color(t::TEXT_PRIMARY).font(Font::MONOSPACE),
            text(dur_str).size(12).color(t::TEXT_DIM).font(Font::MONOSPACE),
            container(column![]).width(1).height(14)
                .style(|_| iced::widget::container::Style {
                    background: Some(Background::Color(t::SEPARATOR)),
                    ..Default::default()
                }),
            text(key_str).size(13).color(t::TEXT_SECONDARY),
            container(column![]).width(1).height(14)
                .style(|_| iced::widget::container::Style {
                    background: Some(Background::Color(t::SEPARATOR)),
                    ..Default::default()
                }),
            text(bpm_str).size(14).color(t::TEXT_PRIMARY).font(Font::MONOSPACE),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .align_y(Alignment::Center)
    .spacing(8))
    .height(INFO_ROW_H)
    .align_y(Alignment::Center)
    .padding([0, 8]);

    let wf_dur = dur.max(1.0);

    let overview = canvas(OverviewCanvas {
        color: state.color_waveform.clone(),
        overview: state.overview_waveform.clone(),
        cue_points: state.cue_points.clone(),
        duration: wf_dur,
        pos,
    })
    .width(Fill)
    .height(OVERVIEW_H);

    let zoomed = canvas(ZoomedCanvas {
        color: state.color_waveform.clone(),
        cue_points: state.cue_points.clone(),
        duration: wf_dur,
        pos,
    })
    .width(Fill)
    .height(ZOOMED_H);

    let center = column![info_row, overview, zoomed].spacing(2).width(Fill);

    // ── Right: cue points list ────────────────────────────────────────────────
    let hot_cues: Vec<Element<Message>> = state.cue_points.iter()
        .filter(|c| c.kind > 0)
        .map(|c| {
            let color = cue_color(c.kind);
            let label = cue_slot_label(c.kind).to_string();
            let s = c.in_secs as u32;
            let time = format!("{:02}:{:02}", s / 60, s % 60);
            let comment = c.comment.clone();
            row![
                container(text(label).size(10).color(Color::BLACK))
                    .width(16).height(16)
                    .align_x(Alignment::Center)
                    .align_y(Alignment::Center)
                    .style(move |_| iced::widget::container::Style {
                        background: Some(Background::Color(color)),
                        border: Border { radius: 2.0.into(), ..Default::default() },
                        ..Default::default()
                    }),
                text(time).size(11).color(t::TEXT_DIM).font(Font::MONOSPACE),
                container(text(comment).size(11).color(t::TEXT_PRIMARY))
                    .width(Fill).clip(true),
            ]
            .spacing(4).align_y(Alignment::Center)
            .into()
        })
        .collect();

    let memory_cues: Vec<Element<Message>> = state.cue_points.iter()
        .filter(|c| c.kind == 0)
        .map(|c| {
            let s = c.in_secs as u32;
            let time = format!("{:02}:{:02}", s / 60, s % 60);
            let comment = c.comment.clone();
            row![
                text("▶").size(10).color(t::ACCENT_BLUE),
                text(time).size(11).color(t::TEXT_DIM).font(Font::MONOSPACE),
                container(text(comment).size(11).color(t::TEXT_PRIMARY))
                    .width(Fill).clip(true),
            ]
            .spacing(4).align_y(Alignment::Center)
            .into()
        })
        .collect();

    let mut all_rows: Vec<Element<Message>> = hot_cues;
    all_rows.extend(memory_cues);

    let cue_list_content: Element<Message> = if all_rows.is_empty() {
        container(text(if loaded { "No cues" } else { "" }).size(11).color(t::TEXT_DIM))
            .width(Fill).height(Fill)
            .align_x(Alignment::Center).align_y(Alignment::Center)
            .into()
    } else {
        scrollable(Column::with_children(all_rows).spacing(4).padding([4, 6]))
            .height(Fill)
            .into()
    };

    let cue_panel = container(cue_list_content)
        .width(CUE_LIST_W)
        .height(Fill)
        .style(|_| iced::widget::container::Style {
            background: Some(Background::Color(t::BG_BASE)),
            border: Border { color: t::SEPARATOR, width: 1.0, radius: 0.0.into() },
            ..Default::default()
        });

    // ── Assemble ──────────────────────────────────────────────────────────────
    container(
        row![controls, center, cue_panel].height(Fill).spacing(0)
    )
    .width(Fill)
    .height(PLAYER_H)
    .padding([4, 0])
    .style(|_| iced::widget::container::Style {
        background: Some(Background::Color(t::BG_PANEL)),
        border: Border { color: t::SEPARATOR, width: 0.0, radius: 0.0.into() },
        ..Default::default()
    })
    .into()
}

fn transport_btn(label: &str, accent: Color, enabled: bool, msg: Message) -> Element<'static, Message> {
    let label = label.to_string();
    let btn = button(
        container(text(label).size(14).color(Color::WHITE))
            .width(TRANSPORT_BTN).height(TRANSPORT_BTN)
            .align_x(Alignment::Center).align_y(Alignment::Center),
    )
    .padding(0)
    .style(move |_, status| button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered if enabled => Color { a: 0.85, ..accent },
            button::Status::Pressed if enabled => Color { a: 0.70, ..accent },
            _ if enabled => accent,
            _ => t::BG_HOVER,
        })),
        border: Border { radius: (TRANSPORT_BTN / 2.0).into(), ..Default::default() },
        text_color: Color::WHITE,
        ..Default::default()
    });
    if enabled { btn.on_press(msg).into() } else { btn.into() }
}

// ── Overview canvas ───────────────────────────────────────────────────────────

struct OverviewCanvas {
    color: Option<Vec<u8>>,
    overview: Option<Vec<u8>>,
    cue_points: Vec<CuePoint>,
    duration: f64,
    pos: f64,
}

impl canvas::Program<Message, Theme, Renderer> for OverviewCanvas {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme,
        bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry<Renderer>>
    {
        let w = bounds.width;
        let h = bounds.height;
        let mut frame = Frame::new(renderer, bounds.size());
        let half_h = h / 2.0;

        frame.fill_rectangle(Point::ORIGIN, bounds.size(),
            Color { r: 0.07, g: 0.07, b: 0.07, a: 1.0 });

        if let Some(ref data) = self.color {
            let n = data.len() / 3;
            if n > 0 {
                let col_w = (w / n as f32).max(1.0);
                for i in 0..n {
                    let x    = i as f32 / n as f32 * w;
                    let b0   = data[i * 3];
                    let b1   = data[i * 3 + 1];
                    let b2   = data[i * 3 + 2];
                    let bass = (b0 & 0x1F) as f32 / 31.0;
                    let mid  = (b1 & 0x1F) as f32 / 31.0;
                    let high = (b2 & 0x1F) as f32 / 31.0;
                    let bw   = (b0 >> 5) as f32 / 7.0;
                    let mw   = (b1 >> 5) as f32 / 7.0;
                    let hw   = (b2 >> 5) as f32 / 7.0;
                    let energy = bass.max(mid).max(high);
                    let bar_half = energy * half_h;
                    if bar_half < 0.5 { continue; }
                    let total = (bass + mid + high).max(0.001);
                    let bf = bass / total;
                    let hf = high / total;
                    let w_avg = (bw + mw + hw) / 3.0;
                    let r = (bf * 0.05 + hf * 1.0  + w_avg * 0.4).min(1.0);
                    let g = (bf * 0.40 + hf * 0.60 + w_avg * 0.3).min(1.0);
                    let b = (bf * 1.0  + hf * 0.05 + w_avg * 0.2).min(1.0);
                    frame.fill_rectangle(
                        Point::new(x, half_h - bar_half),
                        Size::new(col_w, bar_half * 2.0),
                        Color::from_rgb(r, g, b),
                    );
                }
            }
        } else if let Some(ref data) = self.overview {
            let n = data.len() as f32;
            if n > 0.0 {
                let col_w = (w / n).max(1.0);
                for (i, &byte) in data.iter().enumerate() {
                    let x        = i as f32 / n * w;
                    let bar_half = (byte & 0x1F) as f32 / 31.0 * half_h;
                    let white    = ((byte >> 5) & 0x07) as f32 / 7.0;
                    let v        = 0.35 + white * 0.45;
                    frame.fill_rectangle(
                        Point::new(x, half_h - bar_half),
                        Size::new(col_w, bar_half * 2.0),
                        Color::from_rgb(v, v, v),
                    );
                }
            }
        } else {
            // placeholder
            frame.stroke(
                &Path::line(Point::new(0.0, half_h), Point::new(w, half_h)),
                Stroke::default().with_color(Color { r: 0.25, g: 0.25, b: 0.25, a: 1.0 }).with_width(1.0),
            );
        }

        // Cue markers
        for c in &self.cue_points {
            if c.kind == 0 { continue; }
            let x = (c.in_secs / self.duration) as f32 * w;
            frame.fill_rectangle(
                Point::new(x - 0.5, 0.0), Size::new(1.5, h), cue_color(c.kind),
            );
        }

        // Playhead
        let px = (self.pos / self.duration) as f32 * w;
        frame.fill_rectangle(Point::new(px - 1.0, 0.0), Size::new(2.0, h), Color::WHITE);

        // Played region dim
        if px > 0.0 {
            frame.fill_rectangle(
                Point::ORIGIN, Size::new(px, h),
                Color { r: 0.0, g: 0.0, b: 0.0, a: 0.35 },
            );
        }

        vec![frame.into_geometry()]
    }
}

// ── Zoomed canvas ─────────────────────────────────────────────────────────────

struct ZoomedCanvas {
    color: Option<Vec<u8>>,
    cue_points: Vec<CuePoint>,
    duration: f64,
    pos: f64,
}

impl canvas::Program<Message, Theme, Renderer> for ZoomedCanvas {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme,
        bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry<Renderer>>
    {
        let w = bounds.width;
        let h = bounds.height;
        let mut frame = Frame::new(renderer, bounds.size());

        let wf_h    = h - CUE_STRIP_H;
        let center  = CUE_STRIP_H + wf_h / 2.0;
        let ph_x    = w * PLAYHEAD_FRAC as f32;
        let px_per_s = w / ZOOM_WINDOW as f32;

        frame.fill_rectangle(Point::ORIGIN, bounds.size(),
            Color { r: 0.06, g: 0.06, b: 0.06, a: 1.0 });

        frame.stroke(
            &Path::line(Point::new(0.0, CUE_STRIP_H), Point::new(w, CUE_STRIP_H)),
            Stroke::default().with_color(Color { r: 0.18, g: 0.18, b: 0.18, a: 1.0 }).with_width(1.0),
        );

        if let Some(ref data) = self.color {
            let n_cols = data.len() / 3;
            if n_cols > 0 && self.duration > 0.0 {
                let sps    = n_cols as f64 / self.duration;
                let col_w  = (px_per_s as f64 / sps).max(1.0) as f32;
                let t0     = self.pos - PLAYHEAD_FRAC * ZOOM_WINDOW;
                let t1     = self.pos + (1.0 - PLAYHEAD_FRAC) * ZOOM_WINDOW;
                let c_from = ((t0 * sps).floor() as i64).max(0) as usize;
                let c_to   = (((t1 * sps).ceil() as usize) + 1).min(n_cols);

                for col in c_from..c_to {
                    let t = col as f64 / sps;
                    let x = ph_x + ((t - self.pos) * px_per_s as f64) as f32;
                    if x + col_w < 0.0 || x > w { continue; }

                    let i    = col * 3;
                    let b0   = data[i];
                    let b1   = data[i + 1];
                    let b2   = data[i + 2];
                    let bass = (b0 & 0x1F) as f32 / 31.0;
                    let mid  = (b1 & 0x1F) as f32 / 31.0;
                    let high = (b2 & 0x1F) as f32 / 31.0;
                    let bw   = (b0 >> 5) as f32 / 7.0;
                    let mw   = (b1 >> 5) as f32 / 7.0;
                    let hw   = (b2 >> 5) as f32 / 7.0;
                    let energy   = bass.max(mid).max(high);
                    let bar_half = energy * wf_h / 2.0;
                    if bar_half < 0.5 { continue; }
                    let total = (bass + mid + high).max(0.001);
                    let bf = bass / total;
                    let hf = high / total;
                    let w_avg = (bw + mw + hw) / 3.0;
                    let r = (bf * 0.05 + hf * 1.0  + w_avg * 0.4).min(1.0);
                    let g = (bf * 0.40 + hf * 0.60 + w_avg * 0.3).min(1.0);
                    let b = (bf * 1.0  + hf * 0.05 + w_avg * 0.2).min(1.0);
                    frame.fill_rectangle(
                        Point::new(x, center - bar_half),
                        Size::new(col_w, bar_half * 2.0),
                        Color::from_rgb(r, g, b),
                    );
                }
            }
        } else {
            frame.stroke(
                &Path::line(Point::new(0.0, center), Point::new(w, center)),
                Stroke::default().with_color(Color { r: 0.25, g: 0.25, b: 0.25, a: 1.0 }).with_width(1.0),
            );
        }

        // Cue markers
        for c in &self.cue_points {
            let color = cue_color(c.kind);
            let x_in = ph_x + ((c.in_secs - self.pos) * px_per_s as f64) as f32;

            // Loop region
            if let Some(out_s) = c.out_secs {
                let x_out = ph_x + ((out_s - self.pos) * px_per_s as f64) as f32;
                let x0 = x_in.max(0.0);
                let x1 = x_out.min(w);
                if x1 > x0 {
                    frame.fill_rectangle(
                        Point::new(x0, CUE_STRIP_H), Size::new(x1 - x0, wf_h),
                        Color { a: 0.18, ..color },
                    );
                    frame.stroke(
                        &Path::line(Point::new(x_out, CUE_STRIP_H), Point::new(x_out, h)),
                        Stroke::default().with_color(Color { a: 0.8, ..color }).with_width(1.5),
                    );
                }
            }

            if x_in < -8.0 || x_in > w + 8.0 { continue; }

            // Vertical line
            frame.stroke(
                &Path::line(Point::new(x_in, CUE_STRIP_H), Point::new(x_in, h)),
                Stroke::default().with_color(color).with_width(1.5),
            );

            // Downward triangle in cue strip
            let mut b = path::Builder::new();
            b.move_to(Point::new(x_in - 7.0, 1.0));
            b.line_to(Point::new(x_in + 7.0, 1.0));
            b.line_to(Point::new(x_in, CUE_STRIP_H - 1.0));
            b.close();
            frame.fill(&b.build(), color);
        }

        // Playhead white line
        frame.stroke(
            &Path::line(Point::new(ph_x, CUE_STRIP_H), Point::new(ph_x, h)),
            Stroke::default().with_color(Color::WHITE).with_width(2.0),
        );

        // Red playhead triangle
        let mut b = path::Builder::new();
        b.move_to(Point::new(ph_x - 7.0, 1.0));
        b.line_to(Point::new(ph_x + 7.0, 1.0));
        b.line_to(Point::new(ph_x, CUE_STRIP_H - 1.0));
        b.close();
        frame.fill(&b.build(), Color::from_rgb(0.9, 0.15, 0.15));

        vec![frame.into_geometry()]
    }
}
