extern crate gtk;

use gtk::prelude::*;
use gtk::{ButtonsType, DialogFlags, MessageType, MessageDialog, Window};

fn main() {
    gtk::init().expect("Failed to initialize GTK");

    MessageDialog::new(None::<&Window>,
        DialogFlags::empty(),
        MessageType::Info,
        ButtonsType::Ok,
        "Hello Rust DJ").run();
}
