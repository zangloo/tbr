use std::borrow::Cow;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::str::FromStr;

use gtk4::{Align, EventControllerKey, glib, Label, ListBox, ListBoxRow, Orientation, Popover, SearchEntry, SelectionMode, StringList, StringObject, Widget};
use gtk4::gdk::Key;
use gtk4::glib::markup_escape_text;
use gtk4::pango::EllipsizeMode;
use gtk4::prelude::{BoxExt, Cast, EditableExt, IsA, ListBoxRowExt, ListModelExt, PopoverExt, WidgetExt};

use crate::config::{match_filename, ReadingInfo};
use crate::gui::{GuiContext, ignore_cap, MODIFIER_NONE};
use crate::gui::view::GuiView;

pub(super) struct HistoryList {
	search: SearchEntry,
	list_box: ListBox,
	list: StringList,
	popover: Popover,

	filter_pattern: Rc<RefCell<Option<String>>>,
}

impl HistoryList {
	#[inline]
	pub fn new(view: &GuiView) -> Self
	{
		let container = gtk4::Box::new(Orientation::Vertical, 10);
		let search = SearchEntry::builder()
			.build();
		let filter_pattern = Rc::new(RefCell::new(None));
		let list_box = ListBox::builder()
			.selection_mode(SelectionMode::Single)
			.build();
		let list = StringList::new(&[]);
		{
			let pattern = filter_pattern.clone();
			list_box.bind_model(Some(&list), move |obj| {
				let obj = obj.downcast_ref::<StringObject>().unwrap();
				gtk4::Widget::from(create_history_entry(
					obj.string().as_str(),
					pattern.borrow().as_ref().map(|s: &String| s.as_str()),
				))
			});
		}

		container.append(&search);
		container.append(&list_box);
		let popover = Popover::builder()
			.child(&container)
			.default_widget(&search)
			.build();
		{
			let view = view.clone();
			let input = search.clone();
			popover.connect_visible_notify(move |p| {
				if !p.get_visible() {
					view.grab_focus();
					input.set_text("");
				}
			});
		}
		{
			#[inline]
			fn next_row(list: &ListBox) -> Option<ListBoxRow>
			{
				let selected = list.selected_row()?;
				let idx = selected.index() + 1;
				if let Some(row) = list.row_at_index(idx) {
					Some(row)
				} else {
					list.row_at_index(0)
				}
			}
			#[inline]
			fn prev_row(list: &ListBox) -> Option<ListBoxRow>
			{
				let selected = list.selected_row()?;
				let idx = selected.index();
				if idx == 0 {
					let last = list.last_child()?;
					let row = last.downcast::<ListBoxRow>()
						.ok()?;
					list.row_at_index(row.index())
				} else {
					list.row_at_index(idx - 1)
				}
			}
			let list = list_box.clone();
			let key_event = EventControllerKey::new();
			key_event.connect_key_pressed(move |_, key, _, modifier| {
				let (key, modifier) = ignore_cap(key, modifier);
				let target = match (key, modifier) {
					(Key::Down, MODIFIER_NONE) => next_row(&list),
					(Key::Up, MODIFIER_NONE) => prev_row(&list),
					_ => {
						None
					}
				};
				if let Some(row) = target {
					list.select_row(Some(&row));
					glib::Propagation::Stop
				} else {
					glib::Propagation::Proceed
				}
			});
			search.add_controller(key_event);
		}
		{
			let history_popover = popover.clone();
			search.connect_stop_search(move |_| history_popover.set_visible(false));
		}
		Self {
			search,
			list_box,
			list,
			popover,
			filter_pattern,
		}
	}
	#[inline]
	pub fn setup(&self, parent: &impl IsA<Widget>, gc: &GuiContext)
	{
		#[inline]
		fn open(gc: &GuiContext, index: i32, list: &StringList)
		{
			if index < 0 {
				return;
			}
			let index = index as u32;
			if let Some(str) = list.string(index) {
				if let Ok(path) = PathBuf::from_str(str.as_str()) {
					gc.open_file(&path);
				}
			}
			gc.history_list.popover.set_visible(false);
		}

		self.popover.set_parent(parent);

		{
			let gc = gc.clone();
			let list_box = self.list_box.clone();
			let list = self.list.clone();
			self.search.connect_activate(move |_| {
				if let Some(row) = list_box.selected_row() {
					open(&gc, row.index(), &list)
				}
			});
		}

		{
			let gc = gc.clone();
			let list = self.list.clone();
			self.list_box.connect_row_activated(move |_, row| open(&gc, row.index(), &list));
		}
		{
			let filter_pattern = self.filter_pattern.clone();
			let gc = gc.clone();
			let list = self.list.clone();
			let list_box = self.list_box.clone();
			self.search.connect_search_changed(move |entry| {
				let text = entry.text();
				let text = text.as_str().trim();

				let mut pattern = filter_pattern.borrow_mut();
				if text.is_empty() {
					*pattern = None;
				} else {
					*pattern = Some(text.to_owned());
				}
				if let Some(infos) = gc.filter_history(pattern.as_ref()) {
					drop(pattern);
					update_history(infos, &list, &list_box);
				}
			});
		}
	}

	#[inline]
	pub fn popup(&self, infos: Vec<ReadingInfo>)
	{
		update_history(infos, &self.list, &self.list_box);
		self.popover.popup();
	}
}

#[inline]
fn create_history_entry(path_str: &str, pattern: Option<&str>) -> Label
{
	if let Some(pattern) = pattern {
		let markup = path_markup(path_str, pattern);
		let str = markup.as_ref();
		Label::builder()
			.use_markup(true)
			.label(str)
			.halign(Align::Start)
			.ellipsize(EllipsizeMode::End)
			.tooltip_markup(str)
			.build()
	} else {
		Label::builder()
			.label(path_str)
			.halign(Align::Start)
			.ellipsize(EllipsizeMode::End)
			.tooltip_text(path_str)
			.build()
	}
}

#[inline]
fn path_markup<'a>(path: &'a str, pattern: &str) -> Cow<'a, str>
{
	if let Some(indexes) = match_filename(path, pattern) {
		let mut index_iter = indexes.into_iter();
		if let Some(mut matched_index) = index_iter.next() {
			let mut text = String::new();
			let mut found_matched = false;
			for (fi, fc) in path.chars().enumerate() {
				if fi == matched_index {
					if !found_matched {
						text.push_str(r#"<span color="white" background="lightgray">"#);
						found_matched = true;
					}
					matched_index = index_iter.next().unwrap_or(usize::MAX);
				} else if found_matched {
					found_matched = false;
					text.push_str(r#"</span>"#);
				}
				text.push_str(markup_escape_text(&fc.to_string()).as_str());
			}
			if found_matched {
				text.push_str(r#"</span>"#);
			}
			return Cow::Owned(text);
		}
	}
	Cow::Borrowed(path)
}

#[inline]
fn update_history(infos: Vec<ReadingInfo>, list: &StringList, list_box: &ListBox)
{
	let mut vec = vec![];
	for ri in &infos {
		vec.push(ri.filename.as_str());
	}
	list.splice(0, list.n_items(), &vec);
	list_box.select_row(list_box.row_at_index(0).as_ref());
}
