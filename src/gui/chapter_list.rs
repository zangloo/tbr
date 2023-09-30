use std::cell::{Cell, Ref, RefCell};
use std::rc::Rc;
use gtk4::{Align, CustomFilter, Filter, FilterChange, FilterListModel, gdk, GestureClick, Image, Label, ListBox, ListBoxRow, Orientation, PolicyType, SearchEntry, SelectionMode};
use gtk4::gio::ListStore;
use gtk4::glib::{Cast, Object};
use gtk4::glib;
use gtk4::pango::EllipsizeMode;
use gtk4::prelude::{AdjustmentExt, BoxExt, EditableExt, FilterExt, ListBoxRowExt, ListModelExt, WidgetExt};
use crate::gui::{GuiController, README_TEXT_FILENAME, ChapterListSyncMode, IconMap};

pub const BOOK_NAME_LABEL_CLASS: &str = "book-name";
pub const TOC_LABEL_CLASS: &str = "toc";

pub struct ChapterListInner {
	collapse: Rc<Cell<bool>>,
	list: ListBox,
	model: FilterListModel,
	ctrl: Rc<RefCell<GuiController>>,
	syncing: Rc<Cell<bool>>,
}

#[derive(Clone)]
pub struct ChapterList {
	inner: Rc<ChapterListInner>,
}

impl ChapterList {
	pub fn create<F>(icons: &Rc<IconMap>, ctrl: &Rc<RefCell<GuiController>>,
		item_clicked: F) -> (Self, gtk4::Box)
		where F: Fn(bool, usize) + 'static
	{
		let model = ListStore::new::<ChapterListEntry>();
		let model = FilterListModel::new(Some(model), None::<Filter>);
		let list = ListBox::builder()
			.selection_mode(SelectionMode::Single)
			.build();
		list.add_css_class("navigation-sidebar");
		list.add_css_class("boxed-list");
		{
			let icons = icons.clone();
			list.bind_model(Some(&model), move |obj| {
				gtk4::Widget::from(create_list_row(obj, &icons))
			});
		}
		let syncing = Default::default();
		let collapse = Rc::new(Cell::new(false));
		let filter_input = SearchEntry::new();
		let filter = {
			let collapse = collapse.clone();
			let filter_input = filter_input.clone();
			let filter = CustomFilter::new(move |obj| {
				let entry = obj.downcast_ref::<ChapterListEntry>().unwrap();
				if collapse.get() && !entry.book() {
					return false;
				}
				let text = filter_input.text();
				let str = text.as_str().trim();
				if str.is_empty() {
					true
				} else {
					entry.title()
						.to_lowercase()
						.contains(&str.to_lowercase())
				}
			});
			model.set_filter(Some(&filter));
			filter
		};
		{
			filter_input.connect_search_changed(move |_| {
				filter.changed(FilterChange::Different);
			});
		}

		let container = gtk4::Box::builder()
			.orientation(Orientation::Vertical)
			.spacing(0)
			.vexpand(true)
			.build();
		container.append(&filter_input);
		container.append(&gtk4::ScrolledWindow::builder()
			.child(&list)
			.hscrollbar_policy(PolicyType::Never)
			.vexpand(true)
			.build());

		let chapter_list = ChapterList {
			inner: Rc::new(ChapterListInner {
				collapse,
				list,
				model,
				ctrl: ctrl.clone(),
				syncing,
			})
		};
		load_model(&chapter_list);

		{
			let chapter_list2 = chapter_list.clone();
			chapter_list.inner.list.connect_row_selected(move |_, row| {
				if chapter_list2.inner.syncing.get() {
					return;
				}
				if let Some(row) = row {
					let model = &chapter_list2.inner.model;
					let row_index = row.index();
					if row_index >= 0 {
						if let Some(obj) = model.item(row_index as u32) {
							let entry = entry_cast(&obj);
							if entry.book() {
								chapter_list2.collapse(!chapter_list2.inner.collapse.get());
								item_clicked(true, entry.index() as usize);
								chapter_list2.sync_chapter_list(ChapterListSyncMode::Reload);
							} else {
								item_clicked(false, entry.index() as usize);
							}
						}
					}
				}
			})
		};

		(chapter_list, container)
	}

	#[inline]
	pub fn collapse(&self, yes: bool)
	{
		self.inner.collapse.replace(yes);
		self.inner.model.filter().unwrap().changed(if yes {
			FilterChange::MoreStrict
		} else {
			FilterChange::LessStrict
		});
	}

	#[inline]
	pub fn block_reactive(&self, block: bool)
	{
		self.inner.syncing.replace(block);
	}

	pub fn scroll_to_current(&self)
	{
		let list = &self.inner.list;
		if let Some(row) = list.selected_row() {
			if let Some((_, y)) = row.translate_coordinates(list, 0., 0.) {
				if let Some(adj) = list.adjustment() {
					let (_, height) = row.preferred_size();
					adj.set_value(y - (adj.page_size() - height.height() as f64) / 2.);
				}
			}
		}
	}

	pub(super) fn sync_chapter_list(&self, sync_mode: ChapterListSyncMode)
	{
		#[inline]
		fn do_sync(sync_mode: ChapterListSyncMode, chapter_list: &ChapterList,
			controller: &GuiController)
		{
			let inner_book = controller.reading.inner_book;
			if match sync_mode {
				ChapterListSyncMode::NoReload => false,
				ChapterListSyncMode::Reload => true,
				ChapterListSyncMode::ReloadIfNeeded(orig_inner_book) => orig_inner_book != inner_book,
			} {
				load_model(chapter_list);
				return;
			}

			let list = &chapter_list.inner.list;
			let model = &chapter_list.inner.model;
			let toc_index = controller.toc_index() as u64;
			if let Some(row) = list.selected_row() {
				let index = row.index();
				if index >= 0 {
					if let Some(obj) = model.item(index as u32) {
						let entry = entry_cast(&obj);
						if entry.index() == toc_index {
							return;
						}
					}
				}
			}

			for i in 0..model.n_items() {
				if let Some(obj) = model.item(i) {
					let entry = entry_cast(&obj);
					if !entry.book() && entry.index() == toc_index {
						if let Some(row) = list.row_at_index(i as i32) {
							list.select_row(Some(&row));
						}
					}
				}
			}
		}
		self.block_reactive(true);
		do_sync(sync_mode, &self, &self.ctrl());
		self.scroll_to_current();
		self.block_reactive(false);
	}

	#[inline]
	fn ctrl(&self) -> Ref<GuiController>
	{
		self.inner.ctrl.borrow()
	}
}

pub fn load_model(chapter_list: &ChapterList)
{
	chapter_list.collapse(false);
	let model = &chapter_list.inner.model;
	let model = model.model().unwrap();
	let model = model
		.downcast_ref::<ListStore>()
		.unwrap();
	model.remove_all();

	let controller = chapter_list.ctrl();
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
	let list = &chapter_list.inner.list;
	if let Some(row) = list.row_at_index(current_toc_idx) {
		list.select_row(Some(&row));
	}
	if let Some(row) = list.row_at_index(current_book_idx) {
		row.set_selectable(false);
		let click = GestureClick::builder().button(gdk::BUTTON_PRIMARY).build();
		let chapter_list = chapter_list.clone();
		click.connect_released(move |_, _, _, _, | {
			let collapse = !chapter_list.inner.collapse.get();
			if collapse {
				chapter_list.collapse(true);
			} else {
				chapter_list.collapse(false);
				chapter_list.sync_chapter_list(ChapterListSyncMode::NoReload);
			}
		});
		row.add_controller(click);
	}
}

fn create_list_row(obj: &Object, icons: &IconMap) -> ListBoxRow
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
		Image::from_pixbuf(icons.get(icon_name))
	} else {
		view.add_css_class(TOC_LABEL_CLASS);
		label.set_label(&format!("    {}", title));
		Image::from_pixbuf(icons.get("chapter.svg"))
	};

	view.append(&icon);
	view.append(&label);

	let row = ListBoxRow::new();
	row.set_child(Some(&view));

	row
}

#[inline]
fn entry_cast(obj: &Object) -> &ChapterListEntry
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