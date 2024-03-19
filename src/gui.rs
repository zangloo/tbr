use std::env;
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::ops::{Deref, Index};
use std::path::PathBuf;
use std::rc::Rc;
use std::str::FromStr;
use anyhow::{bail, Result};
use cursive::theme::{BaseColor, Color, PaletteColor, Theme};
use gtk4::{AlertDialog, Align, Application, ApplicationWindow, Button, CssProvider, DirectionType, DropTarget, EventControllerKey, FileDialog, FileFilter, gdk, GestureClick, HeaderBar, Image, Label, Orientation, Paned, Popover, PopoverMenu, PositionType, SearchEntry, Separator, Stack, ToggleButton, Widget, Window};
use gtk4::gdk::{Display, DragAction, Key, ModifierType, Rectangle, Texture};
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::gio::{ApplicationFlags, Cancellable, File, MemoryInputStream, Menu, MenuItem, MenuModel, SimpleAction, SimpleActionGroup};
use gtk4::glib;
use gtk4::glib::{Bytes, closure_local, ExitCode, format_size, ObjectExt, StaticType, ToVariant, Variant};
use gtk4::graphene::Point;
use gtk4::prelude::{ActionExt, ActionGroupExt, ActionMapExt, ApplicationExt, ApplicationExtManual, BoxExt, ButtonExt, DisplayExt, DrawingAreaExt, EditableExt, EventControllerExt, FileExt, GtkApplicationExt, GtkWindowExt, IsA, NativeExt, OrientableExt, PopoverExt, SeatExt, SurfaceExt, ToggleButtonExt, WidgetExt};
use pangocairo::glib::Propagation;
use pangocairo::pango::EllipsizeMode;
use resvg::{tiny_skia, usvg};
use resvg::usvg::TreeParsing;

use crate::{Asset, I18n, package_name};
use crate::book::{Book, Colors, Line};
use crate::color::Color32;
use crate::common::{Position, txt_lines};
use crate::config::{BookLoadingInfo, Configuration, ReadingInfo, SidebarPosition, Themes};
use crate::container::{BookContent, BookName, Container, load_book, load_container};
use crate::controller::Controller;
use crate::gui::chapter_list::ChapterList;
use crate::gui::dict::{DictionaryBook, DictionaryManager};
pub use crate::gui::font::HtmlFonts;
use crate::gui::font::UserFonts;
use crate::gui::render::RenderContext;
use crate::gui::settings::Settings;
use crate::gui::view::{GuiView, update_mouse_pointer};
use crate::open::Opener;

mod render;
mod dict;
mod view;
mod math;
mod settings;
mod chapter_list;
mod font;
mod custom_style;

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

const OPEN_FILE_KEY: &str = "file-open";
const HISTORY_KEY: &str = "history";
const RELOAD_KEY: &str = "reload";
const BOOK_INFO_KEY: &str = "book-info";
const SIDEBAR_KEY: &str = "sidebar";
const THEME_KEY: &str = "dark-theme";
const CUSTOM_COLOR_KEY: &str = "with-custom-color";
const CUSTOM_FONT_KEY: &str = "with-custom-font";
const CUSTOM_STYLE_KEY: &str = "custom-style";
const SETTINGS_KEY: &str = "settings-dialog";

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
	#[inline]
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

	fn book_name(&self, _inner_index: usize) -> &str
	{
		"The e-book reader"
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

fn custom_settings(book: &dyn Book, reading: &ReadingInfo)
	-> (Option<bool>, Option<bool>, Option<Option<String>>)
{
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
	let custom_style = if book.style_customizable() {
		Some(reading.custom_style.clone())
	} else {
		None
	};
	(custom_color, custom_font, custom_style)
}

fn build_ui(app: &Application, cfg: Rc<RefCell<Configuration>>,
	themes: &Rc<Themes>, gcs: &Rc<RefCell<Vec<GuiContext>>>)
	-> Result<Option<GuiContext>>
{
	let configuration = cfg.borrow_mut();
	let mut gui_contexts = gcs.borrow_mut();
	let (loading, gc_idx) = if let Some(current) = &configuration.current {
		let current = configuration.reading(current)?;
		let filename = current.filename();
		match get_gc(&gui_contexts, filename) {
			Ok(idx) => {
				gui_contexts[idx].window.present();
				return Ok(None);
			}
			Err(idx) => (Some(current), idx)
		}
	} else {
		// start tbr without filename
		if gui_contexts.is_empty() {
			(None, 0)
		} else {
			return Ok(None);
		}
	};

	let dark_colors = convert_colors(themes.get(true));
	let bright_colors = convert_colors(themes.get(false));
	let colors = if configuration.dark_theme {
		dark_colors.clone()
	} else {
		bright_colors.clone()
	};

	let (i18n, icons, fonts, db, css_provider) = if let Some(gc) = gui_contexts.get(0) {
		(gc.i18n.clone(), gc.icons.clone(), gc.fonts.clone(), gc.db.clone(), gc.css_provider.clone())
	} else {
		let i18n = I18n::new(&configuration.gui.lang).unwrap();
		let i18n = Rc::new(i18n);
		let icons = load_icons();
		let icons = Rc::new(icons);
		let fonts = font::user_fonts(&configuration.gui.fonts)?;
		let fonts = Rc::new(fonts);
		let db = DictionaryBook::load(&configuration.gui.dictionaries, configuration.gui.cache_dict);
		let db = Rc::new(RefCell::new(db));
		let css_provider = view::init_css("main", &colors.background);
		(i18n, icons, fonts, db, css_provider)
	};

	let container_manager = Default::default();
	let (container, book, reading) = if let Some(loading) = loading {
		let mut container = load_container(&container_manager, loading.filename())?;
		let (book, reading) = load_book(&container_manager, &mut container, loading)?;
		(container, book, reading)
	} else {
		let readme = i18n.msg("readme");
		let container: Box<dyn Container> = Box::new(ReadmeContainer::new(readme.as_ref()));
		let book: Box<dyn Book> = Box::new(ReadmeBook::new(readme.as_ref()));
		(container, book, ReadingInfo::fake(README_TEXT_FILENAME))
	};

	let mut render_context = RenderContext::new(
		colors,
		reading.font_size,
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
	let (dm, dict_view, lookup_entry) = DictionaryManager::new(
		db.clone(),
		&configuration.gui.dictionaries,
		configuration.gui.cache_dict,
		configuration.gui.dict_font_size,
		fonts.clone(),
		&i18n,
		&icons,
	);

	let dark_theme = configuration.dark_theme;
	drop(configuration);

	let (custom_color, custom_font, custom_style) = custom_settings(book.as_ref(), &reading);
	let controller = Controller::from_data(
		reading,
		container_manager,
		container,
		book,
		Box::new(view.clone()),
		&mut render_context);

	let ctx = Rc::new(RefCell::new(render_context));
	let ctrl = Rc::new(RefCell::new(controller));
	let settings = Settings::new(gcs.clone());
	let (gc, chapter_list_view) = GuiContext::new(app, settings,
		&cfg, &ctrl, &ctx, db, dm,
		icons, i18n.clone(), fonts,
		dark_colors, bright_colors, css_provider);

	// now setup ui
	setup_sidebar(&gc, &view, &dict_view, chapter_list_view);
	setup_view(&gc, &view);
	setup_chapter_list(&gc);

	let (toolbar, search_box)
		= setup_toolbar(&gc, &view, &lookup_entry, dark_theme,
		custom_color, custom_font, custom_style);

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
						let reading = &mut controller.reading;
						if reading.font_size < MAX_FONT_SIZE {
							reading.font_size += 2;
							controller.render.set_font_size(
								reading.font_size,
								controller.book.custom_fonts(),
								render_context);
							controller.redraw(render_context);
						}
					});
					glib::Propagation::Stop
				}
				(Key::minus, ModifierType::CONTROL_MASK) => {
					apply(&gc, |controller, render_context| {
						let reading = &mut controller.reading;
						if reading.font_size > MIN_FONT_SIZE {
							reading.font_size -= 2;
							controller.render.set_font_size(
								reading.font_size,
								controller.book.custom_fonts(),
								render_context);
							controller.redraw(render_context);
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

	setup_window(&gc, toolbar, view, search_box);

	{
		let gcs = gcs.clone();
		gc.window.connect_close_request(move |win| {
			gcs.borrow_mut().retain(|c| c.window != *win);
			Propagation::Proceed
		});
	}

	gui_contexts.insert(gc_idx, gc.clone());
	Ok(Some(gc))
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
					if gc.cfg().gui.scroll_for_page{
						handle(&gc, |controller, render_context|
							controller.next_page(render_context));
					} else {
						handle(&gc, |controller, render_context|
							controller.step_next(render_context));
					}
				} else {
					if gc.cfg().gui.scroll_for_page{
						handle(&gc, |controller, render_context|
							controller.prev_page(render_context));
					} else {
						handle(&gc, |controller, render_context|
							controller.step_prev(render_context));
					}
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

	let sidebar_position = &gc.cfg().gui.sidebar_position;
	set_sidebar_position(gc, sidebar_position);

	let paned = &gc.paned;
	paned.set_start_child(Some(&sidebar));
	paned.set_end_child(Some(view));
	paned.set_position(0);

	let gc = gc.clone();
	paned.connect_position_notify(move |paned| {
		let position = paned.position();
		if position > 0 {
			sidebar_updated(
				&mut gc.cfg_mut(),
				&mut gc.dm_mut(),
				position)
		}
	});
}

fn sidebar_updated(configuration: &mut Configuration,
	dictionary_manager: &mut DictionaryManager,
	position: i32)
{
	configuration.gui.sidebar_size = position as u32;
	dictionary_manager.resize(position, None);
}

#[inline]
fn set_sidebar_position(gc: &GuiContext, position: &SidebarPosition)
{
	gc.paned.set_orientation(position.paned_orientation());
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
				gc.toggle_sidebar();
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
		gc.toggle_sidebar();
		true
	} else if let Some(current_tab_name) = stack.visible_child_name() {
		if current_tab_name == tab_name {
			if toggle {
				gc.toggle_sidebar();
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
	search_box: SearchEntry)
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
	update_title(window, &gc.ctrl());

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
						gc.toggle_sidebar();
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
					gc.switch_theme();
					glib::Propagation::Stop
				}
				(Key::T, ModifierType::SHIFT_MASK) => {
					gc.custom_color_action.activate(None);
					glib::Propagation::Stop
				}
				(Key::F, ModifierType::SHIFT_MASK) => {
					gc.custom_font_action.activate(None);
					glib::Propagation::Stop
				}
				(Key::S, ModifierType::SHIFT_MASK) => {
					gc.custom_style_dialog();
					glib::Propagation::Stop
				}
				(Key::s, ModifierType::CONTROL_MASK) => {
					gc.show_settings();
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
				(Key::F11, MODIFIER_NONE) => {
					let win = &gc.window;
					win.set_fullscreened(!win.is_fullscreened());
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
			let mut configuration = gc.cfg_mut();
			configuration.gui.dict_font_size = gc.dm.borrow().font_size();
			if let Err(e) = configuration.save() {
				eprintln!("Failed save configuration: {}", e.to_string());
			}
			glib::Propagation::Proceed
		});
	}

	window.present();
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

#[inline]
fn setup_toolbar(gc: &GuiContext, view: &GuiView, lookup_entry: &SearchEntry,
	dark_theme: bool, custom_color: Option<bool>, custom_font: Option<bool>,
	custom_style: Option<Option<String>>) -> (gtk4::Box, SearchEntry)
{
	let i18n = &gc.i18n;

	let toolbar = gtk4::Box::builder()
		.css_classes(vec!["toolbar"])
		.build();

	let sidebar_button = &gc.sidebar_btn;
	{
		let gc = gc.clone();
		sidebar_button.connect_clicked(move |_| {
			gc.toggle_sidebar();
		});
		toolbar.append(sidebar_button);
	}

	{
		let gc = gc.clone();
		lookup_entry.connect_stop_search(move |_| {
			gc.toggle_sidebar();
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

	setup_main_menu(gc, view, dark_theme, custom_color, custom_font, custom_style);
	toolbar.append(&gc.menu_btn);

	let search_box = SearchEntry::builder()
		.placeholder_text(i18n.msg("search-hint"))
		.activates_default(true)
		.enable_undo(true)
		.build();
	toolbar.append(&search_box);

	(toolbar, search_box)
}

fn setup_main_menu(gc: &GuiContext, view: &GuiView, dark_theme: bool,
	custom_color: Option<bool>, custom_font: Option<bool>,
	custom_style: Option<Option<String>>)
{
	#[inline]
	fn create_action<F>(menu: &Menu, action_group: &SimpleActionGroup,
		i18n: &Rc<I18n>, key: &str, callback: F)
		where F: Fn(&SimpleAction, Option<&Variant>) + 'static
	{
		let action = SimpleAction::new(key, None);
		append_action(menu, action_group, i18n, key, &action, callback)
	}
	fn append_action<F>(menu: &Menu, action_group: &SimpleActionGroup,
		i18n: &Rc<I18n>, key: &str, action: &SimpleAction, callback: F)
		where F: Fn(&SimpleAction, Option<&Variant>) + 'static
	{
		action.connect_activate(callback);
		let title = i18n.msg(key);
		let action_name = format!("main.{}", key);
		let menu_item = MenuItem::new(Some(&title), Some(&action_name));
		menu.append_item(&menu_item);
		action_group.add_action(action);
	}

	fn append_toggle_action<F>(menu: &Menu, action_group: &SimpleActionGroup,
		i18n: &Rc<I18n>, key: &str, action: &SimpleAction,
		toggle: Option<bool>, callback: F)
		where F: Fn(&SimpleAction, Option<&Variant>) + 'static
	{
		if let Some(active) = toggle {
			action.set_state(&active.to_variant());
		} else {
			action.set_state(&false.to_variant());
			action.set_enabled(false);
		}
		action.connect_activate(callback);
		let title = i18n.msg(key);
		let action_name = format!("main.{}", key);
		let menu_item = MenuItem::new(Some(&title), Some(&action_name));
		menu.append_item(&menu_item);
		action_group.add_action(action);
	}

	let button = &gc.menu_btn;

	let action_group = SimpleActionGroup::new();
	let menu = Menu::new();
	let i18n = &gc.i18n;
	let section = Menu::new();
	menu.append_section(None, &section);

	button.insert_action_group("main", Some(&action_group));

	{
		let gc = gc.clone();
		create_action(&section, &action_group, i18n,
			OPEN_FILE_KEY, move |_, _| {
				gc.open_dialog();
			});
	}

	gc.history_popover.set_parent(button);
	button.insert_action_group("history", Some(&gc.action_group));
	gc.reload_history();
	{
		let gc = gc.clone();
		create_action(&section, &action_group, i18n,
			HISTORY_KEY, move |_, _| {
				gc.show_history();
			});
	}

	{
		let gc = gc.clone();
		create_action(&section, &action_group, i18n,
			RELOAD_KEY, move |_, _| {
				gc.reload_book();
			});
	}

	{
		let gc = gc.clone();
		create_action(&section, &action_group, i18n,
			BOOK_INFO_KEY, move |_, _| {
				if let Err(err) = gc.book_info() {
					gc.error(&gc.i18n.args_msg("failed-load-reading", vec![
						("error", err.to_string()),
					]));
				}
			});
	}

	{
		let gc = gc.clone();
		create_action(&section, &action_group, i18n,
			SETTINGS_KEY, move |_, _| gc.show_settings());
	}

	{
		let action = &gc.theme_action;
		let gc = gc.clone();
		append_toggle_action(&section, &action_group, i18n,
			THEME_KEY, action, Some(dark_theme), move |_, _| {
				gc.switch_theme();
			});
	}

	let section = Menu::new();
	menu.append_section(None, &section);

	{
		let action = &gc.custom_color_action;
		let gc = gc.clone();
		append_toggle_action(&section, &action_group, i18n,
			CUSTOM_COLOR_KEY, action, custom_color, move |_, _| {
				gc.toggle_custom_color();
			});
	}

	{
		let action = &gc.custom_font_action;
		let gc = gc.clone();
		append_toggle_action(&section, &action_group, i18n,
			CUSTOM_FONT_KEY, action, custom_font, move |_, _| {
				gc.toggle_custom_font();
			});
	}

	{
		let action = &gc.custom_style_action;
		if custom_style.is_none() {
			action.set_enabled(false);
		}
		let gc = gc.clone();
		append_action(&section, &action_group, i18n,
			CUSTOM_STYLE_KEY, action, move |_, _| {
				gc.custom_style_dialog();
			});
	}

	let pm = PopoverMenu::builder()
		.has_arrow(false)
		.position(PositionType::Bottom)
		.menu_model(&MenuModel::from(menu))
		.build();
	pm.set_parent(button);
	{
		let view = view.clone();
		pm.connect_visible_notify(move |_| {
			view.grab_focus();
		});
	}

	button.connect_clicked(move |_| pm.popup());
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
fn create_action(name: &str) -> SimpleAction
{
	SimpleAction::new(name, None)
}

#[inline]
fn create_toggle_action(name: &str) -> SimpleAction
{
	SimpleAction::new_stateful(name, None, &false.to_variant())
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
fn create_button(name: &str, tooltip: Option<&str>, icons: &IconMap, inline: bool) -> Button
{
	let image = load_button_image(name, icons, inline);
	let button = Button::builder()
		.child(&image)
		.focus_on_click(false)
		.focusable(false)
		.build();
	button.set_tooltip_text(tooltip);

	if inline {
		button.add_css_class("inline");
		button.set_valign(Align::Center);
	}
	button
}

#[inline(always)]
fn update_title(window: &ApplicationWindow, controller: &GuiController)
{
	let name = controller.reading_book_name();
	let title = format!("{} - {}", package_name!(), name);
	window.set_title(Some(&title));
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
	sidebar_btn: ToggleButton,
	theme_action: SimpleAction,
	custom_color_action: SimpleAction,
	custom_font_action: SimpleAction,
	custom_style_action: SimpleAction,
	menu_btn: Button,
	chapter_list: ChapterList,
	icons: Rc<IconMap>,
	i18n: Rc<I18n>,
	fonts: Rc<Option<UserFonts>>,
	dark_colors: Colors,
	bright_colors: Colors,
	css_provider: CssProvider,
	file_dialog: FileDialog,
	settings: Settings,
	db: Rc<RefCell<DictionaryBook>>,
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
	fn new(app: &Application, settings: Settings,
		cfg: &Rc<RefCell<Configuration>>, ctrl: &Rc<RefCell<GuiController>>,
		ctx: &Rc<RefCell<RenderContext>>, db: Rc<RefCell<DictionaryBook>>,
		dm: Rc<RefCell<DictionaryManager>>,
		icons: Rc<IconMap>, i18n: Rc<I18n>, fonts: Rc<Option<UserFonts>>,
		dark_colors: Colors, bright_colors: Colors, css_provider: CssProvider)
		-> (Self, gtk4::Box)
	{
		#[inline]
		fn create_history_menu(view: &GuiView) -> (SimpleActionGroup, Menu, PopoverMenu)
		{
			let action_group = SimpleActionGroup::new();
			let history_menu = Menu::new();
			let history_popover = PopoverMenu::builder()
				.menu_model(&history_menu)
				.has_arrow(false)
				.build();
			let view = view.clone();
			history_popover.connect_visible_notify(move |p| {
				if !p.get_visible() {
					view.grab_focus();
				}
			});
			let key_event = EventControllerKey::new();
			key_event.connect_key_pressed(move |ev, key, _, modifier| {
				let (key, modifier) = ignore_cap(key, modifier);
				match (key, modifier) {
					(Key::j, MODIFIER_NONE) => {
						ev.widget().emit_move_focus(DirectionType::Down);
						glib::Propagation::Stop
					}
					(Key::k, MODIFIER_NONE) => {
						ev.widget().emit_move_focus(DirectionType::Up);
						glib::Propagation::Stop
					}
					(Key::h, MODIFIER_NONE) |
					(Key::q, MODIFIER_NONE) => {
						ev.widget().set_visible(false);
						glib::Propagation::Stop
					}
					_ => {
						// println!("view, key: {key}, modifier: {modifier}");
						glib::Propagation::Proceed
					}
				}
			});
			history_popover.add_controller(key_event);

			(action_group, history_menu, history_popover)
		}

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
		let sidebar_btn = create_toggle_button(false, "sidebar.svg",
			SIDEBAR_KEY, &icons, &i18n);
		let theme_action = create_toggle_action(THEME_KEY);
		let custom_color_action = create_toggle_action(CUSTOM_COLOR_KEY);
		let custom_font_action = create_toggle_action(CUSTOM_FONT_KEY);
		let custom_style_action = create_action(CUSTOM_STYLE_KEY);

		let file_dialog = FileDialog::new();
		file_dialog.set_title(&i18n.msg("file-open-title"));
		file_dialog.set_modal(true);
		let filter = FileFilter::new();
		for ext in controller.container_manager.book_loader.extension() {
			filter.add_suffix(&ext[1..]);
		}
		file_dialog.set_default_filter(Some(&filter));

		let (action_group, history_menu, history_popover) = create_history_menu(controller.render.as_ref());
		let menu_btn = create_button("menu.svg", Some(&i18n.msg("menu")), &icons, false);

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
			theme_action,
			custom_color_action,
			custom_font_action,
			custom_style_action,
			menu_btn,
			chapter_list,
			icons,
			i18n,
			fonts,
			dark_colors,
			bright_colors,
			css_provider,
			file_dialog,
			settings,
			db,
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
				if let Some(app) = self.window.application() {
					app_open(&app, filepath);
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
		fn label(title: &str, text: &mut String) -> Label
		{
			text.push('\n');
			text.push_str(title);
			Label::builder()
				.halign(Align::Start)
				.label(title)
				.build()
		}

		let mut text = String::new();
		let controller = self.ctrl_mut();
		let reading = &controller.reading;
		let path = PathBuf::from_str(&reading.filename)?;
		let meta = path.metadata()?;
		let container = gtk4::Box::new(Orientation::Vertical, 10);
		container.append(&label(&reading.filename, &mut text));
		container.append(&label(&format_size(meta.len()), &mut text));
		container.append(&Separator::new(Orientation::Horizontal));
		if let Some(book_names) = controller.container.inner_book_names() {
			if let Some(name) = book_names.get(reading.inner_book) {
				container.append(&label(&name.name(), &mut text));
			}
		}
		let status = controller.status();
		if let Some(title) = status.title {
			container.append(&label(title, &mut text));
		}
		container.append(&label(&status.position(), &mut text));
		let popover = Popover::builder()
			.child(&container)
			.build();
		popover.set_parent(&self.menu_btn);

		let key_event = EventControllerKey::new();
		key_event.connect_key_pressed(move |ev, key, _, modifier| {
			let (key, modifier) = ignore_cap(key, modifier);
			match (key, modifier) {
				(Key::i, MODIFIER_NONE) |
				(Key::q, MODIFIER_NONE) => {
					ev.widget().set_visible(false);
					glib::Propagation::Stop
				}
				(Key::c, ModifierType::CONTROL_MASK) => {
					copy_to_clipboard(&text);
					glib::Propagation::Stop
				}
				_ => {
					// println!("view, key: {key}, modifier: {modifier}");
					glib::Propagation::Proceed
				}
			}
		});
		popover.add_controller(key_event);
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
		let menu_action_name = format!("history.{}", action_name);
		menu.append(Some(&path_str), Some(&menu_action_name));
	}

	fn toggle_sidebar(&self)
	{
		let paned = &self.paned;
		let (on, position) = if paned.position() == 0 {
			(true, self.cfg().gui.sidebar_size as i32)
		} else {
			(false, 0)
		};
		self.sidebar_btn.set_active(on);
		paned.set_position(position);
	}

	fn switch_theme(&self)
	{
		let mut configuration = self.cfg_mut();
		let dark_theme = !configuration.dark_theme;
		self.theme_action.set_state(&dark_theme.to_variant());
		configuration.dark_theme = dark_theme;
		let mut render_context = self.ctx_mut();
		render_context.colors = if dark_theme {
			self.dark_colors.clone()
		} else {
			self.bright_colors.clone()
		};
		let mut controller = self.ctrl_mut();
		controller.redraw(&mut render_context);
		view::update_css(&self.css_provider, "main", &render_context.colors.background);
	}

	fn toggle_custom_color(&self)
	{
		let mut controller = self.ctrl_mut();
		let custom_color = !controller.reading.custom_color;
		self.custom_color_action.set_state(&custom_color.to_variant());
		controller.reading.custom_color = custom_color;
		let mut render_context = self.ctx_mut();
		render_context.custom_color = custom_color;
		controller.redraw(&mut render_context);
	}

	fn toggle_custom_font(&self)
	{
		let mut controller = self.ctrl_mut();
		let custom_font = !controller.reading.custom_font;
		self.custom_font_action.set_state(&custom_font.to_variant());
		controller.reading.custom_font = custom_font;
		let mut render_context = self.ctx_mut();
		controller.render.set_custom_font(
			custom_font,
			controller.book.custom_fonts(),
			&mut render_context);
		controller.redraw(&mut render_context);
	}

	fn custom_style_dialog(&self)
	{
		let controller = self.ctrl();
		let reading = &controller.reading;
		let gc = self.clone();
		custom_style::dialog(&reading.custom_style, &self.i18n, &self.window, move |new_style| {
			let mut controller = gc.ctrl_mut();
			let custom_style = if let Some(custom_style) = &controller.reading.custom_style {
				if new_style == *custom_style {
					return;
				} else if new_style.is_empty() {
					None
				} else {
					Some(new_style)
				}
			} else if new_style.is_empty() {
				return;
			} else {
				Some(new_style)
			};
			controller.reading.custom_style = custom_style;
			drop(controller);
			gc.reload_book();
		});
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

	#[inline]
	fn show_settings(&self)
	{
		self.settings.dialog(self);
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
	gcs: &Rc<RefCell<Vec<GuiContext>>>)
{
	match build_ui(app, cfg.clone(), &themes, gcs) {
		Ok(Some(gc)) => {
			// clean temp files
			app.connect_shutdown(move |_| gc.opener().cleanup());
		}
		// previous opened
		Ok(None) => {}
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
		.flags(ApplicationFlags::HANDLES_OPEN)
		.application_id(APP_ID)
		.build();

	{
		app.connect_startup(|app| {
			let css_provider = CssProvider::new();
			css_provider.load_from_string(include_str!("../assets/gui/gtk.css"));
			gtk4::style_context_add_provider_for_display(
				&Display::default().expect("Could not connect to a display."),
				&css_provider,
				gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
			);
			Window::set_default_icon_name("tbr-icon");

			#[cfg(unix)]
			{
				handle_signal(2, app.clone());
				handle_signal(15, app.clone());
			}
		});
	}

	let current = configuration.current.clone();
	let cfg = Rc::new(RefCell::new(configuration));
	let themes = Rc::new(themes);
	let gcs = Rc::new(RefCell::new(vec![]));
	{
		let cfg = cfg.clone();
		let themes = themes.clone();
		let gcs = gcs.clone();
		app.connect_open(move |app, files, _| {
			if !files.is_empty() {
				if let Some(path) = files[0].path() {
					if let Some(path) = path.to_str() {
						cfg.borrow_mut().current = Some(path.to_owned());
						show(app, &cfg, &themes, &gcs);
						let mut gui_contexts = gcs.borrow_mut();
						if let Ok(idx) = get_gc(gui_contexts.as_ref(), README_TEXT_FILENAME) {
							let gc = gui_contexts.remove(idx);
							drop(gui_contexts);
							gc.window.close();
						}
					}
				}
			}
		});
	}
	app.connect_activate(move |app| {
		show(app, &cfg, &themes, &gcs);
	});

	let mut args = env::args().collect::<Vec<_>>();
	args.drain(1..);
	if let Some(filename) = current {
		args.push(filename);
	}
	if app.run_with_args::<String>(&args) == ExitCode::FAILURE {
		bail!("Failed start tbr")
	}

	Ok(None)
}

#[cfg(unix)]
fn handle_signal(signum: i32, app: Application)
{
	glib::unix_signal_add_local_once(signum, move || {
		for win in app.windows() {
			win.close();
		}
	});
}

#[inline]
fn alert(title: &str, msg: &str, parent: &impl IsA<Window>)
{
	AlertDialog::builder()
		.message(title)
		.detail(msg)
		.modal(true)
		.build()
		.show(Some(parent))
}

#[inline]
fn app_open(app: &Application, filepath: &str)
{
	app.open(&vec![gtk4::gio::File::for_commandline_arg(filepath)], "");
}

#[inline]
fn get_gc(gcs: &Vec<GuiContext>, filename: &str) -> core::result::Result<usize, usize>
{
	gcs.binary_search_by(|gc| gc.ctrl().reading.filename.as_str().cmp(filename))
}
