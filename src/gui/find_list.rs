use crate::book::{SearchError, Line};
use crate::common::{byte_index_for_char, char_width};
use crate::config::BookLoadingInfo;
use crate::container::{load_book, load_container, Container, ContainerManager};
use crate::gui::{load_button_image, IconMap};
use crate::i18n::I18n;
use anyhow::Result;
use fancy_regex::Regex;
use gtk4::glib::{idle_add_local, markup_escape_text, ControlFlow};
use gtk4::pango::EllipsizeMode;
use gtk4::prelude::{BoxExt, ButtonExt, CheckButtonExt, EditableExt, ListBoxRowExt, WidgetExt};
use gtk4::{Align, Button, CheckButton, Image, Label, ListBox, Orientation, PolicyType, SearchEntry, SelectionMode};
use std::borrow::Cow;
use std::cell::{RefCell, RefMut};
use std::ops::Range;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::spawn;

#[derive(Clone)]
enum FindState {
	Idle,
	Finding,
	Stopping,
}

pub struct FoundEntry {
	pub inner_book: usize,
	pub chapter: usize,
	pub chapter_title: Option<String>,
	pub toc_title: Option<String>,
	pub line: usize,
	pub range: Range<usize>,
	pub display_text: String,
	pub highlight_display_bytes: Range<usize>,
}

struct FindListInner {
	filename: Option<String>,
	inner_book: usize,
	list: ListBox,
	rows: Vec<FoundEntry>,
	i18n: Rc<I18n>,
}

// create too much label in idle thread will freeze the UI
const BATCH_CREATE_SIZE: usize = 100;

#[derive(Clone)]
pub struct FindList {
	inner: Rc<RefCell<FindListInner>>,
}

impl FindList {
	pub fn create(filename: &Option<String>, i18n: &Rc<I18n>, icons: &Rc<IconMap>)
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
			.hexpand(true)
			.build();
		let start_icon = load_button_image("find-start.svg", icons, false);
		let stop_icon = load_button_image("find-stop.svg", icons, false);
		let ctrl_btn = Button::builder()
			.child(&start_icon)
			.focus_on_click(false)
			.focusable(false)
			.build();
		ctrl_btn.set_tooltip_text(Some(&i18n.msg("find-toggle-tooltip")));
		let all_book = CheckButton::builder()
			.label(i18n.msg("find-all-book"))
			.tooltip_text(i18n.msg("find-all-book-tooltip"))
			.build();
		let input_box = gtk4::Box::builder()
			.orientation(Orientation::Horizontal)
			.spacing(0)
			.hexpand(true)
			.build();
		input_box.append(&all_book);
		input_box.append(&input);
		input_box.append(&ctrl_btn);

		let container = gtk4::Box::builder()
			.orientation(Orientation::Vertical)
			.spacing(0)
			.vexpand(true)
			.build();
		container.append(&input_box);
		container.append(&gtk4::ScrolledWindow::builder()
			.child(&list)
			.hscrollbar_policy(PolicyType::Never)
			.vexpand(true)
			.build());

		let inner = FindListInner {
			filename: filename.to_owned(),
			inner_book: 0,
			list,
			rows: Default::default(),
			i18n: i18n.clone(),
		};
		let find_list = FindList { inner: Rc::new(RefCell::new(inner)) };

		if filename.is_some() {
			{
				let state = Arc::new(Mutex::new(FindState::Idle));
				{
					let ctrl_btn = ctrl_btn.clone();
					input.connect_activate(move |_| ctrl_btn.emit_clicked());
				}

				{
					let input = input.clone();
					let find_list = find_list.clone();
					let ctrl_btn2 = ctrl_btn.clone();
					ctrl_btn.connect_clicked(move |_| {
						if let Ok(mut guard) = state.try_lock() {
							match *guard {
								FindState::Idle => {
									drop(guard);
									start_find(
										&input,
										&all_book,
										&ctrl_btn2,
										&start_icon,
										&stop_icon,
										&find_list,
										&state);
									return;
								}
								FindState::Finding => *guard = FindState::Stopping,
								FindState::Stopping => {}
							};
						}
					});
				}
			}
		}

		(find_list, container, input)
	}

	pub fn set_inner_book(&self, inner_book: usize)
	{
		self.inner.borrow_mut().inner_book = inner_book;
	}

	pub fn set_callback<F>(&self, f: F)
	where
		F: Fn(&FoundEntry) -> bool + 'static,
	{
		let inner = self.inner.clone();
		self.inner.borrow().list.connect_row_selected(move |_, row| {
			if let Some(row) = row {
				let index = row.index();
				if index < 0 {
					return;
				}
				if let Ok(mut inner) = inner.try_borrow_mut() {
					if let Some(entry) = inner.rows.get(index as usize) {
						if f(entry) {
							inner.inner_book = entry.inner_book;
						}
					}
				}
			}
		});
	}
}

fn find_in_book(container_manager: &ContainerManager,
	container: &mut Box<dyn Container>, filename: &str, inner_book: usize,
	regex: &Regex, tx: &Sender<FoundEntry>, state: &Arc<Mutex<FindState>>) -> Result<(), SearchError>
{
	let loading = BookLoadingInfo::NewReading(&filename, inner_book, 0, 16);
	if let Ok((mut book, _)) = load_book(&container_manager, container, loading) {
		let mut chapter = 0;
		loop {
			let chapter_title = book.title(0, 0);
			for (idx, line) in book.lines().iter().enumerate() {
				line.search_pattern(&regex, |text, range| {
					let (display_text, highlight_display_bytes) = make_display_text(line, text, &range)
						.ok_or(SearchError::Custom(Cow::Borrowed("Failed setup display text for found")))?;
					tx.send(FoundEntry {
						inner_book,
						chapter,
						chapter_title: chapter_title.map(|t| t.to_owned()),
						toc_title: book.title(idx, range.start).map(|t| t.to_owned()),
						line: idx,
						range,
						display_text,
						highlight_display_bytes,
					}).map_err(|_| SearchError::Canceled)?;
					Ok(())
				})?;
			}
			if let Ok(state) = state.try_lock() {
				if matches!(*state, FindState::Stopping) {
					return Ok(());
				}
			}
			match book.next_chapter() {
				Ok(Some(c)) => chapter = c,
				_ => break,
			}
		}
	}
	Ok(())
}

#[inline]
fn do_find(filename: String, search_book: Option<usize>, regex: Regex,
	tx: Sender<FoundEntry>, state: Arc<Mutex<FindState>>) -> Result<(), SearchError>
{
	let container_manager = Default::default();
	if let Ok(mut container) = load_container(&container_manager, &filename) {
		if let Some(inner_book) = search_book {
			find_in_book(
				&container_manager,
				&mut container,
				&filename,
				inner_book,
				&regex,
				&tx,
				&state)?;
		} else if let Some(book_names) = container.inner_book_names() {
			for i in 0..book_names.len() {
				find_in_book(
					&container_manager,
					&mut container,
					&filename,
					i,
					&regex,
					&tx,
					&state)?;
			}
		} else {
			find_in_book(
				&container_manager,
				&mut container,
				&filename,
				0,
				&regex,
				&tx,
				&state)?;
		}
	}
	Ok(())
}

fn find(mut inner: RefMut<FindListInner>, input: &SearchEntry,
	all_book: &CheckButton, tx: Sender<FoundEntry>,
	state: Arc<Mutex<FindState>>) -> bool
{
	let filename = match &inner.filename {
		None => return false,
		Some(filename) => filename,
	};
	let text = input.text();
	let pattern = text.as_str().trim();
	let regex = match Regex::new(&pattern) {
		Ok(regex) => regex,
		Err(_) => return true,
	};
	let filename = filename.to_owned();
	let inner_book = inner.inner_book;
	inner.list.remove_all();
	inner.rows.clear();
	drop(inner);
	let search_book = if all_book.is_active() {
		None
	} else {
		Some(inner_book)
	};
	match state.try_lock() {
		Ok(mut state) => *state = FindState::Finding,
		Err(_) => return false,
	}
	spawn(move || if let Err(err) = do_find(filename, search_book, regex, tx, state) {
		match err {
			SearchError::Canceled => {}
			SearchError::Custom(msg) =>
				eprintln!("Finding stopped with error: {}", &msg),
		}
	});
	true
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
		.label(&format!("{} : {}", entry.line + 1, entry.range.start + 1))
		.build());

	let head = markup_escape_text(&entry.display_text[..entry.highlight_display_bytes.start]);
	let middle = markup_escape_text(&entry.display_text[entry.highlight_display_bytes.start..entry.highlight_display_bytes.end]);
	let tail = markup_escape_text(&entry.display_text[entry.highlight_display_bytes.end..]);
	let display_text = format!(r#"<small>{}</small><span font="small" foreground='white' background="black">{}</span><small>{}</small>"#,
		head, middle, tail);
	let display_label = Label::builder()
		.halign(Align::Start)
		.hexpand(true)
		.wrap(true)
		.use_markup(true)
		.label(&display_text)
		.build();

	let entry_box = gtk4::Box::builder()
		.orientation(Orientation::Vertical)
		.spacing(0)
		.build();
	entry_box.append(&entry_label);
	entry_box.append(&display_label);
	entry_box
}

const PADDING_SIZE: usize = 20;

#[inline]
fn make_display_text(line: &Line, text: &str, range: &Range<usize>) -> Option<(String, Range<usize>)>
{
	let mut padding = 0;
	let mut start = range.start;
	while start > 0 && padding < PADDING_SIZE {
		let idx = start - 1;
		if let Some(char) = line.char_at(idx) {
			padding += char_width(char);
		} else {
			break;
		}
		start = idx;
	}
	padding = 0;
	let mut end = range.end;
	while let Some(char) = line.char_at(end) {
		padding += char_width(char);
		if padding >= PADDING_SIZE {
			break;
		}
		end += 1;
	}
	let chars = line.len();
	let byte_start = byte_index_for_char(text, chars, start)?;
	let byte_end = byte_index_for_char(text, chars, end)?;
	let highlight_byte_start = byte_index_for_char(text, chars, range.start)?;
	let highlight_byte_end = byte_index_for_char(text, chars, range.end)?;
	let display_text = text[byte_start..byte_end].to_owned();
	let highlight_byte_range = highlight_byte_start - byte_start..highlight_byte_end - byte_start;
	Some((display_text, highlight_byte_range))
}

#[inline]
fn start_find(input: &SearchEntry, all_book: &CheckButton,
	ctrl_btn: &Button, start_icon: &Image, stop_icon: &Image,
	find_list: &FindList, state: &Arc<Mutex<FindState>>)
{
	let input = input.clone();
	let all_book = all_book.clone();
	let ctrl_btn = ctrl_btn.clone();
	let start_icon = start_icon.clone();
	let stop_icon = stop_icon.clone();
	let find_list = find_list.clone();
	let state = state.clone();
	let (tx, rx) = mpsc::channel();
	if let Ok(inner) = find_list.inner.try_borrow_mut() {
		if find(inner, &input, &all_book, tx.clone(), state.clone()) {
			toggle_find(false, &input, &all_book, &ctrl_btn, &start_icon, &stop_icon);
		} else {
			return;
		}
	}
	idle_add_local(move || {
		let next = retrieve_entries(
			&find_list,
			&state,
			&rx);
		if next {
			ControlFlow::Continue
		} else {
			if let Ok(mut state) = state.lock() {
				*state = FindState::Idle;
			}
			toggle_find(true, &input, &all_book, &ctrl_btn, &start_icon, &stop_icon);
			ControlFlow::Break
		}
	});
}
#[inline]
fn retrieve_entries(find_list: &FindList, state: &Arc<Mutex<FindState>>,
	rx: &Receiver<FoundEntry>) -> bool
{
	for _ in 0..BATCH_CREATE_SIZE {
		if let Ok(state) = state.try_lock() {
			if matches!(*state, FindState::Stopping) {
				return false;
			}
		}
		match rx.try_recv() {
			Ok(entry) =>
				if let Ok(mut inner) = find_list.inner.try_borrow_mut() {
					inner.list.append(&create_entry_label(&entry, &inner.i18n));
					inner.rows.push(entry);
				}
			Err(TryRecvError::Empty) => {
				break;
			}
			Err(TryRecvError::Disconnected) => return false,
		}
	}
	true
}

fn toggle_find(enable: bool, input: &SearchEntry, all_book: &CheckButton, ctrl_btn: &Button, start_icon: &Image, stop_icon: &Image)
{
	if enable {
		input.set_sensitive(true);
		all_book.set_sensitive(true);
		ctrl_btn.set_child(Some(start_icon));
	} else {
		input.set_sensitive(false);
		all_book.set_sensitive(false);
		ctrl_btn.set_child(Some(stop_icon));
	}
}
