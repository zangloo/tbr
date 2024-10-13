use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread::spawn;

use fancy_regex::Regex;
use gtk4::{Align, Label, ListBox, Orientation, PolicyType, SearchEntry, SelectionMode};
use gtk4::glib::{ControlFlow, idle_add_local};
use gtk4::pango::EllipsizeMode;
use gtk4::prelude::{BoxExt, EditableExt, ListBoxRowExt, WidgetExt};

use crate::common::TraceInfo;
use crate::config::BookLoadingInfo;
use crate::container::{load_book, load_container};
use crate::i18n::I18n;

pub struct FoundEntry {
	inner_book: usize,
	chapter: usize,
	chapter_title: Option<String>,
	toc_title: Option<String>,
	line: usize,
	offset: usize,
}

struct FindListInner {
	filename: Option<String>,
	inner_book: usize,
	input: SearchEntry,
	list: ListBox,
	rows: Vec<FoundEntry>,
	i18n: Rc<I18n>,
}

impl FindListInner {
	fn retrieve_entries(&mut self, rx: &Receiver<FoundEntry>) -> ControlFlow
	{
		loop {
			match rx.try_recv() {
				Ok(entry) => {
					self.list.append(&create_entry_label(&entry, &self.i18n));
					self.rows.push(entry);
				}
				Err(TryRecvError::Empty) => {
					break ControlFlow::Continue;
				}
				Err(TryRecvError::Disconnected) => {
					self.input.set_sensitive(true);
					break ControlFlow::Break;
				}
			}
		}
	}
}

#[derive(Clone)]
pub struct FindList {
	inner: Rc<RefCell<FindListInner>>,
}

impl FindList {
	pub fn create(filename: &Option<String>, i18n: &Rc<I18n>)
		-> (Self, gtk4::Box, SearchEntry)
	{
		let list = ListBox::builder()
			.selection_mode(SelectionMode::Single)
			.build();
		list.add_css_class("navigation-sidebar");
		list.add_css_class("boxed-list");

		let input = SearchEntry::builder()
			.placeholder_text(i18n.msg("find-text").as_ref())
			.activates_default(true)
			.enable_undo(true)
			.build();
		let container = gtk4::Box::builder()
			.orientation(Orientation::Vertical)
			.spacing(0)
			.vexpand(true)
			.build();
		container.append(&input);
		container.append(&gtk4::ScrolledWindow::builder()
			.child(&list)
			.hscrollbar_policy(PolicyType::Never)
			.vexpand(true)
			.build());

		let inner = FindListInner {
			filename: filename.to_owned(),
			inner_book: 0,
			input: input.clone(),
			list,
			rows: Default::default(),
			i18n: i18n.clone(),
		};
		let find_list = FindList { inner: Rc::new(RefCell::new(inner)) };

		if filename.is_some() {
			let find_list = find_list.clone();
			input.connect_activate(move |input| {
				if let Ok(mut inner) = find_list.inner.try_borrow_mut() {
					if let Some(filename) = &inner.filename {
						input.set_sensitive(false);
						let text = input.text();
						let pattern = text.as_str().trim();
						let filename = filename.to_owned();
						let inner_book = inner.inner_book;
						inner.list.remove_all();
						inner.rows.clear();
						drop(inner);
						if let Ok(regex) = Regex::new(&pattern) {
							find(regex, filename, inner_book, find_list.inner.clone());
						}
					}
				}
			});
		}

		(find_list, container, input)
	}

	pub fn set_inner_book(&self, inner_book: usize)
	{
		self.inner.borrow_mut().inner_book = inner_book;
	}

	/// F:(inner_book: usize, position: TraceInfo)
	pub fn set_callback<F>(&self, f: F)
		where F: Fn(usize, TraceInfo) -> bool + 'static
	{
		let inner = self.inner.clone();
		self.inner.borrow().list.connect_row_activated(move |_, row| {
			let index = row.index();
			if index < 0 {
				return;
			}
			if let Ok(mut inner) = inner.try_borrow_mut() {
				if let Some(entry) = inner.rows.get(index as usize) {
					if f(entry.inner_book, TraceInfo {
						chapter: entry.chapter,
						line: entry.line,
						offset: entry.offset,
					}) {
						inner.inner_book = entry.inner_book;
					}
				}
			}
		});
	}
}

fn find(regex: Regex, filename: String, inner_book: usize,
	inner: Rc<RefCell<FindListInner>>)
{
	let (tx, rx) = mpsc::channel();
	spawn(move || {
		let container_manager = Default::default();
		if let Ok(mut container) = load_container(&container_manager, &filename) {
			if let Ok((mut book, _)) = load_book(&container_manager, &mut container,
				BookLoadingInfo::NewReading(&filename, inner_book, 0, 16)) {
				let mut chapter = 0;
				loop {
					let chapter_title = book.title(0, 0);
					for (idx, line) in book.lines().iter().enumerate() {
						line.search_pattern(&regex, |range| {
							tx.send(FoundEntry {
								inner_book,
								chapter,
								chapter_title: chapter_title.map(|t| t.to_owned()),
								toc_title: book.title(idx, range.start).map(|t| t.to_owned()),
								line: idx,
								offset: range.start,
							}).is_ok()
						})
					}
					match book.next_chapter() {
						Ok(Some(c)) => chapter = c,
						_ => break,
					}
				}
			}
		}
	});

	idle_add_local(move || {
		if let Ok(mut inner) = inner.try_borrow_mut() {
			inner.retrieve_entries(&rx)
		} else {
			ControlFlow::Continue
		}
	});
}

#[inline]
fn create_entry_label(entry: &FoundEntry, i18n: &I18n) -> gtk4::Box
{
	let entry_label = gtk4::Box::builder()
		.orientation(Orientation::Horizontal)
		.spacing(0)
		.hexpand(true)
		.build();
	let chapter_title = entry
		.chapter_title
		.as_ref()
		.map_or_else(|| Cow::Owned(i18n.args_msg(
			"found-chapter-title",
			vec![("index", entry.chapter + 1)])),
			|t| Cow::Borrowed(t.as_str()));
	let title = if let Some(toc_title) = &entry.toc_title {
		if chapter_title.as_ref() != toc_title.as_str() {
			Cow::Owned(format!("{} : {}", chapter_title, toc_title))
		} else {
			chapter_title
		}
	} else {
		chapter_title
	};
	entry_label.append(&Label::builder()
		.halign(Align::Start)
		.hexpand(true)
		.ellipsize(EllipsizeMode::End)
		.label(title)
		.build());
	entry_label.append(&Label::builder()
		.halign(Align::End)
		.label(&format!("{} : {}", entry.line, entry.offset))
		.build());
	entry_label
}
