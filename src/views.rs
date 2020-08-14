use gtk::{Grid};
use gtk::prelude::*;

pub struct MainView {
    pub container: Grid,
}

impl MainView {
    pub fn new() -> Self {
        let container = Grid::new();
        let player_box = gtk::Box::new(
            gtk::Orientation::Horizontal,
            0,
        );

        let player1 = PlayerView::new();
        let player2 = PlayerView::new();

        player_box.pack_start(&player1.container, true, true, 5);
        player_box.pack_end(&player2.container, true, true, 5);

        container.add(&player_box);

        container.set_row_spacing(12);
        container.set_border_width(6);
        container.set_vexpand(true);
        container.set_hexpand(true);

        MainView {
            container,
        }
    }
}

pub struct PlayerView {
    pub container: gtk::Scale,
}

impl PlayerView {
    pub fn new() -> Self {
        let adjustment = gtk::Adjustment::new(
            0.0,
            0.0,
            255.0,
            1.0,
            1.0,
            1.0,
        );

        let track_position = gtk::Scale::new(
            gtk::Orientation::Horizontal,
            Some(&adjustment),
        );

        track_position.set_hexpand(true);

        PlayerView {
            container: track_position,
        }
    }
}

pub struct BrowserView;
