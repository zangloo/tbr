use anyhow::{anyhow, Result};
use cursive::Cursive;
use cursive::CursiveExt;
use cursive::event::{Callback, Event};
use cursive::event::Key::Esc;
use cursive::traits::Resizable;
use cursive::view::{Nameable, SizeConstraint};
use cursive::views::{EditView, LinearLayout, OnEventView, TextView, ViewRef};

use crate::{version_string, description, version};
use crate::list::{list_dialog, ListIterator};
use crate::config::{Configuration, Themes};
use view::ReadingView;

pub mod view;

const STATUS_VIEW_NAME: &str = "status";
const TEXT_VIEW_NAME: &str = "text";
const STATUS_LAYOUT_NAME: &str = "status_layout";
const INPUT_VIEW_NAME: &str = "input";
const INPUT_LAYOUT_NAME: &str = "input_layout";
const SEARCH_LABEL_TEXT: &str = "Search: ";
const GOTO_LABEL_TEXT: &str = "Goto line: ";

struct TerminalContext {
	configuration: Configuration,
	themes: Themes,
}

pub trait Listable {
	fn title(&self) -> &str;
	fn id(&self) -> usize;
}

impl<'a> Listable for (&'a str, usize) {
	#[inline]
	fn title(&self) -> &str
	{
		self.0
	}

	#[inline]
	fn id(&self) -> usize
	{
		self.1
	}
}

pub fn start(mut configuration: Configuration, themes: Themes) -> Result<()> {
	if configuration.current.is_none() {
		return Err(anyhow!("No file to open."));
	}
	let current = configuration.current.as_ref().unwrap();
	println!("Loading {} ...", current);
	let (_history, reading) = configuration.reading(current)?;
	let mut app = Cursive::new();
	let theme = themes.get(configuration.dark_theme);
	app.set_theme(theme.clone());
	let reading_view = ReadingView::new(configuration.render_han, reading.clone())?;
	app.set_user_data(TerminalContext { configuration, themes });
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
			.on_event('b', |s| select_book(s))
			.on_event('h', |s| select_history(s))
			.on_event('t', |s| switch_theme(s))
			.on_event('c', move |s| {
				let reading_view: ViewRef<ReadingView> = s.find_name(TEXT_VIEW_NAME).unwrap();
				let book = reading_view.reading_book();
				let option = book.toc_iterator();
				if option.is_none() {
					drop(option);
					drop(reading_view);
					select_book(s);
					return;
				}
				let toc_index = reading_view.toc_index();
				let dialog = list_dialog("Select TOC", option.unwrap(), toc_index, move |s, new_index| {
					let mut reading_view: ViewRef<ReadingView> = s.find_name(TEXT_VIEW_NAME).unwrap();
					if toc_index != new_index {
						if let Some(status) = reading_view.goto_toc(new_index) {
							update_status(s, &status);
						}
					}
				});
				s.add_layer(dialog);
			}))
		.child(status_view);
	app.add_fullscreen_layer(layout);
	app.run();
	let reading_view: ViewRef<ReadingView> = app.find_name(TEXT_VIEW_NAME).unwrap();
	let mut reading_now = reading_view.reading_info();
	let controller_context: TerminalContext = app.take_user_data().unwrap();
	configuration = controller_context.configuration;
	configuration.current = Some(reading_now.filename.clone());
	configuration.save_reading(&mut reading_now)?;
	configuration.save()?;
	Ok(())
}

pub(crate) fn update_status_callback(status: String) -> Callback {
	Callback::from_fn(move |s| {
		update_status(s, &status);
	})
}

fn switch_render(s: &mut Cursive) {
	let mut reading_view: ViewRef<ReadingView> = s.find_name(TEXT_VIEW_NAME).unwrap();
	s.with_user_data(|controller_context: &mut TerminalContext| {
		let configuration = &mut controller_context.configuration;
		configuration.render_han = !configuration.render_han;
		reading_view.switch_render(configuration.render_han);
	});
}

fn select_book(s: &mut Cursive) {
	let reading_view: ViewRef<ReadingView> = s.find_name(TEXT_VIEW_NAME).unwrap();
	let container = reading_view.reading_container();
	let size = container.inner_book_names().len();
	if size == 1 {
		return;
	}
	let names = container.inner_book_names();
	let li = ListIterator::new(|position| {
		let bn = names.get(position)?;
		Some((bn.name() as &str, position))
	});
	let reading = &reading_view.reading_info();
	let dialog = list_dialog("Select inner book", li, reading.inner_book, |s, selected| {
		let mut reading_view: ViewRef<ReadingView> = s.find_name(TEXT_VIEW_NAME).unwrap();
		let reading_now = reading_view.reading_info();
		if reading_now.inner_book == selected {
			return;
		}
		let new_reading = reading_now.with_inner_book(selected);
		let msg = reading_view.switch_book(new_reading);
		update_status(s, &msg);
	});
	s.add_layer(dialog);
}

fn select_history(s: &mut Cursive)
{
	#[inline]
	fn chk<T, F>(result: Result<T>, f: F) -> String
		where F: FnOnce(T) -> String
	{
		match result {
			Ok(v) => f(v),
			Err(err) => err.to_string(),
		}
	}

	let option = s.with_user_data(|controller_context: &mut TerminalContext| {
		let configuration = &mut controller_context.configuration;
		let history = match configuration.history() {
			Ok(history) => history,
			Err(_) => {
				// update_status(s, &err.to_string());
				return None;
			}
		};
		let size = history.len();
		if size == 0 {
			return None;
		}
		let dialog = list_dialog("Reopen", history.into_iter(), 0, |s, selected| {
			let mut reading_view: ViewRef<ReadingView> = s.find_name(TEXT_VIEW_NAME).unwrap();
			let mut reading_now = reading_view.reading_info();
			let msg = s.with_user_data(|controller_context: &mut TerminalContext| {
				let configuration = &mut controller_context.configuration;
				chk(configuration.reading_by_id(selected as i64), |reading|
					chk(reading_view.switch_container(reading), |msg| {
						configuration.current = Some(reading_view.reading_info().filename);
						chk(configuration.save_reading(&mut reading_now), |()|
							msg)
					}))
			}).unwrap();
			update_status(s, &msg);
		});
		Some(dialog)
	}).unwrap();
	match option {
		Some(dialog) => s.add_layer(dialog),
		None => (),
	}
}

fn switch_theme(s: &mut Cursive) {
	let theme = s.with_user_data(|controller_context: &mut TerminalContext| {
		let dark = !controller_context.configuration.dark_theme;
		controller_context.configuration.dark_theme = dark;
		let theme = controller_context.themes.get(dark);
		theme.clone()
	}).unwrap();
	s.set_theme(theme.clone());
}

fn update_status(s: &mut Cursive, msg: &str) {
	s.call_on_name(STATUS_VIEW_NAME, |view: &mut TextView| {
		view.set_content(msg);
	});
}

fn goto_line(app: &mut Cursive) {
	let reading_view: ViewRef<ReadingView> = app.find_name(TEXT_VIEW_NAME).unwrap();
	let line_str = (reading_view.reading_info().line + 1).to_string();
	setup_input_view(app, GOTO_LABEL_TEXT, &line_str, |s, line_no| {
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
		reading_view.search(pattern)?;
		Ok(())
	});
}

fn setup_input_view<F>(app: &mut Cursive, prefix: &str, preset: &str, submit: F)
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
			.content(preset)
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
