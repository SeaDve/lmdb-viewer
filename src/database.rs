use anyhow::{anyhow, Context, Result};
use gtk::{gio, glib, prelude::*, subclass::prelude::*};
use heed::types::ByteSlice;
use indexmap::IndexMap;

use crate::database_item::DatabaseItem;

type Inner = heed::Database<ByteSlice, ByteSlice>;

mod imp {
    use std::cell::{OnceCell, RefCell};

    use super::*;

    #[derive(Default)]
    pub struct Database {
        pub(super) env: OnceCell<heed::Env>,
        pub(super) inner: OnceCell<Inner>,
        pub(super) items: RefCell<IndexMap<glib::Bytes, DatabaseItem>>,
        pub(super) name: OnceCell<Option<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Database {
        const NAME: &'static str = "LvDatabase";
        type Type = super::Database;
        type Interfaces = (gio::ListModel,);
    }

    impl ObjectImpl for Database {}

    impl ListModelImpl for Database {
        fn item_type(&self) -> glib::Type {
            DatabaseItem::static_type()
        }

        fn n_items(&self) -> u32 {
            self.items.borrow().len() as u32
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.items
                .borrow()
                .get_index(position as usize)
                .map(|(_, v)| v.upcast_ref::<glib::Object>())
                .cloned()
        }
    }
}

glib::wrapper! {
     pub struct Database(ObjectSubclass<imp::Database>)
        @implements gio::ListModel;
}

impl Database {
    pub fn load(env: &heed::Env, name: Option<&str>) -> Result<Self> {
        let this = glib::Object::new::<Self>();

        let rtxn = env.read_txn()?;
        let db = env
            .open_database(&rtxn, name)?
            .ok_or_else(|| anyhow!("database not found"))?;
        let items = db
            .iter(&rtxn)?
            .map(|item| {
                let (key, data) = item?;
                let key = glib::Bytes::from(key);
                let data = glib::Bytes::from(data);
                let item = DatabaseItem::new(&key, &data);
                Ok::<_, heed::Error>((key, item))
            })
            .collect::<Result<IndexMap<_, _>, _>>()?;

        let imp = this.imp();
        imp.inner.set(db).unwrap();
        imp.env.set(env.clone()).unwrap();
        imp.items.replace(items);
        imp.name.set(name.map(|s| s.to_string())).unwrap();

        Ok(this)
    }

    pub fn reload(&self) -> Result<()> {
        let env = self.env();
        let db = self.inner();

        let prev_len = self.n_items();

        // TODO update only what changed
        let rtxn = env.read_txn().context("Failed to create read txn")?;
        let items = db
            .iter(&rtxn)
            .context("Failed to iter db")?
            .map(|item| {
                let (key, val) = item?;
                let key = glib::Bytes::from(key);
                let val = glib::Bytes::from(val);
                let item = DatabaseItem::new(&key, &val);
                Ok::<_, heed::Error>((key, item))
            })
            .collect::<Result<IndexMap<_, _>, _>>()
            .context("Failed to collect db")?;

        let imp = self.imp();
        imp.items.replace(items);

        let new_len = self.n_items();

        dbg!(prev_len, new_len);

        match new_len.cmp(&prev_len) {
            std::cmp::Ordering::Less => self.items_changed(0, new_len, prev_len),
            std::cmp::Ordering::Equal => self.items_changed(0, prev_len, prev_len),
            std::cmp::Ordering::Greater => self.items_changed(0, prev_len, new_len),
        }

        Ok(())
    }

    fn env(&self) -> &heed::Env {
        self.imp().env.get().unwrap()
    }

    fn inner(&self) -> &Inner {
        self.imp().inner.get().unwrap()
    }
}
