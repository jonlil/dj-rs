use gtk::{Grid};
use gtk::prelude::*;

pub struct MainView {
    pub container: Grid,
}

impl MainView {
    pub fn new() -> Self {
        let button = gtk::Button::new();
        button.set_label("Open");
        button.set_halign(gtk::Align::Center);

        let container = Grid::new();

        container.attach(&button, 0, 1, 1, 1);

        container.set_row_spacing(12);
        container.set_border_width(6);
        container.set_vexpand(true);
        container.set_hexpand(true);

        MainView {
            container,
        }
    }
}
