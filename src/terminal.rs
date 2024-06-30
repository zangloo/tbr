use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use cursive::Cursive;
use cursive::CursiveExt;
use cursive::event::{Callback, Event};
use cursive::event::Key::Esc;
use cursive::theme::{Error, load_theme_file, load_toml, Theme};
use cursive::traits::Resizable;
use cursive::view::{Nameable, SizeConstraint};
use cursive::views::{EditView, LinearLayout, OnEventView, TextView, ViewRef};

use view::ReadingView;

use crate::{Asset, description, version, version_string};
use crate::config::{BookLoadingInfo, Configuration};
use crate::list::{list_dialog, ListIterator};
use crate::terminal::input_method::{InputMethod, setup_im};

pub mod view;
mod input_method;

const STATUS_VIEW_NAME: &str = "status";
const TEXT_VIEW_NAME: &str = "text";
const STATUS_LAYOUT_NAME: &str = "status_layout";
const INPUT_VIEW_NAME: &str = "input";
const INPUT_LAYOUT_NAME: &str = "input_layout";
const SEARCH_LABEL_TEXT: &str = "Search: ";
const GOTO_LABEL_TEXT: &str = "Goto line: ";

struct Themes {
	bright: Theme,
	dark: Theme,
}

impl Themes {
	pub fn get(&self, dark: bool) -> &Theme
	{
		if dark {
			&self.dark
		} else {
			&self.bright
		}
	}
}

struct TerminalContext {
	current: String,
	configuration: Configuration,
	themes: Themes,
	im: Option<Box<dyn InputMethod>>,
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

pub fn start(current: Option<String>, mut configuration: Configuration,
	config_dir: PathBuf) -> Result<()>
{
	let current = current.ok_or(anyhow!("No file to open."))?;
	println!("Loading {} ...", current);
	let loading = configuration.reading(&current)?;
	let mut app = Cursive::new();
	let themes = load_themes(&config_dir)?;
	let theme = themes.get(configuration.dark_theme);
	app.set_theme(theme.clone());
	let reading_view = ReadingView::new(configuration.render_han, loading)?;
	// turn off ime at start
	let im = setup_im();
	app.set_user_data(TerminalContext { current, configuration, themes, im });
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
	if let Some(names) = container.inner_book_names() {
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
			let msg = reading_view.switch_book(selected);
			update_status(s, &msg);
		});
		s.add_layer(dialog);
	}
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
		let history = match configuration.history(Some(&controller_context.current), None) {
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
				chk(configuration.reading_by_id(selected as i64), |reading| {
					let loading = BookLoadingInfo::History(reading);
					chk(reading_view.switch_container(loading), |msg| {
						controller_context.current = reading_view.reading_info().filename;
						chk(configuration.save_reading(&mut reading_now), |()|
							msg)
					})
				})
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
		if let Some(line_no) = line_no {
			let line_no = line_no.parse::<usize>()?;
			let mut reading_view: ViewRef<ReadingView> = s.find_name(TEXT_VIEW_NAME).unwrap();
			reading_view.goto_line(line_no)
		} else {
			Ok(())
		}
	}, |_| {});
}

fn setup_search_view(app: &mut Cursive) {
	fn set_im_active(s: &mut Cursive, active: Option<bool>, update_restore: bool)
	{
		s.with_user_data(|context: &mut TerminalContext| {
			if let Some(im) = &mut context.im {
				im.set_active(active, update_restore);
			}
		});
	}
	let reading_view: ViewRef<ReadingView> = app.find_name(TEXT_VIEW_NAME).unwrap();
	let search_pattern = reading_view.search_pattern();
	set_im_active(app, None, false);
	setup_input_view(app, SEARCH_LABEL_TEXT, search_pattern, |s, pattern| {
		set_im_active(s, Some(false), true);
		if let Some(pattern) = pattern {
			let mut reading_view: ViewRef<ReadingView> = s.find_name(TEXT_VIEW_NAME).unwrap();
			reading_view.search(pattern)?;
		}
		Ok(())
	}, |s| set_im_active(s, Some(false), true));
}

fn setup_input_view<F, C>(app: &mut Cursive, prefix: &str, preset: &str, submit: F, cancel: C)
	where
		F: Fn(&mut Cursive, Option<&str>) -> Result<()> + 'static,
		C: Fn(&mut Cursive) + 'static,
{
	let input_view = EditView::new()
		.on_submit(move |app, str| {
			let pattern_len = str.len();
			let result = if pattern_len == 0 {
				(submit)(app, None)
			} else {
				(submit)(app, Some(str))
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
			.on_event(Esc, move |s| {
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
				cancel(s);
			})
		);

	let mut status_layout: ViewRef<LinearLayout> = app.find_name(STATUS_LAYOUT_NAME).unwrap();
	status_layout.insert_child(0, input_layout
		.with_name(INPUT_LAYOUT_NAME)
		.resized(SizeConstraint::Fixed(prefix.len() + 20), SizeConstraint::Fixed(1)));
	drop(status_layout);
	app.focus_name(INPUT_VIEW_NAME).unwrap();
}

fn load_themes(config_dir: &PathBuf) -> Result<Themes>
{
	let dark = load_theme(config_dir, "dark.toml")?;
	let bright = load_theme(config_dir, "bright.toml")?;
	Ok(Themes { dark, bright })
}

fn load_theme(config_dir: &PathBuf, file: &str) -> Result<Theme>
{
	fn process_theme_result(result: Result<Theme, Error>) -> Result<Theme> {
		match result {
			Ok(theme) => Ok(theme),
			Err(e) => Err(anyhow!(match e {
					Error::Io(e) => e.to_string(),
					Error::Parse(e) => e.to_string(),
				}))
		}
	}

	let theme_file = config_dir.join(file);
	if theme_file.exists() {
		process_theme_result(load_theme_file(theme_file))
	} else {
		let utf8 = Asset::get(file).unwrap();
		let str = std::str::from_utf8(utf8.data.as_ref())?;
		let theme = process_theme_result(load_toml(str))?;
		fs::write(theme_file, str)?;
		Ok(theme)
	}
}