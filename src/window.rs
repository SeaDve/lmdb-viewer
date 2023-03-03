use adw::{prelude::*, subclass::prelude::*};
use anyhow::{Context, Result};
use gettextrs::gettext;
use gtk::{
    gio,
    glib::{self, clone, closure},
};
use heed::{Env, EnvFlags};

use std::cell::RefCell;

use crate::{
    application::Application,
    config::{APP_ID, PROFILE},
    database::Database,
    database_item::DatabaseItem,
};

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/seadve/LmdbViewer/ui/window.ui")]
    pub struct Window {
        #[template_child]
        pub(super) drop_down: TemplateChild<gtk::DropDown>,
        #[template_child]
        pub(super) column_view: TemplateChild<gtk::ColumnView>,
        #[template_child]
        pub(super) column_view_model: TemplateChild<gtk::NoSelection>,

        pub(super) env: RefCell<Option<Env>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "LvWindow";
        type Type = super::Window;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();

            klass.install_action_async("win.open-database", None, |obj, _, _| async move {
                if let Err(err) = obj.open_database().await {
                    tracing::error!("Failed to open database: {:?}", &err);

                    let dialog = adw::MessageDialog::builder()
                        .heading(gettext("Cannot Open Database"))
                        .body(err.to_string())
                        .transient_for(&obj)
                        .modal(true)
                        .build();
                    dialog.add_response("ok", &gettext("Ok, Got It"));
                    dialog.set_default_response(Some("ok"));
                    dialog.present();
                }
            });

            klass.install_action("win.update-database", None, move |obj, _, _| {
                let imp = obj.imp();

                if let Some(model) = imp.drop_down.model() {
                    let db = model.downcast_ref::<Database>().unwrap();

                    if let Err(err) = db.reload() {
                        tracing::error!("Failed to reload database on drop down: {:?}", &err);
                    }
                }

                if let Some(model) = imp.column_view_model.model() {
                    let db = model.downcast_ref::<Database>().unwrap();

                    if let Err(err) = db.reload() {
                        tracing::error!("Failed to reload database on view: {:?}", &err);
                    }
                }
            });
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for Window {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            if PROFILE == "Devel" {
                obj.add_css_class("devel");
            }

            obj.setup_view();

            obj.load_window_size();
        }
    }

    impl WidgetImpl for Window {}

    impl WindowImpl for Window {
        fn close_request(&self) -> glib::Propagation {
            if let Err(err) = self.obj().save_window_size() {
                tracing::warn!("Failed to save window state: {:?}", &err);
            }

            self.parent_close_request()
        }
    }

    impl ApplicationWindowImpl for Window {}
    impl AdwApplicationWindowImpl for Window {}
}

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionMap, gio::ActionGroup, gtk::Root;
}

impl Window {
    pub fn new(app: &Application) -> Self {
        glib::Object::builder().property("application", app).build()
    }

    async fn open_database(&self) -> Result<()> {
        let imp = self.imp();

        let dialog = gtk::FileDialog::builder()
            .title("Open Database")
            .modal(true)
            .build();

        let res = dialog.select_folder_future(Some(self)).await;

        let db_dir = match res {
            Ok(db_dir) => db_dir,
            Err(err) => {
                if err.matches(gtk::DialogError::Dismissed)
                    | err.matches(gtk::DialogError::Cancelled)
                {
                    return Ok(());
                } else {
                    return Err(err.into());
                }
            }
        };

        let env = unsafe {
            heed::EnvOpenOptions::new()
                .map_size(100 * 1024 * 1024) // 100 MiB
                .max_dbs(100)
                .flags(EnvFlags::READ_ONLY | EnvFlags::NO_LOCK)
                .open(db_dir.path().expect("file must have a path"))
                .with_context(|| format!("Failed to open env at `{}`", db_dir.uri()))?
        };
        tracing::debug!("Opened env at `{}`", db_dir.uri());

        let db = Database::load(&env, None).context("Failed to load unnamed db")?;
        imp.drop_down.set_model(Some(&db));

        imp.env.replace(Some(env));

        Ok(())
    }

    fn save_window_size(&self) -> Result<(), glib::BoolError> {
        let settings = gio::Settings::new(APP_ID);

        let (width, height) = self.default_size();

        settings.set_int("window-width", width)?;
        settings.set_int("window-height", height)?;

        settings.set_boolean("is-maximized", self.is_maximized())?;

        Ok(())
    }

    fn load_window_size(&self) {
        let settings = gio::Settings::new(APP_ID);

        let width = settings.int("window-width");
        let height = settings.int("window-height");
        let is_maximized = settings.boolean("is-maximized");

        self.set_default_size(width, height);

        if is_maximized {
            self.maximize();
        }
    }

    fn setup_view(&self) {
        let imp = self.imp();

        let key_column_factory = gtk::SignalListItemFactory::new();
        key_column_factory.connect_setup(|_, list_item| {
            let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
            let buffer = gtk::TextBuffer::builder().build();
            let text_view = gtk::TextView::builder().buffer(&buffer).monospace(true).build();
            list_item.connect_item_notify(clone!(@weak buffer => move |item| {
                if let Some(item) = item.item() {
                    let item = item.downcast_ref::<DatabaseItem>().unwrap();
                    buffer.set_text(&String::from_utf8_lossy(item.key().as_ref()).replace('\x00', "0"));
                } else {
                    buffer.set_text("<None>");
                }
            }));
            list_item.set_child(Some(&text_view));
        });
        let key_column = gtk::ColumnViewColumn::new(Some("Key"), Some(key_column_factory));
        key_column.set_resizable(true);
        key_column.set_expand(true);
        imp.column_view.insert_column(0, &key_column);

        let val_column_factory = gtk::SignalListItemFactory::new();
        val_column_factory.connect_setup(|_, list_item| {
                let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
                let buffer = gtk::TextBuffer::builder().build();
                let text_view = gtk::TextView::builder().buffer(&buffer).monospace(true).build();
                list_item.connect_item_notify(clone!(@weak buffer => move |item| {
                    if let Some(item) = item.item() {
                        let item = item.downcast_ref::<DatabaseItem>().unwrap();
                        buffer.set_text(&String::from_utf8_lossy(item.data().as_ref()).replace('\x00', "0"));
                    } else {
                        buffer.set_text("<None>");
                    }
                }));
                list_item.set_child(Some(&text_view));
            });
        let val_column = gtk::ColumnViewColumn::new(Some("Value"), Some(val_column_factory));
        val_column.set_resizable(true);
        val_column.set_expand(true);
        imp.column_view.insert_column(1, &val_column);

        imp.drop_down
            .set_expression(Some(&gtk::ClosureExpression::new::<glib::GString>(
                &[] as &[gtk::Expression],
                closure!(|list_item: DatabaseItem| {
                    String::from_utf8_lossy(list_item.key().as_ref()).to_string()
                }),
            )));
        imp.drop_down
            .connect_selected_item_notify(clone!(@weak self as obj => move |drop_down| {
                let imp = obj.imp();
                let env = imp.env.borrow();

                if let Some(env) = env.as_ref() {
                    let selected_item = drop_down.selected_item();

                    if let Some(item) = selected_item {
                        let item = item.downcast_ref::<DatabaseItem>().unwrap();
                        let item_key = item.key();
                        let db_name = std::str::from_utf8(&item_key).unwrap();
                        let db = Database::load(env, Some(db_name)).unwrap();
                        imp.column_view_model.set_model(Some(&db));
                    } else {
                        imp.column_view_model.set_model(gtk::SelectionModel::NONE);
                    }
                } else {
                    tracing::error!("No env set!");
                }
            }));
    }
}
