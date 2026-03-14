use gtk::prelude::*;

pub(super) fn populate_gig_sidebar_from_library(
    list_box:          &gtk::ListBox,
    store:             &crate::gig::GigStore,
    playlists:         &[crate::rekordbox::Playlist],
    expanded_contacts: &std::collections::HashSet<String>,
) {
    for child in list_box.get_children() {
        list_box.remove(&child);
    }

    for contact in &store.contacts {
        let expanded = expanded_contacts.contains(&contact.id);
        list_box.add(&make_contact_header_row(contact, expanded));

        if !expanded {
            continue;
        }

        if let Some(folder_id) = contact.rekordbox_folder_id {
            let mut children: Vec<_> = playlists.iter()
                .filter(|pl| pl.parent_id == Some(folder_id))
                .collect();
            children.sort_by_key(|pl| pl.id);

            for child_pl in children {
                if child_pl.attribute == 1 {
                    let gig = store.gigs.iter()
                        .find(|g| g.rekordbox_folder_id == Some(child_pl.id));
                    list_box.add(&make_gig_folder_row(child_pl, gig));

                    let mut set_pls: Vec<_> = playlists.iter()
                        .filter(|pl| pl.parent_id == Some(child_pl.id))
                        .collect();
                    set_pls.sort_by_key(|pl| pl.id);
                    for set_pl in set_pls {
                        list_box.add(&make_set_playlist_row(set_pl));
                    }
                } else {
                    list_box.add(&make_pool_row(child_pl));
                }
            }
        } else {
            for gig in store.gigs_for_contact(&contact.id) {
                list_box.add(&make_gig_row_simple(gig));
            }
        }
    }

    list_box.show_all();
}

/// Fallback used before library is loaded and when creating new gigs.
pub(super) fn populate_contacts_and_gigs(
    list_box:          &gtk::ListBox,
    store:             &crate::gig::GigStore,
    expanded_contacts: &std::collections::HashSet<String>,
) {
    for child in list_box.get_children() {
        list_box.remove(&child);
    }
    for contact in &store.contacts {
        let expanded = expanded_contacts.contains(&contact.id);
        list_box.add(&make_contact_header_row(contact, expanded));
        if expanded {
            for gig in store.gigs_for_contact(&contact.id) {
                list_box.add(&make_gig_row_simple(gig));
            }
        }
    }
    list_box.show_all();
}

pub(super) fn make_contact_header_row(contact: &crate::gig::Contact, expanded: bool) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_widget_name(&format!("contact:{}", contact.id));
    let arrow = if expanded { "▼" } else { "▶" };
    let lbl = gtk::Label::new(None);
    lbl.set_markup(&format!(
        "{} <b>{}</b>  <small>{}</small>",
        arrow,
        glib::markup_escape_text(&contact.name),
        contact.customer_type.label(),
    ));
    lbl.set_xalign(0.0);
    lbl.set_margin_start(6);
    lbl.set_margin_top(5);
    lbl.set_margin_bottom(5);
    row.add(&lbl);
    row
}

pub(super) fn make_gig_folder_row(
    pl:  &crate::rekordbox::Playlist,
    gig: Option<&crate::gig::Gig>,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let widget_name = gig
        .map(|g| format!("gig:{}", g.id))
        .unwrap_or_else(|| format!("rb_folder:{}", pl.id));
    row.set_widget_name(&widget_name);
    let lbl = gtk::Label::new(Some(&pl.name));
    lbl.set_xalign(0.0);
    lbl.set_margin_start(18);
    lbl.set_margin_top(4);
    lbl.set_margin_bottom(4);
    row.add(&lbl);
    row
}

pub(super) fn make_set_playlist_row(pl: &crate::rekordbox::Playlist) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_widget_name(&format!("pl:{}", pl.id));
    let label_text = if pl.track_count > 0 {
        format!("  {} ({})", pl.name, pl.track_count)
    } else {
        format!("  {}", pl.name)
    };
    let lbl = gtk::Label::new(Some(&label_text));
    lbl.set_xalign(0.0);
    lbl.set_margin_start(32);
    lbl.set_margin_top(2);
    lbl.set_margin_bottom(2);
    row.add(&lbl);
    row
}

pub(super) fn make_pool_row(pl: &crate::rekordbox::Playlist) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_widget_name(&format!("pool:{}", pl.id));
    let label_text = if pl.track_count > 0 {
        format!("{} ({})", pl.name, pl.track_count)
    } else {
        pl.name.clone()
    };
    let lbl = gtk::Label::new(Some(&label_text));
    lbl.set_xalign(0.0);
    lbl.set_margin_start(18);
    lbl.set_margin_top(3);
    lbl.set_margin_bottom(3);
    row.add(&lbl);
    row
}

pub(super) fn make_gig_row_simple(gig: &crate::gig::Gig) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_widget_name(&format!("gig:{}", gig.id));
    let lbl = gtk::Label::new(Some(&gig.format_label()));
    lbl.set_xalign(0.0);
    lbl.set_margin_start(18);
    lbl.set_margin_top(3);
    lbl.set_margin_bottom(3);
    row.add(&lbl);
    row
}
