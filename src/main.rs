mod custom_button;
mod custom_window;
mod player;

use gio::{Settings, SettingsBindFlags};
use glib::BindingFlags;
use gtk4 as gtk;
use gtk::prelude::*;
use gtk::Application;
use custom_button::CustomButton;
use gstreamer::prelude::*;
use gstreamer as gst;

fn main() {
    let app = Application::builder()
        .application_id("se.jl-media.dj")
        .build();

    app.connect_activate(build_ui);

    app.run();
}

fn build_ui(app: &Application) {
    let settings = Settings::new("se.jl-media.dj");
    let button_1 = CustomButton::new();
    let button_2 = CustomButton::new();
    let switch = gtk::Switch::builder()
        .margin_top(48)
        .margin_bottom(48)
        .margin_start(48)
        .margin_end(48)
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Center)
        .build();

    settings
        .bind("is-switch-enabled", &switch, "state")
        .flags(SettingsBindFlags::DEFAULT)
        .build();

    button_1
        .bind_property("number", &button_2, "number")
        .transform_to(|_, value| {
            let number = value
                .get::<i32>()
                .expect("The property needs to be of type `i32`");
            let incremented_number = number + 1;
            Some(incremented_number.to_value())
        })
        .transform_from(|_, value| {
            let number = value
                .get::<i32>()
                .expect("The property needs to be of type `i32`");
            let decremented_number = number - 1;
            Some(decremented_number.to_value())
        })
        .flags(BindingFlags::BIDIRECTIONAL | BindingFlags::SYNC_CREATE)
        .build();

    gstreamer::init().expect("Failed initializing gstreamer");

    let gtk_box = gtk::Box::builder()
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Center)
        .spacing(12)
        .orientation(gtk::Orientation::Vertical)
        .build();

    gtk_box.append(&button_1);
    gtk_box.append(&button_2);
    gtk_box.append(&switch);
    build_player_ui(&gtk_box);
    build_library_browser_ui(&gtk_box);

    let window = custom_window::Window::new(app);
    window.set_title(Some("My DJ App"));
    window.set_child(Some(&gtk_box));

    window.present();
}

fn build_library_browser_ui(container: &gtk::Box) {
    let list_box = gtk::ListBox::new();
    for number in 0..=100 {
        let label = gtk::Label::new(Some(&number.to_string()));
        list_box.append(&label);
    }

    let scrolled_window = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_width(600)
        .min_content_height(300)
        .child(&list_box)
        .build();
    container.append(&scrolled_window);
}

fn build_player_ui(container: &gtk::Box) {
    player::build(&container);
}
