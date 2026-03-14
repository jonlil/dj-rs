use gtk::prelude::*;

pub(super) fn fmt_time(secs: f64) -> String {
    let s = secs as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

pub(super) fn rating_stars(r: i32) -> &'static str {
    match r {
        1 => "★",
        2 => "★★",
        3 => "★★★",
        4 => "★★★★",
        5 => "★★★★★",
        _ => "",
    }
}

pub(super) fn browser_fmt_duration(secs: i32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

pub(super) fn find_widget(parent: &gtk::Box, name: &str) -> Option<gtk::Widget> {
    find_in_widget(parent.upcast_ref(), name)
}

pub(super) fn find_in_widget(widget: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
    if widget.get_widget_name() == name {
        return Some(widget.clone());
    }
    if let Some(container) = widget.downcast_ref::<gtk::Container>() {
        for child in container.get_children() {
            if let Some(found) = find_in_widget(&child, name) {
                return Some(found);
            }
        }
    }
    None
}
