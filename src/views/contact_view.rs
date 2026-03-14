use gtk::prelude::*;
use super::utils::find_widget;

pub(super) fn build_contact_view() -> gtk::Box {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);

    // Header bar
    let header_bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    header_bar.set_margin_start(6);
    header_bar.set_margin_top(6);
    header_bar.set_margin_bottom(4);

    let back_btn = gtk::Button::with_label("← Library");
    back_btn.set_relief(gtk::ReliefStyle::None);
    back_btn.set_widget_name("contact_back_btn");

    let header = gtk::Label::new(Some("Contact"));
    header.set_widget_name("contact_header");
    header.set_xalign(0.0);
    header.set_use_markup(true);
    header.set_hexpand(true);

    let add_gig_btn = gtk::Button::with_label("+ New Gig");
    add_gig_btn.set_widget_name("contact_add_gig_btn");
    add_gig_btn.set_relief(gtk::ReliefStyle::None);

    let delete_btn = gtk::Button::with_label("Delete");
    delete_btn.set_widget_name("contact_delete_btn");
    delete_btn.set_relief(gtk::ReliefStyle::None);

    let saved_lbl = gtk::Label::new(None);
    saved_lbl.set_widget_name("contact_saved_lbl");
    saved_lbl.set_margin_end(4);

    header_bar.pack_start(&back_btn,    false, false, 0);
    header_bar.pack_start(&header,      true,  true,  4);
    header_bar.pack_end  (&add_gig_btn, false, false, 0);
    header_bar.pack_end  (&delete_btn,  false, false, 0);
    header_bar.pack_end  (&saved_lbl,   false, false, 0);

    outer.pack_start(&header_bar, false, false, 0);
    outer.pack_start(&gtk::Separator::new(gtk::Orientation::Horizontal), false, false, 0);

    let scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_border_width(12);

    // ── Fields ────────────────────────────────────────────────────────────────
    let grid = gtk::Grid::new();
    grid.set_row_spacing(6);
    grid.set_column_spacing(8);

    macro_rules! field_lbl { ($t:expr) => {{
        let l = gtk::Label::new(Some($t));
        l.set_xalign(1.0);
        l
    }}; }

    let name_entry = gtk::Entry::new();
    name_entry.set_widget_name("contact_name");
    name_entry.set_placeholder_text(Some("Contact name"));
    name_entry.set_hexpand(true);

    let type_combo = gtk::ComboBoxText::new();
    type_combo.set_widget_name("contact_type");
    type_combo.append(Some("private"),   "Private");
    type_combo.append(Some("venue"),     "Venue");
    type_combo.append(Some("corporate"), "Corporate");
    type_combo.set_active_id(Some("private"));

    grid.attach(&field_lbl!("Name"),         0, 0, 1, 1);
    grid.attach(&name_entry,                 1, 0, 1, 1);
    grid.attach(&field_lbl!("Type"),         0, 1, 1, 1);
    grid.attach(&type_combo,                 1, 1, 1, 1);

    content.pack_start(&grid, false, false, 0);

    // ── Notes ─────────────────────────────────────────────────────────────────
    let notes_lbl = gtk::Label::new(Some("Music preferences / notes"));
    notes_lbl.set_xalign(0.0);

    let notes_view = gtk::TextView::new();
    notes_view.set_widget_name("contact_notes");
    notes_view.set_wrap_mode(gtk::WrapMode::Word);
    notes_view.set_accepts_tab(false);

    let notes_scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
    notes_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    notes_scroll.set_min_content_height(100);
    notes_scroll.add(&notes_view);

    content.pack_start(&notes_lbl,    false, false, 0);
    content.pack_start(&notes_scroll, false, false, 0);

    // ── Gig list ──────────────────────────────────────────────────────────────
    let gigs_lbl = gtk::Label::new(Some("Gigs"));
    gigs_lbl.set_xalign(0.0);
    gigs_lbl.set_margin_top(4);

    let gig_list = gtk::ListBox::new();
    gig_list.set_widget_name("contact_gig_list");
    gig_list.set_selection_mode(gtk::SelectionMode::Single);

    content.pack_start(&gigs_lbl, false, false, 0);
    content.pack_start(&gig_list, false, false, 0);

    scroll.add(&content);
    outer.pack_start(&scroll, true, true, 0);
    outer
}

pub(super) fn load_contact_into_view(
    view:      &gtk::Box,
    contact:   &crate::gig::Contact,
    gigs:      &[&crate::gig::Gig],
    playlists: &[crate::rekordbox::Playlist],
) {
    // Header
    if let Some(w) = find_widget(view, "contact_header") {
        if let Ok(lbl) = w.downcast::<gtk::Label>() {
            lbl.set_markup(&format!(
                "<b>{}</b>  <small>{}</small>",
                glib::markup_escape_text(&contact.name),
                contact.customer_type.label(),
            ));
        }
    }

    // Store the contact ID on the view widget for auto-save
    view.set_widget_name(&format!("contact_view:{}", contact.id));

    if let Some(w) = find_widget(view, "contact_name") {
        if let Ok(e) = w.downcast::<gtk::Entry>() {
            e.set_text(&contact.name);
        }
    }

    if let Some(w) = find_widget(view, "contact_type") {
        if let Ok(combo) = w.downcast::<gtk::ComboBoxText>() {
            let id = match contact.customer_type {
                crate::gig::CustomerType::Corporate => "corporate",
                crate::gig::CustomerType::Venue     => "venue",
                crate::gig::CustomerType::Private   => "private",
            };
            combo.set_active_id(Some(id));
        }
    }

    if let Some(w) = find_widget(view, "contact_notes") {
        if let Ok(tv) = w.downcast::<gtk::TextView>() {
            if let Some(buf) = tv.get_buffer() {
                buf.set_text(&contact.notes);
            }
        }
    }

    // Build a set of pool playlist IDs (attribute=0, direct child of contact folder)
    let pool_ids: std::collections::HashSet<i64> = {
        let contact_folder_id = contact.rekordbox_folder_id;
        playlists.iter()
            .filter(|pl| pl.attribute == 0 && contact_folder_id.map_or(false, |cid| pl.parent_id == Some(cid)))
            .map(|pl| pl.id)
            .collect()
    };

    // Populate gig list (exclude pool playlists)
    if let Some(w) = find_widget(view, "contact_gig_list") {
        if let Ok(lb) = w.downcast::<gtk::ListBox>() {
            for child in lb.get_children() { lb.remove(&child); }
            for gig in gigs.iter().filter(|g| g.rekordbox_folder_id.map_or(true, |rid| !pool_ids.contains(&rid))) {
                let row = gtk::ListBoxRow::new();
                row.set_widget_name(&format!("gig:{}", gig.id));
                let label_text = if gig.name.is_empty() {
                    gig.date.as_deref().unwrap_or("Unnamed gig").to_string()
                } else if let Some(date) = &gig.date {
                    format!("{} ({})", gig.name, date)
                } else {
                    gig.name.clone()
                };
                let lbl = gtk::Label::new(Some(&label_text));
                lbl.set_xalign(0.0);
                lbl.set_margin_start(8);
                lbl.set_margin_top(4);
                lbl.set_margin_bottom(4);
                row.add(&lbl);
                lb.add(&row);
            }
            lb.show_all();
        }
    }
}
