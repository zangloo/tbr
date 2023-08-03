use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use gtk4::{AlertDialog, Align, Button, CheckButton, DropDown, EventControllerKey, FileDialog, FileFilter, glib, Label, ListBox, ListBoxRow, Orientation, ScrolledWindow, SelectionMode, Separator, StringList, Window};
use gtk4::gdk::{Key, ModifierType};
use gtk4::gio::{Cancellable, File, ListStore};
use gtk4::glib::{Cast, Object};
use gtk4::prelude::{BoxExt, ButtonExt, CheckButtonExt, FileExt, GtkWindowExt, ListBoxRowExt, ListModelExt, WidgetExt};
use gtk4::subclass::prelude::ObjectSubclassIsExt;
use crate::{I18n, package_name, PathConfig};
use crate::gui::{apply_settings, create_button, FONT_FILE_EXTENSIONS, GuiContext, IconMap};
use crate::gui::dict::DictionaryManager;

pub(super) fn show(gc: &GuiContext, dm: &Rc<RefCell<DictionaryManager>>) -> Window
{
	let i18n = gc.i18n();
	let dialog = Window::builder()
		.title(i18n.msg("settings-dialog-title"))
		.transient_for(gc.win())
		.default_width(500)
		.default_height(500)
		.resizable(false)
		.modal(true)
		.build();

	let main = gtk4::Box::new(Orientation::Vertical, 10);
	main.set_margin_top(10);
	main.set_margin_bottom(10);
	main.set_margin_start(10);
	main.set_margin_end(10);
	dialog.set_child(Some(&main));

	let configuration = gc.cfg();

	let locale_dropdown = {
		let locale_box = gtk4::Box::new(Orientation::Horizontal, 0);
		let locale_list = StringList::default();
		let mut current_locale = 0;
		for (idx, entry) in i18n.locales().iter().enumerate() {
			locale_list.append(&entry.name);
			if entry.locale == configuration.gui.lang {
				current_locale = idx;
			}
		};
		let locale_dropdown = DropDown::builder()
			.margin_start(10)
			.model(&locale_list)
			.selected(current_locale as u32)
			.build();

		locale_box.append(&title_label(&i18n.msg("lang")));
		locale_box.append(&locale_dropdown);
		main.append(&locale_box);
		locale_dropdown
	};

	let font_list = {
		let title = i18n.msg("font-files");
		let (label, view, font_list, font_add_btn) = create_list(
			&title,
			&configuration.gui.fonts,
			gc,
		);
		let font_dialog = FileDialog::new();
		font_dialog.set_title(&title);
		font_dialog.set_modal(true);
		let filter = FileFilter::new();
		for ext in FONT_FILE_EXTENSIONS {
			filter.add_suffix(ext);
		}
		font_dialog.set_default_filter(Some(&filter));
		{
			let font_list = font_list.clone();
			let dialog = dialog.clone();
			font_add_btn.connect_clicked(move |_| {
				let font_list = font_list.clone();
				font_dialog.open_multiple(Some(&dialog), None::<&Cancellable>, move |result| {
					if let Ok(files) = result {
						for i in 0..files.n_items() {
							if let Some(obj) = files.item(i) {
								if let Some(file) = obj.downcast_ref::<File>() {
									if let Some(path) = file.path() {
										check_and_add(&path, &font_list);
									}
								}
							}
						}
					}
				});
			});
		}
		main.append(&label);
		main.append(&view);
		font_list
	};

	let dict_list = {
		let title = i18n.msg("dictionary-folders");
		let (label, view, dict_list, dict_add_btn) = create_list(
			&title,
			&configuration.gui.dictionaries,
			gc,
		);
		let dict_dialog = FileDialog::new();
		dict_dialog.set_title(&title);
		dict_dialog.set_modal(true);
		{
			let dialog = dialog.clone();
			let dict_list = dict_list.clone();
			let gc = gc.clone();
			let title = title.to_string();
			dict_add_btn.connect_clicked(move |_| {
				let dict_list = dict_list.clone();
				let dialog2 = dialog.clone();
				let gc = gc.clone();
				let title = title.clone();
				dict_dialog.select_folder(Some(&dialog), None::<&Cancellable>, move |result| {
					if let Ok(file) = result {
						if let Some(path) = file.path() {
							if stardict::with_sqlite(&path, package_name!()).is_ok() {
								check_and_add(&path, &dict_list);
							} else {
								AlertDialog::builder()
									.modal(true)
									.message(&title)
									.detail(gc.i18n().args_msg("invalid-path", vec![
										("title", title),
										("path", path_str(&path)),
									]))
									.build()
									.show(Some(&dialog2));
							}
						}
					}
				});
			});
		}
		main.append(&label);
		main.append(&view);
		dict_list
	};

	let button_box = gtk4::Box::new(Orientation::Horizontal, 10);
	button_box.set_halign(Align::End);
	{
		let dialog = dialog.clone();
		let ok_btn = Button::builder()
			.label(i18n.msg("ok-title"))
			.build();
		let locale_dropdown = locale_dropdown.clone();
		let gc = gc.clone();
		let dm = dm.clone();
		ok_btn.connect_clicked(move |_| {
			let locale = {
				let idx = locale_dropdown.selected();
				let locales = gc.i18n().locales();
				&locales.get(idx as usize)
					.unwrap_or(locales.get(0).unwrap())
					.locale
			};
			let fonts = collect_path_list(&font_list, |path|
				path.exists() && path.is_file());
			let dictionaries = collect_path_list(&dict_list, |path|
				stardict::with_sqlite(path, package_name!()).is_ok());

			if let Err((title, message)) = apply_settings(
				locale, fonts, dictionaries, &gc,
				&mut dm.borrow_mut()) {
				AlertDialog::builder()
					.modal(true)
					.message(&title)
					.detail(&message)
					.build()
					.show(Some(&dialog));
			} else {
				dialog.close();
			}
		});
		button_box.append(&ok_btn);
	}
	{
		let dialog = dialog.clone();
		let cancel_btn = Button::builder()
			.label(i18n.msg("cancel-title"))
			.build();
		cancel_btn.connect_clicked(move |_| {
			dialog.close();
		});
		button_box.append(&cancel_btn);
	}

	let bottom_box = gtk4::Box::builder()
		.orientation(Orientation::Vertical)
		.spacing(0)
		.vexpand(true)
		.valign(Align::End)
		.build();
	bottom_box.append(&Separator::builder()
		.orientation(Orientation::Horizontal)
		.margin_top(10)
		.margin_bottom(10)
		.build());
	bottom_box.append(&button_box);
	main.append(&bottom_box);

	let key_event = EventControllerKey::new();
	{
		let dialog = dialog.clone();
		key_event.connect_key_pressed(move |_, key, _, modifier| {
			const MODIFIER_NONE: ModifierType = ModifierType::empty();
			match (key, modifier) {
				(Key::Escape, MODIFIER_NONE) => {
					dialog.close();
					glib::Propagation::Stop
				}
				_ => glib::Propagation::Proceed,
			}
		});
	}
	dialog.add_controller(key_event);

	dialog.present();
	dialog
}

#[inline]
fn path_str(path: &PathBuf) -> String {
	if let Some(path_str) = path.to_str() {
		path_str.to_owned()
	} else {
		String::new()
	}
}

#[inline]
fn title_label(title: &str) -> Label
{
	Label::builder()
		.use_markup(true)
		.label(format!("<b>{}</b>", title))
		.halign(Align::Start)
		.build()
}

fn check_and_add(path: &PathBuf, list: &ListStore)
{
	let mut found = false;
	for j in 0..list.n_items() {
		if let Some(obj) = list.item(j) {
			let entry = obj.downcast_ref::<PathConfigEntry>()
				.expect("Needs to be PathConfigEntry");
			let config = entry.imp().path.borrow();
			if config.path == *path {
				found = true;
			}
		}
	}
	if !found {
		let entry = PathConfigEntry::new(true, &path);
		list.append(&entry);
	}
}

fn create_list(title: &str, paths: &Vec<PathConfig>, gc: &GuiContext)
	-> (gtk4::Box, ScrolledWindow, ListStore, Button)
{
	let model = ListStore::new::<PathConfigEntry>();
	for config in paths {
		model.append(&PathConfigEntry::new(config.enabled, &config.path));
	}

	let list = ListBox::builder()
		.selection_mode(SelectionMode::None)
		.build();
	{
		let gc = gc.clone();
		let model_to_remove = model.clone();
		list.bind_model(Some(&model), move |obj| {
			gtk4::Widget::from(create_list_row(
				obj,
				gc.i18n(),
				gc.icons(),
				&model_to_remove,
			))
		});
	}
	let view = ScrolledWindow::builder()
		.child(&list)
		.has_frame(true)
		.max_content_height(120)
		.min_content_height(120)
		.build();
	let list_label = title_label(title);
	let list_add_btn = create_button("add.svg", &gc.i18n().msg("add-title"), gc.icons(), true);
	let label_box = gtk4::Box::builder()
		.orientation(Orientation::Horizontal)
		.spacing(10)
		.margin_top(10)
		.build();
	label_box.append(&list_label);
	label_box.append(&list_add_btn);
	(label_box, view, model, list_add_btn)
}

fn create_list_row(obj: &Object, i18n: &I18n, icons: &IconMap, list: &ListStore)
	-> ListBoxRow
{
	let entry = obj.downcast_ref::<PathConfigEntry>()
		.expect("Needs to be PathConfigEntry");
	let config = entry.imp().path.borrow();
	let remove_btn = create_button("remove.svg", &i18n.msg("remove-title"), icons, true);
	let checkbox = CheckButton::builder()
		.label(&path_str(&config.path))
		.active(config.enabled)
		.build();
	let entry_box = gtk4::Box::new(Orientation::Horizontal, 10);
	entry_box.append(&remove_btn);
	entry_box.append(&checkbox);
	let row = ListBoxRow::new();
	row.set_child(Some(&entry_box));

	{
		let entry = entry.clone();
		checkbox.connect_toggled(move |cb| {
			entry.imp().path.borrow_mut().enabled = cb.is_active();
		});
	}
	{
		let row = row.clone();
		let list = list.clone();
		remove_btn.connect_clicked(move |_| {
			let idx = row.index();
			if idx >= 0 {
				list.remove(idx as u32);
			}
		});
	}
	row
}

fn collect_path_list<F>(list: &ListStore, validator: F) -> Vec<PathConfig>
	where F: Fn(&PathBuf) -> bool
{
	let mut vec = vec![];
	for i in 0..list.n_items() {
		if let Some(obj) = list.item(i) {
			let entry = obj.downcast_ref::<PathConfigEntry>()
				.expect("Needs to be PathConfigEntry");
			let entry: &PathConfig = &entry.imp().path.borrow();
			if validator(&entry.path) {
				vec.push(entry.clone());
			}
		}
	}
	vec
}

glib::wrapper! {
    pub struct PathConfigEntry(ObjectSubclass<imp::PathConfigEntry>);
}

impl PathConfigEntry {
	pub fn new(enabled: bool, path: &PathBuf) -> Self {
		let entry: PathConfigEntry = Object::builder().build();
		let imp = entry.imp();
		let mut config = imp.path.borrow_mut();
		config.enabled = enabled;
		config.path = path.clone();
		drop(config);
		entry
	}
}

mod imp {
	use std::cell::RefCell;
	use gtk4::glib;
	use gtk4::subclass::prelude::*;
	use crate::PathConfig;

	#[derive(Default)]
	pub struct PathConfigEntry {
		pub path: RefCell<PathConfig>,
	}

	#[glib::object_subclass]
	impl ObjectSubclass for PathConfigEntry {
		const NAME: &'static str = "PathConfigEntry";
		type Type = super::PathConfigEntry;
	}

	impl ObjectImpl for PathConfigEntry {}
}