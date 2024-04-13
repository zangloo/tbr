use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use gtk4::{AlertDialog, Align, ApplicationWindow, Button, CheckButton, DropDown, Entry, EventControllerKey, FileDialog, FileFilter, glib, Label, ListBox, ListBoxRow, Orientation, ScrolledWindow, SelectionMode, Separator, StringList, Window};
use gtk4::gdk::Key;
use gtk4::gio::{Cancellable, File, ListStore};
use gtk4::glib::Object;
use gtk4::glib::prelude::Cast;
use gtk4::prelude::{BoxExt, ButtonExt, CheckButtonExt, EditableExt, FileExt, GtkWindowExt, ListBoxRowExt, ListModelExt, WidgetExt};
use gtk4::subclass::prelude::ObjectSubclassIsExt;
use crate::config::{Configuration, PathConfig, SidebarPosition};
use crate::I18n;
use crate::gui::{create_button, DICT_FILE_EXTENSIONS, FONT_FILE_EXTENSIONS, MODIFIER_NONE, IconMap, MIN_FONT_SIZE, MAX_FONT_SIZE, alert, GuiContext, set_sidebar_position, sidebar_updated, font};
use crate::gui::font::UserFonts;

const SIDEBAR_POSITIONS: [SidebarPosition; 2] = [
	SidebarPosition::Left,
	SidebarPosition::Top,
];

pub(super) struct Settings {
	gcs: Rc<RefCell<Vec<GuiContext>>>,
}

impl Settings {
	#[inline]
	pub fn new(gcs: Rc<RefCell<Vec<GuiContext>>>) -> Self
	{
		Settings { gcs }
	}

	#[inline]
	pub fn dialog(&self, gc: &GuiContext)
	{
		let gcs = self.gcs.clone();
		let gc2 = gc.clone();
		show(&gc.cfg, &gc.window, &gc.i18n, &gc.icons, move |params, new_fonts| {
			apply_settings(&gcs, params, new_fonts, &gc2)
		});
	}
}

struct SettingsParam<'a> {
	render_han: bool,
	locale: &'a str,
	fonts: Vec<PathConfig>,
	dictionaries: Vec<PathConfig>,
	cache_dict: bool,
	ignore_font_weight: bool,
	strip_empty_lines: bool,
	scroll_for_page: bool,
	default_font_size: u8,
	sidebar_position: &'a SidebarPosition,
	select_by_dictionary: bool,
}

#[inline]
fn append_checkbox(title: &str, checked: bool, main_box: &gtk4::Box) -> CheckButton
{
	let cb = CheckButton::builder()
		.label(title)
		.active(checked)
		.build();
	main_box.append(&cb);
	cb
}

fn show<F>(cfg: &Rc<RefCell<Configuration>>, window: &ApplicationWindow,
	i18n: &Rc<I18n>, icons: &Rc<IconMap>, apply: F) -> Window
	where F: Fn(SettingsParam, Option<Option<UserFonts>>) + 'static
{
	let dialog = Window::builder()
		.title(i18n.msg("settings-dialog-title"))
		.transient_for(window)
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

	let configuration = cfg.borrow();

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
		locale_box.append(&Label::new(Some(&i18n.msg("need-restart"))));
		main.append(&locale_box);
		locale_dropdown
	};

	let render_han_cb = {
		let b = gtk4::Box::new(Orientation::Horizontal, 10);
		b.append(&title_label(&i18n.msg("settings-render-label")));
		let han = configuration.render_han;
		let han_cb = append_checkbox(
			&i18n.msg("render-han"),
			han,
			&b);
		let xi_cb = append_checkbox(
			&i18n.msg("render-xi"),
			!han,
			&b);
		xi_cb.set_group(Some(&han_cb));
		main.append(&b);
		han_cb
	};

	let ignore_font_weight_cb = append_checkbox(
		&i18n.msg("ignore-font-weight"),
		configuration.gui.ignore_font_weight,
		&main);
	let strip_empty_lines_cb = append_checkbox(
		&i18n.msg("strip-empty-lines"),
		configuration.gui.strip_empty_lines,
		&main);
	let scroll_for_page_cb = append_checkbox(
		&i18n.msg("scroll-for-page"),
		configuration.gui.scroll_for_page,
		&main);

	let sidebar_position_dropdown = {
		let sidebar_position_box = gtk4::Box::new(Orientation::Horizontal, 0);
		let sidebar_position_list = StringList::default();
		let mut current_sidebar_position = 0;
		for (idx, entry) in SIDEBAR_POSITIONS.iter().enumerate() {
			sidebar_position_list.append(&i18n.msg(entry.i18n_key()));
			if *entry == configuration.gui.sidebar_position {
				current_sidebar_position = idx;
			}
		};
		let sidebar_position_dropdown = DropDown::builder()
			.margin_start(10)
			.model(&sidebar_position_list)
			.selected(current_sidebar_position as u32)
			.build();

		sidebar_position_box.append(&title_label(&i18n.msg("sidebar-position")));
		sidebar_position_box.append(&sidebar_position_dropdown);
		main.append(&sidebar_position_box);
		main.append(&sidebar_position_dropdown);
		sidebar_position_dropdown
	};

	let font_size_entry = {
		let entry = Entry::builder()
			.text(&format!("{}", configuration.gui.default_font_size))
			.build();

		let fs_box = gtk4::Box::new(Orientation::Horizontal, 10);
		fs_box.append(&title_label(&i18n.msg("default-font-size")));
		fs_box.append(&entry);
		fs_box.append(&Label::builder()
			.label(&format!("({} - {})", MIN_FONT_SIZE, MAX_FONT_SIZE))
			.build());

		main.append(&fs_box);
		entry
	};

	let font_list = {
		let title = i18n.msg("font-files");
		let (label, view, font_list, font_add_btn) = create_list(
			&title,
			&configuration.gui.fonts,
			i18n,
			icons,
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
		let title = i18n.msg("dictionary-file");
		let (label, view, dict_list, dict_add_btn) = create_list(
			&title,
			&configuration.gui.dictionaries,
			i18n,
			icons,
		);
		let dict_dialog = FileDialog::new();
		dict_dialog.set_title(&title);
		dict_dialog.set_modal(true);
		let filter = FileFilter::new();
		for ext in DICT_FILE_EXTENSIONS {
			filter.add_suffix(ext);
		}
		dict_dialog.set_default_filter(Some(&filter));
		{
			let dialog = dialog.clone();
			let dict_list = dict_list.clone();
			let title = title.to_string();
			let i18n = i18n.clone();
			dict_add_btn.connect_clicked(move |_| {
				let dict_list = dict_list.clone();
				let dialog2 = dialog.clone();
				let title = title.clone();
				let i18n = i18n.clone();
				dict_dialog.open(Some(&dialog), None::<&Cancellable>, move |result| {
					if let Ok(file) = result {
						if let Some(path) = file.path() {
							if stardict::no_cache(&path).is_ok() {
								check_and_add(&path, &dict_list);
							} else {
								AlertDialog::builder()
									.modal(true)
									.message(&title)
									.detail(i18n.args_msg("invalid-path", vec![
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

	let cache_dict_cb = append_checkbox(
		&i18n.msg("cache-dictionary"),
		configuration.gui.cache_dict,
		&main);

	let disable_select_by_dictionary = dict_list.n_items() == 0;
	let select_by_dictionary_cb = append_checkbox(
		&i18n.msg("select-by-dictionary"),
		if disable_select_by_dictionary { false } else { configuration.gui.select_by_dictionary },
		&main);
	if disable_select_by_dictionary {
		select_by_dictionary_cb.set_sensitive(false);
	}
	{
		let select_by_dictionary_cb = select_by_dictionary_cb.clone();
		dict_list.connect_items_changed(move |list, _, _, _| {
			if list.n_items() == 0 {
				select_by_dictionary_cb.set_sensitive(false);
				select_by_dictionary_cb.set_active(false);
			} else {
				select_by_dictionary_cb.set_sensitive(true);
			}
		});
	}

	let button_box = gtk4::Box::new(Orientation::Horizontal, 10);
	button_box.set_halign(Align::End);
	{
		let dialog = dialog.clone();
		let ok_btn = Button::builder()
			.label(i18n.msg("ok-title"))
			.build();
		let locale_dropdown = locale_dropdown.clone();
		let i18n = i18n.clone();
		let cfg = cfg.clone();
		ok_btn.connect_clicked(move |_| {
			let default_font_size = if let Ok(default_font_size) = font_size_entry
				.text()
				.to_string()
				.trim()
				.parse() {
				if default_font_size < MIN_FONT_SIZE || default_font_size > MAX_FONT_SIZE {
					alert(&i18n.msg("alert-error-title"), &i18n.msg("invalid-default-font-size"), &dialog);
					return;
				}
				default_font_size
			} else {
				alert(&i18n.msg("alert-error-title"), &i18n.msg("invalid-default-font-size"), &dialog);
				return;
			};
			let render_han = render_han_cb.is_active();
			let locale = {
				let idx = locale_dropdown.selected();
				let locales = i18n.locales();
				&locales.get(idx as usize)
					.unwrap_or(locales.get(0).unwrap())
					.locale
			};
			let ignore_font_weight = ignore_font_weight_cb.is_active();
			let strip_empty_lines = strip_empty_lines_cb.is_active();
			let scroll_for_page = scroll_for_page_cb.is_active();
			let fonts = collect_path_list(&font_list, |path|
				path.exists() && path.is_file());
			let dictionaries = collect_path_list(&dict_list, |path|
				stardict::no_cache(path).is_ok());
			let cache_dict = cache_dict_cb.is_active();
			let sidebar_position = {
				let idx = sidebar_position_dropdown.selected();
				&SIDEBAR_POSITIONS[idx as usize]
			};
			let select_by_dictionary = select_by_dictionary_cb.is_active();

			let new_fonts = if paths_modified(&cfg.borrow().gui.fonts, &fonts) {
				let new_fonts = match font::user_fonts(&fonts) {
					Ok(fonts) => fonts,
					Err(err) => {
						let title = i18n.msg("font-files");
						let t = title.to_string();
						let message = i18n.args_msg("invalid-path", vec![
							("title", title),
							("path", err.to_string().into()),
						]);
						alert(&t, &message, &dialog);
						return;
					}
				};
				Some(new_fonts)
			} else {
				None
			};
			let params = SettingsParam {
				render_han,
				locale,
				fonts,
				dictionaries,
				cache_dict,
				ignore_font_weight,
				strip_empty_lines,
				scroll_for_page,
				default_font_size,
				sidebar_position,
				select_by_dictionary,
			};
			apply(params, new_fonts);
			dialog.close();
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

fn create_list(title: &str, paths: &Vec<PathConfig>, i18n: &Rc<I18n>,
	icons: &Rc<IconMap>) -> (gtk4::Box, ScrolledWindow, ListStore, Button)
{
	let model = ListStore::new::<PathConfigEntry>();
	for config in paths {
		model.append(&PathConfigEntry::new(config.enabled, &config.path));
	}

	let list = ListBox::builder()
		.selection_mode(SelectionMode::None)
		.build();
	{
		let i18n = i18n.clone();
		let icons = icons.clone();
		let model_to_remove = model.clone();
		list.bind_model(Some(&model), move |obj| {
			gtk4::Widget::from(create_list_row(
				obj,
				&i18n,
				&icons,
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
	let list_add_btn = create_button("add.svg", Some(&i18n.msg("add-title")), icons, true);
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
	let remove_btn = create_button("remove.svg", Some(&i18n.msg("remove-title")), icons, true);
	let entry_box = gtk4::Box::new(Orientation::Horizontal, 10);
	entry_box.append(&remove_btn);
	let checkbox = append_checkbox(&path_str(&config.path), config.enabled, &entry_box);
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

fn paths_modified(orig: &Vec<PathConfig>, new: &Vec<PathConfig>) -> bool
{
	if new.len() != orig.len() {
		return true;
	}
	for i in 0..new.len() {
		let orig = orig.get(i).unwrap();
		let new = new.get(i).unwrap();
		if orig.enabled != new.enabled || orig.path != new.path {
			return true;
		}
	}
	false
}

fn apply_settings(gcs: &Rc<RefCell<Vec<GuiContext>>>, params: SettingsParam,
	new_fonts: Option<Option<UserFonts>>, gc: &GuiContext)
{
	let gui_contexts = gcs.borrow();
	let mut configuration = gc.cfg_mut();

	// need restart
	configuration.gui.lang = params.locale.to_owned();

	let mut redraw = false;
	let reload_render = if configuration.render_han != params.render_han {
		configuration.render_han = params.render_han;
		redraw = true;
		true
	} else {
		false
	};

	configuration.gui.scroll_for_page = params.scroll_for_page;
	configuration.gui.default_font_size = params.default_font_size;
	configuration.gui.select_by_dictionary = params.select_by_dictionary;

	if configuration.gui.ignore_font_weight != params.ignore_font_weight {
		configuration.gui.ignore_font_weight = params.ignore_font_weight;
		redraw = true;
	};
	if configuration.gui.strip_empty_lines != params.strip_empty_lines {
		configuration.gui.strip_empty_lines = params.strip_empty_lines;
		redraw = true;
	};
	if configuration.gui.sidebar_position != *params.sidebar_position {
		configuration.gui.sidebar_position = params.sidebar_position.clone();
		set_sidebar_position(gc, &configuration.gui.sidebar_position);
		let position = gc.paned.position();
		if position > 0 {
			sidebar_updated(&mut configuration, &mut gc.dm_mut(), position);
			redraw = true;
		}
	}

	if new_fonts.is_some() {
		redraw = true;
	}

	let lookup_for_reload = if paths_modified(&configuration.gui.dictionaries, &params.dictionaries)
		|| configuration.gui.cache_dict != params.cache_dict {
		configuration.gui.dictionaries = params.dictionaries;
		configuration.gui.cache_dict = params.cache_dict;
		gc.db.borrow_mut().reload(&configuration.gui.dictionaries, params.cache_dict);
		true
	} else {
		false
	};

	if lookup_for_reload {
		for gc in gui_contexts.iter() {
			gc.dm_mut().lookup_for_reload();
		}
	}

	if redraw {
		let (set_fonts, fonts_data) = if let Some(new_fonts) = new_fonts {
			let fonts_data = Rc::new(new_fonts);
			configuration.gui.fonts = params.fonts;
			(true, fonts_data)
		} else {
			(false, Rc::new(None))
		};
		for gc in gui_contexts.iter() {
			let mut render_context = gc.ctx_mut();
			let mut controller = gc.ctrl_mut();
			if reload_render {
				controller.render.reload_render(configuration.render_han, &mut render_context);
			}
			if set_fonts {
				gc.dm_mut().set_fonts(fonts_data.clone());
				controller.render.set_fonts(controller.book.custom_fonts(), fonts_data.clone(), &mut render_context);
			}
			render_context.ignore_font_weight = params.ignore_font_weight;
			render_context.strip_empty_lines = params.strip_empty_lines;
			controller.redraw(&mut render_context);
		}
	}
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
	use crate::config::PathConfig;

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