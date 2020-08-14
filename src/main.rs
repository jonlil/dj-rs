extern crate gtk;
extern crate gio;
extern crate glib;

use gtk::prelude::*;
use gio::prelude::*;

use glib::clone;

use gtk::{
    ApplicationWindow,
    Application,
    Orientation,
    Label,
    Menu,
    MenuBar,
    MenuItem
};

use std::env;

fn build_menu(window: &ApplicationWindow) -> MenuBar {
    let menu_bar = MenuBar::new();
    let file = MenuItem::with_label("File");
    let menu = Menu::new();
    let quit = MenuItem::with_label("Quit");

    let about = MenuItem::with_label("About");

    menu.append(&quit);
    file.set_submenu(Some(&menu));

    menu_bar.append(&file);
    menu_bar.append(&about);

    quit.connect_activate(clone!(@weak window => move |_| {
        window.close();
    }));

    menu_bar
}

fn build_ui(application:  &Application) {
    let window = ApplicationWindow::new(application);

    window.set_default_size(640, 640);
    window.set_title("DJ Application");

    let label = Label::new(Some("DJ RS"));
    let v_box = gtk::Box::new(Orientation::Vertical, 10);

    v_box.pack_start(&build_menu(&window), false, false, 0);
    v_box.pack_start(&label, true, true, 0);

    window.add(&v_box);
    window.show_all();
}

fn main() {
    let application = Application::new(
        Some("se.jonlil.dj"),
        Default::default(),
    )
    .expect("Failed to initialize Application");

    application.connect_activate(|app| {
        build_ui(app);
    });

    application.run(&env::args().collect::<Vec<_>>());
}
