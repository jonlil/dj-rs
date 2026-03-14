use gtk::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use crate::config::{Config, PathMapping};
use crate::gig::{Contact, CustomerType, Gig, GigStore};
use crate::rekordbox::Library;

pub(super) fn show_settings_dialog(window: &gtk::ApplicationWindow, config: &Rc<RefCell<Config>>) {
    let dialog = gtk::Dialog::new();
    dialog.set_title("Settings");
    dialog.set_transient_for(Some(window));
    dialog.set_modal(true);
    dialog.set_destroy_with_parent(true);
    dialog.set_default_size(560, 300);
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Save",   gtk::ResponseType::Accept);

    let content = dialog.get_content_area();
    content.set_spacing(6);
    content.set_border_width(12);

    let heading = gtk::Label::new(Some("Path Mappings"));
    heading.set_xalign(0.0);
    content.pack_start(&heading, false, false, 0);

    let hint = gtk::Label::new(Some(
        "Rewrite path prefixes stored in the database to match your local file system.\n\
         Example:  /Volumes/muzika  →  /run/media/jonas/muzika",
    ));
    hint.set_xalign(0.0);
    hint.set_line_wrap(true);
    content.pack_start(&hint, false, false, 0);

    let rows_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_min_content_height(120);
    scroll.add(&rows_box);
    content.pack_start(&scroll, true, true, 0);

    let pairs: Rc<RefCell<Vec<(gtk::Entry, gtk::Entry)>>> = Rc::new(RefCell::new(Vec::new()));

    let add_row = {
        let rows_box = rows_box.clone();
        let pairs    = pairs.clone();
        Rc::new(move |from: &str, to: &str| {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            let from_entry = gtk::Entry::new();
            from_entry.set_placeholder_text(Some("From prefix (e.g. /Volumes/muzika)"));
            from_entry.set_text(from);
            from_entry.set_hexpand(true);
            let arrow = gtk::Label::new(Some("→"));
            let to_entry = gtk::Entry::new();
            to_entry.set_placeholder_text(Some("To prefix (e.g. /run/media/jonas/muzika)"));
            to_entry.set_text(to);
            to_entry.set_hexpand(true);
            let del_btn = gtk::Button::with_label("✕");

            row.pack_start(&from_entry, true, true, 0);
            row.pack_start(&arrow,      false, false, 4);
            row.pack_start(&to_entry,   true, true, 0);
            row.pack_start(&del_btn,    false, false, 0);

            rows_box.pack_start(&row, false, false, 0);
            rows_box.show_all();

            let row_clone  = row.clone();
            let pairs_del  = pairs.clone();
            let fe = from_entry.clone();
            let te = to_entry.clone();
            del_btn.connect_clicked(move |_| {
                row_clone.hide();
                pairs_del.borrow_mut().retain(|(f, t)| {
                    f.as_ptr() != fe.as_ptr() || t.as_ptr() != te.as_ptr()
                });
            });

            pairs.borrow_mut().push((from_entry, to_entry));
        })
    };

    for m in &config.borrow().path_mappings {
        add_row(&m.from, &m.to);
    }

    let add_btn = gtk::Button::with_label("+ Add mapping");
    add_btn.set_halign(gtk::Align::Start);
    {
        let add_row = add_row.clone();
        add_btn.connect_clicked(move |_| add_row("", ""));
    }
    content.pack_start(&add_btn, false, false, 0);

    // ── Spotify section ───────────────────────────────────────────────────────
    content.pack_start(&gtk::Separator::new(gtk::Orientation::Horizontal), false, false, 4);

    let spotify_heading = gtk::Label::new(Some("Spotify"));
    spotify_heading.set_xalign(0.0);
    content.pack_start(&spotify_heading, false, false, 0);

    let spotify_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let connected   = config.borrow().spotify_access_token.is_some();
    let status_text = if connected { "✓ Connected" } else { "Not connected" };
    let spotify_status = gtk::Label::new(Some(status_text));
    spotify_status.set_xalign(0.0);
    spotify_status.set_hexpand(true);
    let connect_btn = gtk::Button::with_label("Connect with Spotify");
    spotify_row.pack_start(&spotify_status, true,  true,  0);
    spotify_row.pack_start(&connect_btn,    false, false, 0);
    content.pack_start(&spotify_row, false, false, 0);

    {
        let config         = config.clone();
        let spotify_status = spotify_status.clone();

        connect_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            spotify_status.set_text("Waiting for browser…");

            let (tx, rx) = std::sync::mpsc::channel::<Result<(String, String), String>>();
            std::thread::spawn(move || {
                let _ = tx.send(crate::spotify::authorize());
            });

            let config         = config.clone();
            let spotify_status = spotify_status.clone();
            let btn            = btn.clone();
            glib::timeout_add_local(200, move || {
                match rx.try_recv() {
                    Ok(Ok((access, refresh))) => {
                        config.borrow_mut().spotify_access_token  = Some(access);
                        config.borrow_mut().spotify_refresh_token = Some(refresh);
                        config.borrow().save();
                        spotify_status.set_text("✓ Connected");
                        btn.set_sensitive(true);
                        glib::Continue(false)
                    }
                    Ok(Err(e)) => {
                        spotify_status.set_text(&format!("Error: {e}"));
                        btn.set_sensitive(true);
                        glib::Continue(false)
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::Continue(true),
                    Err(_) => glib::Continue(false),
                }
            });
        });
    }

    content.show_all();

    let response = dialog.run();
    dialog.close();

    if response == gtk::ResponseType::Accept {
        let mappings: Vec<PathMapping> = pairs.borrow().iter()
            .map(|(f, t)| PathMapping {
                from: f.get_text().to_string(),
                to:   t.get_text().to_string(),
            })
            .filter(|m| !m.from.is_empty())
            .collect();

        config.borrow_mut().path_mappings = mappings;
        config.borrow().save();
    }
}

#[allow(dead_code)]
pub(super) fn show_gig_prep_dialog(
    window:  &gtk::ApplicationWindow,
    config:  &Rc<RefCell<Config>>,
    library: &Rc<RefCell<Option<Library>>>,
) {
    let dialog = gtk::Dialog::new();
    dialog.set_title("Gig Prep");
    dialog.set_transient_for(Some(window));
    dialog.set_modal(true);
    dialog.set_destroy_with_parent(true);
    dialog.set_default_size(560, 520);
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Save",   gtk::ResponseType::Accept);

    let content = dialog.get_content_area();
    content.set_spacing(6);
    content.set_border_width(12);

    let grid = gtk::Grid::new();
    grid.set_row_spacing(6);
    grid.set_column_spacing(8);

    macro_rules! lbl {
        ($text:expr) => {{
            let l = gtk::Label::new(Some($text));
            l.set_xalign(1.0);
            l
        }};
    }
    macro_rules! entry {
        ($placeholder:expr) => {{
            let e = gtk::Entry::new();
            e.set_placeholder_text(Some($placeholder));
            e.set_hexpand(true);
            e
        }};
    }

    // Row 0: Gig type
    let type_combo = gtk::ComboBoxText::new();
    type_combo.append(Some("corporate"), "Corporate");
    type_combo.append(Some("venue"),     "Venue");
    type_combo.append(Some("private"),   "Private");
    type_combo.set_active_id(Some("private"));
    grid.attach(&lbl!("Type"),    0, 0, 1, 1);
    grid.attach(&type_combo,      1, 0, 1, 1);

    // Row 1: Contact
    let contact_entry = entry!("Contact person or client name");
    grid.attach(&lbl!("Contact"), 0, 1, 1, 1);
    grid.attach(&contact_entry,   1, 1, 1, 1);

    // Row 2: Event name
    let name_entry = entry!("Event name (e.g. Wedding, Kick-off 2026)");
    grid.attach(&lbl!("Name"),    0, 2, 1, 1);
    grid.attach(&name_entry,      1, 2, 1, 1);

    // Row 3: Date
    let date_entry = entry!("YYYY-MM-DD");
    grid.attach(&lbl!("Date"),    0, 3, 1, 1);
    grid.attach(&date_entry,      1, 3, 1, 1);

    // Row 4: Start / End time
    let time_box    = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let start_entry = entry!("HH:MM");
    start_entry.set_width_chars(6);
    start_entry.set_hexpand(false);
    let sep_lbl     = gtk::Label::new(Some("–"));
    let end_entry   = entry!("HH:MM");
    end_entry.set_width_chars(6);
    end_entry.set_hexpand(false);
    time_box.pack_start(&start_entry, false, false, 0);
    time_box.pack_start(&sep_lbl,     false, false, 0);
    time_box.pack_start(&end_entry,   false, false, 0);
    grid.attach(&lbl!("Time"),    0, 4, 1, 1);
    grid.attach(&time_box,        1, 4, 1, 1);

    // Row 5: Location
    let location_entry = entry!("Venue name or address");
    grid.attach(&lbl!("Location"),  0, 5, 1, 1);
    grid.attach(&location_entry,    1, 5, 1, 1);

    // Row 6: Spotify playlist URL
    let spotify_entry = entry!("https://open.spotify.com/playlist/…");
    grid.attach(&lbl!("Spotify"),   0, 6, 1, 1);
    grid.attach(&spotify_entry,     1, 6, 1, 1);

    // Row 7: Notes (multi-line)
    let notes_view   = gtk::TextView::new();
    notes_view.set_wrap_mode(gtk::WrapMode::Word);
    let notes_scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
    notes_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    notes_scroll.set_min_content_height(100);
    notes_scroll.add(&notes_view);
    notes_scroll.set_hexpand(true);
    let notes_lbl = lbl!("Notes");
    notes_lbl.set_valign(gtk::Align::Start);
    grid.attach(&notes_lbl,      0, 7, 1, 1);
    grid.attach(&notes_scroll,   1, 7, 1, 1);

    // Spotify connection status hint
    let spotify_hint = if config.borrow().spotify_access_token.is_some() {
        gtk::Label::new(Some("Spotify: ✓ Connected"))
    } else {
        gtk::Label::new(Some("Spotify: not connected — connect via Settings…"))
    };
    spotify_hint.set_xalign(0.0);

    content.pack_start(&grid,        true,  true,  0);
    content.pack_start(&spotify_hint, false, false, 0);
    content.show_all();

    let response = dialog.run();
    dialog.close();

    if response != gtk::ResponseType::Accept {
        return;
    }

    let customer_type = match type_combo.get_active_id().as_deref() {
        Some("corporate") => CustomerType::Corporate,
        Some("venue")     => CustomerType::Venue,
        _                 => CustomerType::Private,
    };

    let notes_buf  = notes_view.get_buffer().unwrap();
    let notes_text = notes_buf.get_text(
        &notes_buf.get_start_iter(),
        &notes_buf.get_end_iter(),
        false,
    ).map(|s| s.to_string()).unwrap_or_default();

    let contact = Contact {
        id:                  uuid::Uuid::new_v4().to_string(),
        name:                contact_entry.get_text().to_string(),
        customer_type,
        notes:               String::new(),
        rekordbox_folder_id: None,
    };

    let spotify_url = {
        let url = spotify_entry.get_text().to_string();
        if url.is_empty() { None } else { Some(url) }
    };

    let mut gig = Gig {
        id:                   uuid::Uuid::new_v4().to_string(),
        contact_id:           contact.id.clone(),
        name:                 name_entry.get_text().to_string(),
        date:                 { let d = date_entry.get_text().to_string(); if d.is_empty() { None } else { Some(d) } },
        start_time:           { let t = start_entry.get_text().to_string(); if t.is_empty() { None } else { Some(t) } },
        end_time:             { let t = end_entry.get_text().to_string(); if t.is_empty() { None } else { Some(t) } },
        location:             { let l = location_entry.get_text().to_string(); if l.is_empty() { None } else { Some(l) } },
        tags:                 Vec::new(),
        notes:                notes_text,
        spotify_playlist_url: spotify_url.clone(),
        accepted_track_ids:   Vec::new(),
        rekordbox_folder_id:  None,
    };

    // If a Spotify URL was given and the library is open, run the match flow
    if let (Some(url), Some(lib)) = (spotify_url, library.borrow().as_ref()) {
        let access_token = config.borrow().spotify_access_token.clone();
        match access_token {
            None => {
                let d = gtk::MessageDialog::new(
                    Some(window),
                    gtk::DialogFlags::MODAL,
                    gtk::MessageType::Warning,
                    gtk::ButtonsType::Ok,
                    "Spotify not connected. Connect via Settings… first.",
                );
                d.run();
                d.close();
            }
            Some(token) => {
                match crate::spotify::fetch_playlist(&token, &url) {
                    Err(e) => {
                        let d = gtk::MessageDialog::new(
                            Some(window),
                            gtk::DialogFlags::MODAL,
                            gtk::MessageType::Error,
                            gtk::ButtonsType::Ok,
                            &format!("Spotify fetch failed: {e}"),
                        );
                        d.run();
                        d.close();
                    }
                    Ok(spotify_tracks) => {
                        let all_tracks = lib.tracks().unwrap_or_default();
                        let results    = crate::matcher::match_tracks(&spotify_tracks, &all_tracks);
                        let folder_id = lib.find_or_create_folder(contact.customer_type.playlist_folder()).ok();
                        gig.rekordbox_folder_id = folder_id;
                        let _ = show_gig_match_results(window, &gig, &results, lib);
                    }
                }
            }
        }
    }

    let mut store = GigStore::load();
    store.contacts.push(contact);
    store.gigs.push(gig);
    store.save();
}

#[allow(dead_code)]
pub(super) fn show_gig_match_results(
    window:  &gtk::ApplicationWindow,
    gig:     &Gig,
    results: &[crate::matcher::MatchResult],
    lib:     &Library,
) -> Option<i64> {
    let matched: Vec<_> = results.iter().filter(|r| r.matched.is_some()).collect();
    let missing: Vec<_> = results.iter().filter(|r| r.matched.is_none()).collect();

    let dialog = gtk::Dialog::new();
    dialog.set_title(&format!("Gig Prep — {}", gig.name));
    dialog.set_transient_for(Some(window));
    dialog.set_modal(true);
    dialog.set_default_size(660, 560);
    dialog.add_button("Close",            gtk::ResponseType::Cancel);
    dialog.add_button("Create Playlist",  gtk::ResponseType::Accept);

    let content = dialog.get_content_area();
    content.set_border_width(12);
    content.set_spacing(8);

    // ── Matched section ───────────────────────────────────────────────────────
    let matched_lbl = gtk::Label::new(Some(&format!("✅ Matched ({} tracks)", matched.len())));
    matched_lbl.set_xalign(0.0);
    content.pack_start(&matched_lbl, false, false, 0);

    let matched_store = gtk::ListStore::new(&[
        glib::types::Type::String, // Spotify title
        glib::types::Type::String, // Spotify artist
        glib::types::Type::String, // Local match title
    ]);
    for r in &matched {
        let local = r.matched.as_ref().unwrap();
        matched_store.insert_with_values(None, &[0, 1, 2], &[
            &r.spotify.title.as_str(),
            &r.spotify.artist.as_str(),
            &local.title.as_str(),
        ]);
    }
    let matched_view = gtk::TreeView::with_model(&matched_store);
    for (i, title) in ["Spotify Title", "Artist", "Local Match"].iter().enumerate() {
        let col  = gtk::TreeViewColumn::new();
        let cell = gtk::CellRendererText::new();
        col.set_title(title);
        col.pack_start(&cell, true);
        col.add_attribute(&cell, "text", i as i32);
        matched_view.append_column(&col);
    }
    let matched_scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
    matched_scroll.set_min_content_height(160);
    matched_scroll.add(&matched_view);
    content.pack_start(&matched_scroll, true, true, 0);

    // ── Missing section ───────────────────────────────────────────────────────
    let missing_lbl = gtk::Label::new(Some(&format!("❌ Missing ({} tracks)", missing.len())));
    missing_lbl.set_xalign(0.0);
    content.pack_start(&missing_lbl, false, false, 0);

    let missing_store = gtk::ListStore::new(&[
        glib::types::Type::String,
        glib::types::Type::String,
    ]);
    for r in &missing {
        missing_store.insert_with_values(None, &[0, 1], &[
            &r.spotify.title.as_str(),
            &r.spotify.artist.as_str(),
        ]);
    }
    let missing_view = gtk::TreeView::with_model(&missing_store);
    for (i, title) in ["Title", "Artist"].iter().enumerate() {
        let col  = gtk::TreeViewColumn::new();
        let cell = gtk::CellRendererText::new();
        col.set_title(title);
        col.pack_start(&cell, true);
        col.add_attribute(&cell, "text", i as i32);
        missing_view.append_column(&col);
    }
    let missing_scroll = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
    missing_scroll.set_min_content_height(120);
    missing_scroll.add(&missing_view);
    content.pack_start(&missing_scroll, true, true, 0);

    // ── Copy shopping list button ─────────────────────────────────────────────
    if !missing.is_empty() {
        let copy_btn = gtk::Button::with_label("Copy shopping list to clipboard");
        copy_btn.set_halign(gtk::Align::Start);
        let missing_spotify: Vec<_> = missing.iter().map(|r| &r.spotify).collect();
        let shopping = crate::matcher::shopping_list(&missing_spotify);
        copy_btn.connect_clicked(move |btn| {
            let clipboard = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
            {
                clipboard.set_text(&shopping);
                btn.set_label("✓ Copied!");
            }
        });
        content.pack_start(&copy_btn, false, false, 0);
    }

    content.show_all();

    let response = dialog.run();
    dialog.close();

    if response != gtk::ResponseType::Accept || matched.is_empty() {
        return None;
    }

    // Create the Rekordbox playlist under the right folder
    let playlist_id = lib.find_or_create_folder("PRIVATE")
        .and_then(|folder_id| lib.create_playlist(&gig.name, Some(folder_id)))
        .and_then(|pl_id| {
            let track_ids: Vec<i64> = matched.iter()
                .map(|r| r.matched.as_ref().unwrap().id)
                .collect();
            lib.add_tracks_to_playlist(pl_id, &track_ids)?;
            Ok(pl_id)
        })
        .ok();

    playlist_id
}
