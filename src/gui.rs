use std::borrow::Cow;
use std::cell::{BorrowMutError, Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Read;
use std::ops::Index;
use std::path::PathBuf;
use std::rc::Rc;
use ab_glyph::FontVec;

use anyhow::{bail, Result};
use cursive::theme::{BaseColor, Color, PaletteColor, Theme};
use gtk4::{Align, Application, ApplicationWindow, Button, CssProvider, DropTarget, EventControllerKey, FileDialog, FileFilter, Image, Label, ListBox, Orientation, Paned, PolicyType, PopoverMenu, SearchEntry, Stack, Window};
use gtk4::gdk::{Display, DragAction, Key, ModifierType};
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::gio::{ApplicationFlags, Cancellable, File, ListStore, MemoryInputStream, Menu, MenuModel, SimpleAction, SimpleActionGroup};
use gtk4::glib;
use gtk4::glib::{Bytes, closure_local, ExitCode, Object, ObjectExt, StaticType};
use gtk4::prelude::{ActionGroupExt, ActionMapExt, ApplicationExt, ApplicationExtManual, BoxExt, ButtonExt, DisplayExt, DrawingAreaExt, EditableExt, FileExt, GtkWindowExt, ListBoxRowExt, ListModelExt, PopoverExt, WidgetExt};
use resvg::{tiny_skia, usvg};
use resvg::usvg::TreeParsing;

use crate::{Asset, PathConfig, Configuration, I18n, package_name, ReadingInfo, Themes};
use crate::book::{Book, Colors, Line};
use crate::color::Color32;
use crate::common::{Position, reading_info, txt_lines};
use crate::container::{BookContent, BookName, Container, load_book, load_container};
use crate::controller::Controller;
use crate::gui::render::RenderContext;
use crate::gui::dict::DictionaryManager;
use crate::gui::view::GuiView;

mod render;
mod dict;
mod view;
mod math;
mod settings;
mod chapter_list;

const APP_ID: &str = "net.lzrj.tbr";
const ICON_SIZE: i32 = 32;
const INLINE_ICON_SIZE: i32 = 16;
const MIN_FONT_SIZE: u8 = 20;
const MAX_FONT_SIZE: u8 = 50;
const FONT_FILE_EXTENSIONS: [&str; 3] = ["ttf", "otf", "ttc"];
const SIDEBAR_CHAPTER_LIST_NAME: &str = "chapter_list";
const SIDEBAR_DICT_NAME: &str = "dictionary_list";
const BOOK_NAME_LABEL_CLASS: &str = "book-name";
const TOC_LABEL_CLASS: &str = "toc";
const COPY_CONTENT_KEY: &str = "copy-content";
const DICT_LOOKUP_KEY: &str = "lookup-dictionary";

const README_TEXT_FILENAME: &str = "readme";

type GuiController = Controller<RenderContext, GuiView>;
type IconMap = HashMap<String, Pixbuf>;

struct ReadmeContainer {
	book_names: Vec<BookName>,
	text: String,
}

impl ReadmeContainer {
	#[inline]
	fn new(text: &str) -> Self
	{
		ReadmeContainer {
			book_names: vec![BookName::new(README_TEXT_FILENAME.to_string(), 0)],
			text: text.to_string(),
		}
	}
}

impl Container for ReadmeContainer {
	#[inline]
	fn inner_book_names(&self) -> &Vec<BookName>
	{
		&self.book_names
	}

	#[inline]
	fn book_content(&mut self, _inner_index: usize) -> Result<BookContent>
	{
		Ok(BookContent::Buf(self.text.as_bytes().to_vec()))
	}
}

struct ReadmeBook {
	lines: Vec<Line>,
}

impl ReadmeBook
{
	#[inline]
	fn new(text: &str) -> Self
	{
		ReadmeBook { lines: txt_lines(text) }
	}
}

impl Book for ReadmeBook
{
	#[inline]
	fn lines(&self) -> &Vec<Line>
	{
		&self.lines
	}
}

fn convert_colors(theme: &Theme) -> Colors
{
	fn convert_base(base_color: &BaseColor) -> Color32
	{
		match base_color {
			BaseColor::Black => Color32::BLACK,
			BaseColor::Red => Color32::RED,
			BaseColor::Green => Color32::GREEN,
			BaseColor::Yellow => Color32::YELLOW,
			BaseColor::Blue => Color32::BLUE,
			BaseColor::Magenta => Color32::from_rgb(255, 0, 255),
			BaseColor::Cyan => Color32::from_rgb(0, 255, 255),
			BaseColor::White => Color32::WHITE,
		}
	}
	fn convert(color: &Color) -> Color32
	{
		match color {
			Color::TerminalDefault => Color32::BLACK,
			Color::Dark(base_color)
			| Color::Light(base_color) => convert_base(base_color),
			Color::Rgb(r, g, b)
			| Color::RgbLowRes(r, g, b) => Color32::from_rgb(*r, *g, *b),
		}
	}
	let color = convert(theme.palette.index(PaletteColor::Primary));
	let background = convert(theme.palette.index(PaletteColor::Background));
	let highlight = convert(theme.palette.index(PaletteColor::HighlightText));
	let highlight_background = convert(theme.palette.index(PaletteColor::Highlight));
	let link = convert(theme.palette.index(PaletteColor::Secondary));
	Colors { color, background, highlight, highlight_background, link }
}

fn setup_fonts(font_paths: &Vec<PathConfig>) -> Result<Option<Vec<FontVec>>>
{
	if font_paths.is_empty() {
		Ok(None)
	} else {
		let mut fonts = vec![];
		for config in font_paths {
			if config.enabled {
				let mut file = OpenOptions::new().read(true).open(&config.path)?;
				let mut buf = vec![];
				file.read_to_end(&mut buf)?;
				fonts.push(FontVec::try_from_vec(buf)?);
			}
		}
		Ok(Some(fonts))
	}
}

pub(self) fn load_image(bytes: &[u8]) -> Option<Pixbuf>
{
	let bytes = Bytes::from(bytes);
	let stream = MemoryInputStream::from_bytes(&bytes);
	let image = Pixbuf::from_stream(&stream, None::<&Cancellable>).ok()?;
	Some(image)
}

fn build_ui(app: &Application, cfg: Rc<RefCell<Configuration>>, themes: Themes) -> Result<GuiContext>
{
	let mut configuration = cfg.borrow_mut();
	let conf_ref: &mut Configuration = &mut configuration;
	let reading = if let Some(current) = &conf_ref.current {
		Some(reading_info(&mut conf_ref.history, current).1)
	} else {
		None
	};
	let dark_colors = convert_colors(themes.get(true));
	let bright_colors = convert_colors(themes.get(false));
	let colors = if configuration.dark_theme {
		dark_colors.clone()
	} else {
		bright_colors.clone()
	};
	let fonts = setup_fonts(&configuration.gui.fonts)?;
	let fonts = Rc::new(fonts);
	let container_manager = Default::default();
	let i18n = I18n::new(&configuration.gui.lang).unwrap();
	let (container, book, reading, book_name) = if let Some(mut reading) = reading {
		let mut container = load_container(&container_manager, &reading)?;
		let book = load_book(&container_manager, &mut container, &mut reading)?;
		let title = reading.filename.clone();
		(container, book, reading, title)
	} else {
		let readme = i18n.msg("readme");
		let container: Box<dyn Container> = Box::new(ReadmeContainer::new(readme.as_ref()));
		let book: Box<dyn Book> = Box::new(ReadmeBook::new(readme.as_ref()));
		(container, book, ReadingInfo::new(README_TEXT_FILENAME), "The e-book reader".to_owned())
	};

	let i18n = Rc::new(i18n);
	let icons = load_icons();
	let icons = Rc::new(icons);

	let mut render_context = RenderContext::new(colors, configuration.gui.font_size,
		reading.custom_color, book.leading_space());
	let view = GuiView::new(
		"main",
		configuration.render_han,
		fonts.clone(),
		&mut render_context);
	let css_provider = view::init_css("main", &render_context.colors.background);
	let (dm, dict_view, lookup_entry) = DictionaryManager::new(
		&configuration.gui.dictionaries,
		configuration.gui.font_size,
		fonts,
		&i18n,
		&icons,
	);

	let controller = Controller::from_data(reading, container_manager, container, book, Box::new(view.clone()));

	let (render_icon, render_tooltip) = get_render_icon(configuration.render_han, &i18n);
	let (theme_icon, theme_tooltip) = get_theme_icon(configuration.dark_theme, &i18n);
	let (custom_color_icon, custom_color_tooltip) = get_custom_color_icon(controller.reading.custom_color, &i18n);
	drop(configuration);

	let ctx = Rc::new(RefCell::new(render_context));
	let ctrl = Rc::new(RefCell::new(controller));
	let gc = GuiContext::new(app, &cfg, &ctrl, &ctx, icons, i18n.clone(),
		dark_colors, bright_colors, css_provider);

	// now setup ui
	let (paned, stack) = setup_sidebar(&gc, &view, &dm, &dict_view);
	setup_view(&gc, &view, &stack, &dm);

	chapter_list::init(&gc);

	let (toolbar, sidebar_btn, render_btn, theme_btn, search_box)
		= setup_toolbar(&gc, &dm, &view, &paned, &lookup_entry,
		render_icon, &render_tooltip, theme_icon, &theme_tooltip,
		custom_color_icon, &custom_color_tooltip);

	{
		let gc = gc.clone();
		search_box.connect_activate(move |entry| {
			let search_pattern = entry.text();
			handle(&gc, |controller, render_context| {
				controller.search(&search_pattern, render_context)?;
				controller.render.grab_focus();
				Ok(())
			});
		});
		let view = view.clone();
		search_box.connect_stop_search(move |_| {
			view.grab_focus();
		});
	}
	{
		let gc = gc.clone();
		let dm = dm.clone();
		let key_event = EventControllerKey::new();
		key_event.connect_key_pressed(move |_, key, _, modifier| {
			const MODIFIER_NONE: ModifierType = ModifierType::empty();
			match (key, modifier) {
				(Key::space | Key::Page_Down, MODIFIER_NONE) => {
					handle(&gc, |controller, render_context|
						controller.next_page(render_context));
					gtk4::Inhibit(true)
				}
				(Key::space, ModifierType::SHIFT_MASK) | (Key::Page_Up, MODIFIER_NONE) => {
					handle(&gc, |controller, render_context|
						controller.prev_page(render_context));
					gtk4::Inhibit(true)
				}
				(Key::Home, MODIFIER_NONE) => {
					apply(&gc, |controller, render_context|
						controller.redraw_at(0, 0, render_context));
					gtk4::Inhibit(true)
				}
				(Key::End, MODIFIER_NONE) => {
					apply(&gc, |controller, render_context|
						controller.goto_end(render_context));
					gtk4::Inhibit(true)
				}
				(Key::Down, MODIFIER_NONE) => {
					apply(&gc, |controller, render_context|
						controller.step_next(render_context));
					gtk4::Inhibit(true)
				}
				(Key::Up, MODIFIER_NONE) => {
					apply(&gc, |controller, render_context|
						controller.step_prev(render_context));
					gtk4::Inhibit(true)
				}
				(Key::n, MODIFIER_NONE) => {
					handle(&gc, |controller, render_context|
						controller.search_again(true, render_context));
					gtk4::Inhibit(true)
				}
				(Key::N, ModifierType::SHIFT_MASK) => {
					handle(&gc, |controller, render_context|
						controller.search_again(false, render_context));
					gtk4::Inhibit(true)
				}
				(Key::d, ModifierType::CONTROL_MASK) => {
					handle(&gc, |controller, render_context|
						controller.switch_chapter(true, render_context));
					gtk4::Inhibit(true)
				}
				(Key::b, ModifierType::CONTROL_MASK) => {
					handle(&gc, |controller, render_context|
						controller.switch_chapter(false, render_context));
					gtk4::Inhibit(true)
				}
				(Key::Right, MODIFIER_NONE) => {
					handle(&gc, |controller, render_context|
						controller.goto_trace(false, render_context));
					gtk4::Inhibit(true)
				}
				(Key::Left, MODIFIER_NONE) => {
					handle(&gc, |controller, render_context|
						controller.goto_trace(true, render_context));
					gtk4::Inhibit(true)
				}
				(Key::Tab, MODIFIER_NONE) => {
					apply(&gc, |controller, render_context|
						controller.switch_link_next(render_context));
					gtk4::Inhibit(true)
				}
				(Key::Tab, ModifierType::SHIFT_MASK) => {
					apply(&gc, |controller, render_context|
						controller.switch_link_prev(render_context));
					gtk4::Inhibit(true)
				}
				(Key::Return, MODIFIER_NONE) => {
					handle(&gc, |controller, render_context|
						controller.try_goto_link(render_context));
					gtk4::Inhibit(true)
				}
				(Key::equal, ModifierType::CONTROL_MASK) => {
					apply(&gc, |controller, render_context| {
						let mut configuration = cfg.borrow_mut();
						if configuration.gui.font_size < MAX_FONT_SIZE {
							configuration.gui.font_size += 2;
							controller.render.set_font_size(configuration.gui.font_size, render_context);
							controller.redraw(render_context);
							dm.borrow_mut().set_font_size(configuration.gui.font_size);
						}
					});
					gtk4::Inhibit(true)
				}
				(Key::minus, ModifierType::CONTROL_MASK) => {
					apply(&gc, |controller, render_context| {
						let mut configuration = gc.cfg_mut();
						if configuration.gui.font_size > MIN_FONT_SIZE {
							configuration.gui.font_size -= 2;
							controller.render.set_font_size(configuration.gui.font_size, render_context);
							controller.redraw(render_context);
							dm.borrow_mut().set_font_size(configuration.gui.font_size);
						}
					});
					gtk4::Inhibit(true)
				}
				(Key::c, ModifierType::CONTROL_MASK) => {
					if let Some(selected_text) = ctrl.borrow().selected() {
						if let Some(display) = Display::default() {
							display.clipboard().set_text(selected_text);
						}
					}
					gtk4::Inhibit(true)
				}
				_ => {
					// println!("view, key: {key}, modifier: {modifier}");
					gtk4::Inhibit(false)
				}
			}
		});
		view.add_controller(key_event);
	}

	let main = gtk4::Box::new(Orientation::Vertical, 0);
	main.append(&toolbar);
	main.append(&paned);

	setup_window(&gc, main, view, stack, paned, sidebar_btn, render_btn, theme_btn,
		search_box, dm, book_name);
	Ok(gc)
}

#[inline]
fn apply<F>(gc: &GuiContext, f: F)
	where F: FnOnce(&mut GuiController, &mut RenderContext)
{
	let mut controller = gc.ctrl_mut();
	let orig_inner_book = controller.reading.inner_book;
	f(&mut controller, &mut gc.ctx_mut());
	gc.update(&controller.status_msg(), orig_inner_book, &controller);
}

#[inline]
fn handle<T, F>(gc: &GuiContext, f: F)
	where F: FnOnce(&mut GuiController, &mut RenderContext) -> Result<T>
{
	let (orig_inner_book, result) = {
		let mut controller = gc.ctrl_mut();
		let orig_inner_book = controller.reading.inner_book;
		let result = f(&mut controller, &mut gc.ctx_mut());
		(orig_inner_book, result)
	};
	match result {
		Ok(_) => {
			let controller = gc.ctrl();
			let msg = controller.status_msg();
			gc.update(&msg, orig_inner_book, &controller);
		}
		Err(err) => gc.error(&err.to_string()),
	}
}

fn load_icons() -> IconMap
{
	const ICONS_PREFIX: &str = "gui/image/";
	let mut map = HashMap::new();
	for file in Asset::iter() {
		if file.starts_with(ICONS_PREFIX) && file.ends_with(".svg") {
			let content = Asset::get(file.as_ref()).unwrap().data;
			let rtree = {
				let opt = usvg::Options::default();
				let tree = usvg::Tree::from_data(&content, &opt).unwrap();
				resvg::Tree::from_usvg(&tree)
			};
			let pixmap_size = rtree.size.to_int_size();
			let mut pixmap = tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height()).unwrap();
			rtree.render(tiny_skia::Transform::default(), &mut pixmap.as_mut());
			let png = pixmap.encode_png().unwrap();
			let bytes = Bytes::from(&png);
			let mis = MemoryInputStream::from_bytes(&bytes);
			let pixbuf = Pixbuf::from_stream(&mis, None::<&Cancellable>).unwrap();
			let name = &file[ICONS_PREFIX.len()..];
			map.insert(name.to_string(), pixbuf);
		}
	}
	map
}

#[allow(unused)]
fn setup_popup_menu(gc: &GuiContext) -> PopoverMenu
{
	let action_group = SimpleActionGroup::new();
	let menu = Menu::new();
	let label = gc.status_bar();
	let i18n = gc.i18n();

	label.insert_action_group("popup", Some(gc.action_group()));

	let copy_action = SimpleAction::new(COPY_CONTENT_KEY, None);
	{
		let gc = gc.clone();
		copy_action.connect_activate(move |_, _| {
			if let Some(selected_text) = gc.ctrl().selected() {
				println!("copy {}", selected_text);
			}
		});
	}
	action_group.add_action(&copy_action);
	let title = i18n.msg(COPY_CONTENT_KEY);
	let action_name = format!("popup.{}", COPY_CONTENT_KEY);
	menu.append(Some(&title), Some(&action_name));

	let lookup_action = SimpleAction::new(DICT_LOOKUP_KEY, None);
	{
		let gc = gc.clone();
		lookup_action.connect_activate(move |_, _| {
			if let Some(selected_text) = gc.ctrl().selected() {
				println!("lookup {}", selected_text);
			}
		});
	}
	action_group.add_action(&lookup_action);
	let title = i18n.msg(DICT_LOOKUP_KEY);
	let menu_action_name = format!("popup.{}", DICT_LOOKUP_KEY);
	menu.append(Some(&title), Some(&menu_action_name));

	let pm = PopoverMenu::builder()
		.has_arrow(false)
		.menu_model(&MenuModel::from(menu))
		.build();
	pm.set_parent(label);
	pm
}

fn setup_view(gc: &GuiContext, view: &GuiView, stack: &Stack,
	dm: &Rc<RefCell<DictionaryManager>>)
{
	#[inline]
	fn select_text(gc: &GuiContext, from_line: u64, from_offset: u64, to_line: u64, to_offset: u64)
	{
		let from = Position::new(from_line as usize, from_offset as usize);
		let to = Position::new(to_line as usize, to_offset as usize);
		gc.ctrl_mut().select_text(from, to, &mut gc.ctx_mut());
	}

	{
		let gc = gc.clone();
		view.connect_resize(move |view, width, height| {
			let mut render_context = gc.ctx_mut();
			let mut controller = gc.ctrl_mut();
			view.resized(width, height, &mut render_context);
			controller.redraw(&mut render_context);
		});
	}

	{
		let gc = gc.clone();
		view.setup_gesture(false, move |view, pos| {
			let controller = gc.ctrl();
			view.link_resolve(pos, controller.book.lines())
		});
	}

	/* no way to position popup menu next to mouse...
	{
		// right click
		let right_click = GestureClick::builder()
			.button(gdk::BUTTON_SECONDARY)
			.build();
		let popup_menu = setup_popup_menu(gc);
		let gc = gc.clone();
		right_click.connect_pressed(move |_, _, _, _| {
			if gc.ctrl().has_selection() {
				popup_menu.popup();
			}
		});
		view.add_controller(right_click);
	}
	*/
	{
		// open link signal
		let gc = gc.clone();
		view.connect_closure(
			GuiView::OPEN_LINK_SIGNAL,
			false,
			closure_local!(move |_: GuiView, line: u64, link_index: u64| {
				handle(&gc, |controller, render_context|
					controller.goto_link(line as usize,	link_index as usize, render_context));
	        }),
		);
	}

	// select text signal
	{
		let gc = gc.clone();
		view.connect_closure(
			GuiView::SELECTING_TEXT_SIGNAL,
			false,
			closure_local!(move |_: GuiView, from_line: u64, from_offset: u64, to_line: u64, to_offset: u64| {
				select_text(&gc, from_line, from_offset, to_line, to_offset);
	        }),
		);
	}

	// text selected signal
	{
		let gc = gc.clone();
		let stack = stack.clone();
		let dm = dm.clone();
		view.connect_closure(
			GuiView::TEXT_SELECTED_SIGNAL,
			false,
			closure_local!(move |_: GuiView, from_line: u64, from_offset: u64, to_line: u64, to_offset: u64| {
				select_text(&gc, from_line, from_offset, to_line, to_offset);
				if let Some(selected_text) = gc.ctrl().selected() {
					if let Some(current_tab) = stack.visible_child_name() {
						if current_tab == SIDEBAR_DICT_NAME {
							dm.borrow_mut().set_lookup(selected_text.to_owned());
						}
					}
				}
			}),
		);
	}

	{
		// clear selection signal
		let gc = gc.clone();
		view.connect_closure(
			GuiView::CLEAR_SELECTION_SIGNAL,
			false,
			closure_local!(move |_: GuiView| {
				gc.ctrl_mut().clear_highlight(&mut gc.ctx_mut());
	        }),
		);
	}

	{
		// scroll signal
		let gc = gc.clone();
		view.connect_closure(
			GuiView::SCROLL_SIGNAL,
			false,
			closure_local!(move |_: GuiView, delta: i32| {
				if delta > 0 {
					apply(&gc, |controller, render_context|
						controller.step_next(render_context));
				} else {
					apply(&gc, |controller, render_context|
						controller.step_prev(render_context));
				}
	        }),
		);
	}
}

fn setup_sidebar(gc: &GuiContext, view: &GuiView,
	dm: &Rc<RefCell<DictionaryManager>>, dict_view: &gtk4::Box)
	-> (Paned, Stack)
{
	let chapter_list_view = gtk4::ScrolledWindow::builder()
		.child(gc.chapter_list())
		.hscrollbar_policy(PolicyType::Never)
		.build();

	let i18n = gc.i18n();
	let stack = Stack::builder()
		.vexpand(true)
		.build();
	stack.add_titled(
		&chapter_list_view,
		Some(SIDEBAR_CHAPTER_LIST_NAME), &i18n.msg("tab-chapter"));
	stack.add_titled(
		dict_view,
		Some(SIDEBAR_DICT_NAME), &i18n.msg("tab-dictionary"));
	stack.set_visible_child(&chapter_list_view);

	let sidebar_tab_switch = gtk4::StackSwitcher::builder()
		.stack(&stack)
		.build();
	let sidebar = gtk4::Box::new(Orientation::Vertical, 0);
	sidebar.append(&sidebar_tab_switch);
	sidebar.append(&stack);

	let paned = Paned::builder()
		.orientation(Orientation::Horizontal)
		.start_child(&sidebar)
		.end_child(view)
		.position(0)
		.build();
	let gc = gc.clone();
	let dm = dm.clone();
	paned.connect_position_notify(move |p| {
		let position = p.position();
		if position > 0 {
			gc.cfg_mut().gui.sidebar_size = position as u32;
			dm.borrow_mut().resize(position, None);
		}
	});

	(paned, stack)
}

#[inline]
fn setup_window(gc: &GuiContext, main: gtk4::Box, view: GuiView, stack: Stack,
	paned: Paned, sidebar_btn: Button, render_btn: Button, theme_btn: Button,
	search_box: SearchEntry, dm: Rc<RefCell<DictionaryManager>>,
	book_name: String)
{
	fn switch_stack(tab_name: &str, stack: &Stack, paned: &Paned,
		sidebar_btn: &Button, gc: &GuiContext) -> bool
	{
		if paned.position() == 0 {
			stack.set_visible_child_name(tab_name);
			toggle_sidebar(sidebar_btn, paned, gc);
			true
		} else if let Some(current_tab_name) = stack.visible_child_name() {
			if current_tab_name == tab_name {
				toggle_sidebar(sidebar_btn, paned, gc);
				false
			} else {
				stack.set_visible_child_name(tab_name);
				true
			}
		} else {
			stack.set_visible_child_name(tab_name);
			true
		}
	}

	let window = gc.win();
	window.set_child(Some(&main));
	window.set_default_widget(Some(&view));
	window.set_focus(Some(&view));
	update_title(window, &book_name);

	{
		let window_key_event = EventControllerKey::new();
		let gc = gc.clone();
		window_key_event.connect_key_pressed(move |_, key, _, modifier| {
			const MODIFIER_NONE: ModifierType = ModifierType::empty();
			match (key, modifier) {
				(Key::c, MODIFIER_NONE) => {
					switch_stack(SIDEBAR_CHAPTER_LIST_NAME, &stack, &paned, &sidebar_btn, &gc);
					gtk4::Inhibit(true)
				}
				(Key::d, MODIFIER_NONE) => {
					if switch_stack(SIDEBAR_DICT_NAME, &stack, &paned, &sidebar_btn, &gc) {
						if let Some(selected_text) = gc.ctrl().selected() {
							dm.borrow_mut().set_lookup(selected_text.to_owned());
						}
					}
					gtk4::Inhibit(true)
				}
				(Key::slash, MODIFIER_NONE) | (Key::f, ModifierType::CONTROL_MASK) => {
					search_box.grab_focus();
					gtk4::Inhibit(true)
				}
				(Key::Escape, MODIFIER_NONE) => {
					if paned.position() != 0 {
						toggle_sidebar(&sidebar_btn, &paned, &gc);
						gtk4::Inhibit(true)
					} else {
						gtk4::Inhibit(false)
					}
				}
				(Key::x, ModifierType::CONTROL_MASK) => {
					switch_render(&render_btn, &gc);
					gtk4::Inhibit(true)
				}
				(Key::t, MODIFIER_NONE) => {
					switch_theme(&theme_btn, &gc);
					gtk4::Inhibit(true)
				}
				_ => {
					// println!("window, key: {key}, modifier: {modifier}");
					gtk4::Inhibit(false)
				}
			}
		});
		window.add_controller(window_key_event);
	}
	{
		let gc = gc.clone();
		window.connect_close_request(move |_| {
			let controller = gc.ctrl();
			if controller.reading.filename != README_TEXT_FILENAME {
				let mut configuration = gc.cfg_mut();
				configuration.current = Some(controller.reading.filename.clone());
				configuration.history.push(controller.reading.clone());
			}
			let configuration = gc.cfg();
			if let Err(e) = configuration.save() {
				println!("Failed save configuration: {}", e.to_string());
			}
			gtk4::Inhibit(false)
		});
	}

	window.present();
}

#[inline(always)]
fn get_render_icon<'a>(render_han: bool, i18n: &'a I18n) -> (&'static str, Cow<'a, str>) {
	if render_han {
		("render_xi.svg", i18n.msg("render-xi"))
	} else {
		("render_han.svg", i18n.msg("render-han"))
	}
}

#[inline(always)]
fn get_theme_icon(dark_theme: bool, i18n: &I18n) -> (&'static str, Cow<str>) {
	if dark_theme {
		("theme_bright.svg", i18n.msg("theme-bright"))
	} else {
		("theme_dark.svg", i18n.msg("theme-dark"))
	}
}

#[inline(always)]
fn get_custom_color_icon(custom_color: bool, i18n: &I18n) -> (&'static str, Cow<str>) {
	if custom_color {
		("custom_color_off.svg", i18n.msg("no-custom-color"))
	} else {
		("custom_color_on.svg", i18n.msg("with-custom-color"))
	}
}

fn toggle_sidebar(sidebar_btn: &Button, paned: &Paned, gc: &GuiContext)
{
	let (icon, tooltip, position) = if paned.position() == 0 {
		("sidebar_off.svg", gc.i18n().msg("sidebar-off"), gc.cfg().gui.sidebar_size as i32)
	} else {
		paned.end_child().unwrap().grab_focus();
		("sidebar_on.svg", gc.i18n().msg("sidebar-on"), 0)
	};
	update_button(sidebar_btn, icon, &tooltip, gc.icons());
	paned.set_position(position);
}

fn switch_render(render_btn: &Button, gc: &GuiContext)
{
	let mut configuration = gc.cfg_mut();
	let render_han = !configuration.render_han;
	configuration.render_han = render_han;
	let (render_icon, render_tooltip) = get_render_icon(render_han, gc.i18n());
	update_button(render_btn, render_icon, &render_tooltip, gc.icons());
	let mut controller = gc.ctrl_mut();
	let mut render_context = gc.ctx_mut();
	controller.render.reload_render(render_han, &mut render_context);
	controller.redraw(&mut render_context);
}

fn switch_theme(theme_btn: &Button, gc: &GuiContext)
{
	let mut configuration = gc.cfg_mut();
	let dark_theme = !configuration.dark_theme;
	configuration.dark_theme = dark_theme;
	let (theme_icon, theme_tooltip) = get_theme_icon(dark_theme, gc.i18n());
	update_button(theme_btn, theme_icon, &theme_tooltip, gc.icons());
	let mut render_context = gc.ctx_mut();
	render_context.colors = if dark_theme {
		gc.dark_colors().clone()
	} else {
		gc.bright_colors().clone()
	};
	let mut controller = gc.ctrl_mut();
	controller.redraw(&mut render_context);
	view::update_css(gc.css_provider(), "main", &render_context.colors.background);
}

fn switch_custom_color(custom_color_btn: &Button, gc: &GuiContext)
{
	let mut controller = gc.ctrl_mut();
	let custom_color = !controller.reading.custom_color;
	controller.reading.custom_color = custom_color;
	let (custom_color_icon, custom_color_tooltip) = get_custom_color_icon(custom_color, gc.i18n());
	update_button(custom_color_btn, custom_color_icon, &custom_color_tooltip, gc.icons());
	let mut render_context = gc.ctx_mut();
	render_context.custom_color = custom_color;
	controller.redraw(&mut render_context);
}

#[inline]
fn setup_toolbar(gc: &GuiContext, dm: &Rc<RefCell<DictionaryManager>>,
	view: &GuiView, paned: &Paned, lookup_entry: &SearchEntry,
	render_icon: &str, render_tooltip: &str,
	theme_icon: &str, theme_tooltip: &str,
	custom_color_icon: &str, custom_color_tooltip: &str,
) -> (gtk4::Box, Button, Button, Button, SearchEntry)
{
	let i18n = gc.i18n();
	let icons = gc.icons();

	let toolbar = gtk4::Box::builder()
		.css_classes(vec!["toolbar"])
		.build();

	let sidebar_button = create_button("sidebar_on.svg", &i18n.msg("sidebar-on"), &icons, false);
	{
		let paned = paned.clone();
		let gc = gc.clone();
		sidebar_button.connect_clicked(move |sidebar_btn| {
			toggle_sidebar(sidebar_btn, &paned, &gc);
		});
		toolbar.append(&sidebar_button);
	}

	{
		let paned = paned.clone();
		let gc = gc.clone();
		let sidebar_button = sidebar_button.clone();
		lookup_entry.connect_stop_search(move |_| {
			toggle_sidebar(&sidebar_button, &paned, &gc);
		});
	}

	// add file drop support
	{
		let drop_target = DropTarget::new(File::static_type(), DragAction::COPY);
		let gc = gc.clone();
		drop_target.connect_drop(move |_, value, _, _| {
			if let Ok(file) = value.get::<File>() {
				if let Some(path) = file.path() {
					gc.open_file(&path);
					return true;
				}
			}
			false
		});
		view.add_controller(drop_target);
	}

	let file_button = create_button("file_open.svg", &i18n.msg("file-open"), &icons, false);
	let file_dialog = FileDialog::new();
	file_dialog.set_title(&i18n.msg("file-open-title"));
	file_dialog.set_modal(true);
	let filter = FileFilter::new();
	for ext in gc.ctrl().container_manager.book_loader.extension() {
		filter.add_suffix(&ext[1..]);
	}
	file_dialog.set_default_filter(Some(&filter));

	{
		let gc = gc.clone();
		file_button.connect_clicked(move |_| {
			let gc2 = gc.clone();
			file_dialog.open(Some(gc.win()), None::<&Cancellable>, move |result| {
				if let Ok(file) = result {
					if let Some(path) = file.path() {
						gc2.open_file(&path);
					}
				}
			});
		});
		toolbar.append(&file_button);
	}
	gc.reload_history();

	let history_button = create_button("history.svg", &i18n.msg("history"), &icons, false);
	history_button.insert_action_group("popup", Some(gc.action_group()));
	let menu_model = MenuModel::from(gc.menu().clone());
	let history_menu = PopoverMenu::builder()
		.menu_model(&menu_model)
		.build();
	history_menu.set_parent(&history_button);
	history_menu.set_has_arrow(false);
	let bv = view.clone();
	history_menu.connect_visible_notify(move |_| {
		bv.grab_focus();
	});
	history_button.connect_clicked(move |_| {
		history_menu.popup();
	});
	toolbar.append(&history_button);

	let render_button = create_button(render_icon, render_tooltip, &icons, false);
	{
		let gc = gc.clone();
		render_button.connect_clicked(move |btn| {
			switch_render(btn, &gc);
		});
		toolbar.append(&render_button);
	}

	let theme_button = create_button(theme_icon, theme_tooltip, &icons, false);
	{
		let gc = gc.clone();
		theme_button.connect_clicked(move |btn| {
			switch_theme(btn, &gc);
		});
		toolbar.append(&theme_button);
	}

	let custom_color_button = create_button(custom_color_icon, custom_color_tooltip, &icons, false);
	{
		let gc = gc.clone();
		custom_color_button.connect_clicked(move |btn| {
			switch_custom_color(btn, &gc);
		});
		toolbar.append(&custom_color_button);
	}

	let settings_button = create_button("setting.svg", &i18n.msg("settings-dialog"), &icons, false);
	{
		let gc = gc.clone();
		let dm = dm.clone();
		settings_button.connect_clicked(move |_| {
			settings::show(&gc, &dm);
		});
		toolbar.append(&settings_button);
	}

	let search_box = SearchEntry::builder()
		.placeholder_text(i18n.msg("search-hint"))
		.activates_default(true)
		.enable_undo(true)
		.build();
	toolbar.append(&search_box);

	let status_bar = gc.status_bar();
	status_bar.set_halign(Align::End);
	status_bar.set_hexpand(true);
	toolbar.append(status_bar);

	(toolbar, sidebar_button, render_button, theme_button, search_box)
}

#[inline]
fn create_button(name: &str, tooltip: &str, icons: &IconMap, inline: bool) -> Button
{
	let pixbuf = icons.get(name).unwrap();
	let image = Image::from_pixbuf(Some(pixbuf));
	if inline {
		image.set_width_request(INLINE_ICON_SIZE);
		image.set_height_request(INLINE_ICON_SIZE);
	} else {
		image.set_width_request(ICON_SIZE);
		image.set_height_request(ICON_SIZE);
	}
	let button = Button::builder()
		.child(&image)
		.focus_on_click(false)
		.focusable(false)
		.tooltip_text(tooltip)
		.build();
	if inline {
		button.add_css_class("inline");
		button.set_valign(Align::Center);
	}
	button
}

#[inline]
fn update_button(btn: &Button, name: &str, tooltip: &str, icons: &IconMap)
{
	let pixbuf = icons.get(name).unwrap();
	let image = Image::from_pixbuf(Some(pixbuf));
	image.set_width_request(ICON_SIZE);
	image.set_height_request(ICON_SIZE);
	btn.set_tooltip_text(Some(tooltip));
	btn.set_child(Some(&image));
}

#[inline(always)]
fn update_title(window: &ApplicationWindow, book_name: &str)
{
	let title = format!("{} - {}", package_name!(), book_name);
	window.set_title(Some(&title));
}

fn apply_settings(locale: &str, fonts: Vec<PathConfig>, dictionaries: Vec<PathConfig>, gc: &GuiContext,
	dictionary_manager: &mut DictionaryManager) -> Result<(), (String, String)>
{
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
	let mut configuration = gc.cfg_mut();
	let i18n = gc.i18n();
	// need restart
	configuration.gui.lang = locale.to_owned();

	let new_fonts = if paths_modified(&configuration.gui.fonts, &fonts) {
		let new_fonts = match setup_fonts(&fonts) {
			Ok(fonts) => fonts,
			Err(err) => {
				let title = i18n.msg("font-files");
				let t = title.to_string();
				let message = i18n.args_msg("invalid-path", vec![
					("title", title),
					("path", err.to_string().into()),
				]);
				return Err((t, message));
			}
		};
		Some(new_fonts)
	} else {
		None
	};

	if paths_modified(&configuration.gui.dictionaries, &dictionaries) {
		dictionary_manager.reload(&dictionaries);
		configuration.gui.dictionaries = dictionaries;
	};

	if let Some(new_fonts) = new_fonts {
		let fonts_data = Rc::new(new_fonts);
		dictionary_manager.set_fonts(fonts_data.clone());
		let mut controller = gc.ctrl_mut();
		let mut render_context = gc.ctx_mut();
		controller.render.set_fonts(fonts_data, &mut render_context);
		controller.redraw(&mut render_context);
		configuration.gui.fonts = fonts;
	}
	Ok(())
}

#[cfg(unix)]
#[inline]
fn setup_icon() -> Result<()>
{
	use std::fs;
	use dirs::home_dir;

	let home_dir = home_dir().expect("No home folder");
	let icon_path = home_dir.join(".local/share/icons/hicolor/256x256/apps");
	let icon_file = icon_path.join("tbr-icon.png");
	if !icon_file.exists() {
		fs::create_dir_all(&icon_path)?;
		fs::write(&icon_file, include_bytes!("../assets/gui/tbr-icon.png"))?;
	}
	Ok(())
}

struct GuiContextInner {
	cfg: Rc<RefCell<Configuration>>,
	ctrl: Rc<RefCell<GuiController>>,
	ctx: Rc<RefCell<RenderContext>>,
	window: ApplicationWindow,
	menu: Menu,
	action_group: SimpleActionGroup,
	status_bar: Label,
	chapter_list: ListBox,
	chapter_model: ListStore,
	icons: Rc<IconMap>,
	i18n: Rc<I18n>,
	dark_colors: Colors,
	bright_colors: Colors,
	css_provider: CssProvider,
}

#[derive(Clone)]
struct GuiContext {
	inner: Rc<GuiContextInner>,
}

impl GuiContext {
	fn new(app: &Application,
		cfg: &Rc<RefCell<Configuration>>, ctrl: &Rc<RefCell<GuiController>>,
		ctx: &Rc<RefCell<RenderContext>>, icons: Rc<IconMap>, i18n: Rc<I18n>,
		dark_colors: Colors, bright_colors: Colors, css_provider: CssProvider) -> Self
	{
		let window = ApplicationWindow::builder()
			.application(app)
			.default_width(800)
			.default_height(600)
			.maximized(true)
			.title(package_name!())
			.build();

		let (chapter_list, chapter_model) = chapter_list::create();

		let status_bar = Label::new(None);
		status_bar.set_label(&ctrl.borrow().status_msg());

		let inner = GuiContextInner {
			cfg: cfg.clone(),
			ctrl: ctrl.clone(),
			ctx: ctx.clone(),
			window,
			menu: Menu::new(),
			action_group: SimpleActionGroup::new(),
			status_bar,
			chapter_list,
			chapter_model,
			icons,
			i18n,
			dark_colors,
			bright_colors,
			css_provider,
		};
		GuiContext { inner: Rc::new(inner) }
	}

	#[inline]
	fn cfg(&self) -> Ref<Configuration>
	{
		self.inner.cfg.borrow()
	}

	#[inline]
	fn cfg_mut(&self) -> RefMut<Configuration>
	{
		self.inner.cfg.borrow_mut()
	}

	#[inline]
	fn ctrl(&self) -> Ref<GuiController>
	{
		self.inner.ctrl.borrow()
	}

	#[inline]
	fn ctrl_mut(&self) -> RefMut<GuiController>
	{
		self.inner.ctrl.borrow_mut()
	}

	#[inline]
	fn try_ctrl_mut(&self) -> Result<RefMut<GuiController>, BorrowMutError>
	{
		self.inner.ctrl.try_borrow_mut()
	}

	#[inline]
	fn ctx_mut(&self) -> RefMut<RenderContext>
	{
		self.inner.ctx.borrow_mut()
	}

	#[inline]
	fn win(&self) -> &ApplicationWindow
	{
		&self.inner.window
	}

	#[inline]
	fn icons(&self) -> &IconMap
	{
		&self.inner.icons
	}

	#[inline]
	fn i18n(&self) -> &I18n
	{
		&self.inner.i18n
	}

	#[inline]
	fn action_group(&self) -> &SimpleActionGroup
	{
		&self.inner.action_group
	}

	#[inline]
	fn menu(&self) -> &Menu
	{
		&self.inner.menu
	}

	#[inline]
	fn chapter_list(&self) -> &ListBox
	{
		&self.inner.chapter_list
	}

	#[inline]
	fn chapter_model(&self) -> &ListStore
	{
		&self.inner.chapter_model
	}

	#[inline]
	fn dark_colors(&self) -> &Colors
	{
		&self.inner.dark_colors
	}

	#[inline]
	fn bright_colors(&self) -> &Colors
	{
		&self.inner.bright_colors
	}

	#[inline]
	fn css_provider(&self) -> &CssProvider
	{
		&self.inner.css_provider
	}

	#[inline]
	fn status_bar(&self) -> &Label
	{
		&self.inner.status_bar
	}

	fn open_file(&self, path: &PathBuf)
	{
		if let Ok(absolute_path) = path.canonicalize() {
			if let Some(filepath) = absolute_path.to_str() {
				let mut controller = self.ctrl_mut();
				if filepath != controller.reading.filename {
					let mut configuration = self.cfg_mut();
					let mut render_context = self.ctx_mut();
					let reading_now = controller.reading.clone();
					let (history, new_reading) = reading_info(&mut configuration.history, filepath);
					let history_entry = if history { Some(new_reading.clone()) } else { None };
					match controller.switch_container(new_reading, &mut render_context) {
						Ok(msg) => {
							configuration.history.push(reading_now);
							update_title(self.win(), &controller.reading.filename);
							controller.redraw(&mut render_context);
							self.update(
								&msg,
								usize::MAX,
								&controller);
							drop(configuration);
							drop(controller);
							drop(render_context);
							self.reload_history();
						}
						Err(e) => {
							if let Some(history_entry) = history_entry {
								configuration.history.push(history_entry);
							}
							self.error(&e.to_string());
						}
					}
				}
			}
		}
	}

	fn reload_history(&self)
	{
		for a in self.action_group().list_actions() {
			self.action_group().remove_action(&a);
		}
		self.menu().remove_all();
		for (idx, ri) in self.cfg().history.iter().rev().enumerate() {
			if idx == 20 {
				break;
			}
			self.add_history_entry(idx, &ri.filename);
		}
	}

	#[inline]
	fn add_history_entry(&self, idx: usize, path_str: &String)
	{
		let path = PathBuf::from(&path_str);
		if !path.exists() || !path.is_file() {
			return;
		}
		let action_name = format!("a{}", idx);
		let action = SimpleAction::new(&action_name, None);
		{
			let gc = self.clone();
			action.connect_activate(move |_, _| {
				gc.open_file(&path);
			});
		}
		self.action_group().add_action(&action);
		let menu_action_name = format!("popup.{}", action_name);
		self.menu().append(Some(&path_str), Some(&menu_action_name));
	}

	#[inline]
	fn update(&self, msg: &str, orig_inner_book: usize, controller: &GuiController)
	{
		self.message(msg);
		self.sync_chapter_list(orig_inner_book, controller);
	}

	#[inline]
	fn message(&self, msg: &str)
	{
		self.update_status(false, msg);
	}

	#[inline]
	fn item(&self, position: u32) -> Option<Object>
	{
		self.inner.chapter_model.item(position)
	}

	fn sync_chapter_list(&self, orig_inner_book: usize, controller: &GuiController)
	{
		let chapter_list = &self.inner.chapter_list;
		let chapter_model = &self.inner.chapter_model;

		let inner_book = controller.reading.inner_book;
		if orig_inner_book != inner_book {
			chapter_model.remove_all();
			chapter_list::load_model(chapter_list, chapter_model, controller);
			return;
		}

		let toc_index = controller.toc_index() as u64;
		if let Some(row) = chapter_list.selected_row() {
			let index = row.index();
			if index >= 0 {
				if let Some(obj) = chapter_model.item(index as u32) {
					let entry = chapter_list::entry_cast(&obj);
					if entry.index() == toc_index {
						return;
					}
				}
			}
		}

		for i in 0..chapter_model.n_items() {
			if let Some(obj) = chapter_model.item(i) {
				let entry = chapter_list::entry_cast(&obj);
				if !entry.book() && entry.index() == toc_index {
					if let Some(row) = chapter_list.row_at_index(i as i32) {
						chapter_list.select_row(Some(&row));
					}
				}
			}
		}
	}

	#[inline]
	fn error(&self, msg: &str)
	{
		self.update_status(true, msg);
	}

	fn update_status(&self, error: bool, msg: &str)
	{
		if error {
			let markup = format!("<span foreground='red'>{msg}</span>");
			self.inner.status_bar.set_markup(&markup);
		} else {
			self.inner.status_bar.set_text(msg);
		};
	}
}

pub fn start(configuration: Configuration, themes: Themes) -> Result<()>
{
	#[cfg(unix)]
	setup_icon()?;

	let app = Application::builder()
		.application_id(APP_ID)
		.flags(ApplicationFlags::HANDLES_OPEN)
		.build();

	let gui_context = Rc::new(RefCell::new(None::<GuiContext>));
	let cfg = Rc::new(RefCell::new(configuration));
	app.connect_open(move |app, files, _| {
		let mut gui_context = gui_context.borrow_mut();
		if let Some(gc) = gui_context.as_ref() {
			if files.len() > 0 {
				if let Some(path) = files[0].path() {
					gc.open_file(&path)
				}
			}
		} else {
			let css_provider = CssProvider::new();
			css_provider.load_from_data(&format!("{}:focus-visible {{outline-style: dashed; outline-offset: -3px; outline-width: 3px;}} button.inline {{padding: 0px;min-height: 16px;}} label.{BOOK_NAME_LABEL_CLASS} {{font-size: large;}}", GuiView::WIDGET_NAME));
			gtk4::style_context_add_provider_for_display(
				&Display::default().expect("Could not connect to a display."),
				&css_provider,
				gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
			);
			Window::set_default_icon_name("tbr-icon");

			match build_ui(app, cfg.clone(), themes.clone()) {
				Ok(context) => {
					*gui_context = Some(context);
				}
				Err(err) => {
					eprintln!("Failed start tbr: {}", err.to_string());
					app.quit();
				}
			}
		}
	});

	// Run the application
	if app.run() == ExitCode::FAILURE {
		bail!("Failed start tbr")
	}

	Ok(())
}
