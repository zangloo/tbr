use std::env;
use std::borrow::Cow;
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::ops::{Deref, Index};
use std::path::PathBuf;
use std::rc::Rc;
use std::str::FromStr;
use anyhow::{bail, Result};
use cursive::theme::{BaseColor, Color, PaletteColor, Theme};
use gtk4::{Align, Application, ApplicationWindow, Button, CssProvider, DropTarget, EventControllerKey, FileDialog, FileFilter, gdk, GestureClick, HeaderBar, Image, Label, Orientation, Paned, Popover, PopoverMenu, PositionType, SearchEntry, Separator, Stack, ToggleButton, Widget, Window};
use gtk4::gdk::{Display, DragAction, Key, ModifierType, Rectangle, Texture};
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::gio::{ApplicationFlags, Cancellable, File, MemoryInputStream, Menu, MenuModel, SimpleAction, SimpleActionGroup};
use gtk4::glib;
use gtk4::glib::{Bytes, closure_local, ExitCode, format_size, ObjectExt, SignalHandlerId, StaticType};
use gtk4::graphene::Point;
use gtk4::prelude::{ActionGroupExt, ActionMapExt, ApplicationExt, ApplicationExtManual, BoxExt, ButtonExt, DisplayExt, DrawingAreaExt, EditableExt, FileExt, GtkWindowExt, IsA, NativeExt, PopoverExt, SeatExt, SurfaceExt, ToggleButtonExt, WidgetExt};
use pangocairo::pango::EllipsizeMode;
use resvg::{tiny_skia, usvg};
use resvg::usvg::TreeParsing;

use crate::{Asset, I18n, package_name};
use crate::book::{Book, Colors, Line};
use crate::color::Color32;
use crate::common::{Position, txt_lines};
use crate::config::{BookLoadingInfo, Configuration, PathConfig, ReadingInfo, Themes};
use crate::container::{BookContent, BookName, Container, load_book, load_container, title_for_filename};
use crate::controller::Controller;
use crate::gui::chapter_list::ChapterList;
use crate::gui::dict::DictionaryManager;
pub use crate::gui::font::HtmlFonts;
use crate::gui::render::RenderContext;
use crate::gui::view::{GuiView, update_mouse_pointer};
use crate::open::Opener;

mod render;
mod dict;
mod view;
mod math;
mod settings;
mod chapter_list;
mod font;

const MODIFIER_NONE: ModifierType = ModifierType::empty();

const APP_ID: &str = "net.lzrj.tbr";
const ICON_SIZE: i32 = 32;
const INLINE_ICON_SIZE: i32 = 16;
const MIN_FONT_SIZE: u8 = 20;
const MAX_FONT_SIZE: u8 = 50;
const FONT_FILE_EXTENSIONS: [&str; 3] = ["ttf", "otf", "ttc"];
const DICT_FILE_EXTENSIONS: [&str; 1] = ["ifo"];
const SIDEBAR_CHAPTER_LIST_NAME: &str = "chapter_list";
const SIDEBAR_DICT_NAME: &str = "dictionary_list";
const COPY_CONTENT_KEY: &str = "copy-content";
const DICT_LOOKUP_KEY: &str = "lookup-dictionary";

const README_TEXT_FILENAME: &str = "readme";

type GuiController = Controller<RenderContext, GuiView>;
type IconMap = HashMap<String, Texture>;

struct ReadmeContainer {
	text: String,
}

impl ReadmeContainer {
	#[inline]
	fn new(text: &str) -> Self
	{
		ReadmeContainer {
			text: text.to_string(),
		}
	}
}

impl Container for ReadmeContainer {
	fn filename(&self) -> &str
	{
		README_TEXT_FILENAME
	}

	#[inline]
	fn inner_book_names(&self) -> Option<&Vec<BookName>>
	{
		None
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

fn load_image(bytes: &[u8]) -> Option<Pixbuf>
{
	let bytes = Bytes::from(bytes);
	let stream = MemoryInputStream::from_bytes(&bytes);
	let image = Pixbuf::from_stream(&stream, None::<&Cancellable>).ok()?;
	Some(image)
}

fn build_ui(app: &Application, cfg: Rc<RefCell<Configuration>>, themes: &Rc<Themes>) -> Result<GuiContext>
{
	let configuration = cfg.borrow_mut();
	let loading = if let Some(current) = &configuration.current {
		Some(configuration.reading(current)?)
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
	let fonts = font::user_fonts(&configuration.gui.fonts)?;
	let fonts = Rc::new(fonts);
	let container_manager = Default::default();
	let i18n = I18n::new(&configuration.gui.lang).unwrap();
	let (container, book, reading, filename) = if let Some(loading) = loading {
		let mut container = load_container(&container_manager, loading.filename())?;
		let (book, reading) = load_book(&container_manager, &mut container, loading)?;
		let filename = reading.filename.clone();
		(container, book, reading, filename)
	} else {
		let readme = i18n.msg("readme");
		let container: Box<dyn Container> = Box::new(ReadmeContainer::new(readme.as_ref()));
		let book: Box<dyn Book> = Box::new(ReadmeBook::new(readme.as_ref()));
		(container, book, ReadingInfo::fake(README_TEXT_FILENAME), "The e-book reader".to_owned())
	};

	let i18n = Rc::new(i18n);
	let icons = load_icons();
	let icons = Rc::new(icons);

	let mut render_context = RenderContext::new(
		colors,
		configuration.gui.font_size,
		reading.custom_color,
		reading.custom_font,
		book.leading_space(),
		configuration.gui.strip_empty_lines,
		configuration.gui.ignore_font_weight);
	let view = GuiView::new(
		"main",
		configuration.render_han,
		book.custom_fonts(),
		fonts.clone(),
		&mut render_context);
	let css_provider = view::init_css("main", &render_context.colors.background);
	let (dm, dict_view, lookup_entry) = DictionaryManager::new(
		&configuration.gui.dictionaries,
		configuration.gui.cache_dict,
		configuration.gui.font_size,
		fonts,
		&i18n,
		&icons,
	);

	let custom_color = if book.color_customizable() {
		Some(reading.custom_color)
	} else {
		None
	};
	let custom_font = if book.fonts_customizable() {
		Some(reading.custom_font)
	} else {
		None
	};
	let controller = Controller::from_data(
		reading,
		container_manager,
		container,
		book,
		Box::new(view.clone()),
		&mut render_context);

	let (theme_icon, theme_tooltip) = get_theme_icon(configuration.dark_theme, &i18n);
	drop(configuration);

	let ctx = Rc::new(RefCell::new(render_context));
	let ctrl = Rc::new(RefCell::new(controller));
	let (gc, chapter_list_view) = GuiContext::new(app, &cfg, &ctrl, &ctx, dm, icons, i18n.clone(),
		dark_colors, bright_colors, css_provider);

	// now setup ui
	setup_sidebar(&gc, &view, &dict_view, chapter_list_view);
	setup_view(&gc, &view);
	setup_chapter_list(&gc);

	let (toolbar, theme_btn, search_box)
		= setup_toolbar(&gc, &view, &lookup_entry, custom_color, custom_font,
		theme_icon, &theme_tooltip);

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
		let key_event = EventControllerKey::new();
		key_event.connect_key_pressed(move |_, key, _, modifier| {
			let (key, modifier) = ignore_cap(key, modifier);
			match (key, modifier) {
				(Key::space | Key::Page_Down, MODIFIER_NONE) => {
					handle(&gc, |controller, render_context|
						controller.next_page(render_context));
					glib::Propagation::Stop
				}
				(Key::space, ModifierType::SHIFT_MASK) | (Key::Page_Up, MODIFIER_NONE) => {
					handle(&gc, |controller, render_context|
						controller.prev_page(render_context));
					glib::Propagation::Stop
				}
				(Key::Home, MODIFIER_NONE) => {
					apply(&gc, |controller, render_context|
						controller.redraw_at(0, 0, render_context));
					glib::Propagation::Stop
				}
				(Key::End, MODIFIER_NONE) => {
					apply(&gc, |controller, render_context|
						controller.goto_end(render_context));
					glib::Propagation::Stop
				}
				(Key::Down, MODIFIER_NONE) => {
					handle(&gc, |controller, render_context|
						controller.step_next(render_context));
					glib::Propagation::Stop
				}
				(Key::Up, MODIFIER_NONE) => {
					handle(&gc, |controller, render_context|
						controller.step_prev(render_context));
					glib::Propagation::Stop
				}
				(Key::n, MODIFIER_NONE) => {
					handle(&gc, |controller, render_context|
						controller.search_again(true, render_context));
					glib::Propagation::Stop
				}
				(Key::N, ModifierType::SHIFT_MASK) => {
					handle(&gc, |controller, render_context|
						controller.search_again(false, render_context));
					glib::Propagation::Stop
				}
				(Key::d, ModifierType::CONTROL_MASK) => {
					handle(&gc, |controller, render_context|
						controller.switch_chapter(true, render_context));
					glib::Propagation::Stop
				}
				(Key::b, ModifierType::CONTROL_MASK) => {
					handle(&gc, |controller, render_context|
						controller.switch_chapter(false, render_context));
					glib::Propagation::Stop
				}
				(Key::Right, MODIFIER_NONE) => {
					handle(&gc, |controller, render_context|
						controller.goto_trace(false, render_context));
					glib::Propagation::Stop
				}
				(Key::Left, MODIFIER_NONE) => {
					handle(&gc, |controller, render_context|
						controller.goto_trace(true, render_context));
					glib::Propagation::Stop
				}
				(Key::Tab, MODIFIER_NONE) => {
					apply(&gc, |controller, render_context|
						controller.switch_link_next(render_context));
					glib::Propagation::Stop
				}
				(Key::Tab, ModifierType::SHIFT_MASK) => {
					apply(&gc, |controller, render_context|
						controller.switch_link_prev(render_context));
					glib::Propagation::Stop
				}
				(Key::Return, MODIFIER_NONE) => {
					handle(&gc, |controller, render_context|
						controller.try_goto_link(render_context));
					glib::Propagation::Stop
				}
				(Key::equal, ModifierType::CONTROL_MASK) => {
					apply(&gc, |controller, render_context| {
						let mut configuration = cfg.borrow_mut();
						if configuration.gui.font_size < MAX_FONT_SIZE {
							configuration.gui.font_size += 2;
							controller.render.set_font_size(
								configuration.gui.font_size,
								controller.book.custom_fonts(),
								render_context);
							controller.redraw(render_context);
							gc.dm_mut().set_font_size(configuration.gui.font_size);
						}
					});
					glib::Propagation::Stop
				}
				(Key::minus, ModifierType::CONTROL_MASK) => {
					apply(&gc, |controller, render_context| {
						let mut configuration = gc.cfg_mut();
						if configuration.gui.font_size > MIN_FONT_SIZE {
							configuration.gui.font_size -= 2;
							controller.render.set_font_size(
								configuration.gui.font_size,
								controller.book.custom_fonts(),
								render_context);
							controller.redraw(render_context);
							gc.dm_mut().set_font_size(configuration.gui.font_size);
						}
					});
					glib::Propagation::Stop
				}
				(Key::c, ModifierType::CONTROL_MASK) => {
					copy_selection(&ctrl.borrow());
					glib::Propagation::Stop
				}
				_ => {
					// println!("view, key: {key}, modifier: {modifier}");
					glib::Propagation::Proceed
				}
			}
		});
		view.add_controller(key_event);
	}

	setup_window(&gc, toolbar, view, theme_btn, search_box, filename);
	Ok(gc)
}

#[inline]
fn copy_selection(ctrl: &GuiController)
{
	if let Some(selected_text) = ctrl.selected() {
		copy_to_clipboard(selected_text);
	}
}

#[inline]
fn copy_to_clipboard(selected_text: &str)
{
	if let Some(display) = Display::default() {
		display.clipboard().set_text(selected_text);
	}
}

#[inline]
fn lookup_selection(gc: &GuiContext)
{
	if let Some(selected_text) = gc.ctrl().selected() {
		gc.dm_mut().set_lookup(selected_text.to_owned());
	}
}

#[inline]
fn apply<F>(gc: &GuiContext, f: F)
	where F: FnOnce(&mut GuiController, &mut RenderContext)
{
	let mut controller = gc.ctrl_mut();
	let orig_inner_book = controller.reading.inner_book;
	f(&mut controller, &mut gc.ctx_mut());
	let msg = controller.status().to_string();
	drop(controller);
	gc.update(&msg, ChapterListSyncMode::ReloadIfNeeded(orig_inner_book));
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
			let msg = controller.status().to_string();
			drop(controller);
			gc.update(&msg, ChapterListSyncMode::ReloadIfNeeded(orig_inner_book));
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
			map.insert(name.to_string(), Texture::for_pixbuf(&pixbuf));
		}
	}
	map
}

fn setup_popup_menu(gc: &GuiContext, view: &GuiView) -> PopoverMenu
{
	let action_group = SimpleActionGroup::new();
	let menu = Menu::new();
	let i18n = &gc.i18n;

	view.insert_action_group("popup", Some(&action_group));

	let copy_action = SimpleAction::new(COPY_CONTENT_KEY, None);
	{
		let gc = gc.clone();
		copy_action.connect_activate(move |_, _| {
			let ctrl = gc.ctrl();
			copy_selection(&ctrl);
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
			switch_stack(SIDEBAR_DICT_NAME, &gc, false);
			lookup_selection(&gc);
		});
	}
	action_group.add_action(&lookup_action);
	let title = i18n.msg(DICT_LOOKUP_KEY);
	let menu_action_name = format!("popup.{}", DICT_LOOKUP_KEY);
	menu.append(Some(&title), Some(&menu_action_name));

	let pm = PopoverMenu::builder()
		.has_arrow(false)
		.position(PositionType::Bottom)
		.menu_model(&MenuModel::from(menu))
		.build();
	pm.set_parent(view);
	pm
}

fn setup_view(gc: &GuiContext, view: &GuiView)
{
	#[inline]
	fn select_text(gc: &GuiContext, from_line: u64, from_offset: u64, to_line: u64, to_offset: u64)
	{
		let from = Position::new(from_line as usize, from_offset as usize);
		let to = Position::new(to_line as usize, to_offset as usize);
		gc.ctrl_mut().select_text(from, to, &mut gc.ctx_mut());
	}

	#[inline]
	fn view_image(controller: &GuiController, line: usize, offset: usize,
		opener: &mut Opener) -> Result<()>
	{
		if let Some(line) = controller.book.lines().get(line) {
			if let Some(url) = line.image_at(offset) {
				if let Some(image_data) = controller.book.image(url) {
					opener.open_image(url, image_data.bytes())?;
				}
			}
		}
		Ok(())
	}

	#[inline]
	fn open_link(controller: &GuiController, line: usize, link_index: usize,
		opener: &mut Opener) -> Result<()>
	{
		if let Some(line) = controller.book.lines().get(line) {
			if let Some(link) = line.link_at(link_index) {
				opener.open_link(link.target)?;
			}
		}
		Ok(())
	}

	view.setup_gesture();
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
		// right click
		let right_click = GestureClick::builder()
			.button(gdk::BUTTON_SECONDARY)
			.build();
		let popup_menu = setup_popup_menu(gc, view);
		let gc = gc.clone();
		right_click.connect_pressed(move |_, _, x, y| {
			if gc.ctrl().has_selection() {
				popup_menu.popup();
				let (_, width, _, _) = popup_menu.measure(Orientation::Horizontal, -1);
				let x = x as i32 + width / 2;
				popup_menu.set_pointing_to(Some(&Rectangle::new(
					x,
					y as i32,
					-1,
					-1,
				)));
			}
		});
		view.add_controller(right_click);
	}

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

	{
		// open image signal
		let gc = gc.clone();
		view.connect_closure(
			GuiView::OPEN_IMAGE_EXTERNAL_SIGNAL,
			false,
			closure_local!(move |_: GuiView, line: u64, offset: u64| {
				handle(&gc, |controller, _render_context|
					view_image(controller, line as usize, offset as usize, &mut gc.opener()))
	        }),
		);
	}

	{
		// open link external signal
		let gc = gc.clone();
		view.connect_closure(
			GuiView::OPEN_LINK_EXTERNAL_SIGNAL,
			false,
			closure_local!(move |_: GuiView, line: u64, link_index: u64| {
				handle(&gc, |controller, _render_context|
					open_link(controller, line as usize, link_index as usize, &mut gc.opener()))
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
		view.connect_closure(
			GuiView::TEXT_SELECTED_SIGNAL,
			false,
			closure_local!(move |_: GuiView, from_line: u64, from_offset: u64, to_line: u64, to_offset: u64| {
				select_text(&gc, from_line, from_offset, to_line, to_offset);
				if let Some(selected_text) = gc.ctrl().selected() {
					if let Some(current_tab) = gc.sidebar_stack.visible_child_name() {
						if current_tab == SIDEBAR_DICT_NAME {
							gc.dm_mut().set_lookup(selected_text.to_owned());
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
					handle(&gc, |controller, render_context|
						controller.step_next(render_context));
				} else {
					handle(&gc, |controller, render_context|
						controller.step_prev(render_context));
				}
	        }),
		);
	}
}

fn setup_sidebar(gc: &GuiContext, view: &GuiView, dict_view: &gtk4::Box,
	chapter_list_view: gtk4::Box)
{
	let i18n = &gc.i18n;
	let stack = &gc.sidebar_stack;
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
	sidebar.append(&gc.sidebar_stack);

	let paned = &gc.paned;
	paned.set_start_child(Some(&sidebar));
	paned.set_end_child(Some(view));
	paned.set_position(0);

	let gc = gc.clone();
	paned.connect_position_notify(move |p| {
		let position = p.position();
		if position > 0 {
			gc.cfg_mut().gui.sidebar_size = position as u32;
			gc.dm_mut().resize(position, None);
		}
	});
}

fn setup_chapter_list(gc1: &GuiContext)
{
	{
		let gc = gc1.clone();
		gc1.chapter_list.handle_item_click(move |is_book, index| {
			let mut controller = gc.ctrl_mut();
			let mut render_context = gc.ctx_mut();
			if is_book {
				let msg = controller.switch_book(index, &mut render_context);
				update_status(false, &msg, &gc.status_bar);
			} else if let Some(msg) = controller.goto_toc(index, &mut render_context) {
				update_status(false, &msg, &gc.status_bar);
			}
		});
	}
	{
		let gc = gc1.clone();
		gc1.chapter_list.handle_cancel(move |empty| {
			// when search entry has no text, empty is true
			if empty {
				toggle_sidebar(&gc);
			} else {
				gc.ctrl().render.grab_focus();
			}
		});
	}
}

fn switch_stack(tab_name: &str, gc: &GuiContext, toggle: bool) -> bool
{
	let paned = &gc.paned;
	let stack = &gc.sidebar_stack;
	if paned.position() == 0 {
		stack.set_visible_child_name(tab_name);
		toggle_sidebar(gc);
		true
	} else if let Some(current_tab_name) = stack.visible_child_name() {
		if current_tab_name == tab_name {
			if toggle {
				toggle_sidebar(gc);
				false
			} else {
				true
			}
		} else {
			stack.set_visible_child_name(tab_name);
			true
		}
	} else {
		stack.set_visible_child_name(tab_name);
		true
	}
}

#[inline]
fn setup_window(gc: &GuiContext, toolbar: gtk4::Box, view: GuiView,
	theme_btn: Button, search_box: SearchEntry, filename: String)
{
	let header_bar = HeaderBar::new();
	header_bar.set_height_request(32);
	header_bar.pack_start(&toolbar);
	header_bar.pack_end(&gc.status_bar);
	let window = &gc.window;
	window.set_titlebar(Some(&header_bar));
	window.set_child(Some(&gc.paned));
	window.set_default_widget(Some(&view));
	window.set_focus(Some(&view));
	window.add_css_class("main-window");
	update_title(window, &filename);

	let window_key_event = EventControllerKey::new();
	{
		let gc = gc.clone();
		window_key_event.connect_key_released(move |_, key, _, _| {
			match key {
				Key::Control_L => {
					let view = &gc.ctrl().render;
					if let Some((x, y)) = mouse_pointer(view.as_ref()) {
						update_mouse_pointer(&view, x, y, MODIFIER_NONE);
					}
				}
				_ => {}
			}
		});
	}
	{
		let gc = gc.clone();
		window_key_event.connect_key_pressed(move |_, key, _, modifier| {
			let (key, modifier) = ignore_cap(key, modifier);
			match (key, modifier) {
				(Key::Control_L, MODIFIER_NONE) => {
					let view = &gc.ctrl().render;
					if let Some((x, y)) = mouse_pointer(view.as_ref()) {
						update_mouse_pointer(&view, x, y, ModifierType::CONTROL_MASK);
					}
					glib::Propagation::Proceed
				}
				(Key::c, MODIFIER_NONE) => {
					gc.chapter_list.block_reactive(true);
					if switch_stack(SIDEBAR_CHAPTER_LIST_NAME, &gc, true) {
						gc.chapter_list.scroll_to_current();
					}
					gc.chapter_list.block_reactive(false);
					glib::Propagation::Stop
				}
				(Key::d, MODIFIER_NONE) => {
					if switch_stack(SIDEBAR_DICT_NAME, &gc, true) {
						lookup_selection(&gc);
					}
					glib::Propagation::Stop
				}
				(Key::slash, MODIFIER_NONE) | (Key::f, ModifierType::CONTROL_MASK) => {
					search_box.grab_focus();
					if let Some(pattern) = gc.ctrl().selected() {
						search_box.set_text(pattern)
					}
					search_box.select_region(0, -1);
					glib::Propagation::Stop
				}
				(Key::Escape, MODIFIER_NONE) => {
					if gc.paned.position() != 0 {
						toggle_sidebar(&gc);
						glib::Propagation::Stop
					} else {
						glib::Propagation::Proceed
					}
				}
				(Key::x, ModifierType::CONTROL_MASK) => {
					switch_render(&gc);
					glib::Propagation::Stop
				}
				(Key::r, ModifierType::CONTROL_MASK) => {
					gc.reload_book();
					glib::Propagation::Stop
				}
				(Key::o, ModifierType::CONTROL_MASK) => {
					gc.open_dialog();
					glib::Propagation::Stop
				}
				(Key::h, MODIFIER_NONE) => {
					gc.show_history();
					glib::Propagation::Stop
				}
				(Key::t, MODIFIER_NONE) => {
					switch_theme(&theme_btn, &gc);
					glib::Propagation::Stop
				}
				(Key::T, ModifierType::SHIFT_MASK) => {
					let active = gc.custom_color_btn.is_active();
					gc.custom_color_btn.set_active(!active);
					glib::Propagation::Stop
				}
				(Key::F, ModifierType::SHIFT_MASK) => {
					let active = gc.custom_font_btn.is_active();
					gc.custom_font_btn.set_active(!active);
					glib::Propagation::Stop
				}
				(Key::s, ModifierType::CONTROL_MASK) => {
					settings::show(&gc);
					glib::Propagation::Stop
				}
				(Key::w, ModifierType::CONTROL_MASK) => {
					gc.window.close();
					glib::Propagation::Stop
				}
				(Key::i, MODIFIER_NONE) => {
					if let Err(err) = gc.book_info() {
						gc.error(&err.to_string());
					}
					glib::Propagation::Stop
				}
				_ => {
					// println!("window pressed, key: {key}, modifier: {modifier}");
					glib::Propagation::Proceed
				}
			}
		});
	}
	window.add_controller(window_key_event);

	{
		let gc = gc.clone();
		window.connect_close_request(move |_| {
			let mut controller = gc.ctrl_mut();
			if controller.reading.filename != README_TEXT_FILENAME {
				let configuration = gc.cfg_mut();
				if let Err(e) = configuration.save_reading(&mut controller.reading) {
					eprintln!("Failed save reading info: {}", e.to_string());
				}
			}
			let configuration = gc.cfg();
			if let Err(e) = configuration.save() {
				eprintln!("Failed save configuration: {}", e.to_string());
			}
			glib::Propagation::Proceed
		});
	}

	window.present();
}

#[inline(always)]
fn get_theme_icon(dark_theme: bool, i18n: &I18n) -> (&'static str, Cow<str>) {
	if dark_theme {
		("theme_bright.svg", i18n.msg("theme-bright"))
	} else {
		("theme_dark.svg", i18n.msg("theme-dark"))
	}
}

fn toggle_sidebar(gc: &GuiContext)
{
	let paned = &gc.paned;
	let sidebar_btn = &gc.sidebar_btn;
	let (icon, tooltip, position) = if paned.position() == 0 {
		("sidebar_off.svg", gc.i18n.msg("sidebar-off"), gc.cfg().gui.sidebar_size as i32)
	} else {
		paned.end_child().unwrap().grab_focus();
		("sidebar_on.svg", gc.i18n.msg("sidebar-on"), 0)
	};
	update_button(sidebar_btn, icon, &tooltip, &gc.icons);
	paned.set_position(position);
}

fn switch_render(gc: &GuiContext)
{
	let mut configuration = gc.cfg_mut();
	let render_han = !configuration.render_han;
	configuration.render_han = render_han;
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
	let (theme_icon, theme_tooltip) = get_theme_icon(dark_theme, &gc.i18n);
	update_button(theme_btn, theme_icon, &theme_tooltip, &gc.icons);
	let mut render_context = gc.ctx_mut();
	render_context.colors = if dark_theme {
		gc.dark_colors.clone()
	} else {
		gc.bright_colors.clone()
	};
	let mut controller = gc.ctrl_mut();
	controller.redraw(&mut render_context);
	view::update_css(&gc.css_provider, "main", &render_context.colors.background);
}

#[inline]
fn setup_toolbar(gc: &GuiContext, view: &GuiView, lookup_entry: &SearchEntry,
	custom_color: Option<bool>, custom_font: Option<bool>,
	theme_icon: &str, theme_tooltip: &str) -> (gtk4::Box, Button, SearchEntry)
{
	let i18n = &gc.i18n;
	let icons = &gc.icons;

	let toolbar = gtk4::Box::builder()
		.css_classes(vec!["toolbar"])
		.build();

	let sidebar_button = &gc.sidebar_btn;
	{
		let gc = gc.clone();
		sidebar_button.connect_clicked(move |_| {
			toggle_sidebar(&gc);
		});
		toolbar.append(sidebar_button);
	}

	{
		let gc = gc.clone();
		lookup_entry.connect_stop_search(move |_| {
			toggle_sidebar(&gc);
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
	{
		let gc = gc.clone();
		file_button.connect_clicked(move |_| {
			gc.open_dialog();
		});
		toolbar.append(&file_button);
	}
	gc.reload_history();

	{
		let history_button = create_button("history.svg", &i18n.msg("history"), &icons, false);
		history_button.insert_action_group("popup", Some(&gc.action_group));
		gc.history_popover.set_parent(&history_button);
		let gc = gc.clone();
		history_button.connect_clicked(move |_| {
			gc.show_history();
		});
		toolbar.append(&history_button);
	}
	{
		let reload_button = create_button("reload.svg", &i18n.msg("reload"), &icons, false);
		let gc = gc.clone();
		reload_button.connect_clicked(move |_| {
			gc.reload_book();
		});
		toolbar.append(&reload_button);
	}

	{
		let info_button = &gc.book_info_btn;
		let gc = gc.clone();
		info_button.connect_clicked(move |_| {
			if let Err(err) = gc.book_info() {
				gc.error(&gc.i18n.args_msg("failed-load-reading", vec![
					("error", err.to_string()),
				]));
			}
		});
		toolbar.append(info_button);
	}

	let theme_button = create_button(theme_icon, theme_tooltip, &icons, false);
	{
		let gc = gc.clone();
		theme_button.connect_clicked(move |btn| {
			switch_theme(btn, &gc);
		});
		toolbar.append(&theme_button);
	}

	{
		let custom_color_button = &gc.custom_color_btn;
		if let Some(custom_color) = custom_color {
			custom_color_button.set_active(custom_color);
		} else {
			custom_color_button.set_active(false);
			custom_color_button.set_sensitive(false)
		}
		toolbar.append(custom_color_button);
	}

	{
		let custom_font_button = &gc.custom_font_btn;
		if let Some(custom_font) = custom_font {
			custom_font_button.set_active(custom_font);
		} else {
			custom_font_button.set_active(false);
			custom_font_button.set_sensitive(false)
		}
		toolbar.append(custom_font_button);
	}

	let settings_button = create_button("setting.svg", &i18n.msg("settings-dialog"), &icons, false);
	{
		let gc = gc.clone();
		settings_button.connect_clicked(move |_| {
			settings::show(&gc);
		});
		toolbar.append(&settings_button);
	}

	let search_box = SearchEntry::builder()
		.placeholder_text(i18n.msg("search-hint"))
		.activates_default(true)
		.enable_undo(true)
		.build();
	toolbar.append(&search_box);

	(toolbar, theme_button, search_box)
}

#[inline]
fn load_button_image(name: &str, icons: &IconMap, inline: bool) -> Image
{
	let texture = icons.get(name).unwrap();
	let image = Image::from_paintable(Some(texture));
	if inline {
		image.set_width_request(INLINE_ICON_SIZE);
		image.set_height_request(INLINE_ICON_SIZE);
	} else {
		image.set_width_request(ICON_SIZE);
		image.set_height_request(ICON_SIZE);
	}
	image
}

#[inline]
fn update_toggle_button(btn: &ToggleButton, handle_id: &SignalHandlerId, active: Option<bool>)
{
	btn.block_signal(handle_id);
	if let Some(active) = active {
		btn.set_sensitive(true);
		btn.set_active(active);
	} else {
		btn.set_sensitive(false);
		btn.set_active(false);
	}
	btn.unblock_signal(handle_id);
}

fn create_toggle_button(active: bool, name: &str, i18n_key: &str,
	icons: &IconMap, i18n: &I18n)
	-> ToggleButton
{
	let image = load_button_image(name, icons, false);
	let tooltip = i18n.msg(i18n_key);
	ToggleButton::builder()
		.child(&image)
		.focus_on_click(false)
		.focusable(false)
		.tooltip_text(tooltip)
		.active(active)
		.build()
}

#[inline]
fn create_button(name: &str, tooltip: &str, icons: &IconMap, inline: bool) -> Button
{
	let image = load_button_image(name, icons, inline);
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
	let texture = icons.get(name).unwrap();
	let image = Image::from_paintable(Some(texture));
	image.set_width_request(ICON_SIZE);
	image.set_height_request(ICON_SIZE);
	btn.set_tooltip_text(Some(tooltip));
	btn.set_child(Some(&image));
}

#[inline(always)]
fn update_title(window: &ApplicationWindow, filename: &str)
{
	let filename = title_for_filename(filename);
	let title = format!("{} - {}", package_name!(), filename);
	window.set_title(Some(&title));
}

fn apply_settings(render_han: bool, locale: &str, fonts: Vec<PathConfig>,
	dictionaries: Vec<PathConfig>, cache_dict: bool, ignore_font_weight: bool,
	strip_empty_lines: bool,
	gc: &GuiContext, dictionary_manager: &mut DictionaryManager)
	-> Result<(), (String, String)>
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
	let i18n = &gc.i18n;
	// need restart
	configuration.gui.lang = locale.to_owned();

	let mut redraw = false;
	let reload_render = if configuration.render_han != render_han {
		configuration.render_han = render_han;
		redraw = true;
		true
	} else {
		false
	};

	if configuration.gui.ignore_font_weight != ignore_font_weight {
		configuration.gui.ignore_font_weight = ignore_font_weight;
		redraw = true;
	};
	if configuration.gui.strip_empty_lines != strip_empty_lines {
		configuration.gui.strip_empty_lines = strip_empty_lines;
		redraw = true;
	};

	let new_fonts = if paths_modified(&configuration.gui.fonts, &fonts) {
		let new_fonts = match font::user_fonts(&fonts) {
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
		redraw = true;
		Some(new_fonts)
	} else {
		None
	};

	if paths_modified(&configuration.gui.dictionaries, &dictionaries)
		|| configuration.gui.cache_dict != cache_dict {
		dictionary_manager.reload(&dictionaries, cache_dict);
		configuration.gui.dictionaries = dictionaries;
		configuration.gui.cache_dict = cache_dict;
	};

	if redraw {
		let mut render_context = gc.ctx_mut();
		let mut controller = gc.ctrl_mut();
		if reload_render {
			controller.render.reload_render(configuration.render_han, &mut render_context);
		}
		if let Some(new_fonts) = new_fonts {
			let fonts_data = Rc::new(new_fonts);
			dictionary_manager.set_fonts(fonts_data.clone());
			configuration.gui.fonts = fonts;
			controller.render.set_fonts(controller.book.custom_fonts(), fonts_data, &mut render_context);
		}
		render_context.ignore_font_weight = ignore_font_weight;
		render_context.strip_empty_lines = strip_empty_lines;
		controller.redraw(&mut render_context);
	}
	Ok(())
}

#[inline]
#[cfg(unix)]
fn setup_env() -> Result<bool>
{
	use dirs::home_dir;
	use std::fs;

	// any better way to know if a usable backend for gtk4 available?
	if !env::var("WAYLAND_DISPLAY")
		.map_or_else(|_|
			env::var("DISPLAY")
				.map_or(false, |_| true),
			|_| true) {
		return Ok(false);
	}

	let home_dir = home_dir().expect("No home folder");
	let icon_path = home_dir.join(".local/share/icons/hicolor/256x256/apps");
	if !icon_path.exists() {
		fs::create_dir_all(&icon_path)?;
	}
	{
		let icon_file = icon_path.join("tbr-icon.png");
		if !icon_file.exists() {
			fs::write(&icon_file, include_bytes!("../assets/gui/tbr-icon.png"))?;
		}
	}
	Ok(true)
}

struct GuiContextInner {
	cfg: Rc<RefCell<Configuration>>,
	ctrl: Rc<RefCell<GuiController>>,
	ctx: Rc<RefCell<RenderContext>>,
	dm: Rc<RefCell<DictionaryManager>>,
	opener: Rc<RefCell<Opener>>,
	window: ApplicationWindow,
	history_menu: Menu,
	history_popover: PopoverMenu,
	action_group: SimpleActionGroup,
	status_bar: Label,
	paned: Paned,
	sidebar_stack: Stack,
	sidebar_btn: Button,
	custom_color_btn: ToggleButton,
	custom_color_handler_id: SignalHandlerId,
	custom_font_btn: ToggleButton,
	custom_font_handler_id: SignalHandlerId,
	book_info_btn: Button,
	chapter_list: ChapterList,
	icons: Rc<IconMap>,
	i18n: Rc<I18n>,
	dark_colors: Colors,
	bright_colors: Colors,
	css_provider: CssProvider,
	file_dialog: FileDialog,
}

enum ChapterListSyncMode {
	NoReload,
	Reload,
	ReloadIfNeeded(usize),
}

#[derive(Clone)]
struct GuiContext {
	inner: Rc<GuiContextInner>,
}

impl Deref for GuiContext {
	type Target = GuiContextInner;

	#[inline(always)]
	fn deref(&self) -> &Self::Target
	{
		&self.inner
	}
}

impl GuiContext {
	fn new(app: &Application,
		cfg: &Rc<RefCell<Configuration>>, ctrl: &Rc<RefCell<GuiController>>,
		ctx: &Rc<RefCell<RenderContext>>, dm: Rc<RefCell<DictionaryManager>>,
		icons: Rc<IconMap>, i18n: Rc<I18n>,
		dark_colors: Colors, bright_colors: Colors, css_provider: CssProvider)
		-> (Self, gtk4::Box)
	{
		let window = ApplicationWindow::builder()
			.application(app)
			.default_width(800)
			.default_height(600)
			.maximized(true)
			.title(package_name!())
			.build();

		let (chapter_list, chapter_list_view) = ChapterList::create(&icons, &i18n, &ctrl);

		let controller = ctrl.borrow();
		let status_msg = controller.status().to_string();
		let status_bar = Label::builder()
			.label(&status_msg)
			.max_width_chars(50)
			.ellipsize(EllipsizeMode::Start)
			.tooltip_text(&status_msg)
			.halign(Align::End)
			.hexpand(true)
			.build();

		let paned = Paned::new(Orientation::Horizontal);
		let sidebar_stack = Stack::builder()
			.vexpand(true)
			.build();
		let sidebar_btn = create_button("sidebar_on.svg", &i18n.msg("sidebar-on"), &icons, false);
		let custom_color_btn = create_toggle_button(
			false,
			"custom_color.svg",
			"with-custom-color",
			&icons,
			&i18n,
		);
		let custom_color_handler_id = {
			let ctrl = ctrl.clone();
			let ctx = ctx.clone();
			custom_color_btn.connect_toggled(move |btn| {
				let custom_color = btn.is_active();
				let mut controller = ctrl.borrow_mut();
				controller.reading.custom_color = custom_color;
				let mut render_context = ctx.borrow_mut();
				render_context.custom_color = custom_color;
				controller.redraw(&mut render_context);
			})
		};
		let custom_font_btn = create_toggle_button(
			false,
			"custom_font.svg",
			"with-custom-font",
			&icons,
			&i18n,
		);
		let custom_font_handler_id = {
			let ctrl = ctrl.clone();
			let ctx = ctx.clone();
			custom_font_btn.connect_toggled(move |btn| {
				let custom_font = btn.is_active();
				let mut controller = ctrl.borrow_mut();
				controller.reading.custom_font = custom_font;
				let mut render_context = ctx.borrow_mut();
				controller.render.set_custom_font(
					custom_font,
					controller.book.custom_fonts(),
					&mut render_context);
				controller.redraw(&mut render_context);
			})
		};

		let file_dialog = FileDialog::new();
		file_dialog.set_title(&i18n.msg("file-open-title"));
		file_dialog.set_modal(true);
		let filter = FileFilter::new();
		for ext in controller.container_manager.book_loader.extension() {
			filter.add_suffix(&ext[1..]);
		}
		file_dialog.set_default_filter(Some(&filter));

		let action_group = SimpleActionGroup::new();
		let history_menu = Menu::new();
		let history_popover = PopoverMenu::builder()
			.menu_model(&history_menu)
			.has_arrow(false)
			.build();
		let view = controller.render.as_ref().clone();
		history_popover.connect_visible_notify(move |_| {
			view.grab_focus();
		});

		let book_info_btn = create_button("file_info.svg", &i18n.msg("book-info"), &icons, false);

		let inner = GuiContextInner {
			cfg: cfg.clone(),
			ctrl: ctrl.clone(),
			ctx: ctx.clone(),
			dm,
			opener: Rc::new(RefCell::new(Default::default())),
			window,
			history_menu,
			history_popover,
			action_group,
			status_bar,
			paned,
			sidebar_stack,
			sidebar_btn,
			custom_color_btn,
			custom_color_handler_id,
			custom_font_btn,
			custom_font_handler_id,
			book_info_btn,
			chapter_list,
			icons,
			i18n,
			dark_colors,
			bright_colors,
			css_provider,
			file_dialog,
		};
		(GuiContext { inner: Rc::new(inner) }, chapter_list_view)
	}

	#[inline]
	fn cfg(&self) -> Ref<Configuration>
	{
		self.cfg.borrow()
	}

	#[inline]
	fn cfg_mut(&self) -> RefMut<Configuration>
	{
		self.cfg.borrow_mut()
	}

	#[inline]
	fn ctrl(&self) -> Ref<GuiController>
	{
		self.ctrl.borrow()
	}

	#[inline]
	fn ctrl_mut(&self) -> RefMut<GuiController>
	{
		self.ctrl.borrow_mut()
	}

	#[inline]
	fn ctx_mut(&self) -> RefMut<RenderContext>
	{
		self.ctx.borrow_mut()
	}

	#[inline]
	fn opener(&self) -> RefMut<Opener>
	{
		self.opener.borrow_mut()
	}

	#[inline]
	fn update_ui(&self, custom_color: Option<bool>, custom_font: Option<bool>)
	{
		update_toggle_button(
			&self.custom_color_btn,
			&self.custom_color_handler_id,
			custom_color);
		update_toggle_button(
			&self.custom_font_btn,
			&self.custom_font_handler_id,
			custom_font);
	}

	#[inline]
	fn dm_mut(&self) -> RefMut<DictionaryManager>
	{
		self.dm.borrow_mut()
	}

	#[inline]
	fn show_history(&self)
	{
		self.history_popover.popup();
	}

	fn open_dialog(&self)
	{
		let gc = self.clone();
		self.file_dialog.open(Some(&self.window), None::<&Cancellable>, move |result| {
			if let Ok(file) = result {
				if let Some(path) = file.path() {
					gc.open_file(&path);
				}
			}
		});
	}

	fn open_file(&self, path: &PathBuf)
	{
		if let Ok(absolute_path) = path.canonicalize() {
			if let Some(filepath) = absolute_path.to_str() {
				let mut controller = self.ctrl_mut();
				if filepath != controller.reading.filename {
					let mut configuration = self.cfg_mut();
					let mut render_context = self.ctx_mut();
					let reading = &mut controller.reading;
					if reading.filename != README_TEXT_FILENAME {
						if let Err(e) = configuration.save_reading(reading) {
							self.error(&e.to_string());
							return;
						}
					}
					match configuration.reading(filepath) {
						Ok(loading) =>
							match controller.switch_container(loading, &mut render_context) {
								Ok(msg) => {
									let custom_color = if controller.book.color_customizable() {
										Some(controller.reading.custom_color)
									} else {
										None
									};
									let custom_font = if controller.book.fonts_customizable() {
										Some(controller.reading.custom_font)
									} else {
										None
									};
									self.update_ui(custom_color, custom_font);
									update_title(&self.window, &controller.reading.filename);
									controller.redraw(&mut render_context);
									configuration.current = Some(controller.reading.filename.clone());
									drop(controller);
									self.update(
										&msg,
										ChapterListSyncMode::Reload);
									drop(configuration);
									drop(render_context);
									self.reload_history();
								}
								Err(e) =>
									self.error(&e.to_string()),
							}
						Err(err) => self.error(&err.to_string()),
					}
				}
			}
		}
	}

	fn reload_history(&self)
	{
		for a in self.action_group.list_actions() {
			self.action_group.remove_action(&a);
		}
		let menu = &self.history_menu;
		menu.remove_all();
		match self.cfg().history() {
			Ok(infos) => {
				for (idx, ri) in infos.iter().enumerate() {
					if idx == 20 {
						break;
					}
					self.add_history_entry(idx, &ri.filename, menu);
				}
			}
			Err(err) => self.error(&err.to_string()),
		}
	}

	fn reload_book(&self)
	{
		let mut controller = self.ctrl_mut();
		let loading = BookLoadingInfo::Reload(controller.reading.clone());
		match controller.switch_container(loading, &mut self.ctx_mut()) {
			Ok(msg) => update_status(false, &msg, &self.status_bar),
			Err(err) => self.error(&err.to_string()),
		}
	}

	fn book_info(&self) -> Result<()>
	{
		#[inline]
		fn label(title: &str) -> Label
		{
			Label::builder()
				.halign(Align::Start)
				.label(title)
				.build()
		}

		let controller = self.ctrl_mut();
		let reading = &controller.reading;
		let path = PathBuf::from_str(&reading.filename)?;
		let meta = path.metadata()?;
		let container = gtk4::Box::new(Orientation::Vertical, 10);
		container.append(&label(&reading.filename));
		container.append(&label(&format_size(meta.len())));
		container.append(&Separator::new(Orientation::Horizontal));
		if let Some(book_names) = controller.container.inner_book_names() {
			if let Some(name) = book_names.get(reading.inner_book) {
				container.append(&label(&name.name()));
			}
		}
		let status = controller.status();
		if let Some(title) = status.title {
			container.append(&label(title));
		}
		container.append(&label(&status.position()));
		let popover = Popover::builder()
			.child(&container)
			.build();
		popover.set_parent(&self.book_info_btn);
		popover.popup();
		Ok(())
	}

	#[inline]
	fn add_history_entry(&self, idx: usize, path_str: &String, menu: &Menu)
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
		self.action_group.add_action(&action);
		let menu_action_name = format!("popup.{}", action_name);
		menu.append(Some(&path_str), Some(&menu_action_name));
	}

	#[inline]
	fn update(&self, msg: &str, chapter_list_sync_mode: ChapterListSyncMode)
	{
		self.message(msg);
		self.chapter_list.sync_chapter_list(chapter_list_sync_mode);
	}

	#[inline]
	fn message(&self, msg: &str)
	{
		update_status(false, msg, &self.status_bar);
	}

	#[inline]
	fn error(&self, msg: &str)
	{
		update_status(true, msg, &self.status_bar);
	}
}

fn update_status(error: bool, msg: &str, status_bar: &Label)
{
	if error {
		let markup = format!("<span foreground='red'>{msg}</span>");
		status_bar.set_markup(&markup);
	} else {
		status_bar.set_text(msg);
	};
	status_bar.set_tooltip_text(Some(msg));
}

fn show(app: &Application, cfg: &Rc<RefCell<Configuration>>, themes: &Rc<Themes>,
	mut gui_context: RefMut<Option<GuiContext>>)
{
	let css_provider = CssProvider::new();
	css_provider.load_from_string(include_str!("../assets/gui/gtk.css"));
	gtk4::style_context_add_provider_for_display(
		&Display::default().expect("Could not connect to a display."),
		&css_provider,
		gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
	);
	Window::set_default_icon_name("tbr-icon");

	match build_ui(app, cfg.clone(), themes) {
		Ok(gc) => {
			{
				// clean temp files
				let gc = gc.clone();
				app.connect_shutdown(move |_| gc.opener().cleanup());
			}
			*gui_context = Some(gc);
		}
		Err(err) => {
			eprintln!("Failed start tbr: {}", err.to_string());
			app.quit();
		}
	}
}

fn mouse_pointer(view: &impl IsA<Widget>) -> Option<(f32, f32)>
{
	let pointer = view.display().default_seat()?.pointer()?;
	let root = view.root()?;
	let (x, y, _) = root.surface().device_position(&pointer)?;
	let point = root.compute_point(view, &Point::new(x as f32, y as f32))?;
	let x = point.x();
	let y = point.y();
	if x < 0. || y < 0. || x > view.width() as f32 || y > view.height() as f32 {
		None
	} else {
		Some((x, y))
	}
}

#[inline]
fn ignore_cap(key: Key, modifier: ModifierType) -> (Key, ModifierType)
{
	if modifier & ModifierType::LOCK_MASK == MODIFIER_NONE {
		(key, modifier)
	} else {
		let modifier = modifier ^ ModifierType::LOCK_MASK;
		let key = key.to_lower();
		(key, modifier)
	}
}

pub fn start(configuration: Configuration, themes: Themes)
	-> Result<Option<(Configuration, Themes)>>
{
	#[cfg(unix)]
	if !setup_env()? {
		return Ok(Some((configuration, themes)));
	};

	let app = Application::builder()
		.application_id(APP_ID)
		.flags(ApplicationFlags::NON_UNIQUE)
		.build();

	let gui_context = Rc::new(RefCell::new(None::<GuiContext>));
	let cfg = Rc::new(RefCell::new(configuration));
	let themes = Rc::new(themes);
	{
		let gui_context = gui_context.clone();
		let cfg = cfg.clone();
		let themes = themes.clone();
		app.connect_activate(move |app| {
			show(app, &cfg, &themes, gui_context.borrow_mut());
		});
	}

	if app.run_with_args::<String>(&[]) == ExitCode::FAILURE {
		bail!("Failed start tbr")
	}

	Ok(None)
}
