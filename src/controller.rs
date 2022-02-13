use anyhow::{anyhow, Result};
use cursive::Cursive;
use cursive::CursiveExt;
use cursive::event::{Callback, Event};
use cursive::event::Key::Esc;
use cursive::theme::Theme;
use cursive::traits::Resizable;
use cursive::view::{Nameable, SizeConstraint};
use cursive::views::{EditView, LinearLayout, OnEventView, TextView, ViewRef};

use crate::{Configuration, ReadingInfo, ThemeEntry};
use crate::list::{list_dialog, ListEntry, ListIterator};
use crate::view::ReadingView;

const STATUS_VIEW_NAME: &str = "status";
const TEXT_VIEW_NAME: &str = "text";
const STATUS_LAYOUT_NAME: &str = "status_layout";
const INPUT_VIEW_NAME: &str = "input";
const INPUT_LAYOUT_NAME: &str = "input_layout";
const SEARCH_LABEL_TEXT: &str = "Search: ";
const GOTO_LABEL_TEXT: &str = "Goto line: ";

macro_rules! description {
    () => ( "Terminal ebook reader," )
}
macro_rules! version {
    () => ( env!("CARGO_PKG_VERSION") )
}
macro_rules! version_string {
    () => ( concat!(description!(), " v", version!()) )
}

pub struct ReverseInfo {
	pub line: usize,
	pub start: usize,
	pub end: usize,
}

fn reading_info(history: &mut Vec<ReadingInfo>, current: &String) -> ReadingInfo {
	let mut i = 0;
	while i < history.len() {
		if history[i].filename.eq(current) {
			return history.remove(i);
		}
		i += 1;
	}
	return ReadingInfo {
		filename: current.clone(),
		chapter: 0,
		line: 0,
		position: 0,
		ts: 0,
		reverse: None,
	};
}

fn get_theme<'a>(theme_name: &String, theme_entries: &'a Vec<ThemeEntry>) -> Result<&'a Theme> {
	for entry in theme_entries {
		if entry.0.eq(theme_name) {
			return Ok(&entry.1);
		}
	}
	Err(anyhow!("No theme defined: {}",theme_name))
}

pub(crate) fn start(mut configuration: Configuration) -> Result<Configuration> {
	println!("Loading {} ...", configuration.current);
	let reading = reading_info(&mut configuration.history, &configuration.current);
	let book = configuration.book_loader.load(&configuration.current, reading.chapter)?;
	let mut app = Cursive::new();
	let theme = get_theme(&configuration.theme_name, &configuration.theme_entries)?;
	app.set_theme(theme.clone());
	let reading_view = ReadingView::new(book, &configuration.render_type, reading.clone(), &configuration.search_pattern)?;
	app.set_user_data(configuration);
	let status_view = LinearLayout::horizontal()
		.child(TextView::new(&reading_view.status_msg())
			.no_wrap()
			.with_name(STATUS_VIEW_NAME)
			.resized(SizeConstraint::Full, SizeConstraint::Fixed(1)))
		.with_name(STATUS_LAYOUT_NAME);
	let layout = LinearLayout::vertical()
		.child(OnEventView::new(reading_view.with_name(TEXT_VIEW_NAME).full_screen())
			.on_event('/', |s| setup_search_view(s))
			.on_event(Event::CtrlChar('x'), |s| switch_render(s))
			.on_event('q', |s| s.quit())
			.on_event('v', |s| update_status(s, version_string!()))
			.on_event('g', |s| goto_line(s))
			.on_event('h', |s| select_history(s))
			.on_event('t', |s| select_theme(s))
			.on_event('c', move |s| {
				let reading_view: ViewRef<ReadingView> = s.find_name(TEXT_VIEW_NAME).unwrap();
				let book = reading_view.reading_book();
				if book.chapter_count() == 1 {
					return;
				}
				let li = ListIterator::new(&book, |book, position| {
					if position >= book.chapter_count() {
						None
					} else {
						Some(ListEntry::new(book.chapter_title(position)?, position))
					}
				});
				let current_chapter = reading_view.reading_info().chapter;
				let dialog = list_dialog("Select chapter", li, current_chapter, |s, chapter| {
					let mut reading_view: ViewRef<ReadingView> = s.find_name(TEXT_VIEW_NAME).unwrap();
					if reading_view.reading_info().chapter != chapter {
						let status = reading_view.switch_chapter(chapter);
						update_status(s, &status);
					}
				});
				s.add_layer(dialog);
			}))
		.child(status_view);
	app.add_fullscreen_layer(layout);
	app.run();
	let reading_view: ViewRef<ReadingView> = app.find_name(TEXT_VIEW_NAME).unwrap();
	let reading_now = reading_view.reading_info();
	let mut configuration: Configuration = app.take_user_data().unwrap();
	configuration.current = reading_now.filename.clone();
	configuration.search_pattern = reading_view.search_pattern().clone();
	configuration.history.push(reading_now);
	Ok(configuration)
}

pub(crate) fn update_status_callback(status: String) -> Callback {
	Callback::from_fn(move |s| {
		update_status(s, &status);
	})
}

fn switch_render(s: &mut Cursive) {
	let mut reading_view: ViewRef<ReadingView> = s.find_name(TEXT_VIEW_NAME).unwrap();
	s.with_user_data(|configuration: &mut Configuration| {
		configuration.render_type = String::from(match configuration.render_type.as_str() {
			"han" => "xi",
			_ => "han",
		});
		reading_view.switch_render(&configuration.render_type);
	});
}

fn select_history(s: &mut Cursive)
{
	let option = s.with_user_data(|configuration: &mut Configuration| {
		let history = &configuration.history;
		let size = history.len();
		if size == 0 {
			return None;
		}
		let li = ListIterator::new(history, |history, position| {
			if position >= size {
				return None;
			}
			let option = history.get(size - position - 1);
			match option {
				Some(ri) => Some(ListEntry::new(&ri.filename, position)),
				None => None,
			}
		});
		let dialog = list_dialog("Select theme", li, 0, |s, selected| {
			let mut reading_view: ViewRef<ReadingView> = s.find_name(TEXT_VIEW_NAME).unwrap();
			let reading_now = reading_view.reading_info();
			let result = s.with_user_data(|configuration: &mut Configuration| {
				let history = &mut configuration.history;
				let position = history.len() - selected - 1;
				let reading = &history[position];
				match configuration.book_loader.load(&reading.filename, reading.chapter) {
					Ok(book) => {
						let reading = history.remove(position);
						history.push(reading_now);
						Ok((book, reading))
					}
					Err(e) => Err(e),
				}
			}).unwrap();
			let msg = match result {
				Ok((book, reading)) => reading_view.switch_book(book, reading),
				Err(e) => e.to_string(),
			};
			update_status(s, &msg);
		});
		Some(dialog)
	}).unwrap();
	match option {
		Some(dialog) => s.add_layer(dialog),
		None => (),
	}
}

fn select_theme(s: &mut Cursive) {
	let option = s.with_user_data(|configuration: &mut Configuration| {
		let theme_entries = &configuration.theme_entries;
		if theme_entries.len() <= 1 {
			return None;
		}
		let mut themes = vec![];
		for (idx, entry) in theme_entries.iter().enumerate() {
			if entry.0.eq(&configuration.theme_name) {
				continue;
			}
			themes.push(ListEntry::new(&entry.0, idx));
		}
		themes.sort();
		let dialog = list_dialog("Select theme", themes.into_iter(), 0, |s, selected| {
			let theme = s.with_user_data(|configuration: &mut Configuration| {
				let theme_entries = &configuration.theme_entries;
				configuration.theme_name = theme_entries[selected].0.clone();
				let theme = &theme_entries[selected].1;
				theme.clone()
			}).unwrap();
			s.set_theme(theme.clone());
		});
		Some(dialog)
	}).unwrap();
	match option {
		Some(dialog) => s.add_layer(dialog),
		None => (),
	}
}

fn update_status(s: &mut Cursive, msg: &str) {
	s.call_on_name(STATUS_VIEW_NAME, |view: &mut TextView| {
		view.set_content(msg);
	});
}

fn goto_line(app: &mut Cursive) {
	let reading_view: ViewRef<ReadingView> = app.find_name(TEXT_VIEW_NAME).unwrap();
	let line_str = (reading_view.reading_info().line + 1).to_string();
	setup_input_view(app, GOTO_LABEL_TEXT, &Some(line_str), |s, line_no| {
		let line_no = line_no.parse::<usize>()?;
		let mut reading_view: ViewRef<ReadingView> = s.find_name(TEXT_VIEW_NAME).unwrap();
		reading_view.goto_line(line_no)
	});
}

fn setup_search_view(app: &mut Cursive) {
	let reading_view: ViewRef<ReadingView> = app.find_name(TEXT_VIEW_NAME).unwrap();
	let search_pattern = reading_view.search_pattern();
	setup_input_view(app, SEARCH_LABEL_TEXT, search_pattern, |s, pattern| {
		let mut reading_view: ViewRef<ReadingView> = s.find_name(TEXT_VIEW_NAME).unwrap();
		reading_view.search(pattern)
	});
}

fn setup_input_view<F>(app: &mut Cursive, prefix: &str, preset: &Option<String>, submit: F)
	where F: Fn(&mut Cursive, &str) -> Result<()> + 'static
{
	let input_view = EditView::new()
		.on_submit(move |app, str| {
			let pattern_len = str.len();
			let result = if pattern_len == 0 {
				Ok(())
			} else {
				(submit)(app, str)
			};
			match result {
				Ok(()) => {
					app.focus_name(TEXT_VIEW_NAME).unwrap();
					let mut layout: ViewRef<LinearLayout> = app.find_name(STATUS_LAYOUT_NAME).unwrap();
					layout.remove_child(0).unwrap();
				}
				Err(e) => {
					update_status(app, e.to_string().as_str());
				}
			}
		});
	let input_layout = LinearLayout::horizontal()
		.child(TextView::new(prefix)
			.resized(SizeConstraint::Fixed(prefix.len()), SizeConstraint::Fixed(1)))
		.child(OnEventView::new(input_view
			.content(match preset {
				Some(t) => t,
				None => "",
			})
			.with_name(INPUT_VIEW_NAME)
			.resized(SizeConstraint::Fixed(20), SizeConstraint::Fixed(1)))
			.on_event(Esc, |s| {
				let mut layout: ViewRef<LinearLayout> = s.find_name(STATUS_LAYOUT_NAME).unwrap();
				match layout.find_child_from_name(INPUT_LAYOUT_NAME) {
					Some(idx) => {
						layout.remove_child(idx).unwrap();
						// release layout, or focus_name will failed.
						drop(layout);
						s.focus_name(TEXT_VIEW_NAME).unwrap();
					}
					None => (),
				};
			})
		);

	let mut status_layout: ViewRef<LinearLayout> = app.find_name(STATUS_LAYOUT_NAME).unwrap();
	status_layout.insert_child(0, input_layout
		.with_name(INPUT_LAYOUT_NAME)
		.resized(SizeConstraint::Fixed(prefix.len() + 20), SizeConstraint::Fixed(1)));
	drop(status_layout);
	app.focus_name(INPUT_VIEW_NAME).unwrap();
}

