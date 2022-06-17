mod imp;

use gtk4 as gtk;
use glib::Object;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::Application;
use gtk::{gio, glib};
use gio::Settings;

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl Window {
    pub fn new(app: &Application) -> Self {
        Object::new(&[("application", app)]).expect("Failed to create `Window`.")
    }

    fn setup_settings(&self) {
        let settings = Settings::new("se.jl-media.dj");
        self.imp()
            .settings
            .set(settings)
            .expect("Could not set `Settings`.");
    }

    fn settings(&self) -> &Settings {
        self.imp().settings.get().expect("Could not get settings.")
    }

    pub fn save_window_size(&self) -> Result<(), glib::BoolError> {
        let settings = &self.imp().settings;

        let size = self.default_size();

        self.settings().set_int("window-width", size.0)?;
        self.settings().set_int("window-height", size.1)?;
        self.settings().set_boolean("is-maximized", self.is_maximized())?;

        Ok(())
    }

    pub fn load_window_size(&self) {
        let settings = &self.imp().settings;

        let width = self.settings().int("window-width");
        let height = self.settings().int("window-height");
        let is_maximized = self.settings().boolean("is-maximized");

        self.set_default_size(width, height);
        if is_maximized {
            self.maximize();
        }
    }
}
