use std::cell::Cell;

use glib::Binding;
use glib::{ParamFlags, ParamSpec, ParamSpecInt};
use gtk4 as gtk;
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use once_cell::sync::Lazy;

#[derive(Default)]
pub struct CustomButton {
    number: Cell<i32>,
}

#[glib::object_subclass]
impl ObjectSubclass for CustomButton {
    const NAME: &'static str = "DjGTKAppCustomButton";
    type Type = super::CustomButton;
    type ParentType = gtk::Button;
}

impl ObjectImpl for CustomButton {
    fn properties() -> &'static [ParamSpec] {
        static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
            vec![ParamSpecInt::new(
                // Name
                "number",
                // Nickname
                "number",
                // Short description
                "number",
                // Minimum value
                i32::MIN,
                // Maximum value
                i32::MAX,
                // Default value
                0,
                // The property can be read and written to
                ParamFlags::READWRITE,
            )]
        });
        PROPERTIES.as_ref()
    }

    fn set_property(&self, _obj: &Self::Type, _id: usize, value: &glib::Value, pspec: &ParamSpec) {
        match pspec.name() {
            "number" => {
                let input_number = value.get().expect("The value needs to be of type `i32`.");
                self.number.replace(input_number);
            }
            _ => unimplemented!(),
        }
    }

    fn property(&self, _obj: &Self::Type, _id: usize, pspec: &ParamSpec) -> glib::Value {
        match pspec.name() {
            "number" => self.number.get().to_value(),
            _ => unimplemented!(),
        }
    }

    fn constructed(&self, obj: &Self::Type) {
        self.parent_constructed(obj);

        obj.bind_property("number", obj, "label")
            .flags(glib::BindingFlags::SYNC_CREATE)
            .build();
    }
}

impl WidgetImpl for CustomButton {}

impl ButtonImpl for CustomButton {
    fn clicked(&self, button: &Self::Type) {
        let incremented_number = self.number.get() + 1;
        button.set_property("number", &incremented_number);
    }
}
