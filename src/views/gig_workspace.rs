use gtk::prelude::*;
use super::utils::find_widget;

pub(super) fn build_gig_workspace() -> gtk::Box {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);

    // ── Header bar ────────────────────────────────────────────────────────────
    let header_bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    header_bar.set_margin_start(6);
    header_bar.set_margin_top(6);
    header_bar.set_margin_bottom(4);

    let back_btn = gtk::Button::with_label("← Contact");
    back_btn.set_relief(gtk::ReliefStyle::None);
    back_btn.set_widget_name("gig_back_btn");

    let header = gtk::Label::new(Some("Gig"));
    header.set_widget_name("gig_header");
    header.set_xalign(0.0);
    header.set_use_markup(true);
    header.set_hexpand(true);

    let saved_lbl = gtk::Label::new(None);
    saved_lbl.set_widget_name("gig_saved_lbl");
    saved_lbl.set_margin_end(6);

    header_bar.pack_start(&back_btn,  false, false, 0);
    header_bar.pack_start(&header,    true,  true,  4);
    header_bar.pack_end  (&saved_lbl, false, false, 0);

    outer.pack_start(&header_bar, false, false, 0);
    outer.pack_start(&gtk::Separator::new(gtk::Orientation::Horizontal), false, false, 0);

    // ── Notebook ──────────────────────────────────────────────────────────────
    let notebook = gtk::Notebook::new();
    notebook.set_widget_name("gig_notebook");

    macro_rules! field_lbl { ($t:expr) => {{
        let l = gtk::Label::new(Some($t));
        l.set_xalign(1.0);
        l
    }}; }
    macro_rules! field_entry { ($name:expr, $ph:expr) => {{
        let e = gtk::Entry::new();
        e.set_widget_name($name);
        e.set_placeholder_text(Some($ph));
        e.set_hexpand(true);
        e
    }}; }

    // ── Tab 1: Info ───────────────────────────────────────────────────────────
    {
        let scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

        let grid = gtk::Grid::new();
        grid.set_row_spacing(8);
        grid.set_column_spacing(8);
        grid.set_border_width(12);

        let contact_lbl = gtk::Label::new(None);
        contact_lbl.set_widget_name("gig_contact_label");
        contact_lbl.set_xalign(0.0);
        contact_lbl.set_use_markup(true);

        let time_box    = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let start_entry = gtk::Entry::new();
        start_entry.set_widget_name("gig_start_time");
        start_entry.set_placeholder_text(Some("HH:MM"));
        start_entry.set_width_chars(7);
        start_entry.set_hexpand(false);
        let sep_lbl     = gtk::Label::new(Some("–"));
        let end_entry   = gtk::Entry::new();
        end_entry.set_widget_name("gig_end_time");
        end_entry.set_placeholder_text(Some("HH:MM"));
        end_entry.set_width_chars(7);
        end_entry.set_hexpand(false);
        time_box.pack_start(&start_entry, false, false, 0);
        time_box.pack_start(&sep_lbl,     false, false, 2);
        time_box.pack_start(&end_entry,   false, false, 0);

        grid.attach(&field_lbl!("Contact"),  0, 0, 1, 1);
        grid.attach(&contact_lbl,            1, 0, 1, 1);
        grid.attach(&field_lbl!("Name"),     0, 1, 1, 1);
        grid.attach(&field_entry!("gig_name", "Event name"), 1, 1, 1, 1);
        grid.attach(&field_lbl!("Date"),     0, 2, 1, 1);
        grid.attach(&field_entry!("gig_date", "YYYY-MM-DD"), 1, 2, 1, 1);
        grid.attach(&field_lbl!("Time"),     0, 3, 1, 1);
        grid.attach(&time_box,               1, 3, 1, 1);
        grid.attach(&field_lbl!("Location"), 0, 4, 1, 1);
        grid.attach(&field_entry!("gig_location", "Venue name or address"), 1, 4, 1, 1);

        scroll.add(&grid);
        notebook.append_page(&scroll, Some(&gtk::Label::new(Some("Info"))));
    }

    // ── Tab 2: Brief ──────────────────────────────────────────────────────────
    {
        let scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 8);
        vbox.set_border_width(12);

        let spotify_lbl = gtk::Label::new(Some("Spotify reference playlist (paste URL, then go to Match tab)"));
        spotify_lbl.set_xalign(0.0);
        spotify_lbl.set_use_markup(true);
        let spotify_entry = gtk::Entry::new();
        spotify_entry.set_widget_name("gig_spotify_url");
        spotify_entry.set_placeholder_text(Some("https://open.spotify.com/playlist/…"));
        spotify_entry.set_hexpand(true);

        let notes_lbl = gtk::Label::new(Some("Vibe / music preferences / client notes"));
        notes_lbl.set_xalign(0.0);
        let notes_view = gtk::TextView::new();
        notes_view.set_widget_name("gig_notes");
        notes_view.set_wrap_mode(gtk::WrapMode::Word);
        notes_view.set_accepts_tab(false);
        let notes_scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
        notes_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        notes_scroll.set_min_content_height(120);
        notes_scroll.set_vexpand(true);
        notes_scroll.add(&notes_view);

        vbox.pack_start(&spotify_lbl,   false, false, 0);
        vbox.pack_start(&spotify_entry, false, false, 0);
        vbox.pack_start(&notes_lbl,     false, false, 0);
        vbox.pack_start(&notes_scroll,  true,  true,  0);

        scroll.add(&vbox);
        notebook.append_page(&scroll, Some(&gtk::Label::new(Some("Brief"))));
    }

    // ── Tab 3: Match ──────────────────────────────────────────────────────────
    {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 6);
        vbox.set_border_width(12);

        let top_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let match_status = gtk::Label::new(Some("No match run yet"));
        match_status.set_widget_name("gig_match_status");
        match_status.set_xalign(0.0);
        match_status.set_hexpand(true);
        let run_match_btn = gtk::Button::with_label("Run Match");
        run_match_btn.set_widget_name("gig_run_match");
        top_bar.pack_start(&match_status,  true,  true,  0);
        top_bar.pack_start(&run_match_btn, false, false, 0);

        let match_list = gtk::ListBox::new();
        match_list.set_widget_name("gig_match_list");
        match_list.set_selection_mode(gtk::SelectionMode::None);
        let match_scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
        match_scroll.set_vexpand(true);
        match_scroll.add(&match_list);

        vbox.pack_start(&top_bar,      false, false, 0);
        vbox.pack_start(&match_scroll, true,  true,  0);

        notebook.append_page(&vbox, Some(&gtk::Label::new(Some("Match"))));
    }

    // ── Tab 4: Finalize ───────────────────────────────────────────────────────
    {
        let scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 8);
        vbox.set_border_width(12);

        let dest_lbl = gtk::Label::new(Some("Destination: —"));
        dest_lbl.set_widget_name("gig_dest_label");
        dest_lbl.set_xalign(0.0);

        let create_status = gtk::Label::new(None);
        create_status.set_widget_name("gig_create_status");
        create_status.set_xalign(0.0);

        let create_btn = gtk::Button::with_label("Create Playlist in Rekordbox");
        create_btn.set_widget_name("gig_create_btn");
        create_btn.set_halign(gtk::Align::Start);

        let shopping_btn = gtk::Button::with_label("Copy shopping list");
        shopping_btn.set_widget_name("gig_shopping_btn");
        shopping_btn.set_halign(gtk::Align::Start);

        vbox.pack_start(&dest_lbl,      false, false, 0);
        vbox.pack_start(&create_status, false, false, 0);
        vbox.pack_start(&create_btn,    false, false, 0);
        vbox.pack_start(&shopping_btn,  false, false, 0);

        scroll.add(&vbox);
        notebook.append_page(&scroll, Some(&gtk::Label::new(Some("Finalize"))));
    }

    outer.pack_start(&notebook, true, true, 0);
    outer
}

pub(super) fn load_gig_into_workspace(
    workspace: &gtk::Box,
    gig: &crate::gig::Gig,
    contact: &crate::gig::Contact,
) {
    // Stamp gig ID on the workspace so the back button and auto-save can use it
    workspace.set_widget_name(&format!("gig_workspace:{}", gig.id));

    // Update header
    if let Some(w) = find_widget(workspace, "gig_header") {
        if let Ok(lbl) = w.downcast::<gtk::Label>() {
            let title = if gig.name.is_empty() {
                contact.name.clone()
            } else {
                format!("{} – {}", contact.name, gig.name)
            };
            lbl.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(&title)));
        }
    }

    // Contact label (Info tab)
    if let Some(w) = find_widget(workspace, "gig_contact_label") {
        if let Ok(lbl) = w.downcast::<gtk::Label>() {
            lbl.set_markup(&format!(
                "{}  <small>{}</small>",
                glib::markup_escape_text(&contact.name),
                contact.customer_type.label(),
            ));
        }
    }

    macro_rules! set_entry { ($name:expr, $val:expr) => {
        if let Some(w) = find_widget(workspace, $name) {
            if let Ok(e) = w.downcast::<gtk::Entry>() {
                e.set_text($val);
            }
        }
    }; }

    // Info tab fields
    set_entry!("gig_name",       &gig.name);
    set_entry!("gig_date",       gig.date.as_deref().unwrap_or(""));
    set_entry!("gig_start_time", gig.start_time.as_deref().unwrap_or(""));
    set_entry!("gig_end_time",   gig.end_time.as_deref().unwrap_or(""));
    set_entry!("gig_location",   gig.location.as_deref().unwrap_or(""));

    // Brief tab fields
    set_entry!("gig_spotify_url", gig.spotify_playlist_url.as_deref().unwrap_or(""));
    if let Some(w) = find_widget(workspace, "gig_notes") {
        if let Ok(tv) = w.downcast::<gtk::TextView>() {
            if let Some(buf) = tv.get_buffer() {
                buf.set_text(&gig.notes);
            }
        }
    }

    // Match tab status
    if let Some(w) = find_widget(workspace, "gig_match_status") {
        if let Ok(lbl) = w.downcast::<gtk::Label>() {
            let status = if !gig.accepted_track_ids.is_empty() {
                format!("{} tracks accepted — ready to finalize", gig.accepted_track_ids.len())
            } else if gig.spotify_playlist_url.is_some() {
                "Spotify URL set — click Run Match".to_string()
            } else {
                "Add a Spotify URL in Brief, then run Match".to_string()
            };
            lbl.set_text(&status);
        }
    }

    // Clear previous match results
    if let Some(w) = find_widget(workspace, "gig_match_list") {
        if let Ok(lb) = w.downcast::<gtk::ListBox>() {
            for child in lb.get_children() {
                lb.remove(&child);
            }
        }
    }

    // Finalize tab
    if let Some(w) = find_widget(workspace, "gig_dest_label") {
        if let Ok(lbl) = w.downcast::<gtk::Label>() {
            lbl.set_text(&format!(
                "Destination: {}/{}/{}/",
                contact.customer_type.playlist_folder(),
                contact.name,
                gig.name,
            ));
        }
    }
    if let Some(w) = find_widget(workspace, "gig_create_status") {
        if let Ok(lbl) = w.downcast::<gtk::Label>() {
            lbl.set_text(if gig.rekordbox_folder_id.is_some() { "Playlist created ✓" } else { "" });
        }
    }
}

pub(super) fn set_match_status(workspace: &gtk::Box, msg: &str) {
    if let Some(w) = find_widget(workspace, "gig_match_status") {
        if let Ok(lbl) = w.downcast::<gtk::Label>() {
            lbl.set_text(msg);
        }
    }
}

/// Populate the Match tab with results and wire up Accept/Skip toggles.
pub(super) fn populate_match_results(
    workspace: &gtk::Box,
    gig_id:    &str,
    results:   &[crate::matcher::MatchResult],
    window:    &gtk::ApplicationWindow,
) {
    let match_list = match find_widget(workspace, "gig_match_list") {
        Some(w) => match w.downcast::<gtk::ListBox>() {
            Ok(lb) => lb,
            Err(_) => return,
        },
        None => return,
    };

    for child in match_list.get_children() {
        match_list.remove(&child);
    }

    let matched: Vec<_> = results.iter().filter(|r| r.matched.is_some()).collect();
    let missing: Vec<_> = results.iter().filter(|r| r.matched.is_none()).collect();

    // Section header for matched
    if !matched.is_empty() {
        let hdr = gtk::Label::new(Some(&format!("Matched ({} tracks)", matched.len())));
        hdr.set_xalign(0.0);
        hdr.set_margin_top(4);
        hdr.set_margin_bottom(2);
        let row = gtk::ListBoxRow::new();
        row.set_selectable(false);
        row.set_activatable(false);
        row.add(&hdr);
        match_list.add(&row);
    }

    // One row per matched track with Accept/Skip toggle
    for r in &matched {
        let local = r.matched.as_ref().unwrap();
        let track_id = local.id;

        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row_box.set_margin_start(4);
        row_box.set_margin_end(4);

        let label = gtk::Label::new(Some(&format!(
            "{} – {}",
            r.spotify.title,
            r.spotify.artist,
        )));
        label.set_xalign(0.0);
        label.set_hexpand(true);

        let local_lbl = gtk::Label::new(Some(&local.title));
        local_lbl.set_xalign(0.0);
        local_lbl.set_width_chars(20);

        // Load current accepted state
        let accepted = {
            let store = crate::gig::GigStore::load();
            store.gigs.iter().find(|g| g.id == gig_id)
                .map(|g| g.accepted_track_ids.contains(&track_id))
                .unwrap_or(true)
        };

        let toggle = gtk::ToggleButton::new();
        toggle.set_label(if accepted { "✓ Accept" } else { "Skip" });
        toggle.set_active(accepted);

        let gig_id_c = gig_id.to_string();
        toggle.connect_toggled(move |btn| {
            let active = btn.get_active();
            btn.set_label(if active { "✓ Accept" } else { "Skip" });
            let mut store = crate::gig::GigStore::load();
            if let Some(gig) = store.gigs.iter_mut().find(|g| g.id == gig_id_c) {
                if active {
                    if !gig.accepted_track_ids.contains(&track_id) {
                        gig.accepted_track_ids.push(track_id);
                    }
                } else {
                    gig.accepted_track_ids.retain(|&id| id != track_id);
                }
                store.save();
            }
        });

        row_box.pack_start(&label,      true,  true,  0);
        row_box.pack_start(&local_lbl,  false, false, 0);
        row_box.pack_start(&toggle,     false, false, 0);

        let row = gtk::ListBoxRow::new();
        row.set_selectable(false);
        row.add(&row_box);
        match_list.add(&row);
    }

    // Section header for missing
    if !missing.is_empty() {
        let hdr = gtk::Label::new(Some(&format!("Missing ({} tracks)", missing.len())));
        hdr.set_xalign(0.0);
        hdr.set_margin_top(8);
        hdr.set_margin_bottom(2);
        let row = gtk::ListBoxRow::new();
        row.set_selectable(false);
        row.set_activatable(false);
        row.add(&hdr);
        match_list.add(&row);
    }

    for r in &missing {
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row_box.set_margin_start(4);
        row_box.set_margin_end(4);

        let label = gtk::Label::new(Some(&format!(
            "{} – {}",
            r.spotify.title,
            r.spotify.artist,
        )));
        label.set_xalign(0.0);
        label.set_hexpand(true);

        let missing_lbl = gtk::Label::new(Some("not in library"));
        missing_lbl.set_xalign(0.0);

        row_box.pack_start(&label,       true,  true,  0);
        row_box.pack_start(&missing_lbl, false, false, 0);

        let row = gtk::ListBoxRow::new();
        row.set_selectable(false);
        row.add(&row_box);
        match_list.add(&row);
    }

    // Wire up initial accepted_track_ids: add all matched tracks
    {
        let mut store = crate::gig::GigStore::load();
        if let Some(gig) = store.gigs.iter_mut().find(|g| g.id == gig_id) {
            let current: std::collections::HashSet<i64> = gig.accepted_track_ids.iter().cloned().collect();
            // Add newly matched tracks that aren't already tracked
            for r in &matched {
                let id = r.matched.as_ref().unwrap().id;
                if !current.contains(&id) {
                    gig.accepted_track_ids.push(id);
                }
            }
            store.save();
        }
    }

    let matched_count = matched.len();
    let missing_count = missing.len();
    set_match_status(workspace, &format!(
        "{} matched, {} missing",
        matched_count, missing_count,
    ));

    // Shopping list button in Finalize tab
    if !missing.is_empty() {
        let missing_spotify: Vec<_> = missing.iter().map(|r| &r.spotify).collect();
        let shopping = crate::matcher::shopping_list(&missing_spotify);
        if let Some(w) = find_widget(workspace, "gig_shopping_btn") {
            if let Ok(btn) = w.downcast::<gtk::Button>() {
                btn.connect_clicked(move |b| {
                    let clipboard = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
                    clipboard.set_text(&shopping);
                    b.set_label("✓ Copied!");
                });
            }
        }
    }

    match_list.show_all();
}
