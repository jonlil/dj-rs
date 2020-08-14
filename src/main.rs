extern crate gtk;
extern crate gio;
extern crate glib;

mod views;

use gtk::prelude::*;
use gio::prelude::*;

use glib::clone;

use gtk::{
    ApplicationWindow,
    Orientation,
    MenuItem
};

use std::rc::Rc;
use std::env;

pub struct Application {
    pub widgets: Rc<Widgets>,
}

impl Application {
    pub fn new(app: &gtk::Application) -> Self {
        let app = Application {
            widgets: Rc::new(Widgets::new(app)),
        };

        app
    }
}

pub struct Widgets {
    pub window: ApplicationWindow,
    pub main_view: views::MainView,
    pub vertical_box: gtk::Box,
}

impl Widgets {
    pub fn new(application: &gtk::Application) -> Self {
        let main_view = views::MainView::new();
        let menu_bar = MenuBar::new();
        let vertical_box = gtk::Box::new(Orientation::Vertical, 10);
        let window = ApplicationWindow::new(application);

        window.set_default_size(640, 640);
        window.set_title("DJ Application");

        vertical_box.pack_start(&menu_bar.container, false, false, 0);
        vertical_box.pack_start(&main_view.container, true, true, 0);

        window.add(&vertical_box);
        window.show_all();

        menu_bar.quit.connect_activate(clone!(@weak window => move |_| {
            window.close();
        }));

        Widgets {
            window,
            main_view,
            vertical_box,
        }
    }
}

pub struct MenuBar {
    container: gtk::MenuBar,
    quit: MenuItem,
}

impl MenuBar {
    pub fn new() -> Self {
        let container = gtk::MenuBar::new();

        let file = MenuItem::with_label("File");
        let menu = gtk::Menu::new();
        let quit = MenuItem::with_label("Quit");

        let about = MenuItem::with_label("About");

        menu.append(&quit);
        file.set_submenu(Some(&menu));

        container.append(&file);
        container.append(&about);

        MenuBar {
            container,
            quit,
        }
    }
}

fn main() {
    glib::set_program_name(Some("Rust DJ Application"));

    let application = gtk::Application::new(
        Some("se.jonlil.dj"),
        Default::default(),
    )
    .expect("Failed to initialize Application");

    application.connect_startup(|app| {
        Application::new(app);
    });

    application.connect_activate(|_| {});
    application.run(&env::args().collect::<Vec<_>>());
}
