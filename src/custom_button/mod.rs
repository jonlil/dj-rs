mod imp;

use gtk4 as gtk;
use gtk::glib;
use glib::Object;

glib::wrapper! {
    pub struct CustomButton(ObjectSubclass<imp::CustomButton>)
        @extends gtk::Button, gtk::Widget,
        @implements gtk::Accessible, gtk::Actionable, gtk::Buildable, gtk::ConstraintTarget;
}

impl CustomButton {
    pub fn new() -> Self {
        Object::new(&[]).expect("Failed to create `CustomButton`.")
    }
}

impl Default for CustomButton {
    fn default() -> Self {
        Self::new()
    }
}
