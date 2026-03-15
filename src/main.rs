extern crate gtk;
extern crate gio;
extern crate glib;

mod config;
mod deck;
mod gig;
mod matcher;
mod rekordbox;
mod server;
mod librespot_player;
mod spotify;
mod tags;
mod views;

use gtk::prelude::*;
use gio::prelude::*;

use glib::clone;

use gtk::{
    ApplicationWindow,
    Orientation,
    MenuItem,
    Paned,
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
    pub browser_view: views::BrowserView,
    pub vertical_box: gtk::Box,
}

impl Widgets {
    pub fn new(application: &gtk::Application) -> Self {
        let window = ApplicationWindow::new(application);
        window.set_default_size(1200, 800);
        window.set_title("DJ Application");

        let cfg = std::rc::Rc::new(std::cell::RefCell::new(config::Config::load()));
        let bridge = server::start_server(
            7879,
            cfg.borrow().clone(),
        );
        let main_view            = views::MainView::new(&window, bridge, cfg.clone());
        let queue_fn             = main_view.queue_fn.clone();
        let current_track_db_id  = main_view.current_track_db_id.clone();
        let on_track_end         = main_view.on_track_end.clone();
        let browser_view = views::BrowserView::new(&window, cfg, Some(queue_fn), current_track_db_id, on_track_end, main_view.spotify_player.clone());
        let menu_bar     = MenuBar::new();
        let vertical_box = gtk::Box::new(Orientation::Vertical, 0);

        // Split window vertically: decks on top, library browser on bottom
        let content_paned = Paned::new(Orientation::Vertical);
        content_paned.pack1(&main_view.container, false, false);
        content_paned.pack2(&browser_view.container, true, true);
        content_paned.set_position(240);

        vertical_box.pack_start(&menu_bar.container, false, false, 0);
        vertical_box.pack_start(&content_paned, true, true, 0);

        window.add(&vertical_box);
        window.show_all();

        menu_bar.quit.connect_activate(clone!(@weak window => move |_| {
            window.close();
        }));

        Widgets {
            window,
            main_view,
            browser_view,
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
        let css = gtk::CssProvider::new();
        let _ = css.load_from_data(b"
            treeview row { min-height: 28px; }
            list row { border-bottom: 1px solid @borders; }
        ");
        if let Some(screen) = gdk::Screen::get_default() {
            gtk::StyleContext::add_provider_for_screen(
                &screen,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        Application::new(app);
    });

    application.connect_activate(|_| {});
    application.run(&env::args().collect::<Vec<_>>());
}
