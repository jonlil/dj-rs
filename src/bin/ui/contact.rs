use iced::widget::{
    button, column, container, row, scrollable, space, text, text_input, text_editor,
    Column,
};
use iced::{Alignment, Background, Border, Color, Element, Fill};
use dj_rs::gig::{Contact, CustomerType, Gig};
use super::theme as t;
use super::Message;

// ── Contact editing state ────────────────────────────────────────────────────

pub struct ContactState {
    pub contact_id: String,
    pub name: String,
    pub customer_type: CustomerType,
    pub notes: text_editor::Content,
    pub dirty: bool,
}

impl ContactState {
    pub fn from_contact(contact: &Contact) -> Self {
        Self {
            contact_id: contact.id.clone(),
            name: contact.name.clone(),
            customer_type: contact.customer_type.clone(),
            notes: text_editor::Content::with_text(&contact.notes),
            dirty: false,
        }
    }

    pub fn notes_text(&self) -> String {
        self.notes.text()
    }
}

// ── View ─────────────────────────────────────────────────────────────────────

pub fn view<'a>(state: &'a ContactState, gigs: &[Gig]) -> Element<'a, Message> {
    // ── Header bar ───────────────────────────────────────────────────────────
    let title = text(if state.name.is_empty() { "New Contact" } else { &state.name })
        .size(18)
        .color(t::TEXT_PRIMARY);

    let type_badge = container(
        text(state.customer_type.label()).size(11).color(t::TEXT_DIM)
    )
    .padding([2, 6])
    .style(|_| iced::widget::container::Style {
        background: Some(Background::Color(t::BG_HOVER)),
        border: Border { radius: 3.0.into(), ..Default::default() },
        ..Default::default()
    });

    let saved_label = if state.dirty {
        text("● unsaved").size(11).color(Color::from_rgb(0.9, 0.7, 0.2))
    } else {
        text("✓ saved").size(11).color(t::ACCENT_GREEN)
    };

    let save_btn = flat_btn("Save", t::ACCENT_BLUE, Message::ContactSave);
    let delete_btn = flat_btn("Delete", Color::from_rgb(0.9, 0.3, 0.3), Message::ContactDelete);
    let add_gig_btn = flat_btn("+ New Gig", t::ACCENT_BLUE, Message::ContactAddGig);

    let header = container(
        row![
            title,
            type_badge,
            space::horizontal(),
            saved_label,
            save_btn,
            delete_btn,
            add_gig_btn,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([8, 12])
    .width(Fill);

    let sep = separator();

    // ── Form fields ──────────────────────────────────────────────────────────
    let name_field = field_row(
        "Name",
        text_input("Contact name", &state.name)
            .on_input(Message::ContactNameChanged)
            .size(14)
            .style(|_, _| iced::widget::text_input::Style {
                background: Background::Color(t::BG_ROW),
                border: Border { color: t::ACCENT_BORDER, width: 1.0, radius: 3.0.into() },
                icon: Color::TRANSPARENT,
                placeholder: t::TEXT_DIM,
                value: t::TEXT_PRIMARY,
                selection: t::ACCENT_BLUE,
            })
            .into(),
    );

    let type_field = field_row(
        "Type",
        type_selector(&state.customer_type),
    );

    let notes_label = text("Music preferences / notes")
        .size(12)
        .color(t::TEXT_SECONDARY);

    let notes_editor = container(
        text_editor(&state.notes)
            .placeholder("Notes about this contact's music preferences...")
            .on_action(Message::ContactNotesAction)
            .height(120)
            .style(|_, _| iced::widget::text_editor::Style {
                background: Background::Color(t::BG_ROW),
                border: Border { color: t::ACCENT_BORDER, width: 1.0, radius: 3.0.into() },
                placeholder: t::TEXT_DIM,
                value: t::TEXT_PRIMARY,
                selection: t::ACCENT_BLUE,
            }),
    )
    .width(Fill);

    // ── Gig list ─────────────────────────────────────────────────────────────
    let gigs_header = row![
        text("Gigs").size(14).color(t::TEXT_PRIMARY),
        space::horizontal(),
        text(format!("{}", gigs.len())).size(12).color(t::TEXT_DIM),
    ]
    .align_y(Alignment::Center);

    let gig_rows: Vec<Element<Message>> = gigs.iter().map(|gig| {
        let label = gig.format_label();
        let date = gig.date.as_deref().unwrap_or("");
        let location = gig.location.as_deref().unwrap_or("");

        let detail = if !location.is_empty() && !date.is_empty() {
            format!("{}  ·  {}", date, location)
        } else if !date.is_empty() {
            date.to_string()
        } else {
            String::new()
        };

        button(
            column![
                text(label).size(14).color(t::TEXT_PRIMARY),
                text(detail).size(12).color(t::TEXT_DIM),
            ]
            .spacing(2)
            .padding([6, 8]),
        )
        .width(Fill)
        .padding(0)
        .style(|_, status| button::Style {
            background: Some(Background::Color(
                if matches!(status, button::Status::Hovered) { t::BG_HOVER } else { t::BG_ROW }
            )),
            border: Border { color: t::SEPARATOR, width: 0.0, radius: 4.0.into() },
            text_color: Color::WHITE,
            ..Default::default()
        })
        .on_press(Message::GigClicked(gig.id.clone()))
        .into()
    }).collect();

    let gig_list: Element<Message> = if gig_rows.is_empty() {
        container(
            text("No gigs yet — click \"+ New Gig\" to create one")
                .size(13)
                .color(t::TEXT_DIM),
        )
        .padding([12, 0])
        .into()
    } else {
        Column::with_children(gig_rows).spacing(2).into()
    };

    // ── Assemble ─────────────────────────────────────────────────────────────
    let form = column![
        name_field,
        type_field,
        notes_label,
        notes_editor,
        separator(),
        gigs_header,
        gig_list,
    ]
    .spacing(10)
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

// ── Type selector (3 toggle buttons) ─────────────────────────────────────────

fn type_selector(active: &CustomerType) -> Element<'static, Message> {
    let types = [
        (CustomerType::Private,   "Private"),
        (CustomerType::Corporate, "Corporate"),
        (CustomerType::Venue,     "Venue"),
    ];

    let buttons: Vec<Element<Message>> = types.iter().map(|(ct, label)| {
        let is_active = ct == active;
        let ct = ct.clone();
        button(
            text(*label).size(13).color(if is_active { Color::WHITE } else { t::TEXT_SECONDARY })
        )
        .padding([4, 12])
        .style(move |_, status| button::Style {
            background: Some(Background::Color(
                if is_active { t::BG_ACTIVE }
                else if matches!(status, button::Status::Hovered) { t::BG_HOVER }
                else { t::BG_ROW }
            )),
            border: Border {
                color: if is_active { t::ACCENT_BLUE } else { t::ACCENT_BORDER },
                width: 1.0,
                radius: 3.0.into(),
            },
            text_color: Color::WHITE,
            ..Default::default()
        })
        .on_press(Message::ContactTypeChanged(ct))
        .into()
    }).collect();

    row![].push(iced::widget::Row::with_children(buttons).spacing(4)).into()
}

// ── Helpers ──────────────────────────────────────────────────────────────────

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
