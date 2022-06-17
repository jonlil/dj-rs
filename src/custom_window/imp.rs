use gio::Settings;
use gtk4 as gtk;
use gtk::{gio, glib, Inhibit};
use gtk::{subclass::prelude::*, ApplicationWindow};
use once_cell::sync::OnceCell;

#[derive(Default)]
pub struct Window {
    pub settings: OnceCell<Settings>,
}

#[glib::object_subclass]
impl ObjectSubclass for Window {
    const NAME: &'static str = "DJAppWindow";
    type Type = super::Window;
    type ParentType = ApplicationWindow;
}

impl ObjectImpl for Window {
    fn constructed(&self, obj: &Self::Type) {
        self.parent_constructed(obj);
        // Load latest window state
        obj.setup_settings();
        obj.load_window_size();
    }
}
impl WidgetImpl for Window {}
impl WindowImpl for Window {
    fn close_request(&self, window: &Self::Type) -> glib::signal::Inhibit {
        window.save_window_size().expect("Failed to save window state");

        Inhibit(false)
    }
}
impl ApplicationWindowImpl for Window {}
