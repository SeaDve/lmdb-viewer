use gtk::{glib, prelude::*, subclass::prelude::*};

mod imp {
    use std::cell::OnceCell;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::DatabaseItem)]
    pub struct DatabaseItem {
        #[property(get, set, construct_only)]
        pub(super) key: OnceCell<glib::Bytes>,
        #[property(get, set, construct_only)]
        pub(super) data: OnceCell<glib::Bytes>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DatabaseItem {
        const NAME: &'static str = "LvDatabaseItem";
        type Type = super::DatabaseItem;
    }

    #[glib::derived_properties]
    impl ObjectImpl for DatabaseItem {}
}

glib::wrapper! {
     pub struct DatabaseItem(ObjectSubclass<imp::DatabaseItem>);
}

impl DatabaseItem {
    pub fn new(key: &glib::Bytes, data: &glib::Bytes) -> Self {
        glib::Object::builder()
            .property("key", key)
            .property("data", data)
            .build()
    }
}
