use std::cell::{Cell, Ref, RefCell};
use std::rc::Rc;
use gtk4::{Align, gdk, GestureClick, Label, ListBox, ListBoxRow, Orientation, PolicyType, SearchEntry, SelectionMode};
use gtk4::graphene::Point;
use gtk4::pango::EllipsizeMode;
use gtk4::prelude::{AdjustmentExt, BoxExt, EditableExt, ListBoxRowExt, WidgetExt};
use crate::gui::{GuiController, README_TEXT_FILENAME, ChapterListSyncMode, IconMap, load_button_image};

pub const BOOK_NAME_LABEL_CLASS: &str = "book-name";
pub const TOC_LABEL_CLASS: &str = "toc";

struct ChapterListEntry {
	title: String,
	book: bool,
	index: usize,
	reading: bool,
}

impl ChapterListEntry {
	pub fn new(title: &str, index: usize, book: bool, reading: bool) -> Self
	{
		ChapterListEntry {
			title: title.to_owned(),
			book,
			index,
			reading,
		}
	}
}

struct ChapterListInner {
	collapse: Cell<bool>,
	list: ListBox,
	ctrl: Rc<RefCell<GuiController>>,
	syncing: Cell<bool>,
	rows: RefCell<Vec<ChapterListEntry>>,
	icons: Rc<IconMap>,
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
		let list = ListBox::builder()
			.selection_mode(SelectionMode::Single)
			.build();
		list.add_css_class("navigation-sidebar");
		list.add_css_class("boxed-list");

		let rows = RefCell::new(vec![]);
		let syncing = Default::default();
		let collapse = Cell::new(false);
		let filter_input = SearchEntry::new();
		let filter_pattern = Rc::new(RefCell::new(String::new()));
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
				ctrl: ctrl.clone(),
				syncing,
				rows,
				icons: icons.clone(),
			})
		};
		load_entries(&chapter_list);

		{
			let chapter_list = chapter_list.clone();
			let filter_pattern = filter_pattern.clone();
			filter_input.connect_search_changed(move |input| {
				let text = input.text();
				let str = text.as_str().trim();
				filter_pattern.replace(str.to_lowercase());
				chapter_list.inner.list.invalidate_filter();
			});
		}
		{
			let chapter_list2 = chapter_list.clone();
			chapter_list.inner.list.set_filter_func(move |row| {
				let row_index = row.index();
				if row_index >= 0 {
					if let Some(entry) = chapter_list2.inner.rows.borrow().get(row_index as usize) {
						if chapter_list2.inner.collapse.get() && !entry.book {
							return false;
						}
						let pattern: &String = &filter_pattern.borrow();
						if pattern.is_empty() {
							true
						} else {
							entry.title
								.to_lowercase()
								.contains(pattern)
						}
					} else {
						true
					}
				} else {
					true
				}
			});
		}
		{
			let chapter_list2 = chapter_list.clone();
			chapter_list.inner.list.connect_row_selected(move |_, row| {
				if chapter_list2.inner.syncing.get() {
					return;
				}
				if let Some(row) = row {
					let entries = chapter_list2.inner.rows.borrow();
					let row_index = row.index();
					if row_index >= 0 {
						if let Some(entry) = entries.get(row_index as usize) {
							let index = entry.index;
							let is_book = entry.book;
							drop(entries);
							if is_book {
								chapter_list2.collapse(!chapter_list2.inner.collapse.get());
								item_clicked(true, index);
								chapter_list2.sync_chapter_list(ChapterListSyncMode::Reload);
							} else {
								item_clicked(false, index);
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
		self.inner.list.invalidate_filter();
	}

	#[inline]
	pub fn block_reactive(&self, block: bool)
	{
		self.inner.syncing.replace(block);
	}

	#[inline]
	pub fn scroll_to_current(&self)
	{
		let list = &self.inner.list;
		if let Some(row) = list.selected_row() {
			if let Some(point) = row.compute_point(list, &Point::new(0., 0.)) {
				if let Some(adj) = list.adjustment() {
					let (_, height) = row.preferred_size();
					adj.set_value(point.y() as f64 - (adj.page_size() - height.height() as f64) / 2.);
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
				load_entries(chapter_list);
				return;
			}

			let list = &chapter_list.inner.list;
			let entries = &chapter_list.inner.rows.borrow();
			let toc_index = controller.toc_index();
			if let Some(row) = list.selected_row() {
				let index = row.index();
				if index >= 0 {
					if let Some(entry) = entries.get(index as usize) {
						if entry.index == toc_index {
							return;
						}
					}
				}
			}

			for i in 0..entries.len() {
				let entry = &entries[i];
				if !entry.book && entry.index == toc_index {
					if let Some(row) = list.row_at_index(i as i32) {
						list.select_row(Some(&row));
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


pub fn load_entries(chapter_list: &ChapterList)
{
	chapter_list.inner.collapse.replace(false);
	let mut entries = chapter_list.inner.rows.borrow_mut();
	entries.clear();

	let list = &chapter_list.inner.list;
	let controller = chapter_list.ctrl();
	let icons = &chapter_list.inner.icons;
	let current_toc = controller.toc_index();
	let mut current_book_idx = None;
	let mut current_book_collapsable = true;
	let mut selected_index = None;
	for (index, bn) in controller.container.inner_book_names().iter().enumerate() {
		let bookname = bn.name();
		if bookname == README_TEXT_FILENAME {
			break;
		}
		if index == controller.reading.inner_book {
			current_book_idx = Some(entries.len());
			entries.push(ChapterListEntry::new(bookname, index, true, true));
			if let Some(toc) = controller.book.toc_iterator() {
				for (title, value) in toc {
					let reading = value == current_toc;
					if reading {
						selected_index = Some(entries.len());
					}
					entries.push(ChapterListEntry::new(title, value, false, reading));
				}
			} else {
				selected_index = Some(entries.len() - 1);
				current_book_collapsable = false;
			}
		} else {
			entries.push(ChapterListEntry::new(bookname, index, true, false));
		}
	}
	let mut rows = vec![];
	for entry in entries.iter() {
		let row = create_list_row(&entry, icons);
		rows.push(row);
	}
	drop(entries);
	list.remove_all();
	for row in rows {
		list.append(&row);
	}
	if let Some(selected_index) = selected_index {
		if let Some(row) = list.row_at_index(selected_index as i32) {
			list.select_row(Some(&row));
		}
	}
	if let Some(current_book_idx) = current_book_idx {
		if let Some(row) = list.row_at_index(current_book_idx as i32) {
			if current_book_collapsable {
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
	}
}

fn create_list_row(entry: &ChapterListEntry, icons: &IconMap) -> ListBoxRow
{
	let title = &entry.title;
	let label = Label::builder()
		.halign(Align::Start)
		.ellipsize(EllipsizeMode::End)
		.tooltip_text(title)
		.build();

	let view = gtk4::Box::new(Orientation::Horizontal, 4);
	let icon_name = if entry.book {
		view.add_css_class(BOOK_NAME_LABEL_CLASS);
		label.set_label(title);
		if entry.reading {
			"book_reading.svg"
		} else {
			"book_closed.svg"
		}
	} else {
		view.add_css_class(TOC_LABEL_CLASS);
		label.set_label(&format!("    {}", title));
		"chapter.svg"
	};

	view.append(&load_button_image(icon_name, icons, false));
	view.append(&label);

	let row = ListBoxRow::new();
	row.set_child(Some(&view));

	row
}
