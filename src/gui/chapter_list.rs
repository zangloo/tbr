use gtk4::{Align, CustomFilter, Filter, FilterListModel, gdk, GestureClick, Image, Label, ListBox, ListBoxRow, Orientation, SelectionMode};
use gtk4::gio::ListStore;
use gtk4::glib::{Cast, Object};
use gtk4::glib;
use gtk4::pango::EllipsizeMode;
use gtk4::prelude::{BoxExt, ListBoxRowExt, ListModelExt, WidgetExt};
use crate::gui::{GuiController, README_TEXT_FILENAME, GuiContext, ChapterListSyncMode};
use crate::ReadingInfo;

pub const BOOK_NAME_LABEL_CLASS: &str = "book-name";
pub const TOC_LABEL_CLASS: &str = "toc";

pub fn create() -> (ListBox, FilterListModel)
{
	let model = ListStore::new::<ChapterListEntry>();
	let chapter_list = ListBox::builder()
		.selection_mode(SelectionMode::Single)
		.build();
	chapter_list.add_css_class("navigation-sidebar");
	chapter_list.add_css_class("boxed-list");
	(chapter_list, FilterListModel::new(Some(model), None::<Filter>))
}

pub(super) fn init(gc: &GuiContext)
{
	let chapter_list = gc.chapter_list();
	let model = gc.chapter_model();
	{
		let gc = gc.clone();
		chapter_list.bind_model(Some(model), move |obj| {
			gtk4::Widget::from(create_list_row(obj, &gc))
		});
	}
	let controller = gc.ctrl();
	load_model(chapter_list, model, &controller, gc);

	let gc = gc.clone();
	chapter_list.connect_row_selected(move |_, row| {
		if gc.is_chapter_syncing() {
			return;
		}
		if let Some(row) = row {
			let row_index = row.index();
			if row_index >= 0 {
				let mut render_context = gc.ctx_mut();
				if let Some(obj) = gc.item(row_index as u32) {
					let entry = entry_cast(&obj);
					let mut controller = gc.ctrl_mut();
					if entry.book() {
						gc.chapter_model().set_filter(None::<&Filter>);
						let new_reading = ReadingInfo::new(&controller.reading.filename)
							.with_inner_book(entry.index() as usize);
						let msg = controller.switch_book(new_reading, &mut render_context);
						gc.update(&msg, ChapterListSyncMode::Reload, &controller);
					} else if let Some(msg) = controller.goto_toc(entry.index() as usize, &mut render_context) {
						gc.message(&msg);
					}
				}
			}
		}
	});
}

pub fn load_model(chapter_list: &ListBox, chapter_model: &FilterListModel,
	controller: &GuiController, gc: &GuiContext)
{
	let model = chapter_model.model().unwrap();
	let model = model.downcast_ref::<ListStore>().unwrap();
	model.remove_all();
	chapter_model.set_filter(None::<&Filter>);

	let current_toc = controller.toc_index();
	let mut current_book_idx = -1;
	let mut current_toc_idx = -1;
	for (index, bn) in controller.container.inner_book_names().iter().enumerate() {
		let bookname = bn.name();
		if bookname == README_TEXT_FILENAME {
			break;
		}
		if index == controller.reading.inner_book {
			current_book_idx = model.n_items() as i32;
			model.append(&ChapterListEntry::new(bookname, index, true, true));
			if let Some(toc) = controller.book.toc_iterator() {
				for (title, value) in toc {
					let reading = value == current_toc;
					if reading {
						current_toc_idx = model.n_items() as i32;
					}
					model.append(&ChapterListEntry::new(title, value, false, reading));
				}
			}
		} else {
			model.append(&ChapterListEntry::new(bookname, index, true, false));
		}
	}
	if let Some(row) = chapter_list.row_at_index(current_toc_idx) {
		chapter_list.select_row(Some(&row));
	}
	if let Some(row) = chapter_list.row_at_index(current_book_idx) {
		row.set_selectable(false);
		let click = GestureClick::builder().button(gdk::BUTTON_PRIMARY).build();
		let gc = gc.clone();
		click.connect_released(move |_, _, _, _, | {
			let chapter_model = gc.chapter_model();
			if chapter_model.filter().is_some() {
				chapter_model.set_filter(None::<&Filter>);
				gc.sync_chapter_list(ChapterListSyncMode::NoReload, &gc.ctrl());
			} else {
				let filter = CustomFilter::new(|obj| {
					obj.downcast_ref::<ChapterListEntry>().unwrap().book()
				});
				chapter_model.set_filter(Some(&filter));
			}
		});
		row.add_controller(click);
	}
}

fn create_list_row(obj: &Object, gc: &GuiContext) -> ListBoxRow
{
	let entry = entry_cast(obj);
	let title = entry.title();
	let label = Label::builder()
		.halign(Align::Start)
		.ellipsize(EllipsizeMode::End)
		.tooltip_text(&title)
		.build();

	let view = gtk4::Box::new(Orientation::Horizontal, 4);
	let is_book = entry.book();
	let icon = if is_book {
		view.add_css_class(BOOK_NAME_LABEL_CLASS);
		label.set_label(&title);
		let icon_name = if entry.reading() {
			"book_reading.svg"
		} else {
			"book_closed.svg"
		};
		Image::from_pixbuf(gc.icons().get(icon_name))
	} else {
		view.add_css_class(TOC_LABEL_CLASS);
		label.set_label(&format!("    {}", title));
		Image::from_pixbuf(gc.icons().get("chapter.svg"))
	};

	view.append(&icon);
	view.append(&label);

	let row = ListBoxRow::new();
	row.set_child(Some(&view));

	row
}

#[inline]
pub fn entry_cast(obj: &Object) -> &ChapterListEntry
{
	obj.downcast_ref::<ChapterListEntry>().expect("Needs to be ChapterListEntry")
}

glib::wrapper! {
    pub struct ChapterListEntry(ObjectSubclass<imp::ChapterListEntry>);
}

impl ChapterListEntry {
	pub fn new(title: &str, index: usize, book: bool, reading: bool) -> Self {
		Object::builder()
			.property("title", title)
			.property("index", index as u64)
			.property("book", book)
			.property("reading", reading)
			.build()
	}
}

mod imp {
	use std::cell::{Cell, RefCell};

	use glib::{ParamSpec, Properties, Value};
	use gtk4::glib;
	use gtk4::prelude::*;
	use gtk4::subclass::prelude::*;

	#[derive(Properties, Default)]
	#[properties(wrapper_type = super::ChapterListEntry)]
	pub struct ChapterListEntry {
		#[property(get, set)]
		title: RefCell<String>,
		#[property(get, set)]
		index: Cell<u64>,
		#[property(get, set)]
		book: Cell<bool>,
		#[property(get, set)]
		reading: Cell<bool>,
	}

	#[glib::object_subclass]
	impl ObjectSubclass for ChapterListEntry {
		const NAME: &'static str = "ChapterListEntry";
		type Type = super::ChapterListEntry;
	}

	// Trait shared by all GObjects
	impl ObjectImpl for ChapterListEntry {
		fn properties() -> &'static [ParamSpec] {
			Self::derived_properties()
		}

		fn set_property(&self, id: usize, value: &Value, pspec: &ParamSpec) {
			self.derived_set_property(id, value, pspec)
		}

		fn property(&self, id: usize, pspec: &ParamSpec) -> Value {
			self.derived_property(id, pspec)
		}
	}
}