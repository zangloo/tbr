use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufReader, Cursor, Read};
use std::ops::Index;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use cursive::theme::{BaseColor, Color, PaletteColor, Theme};
use eframe::{egui, IconData};
use eframe::egui::{Button, FontData, FontDefinitions, Frame, Id, ImageButton, Pos2, Rect, Response, Sense, TextureId, Ui, Vec2, Widget};
use eframe::glow::Context;
use egui::{Align, Area, CursorIcon, DroppedFile, Key, Modifiers, Order, RichText, ScrollArea, TextEdit, TextStyle};
use egui_extras::RetainedImage;
use image::{DynamicImage, ImageFormat};
use image::imageops::FilterType;

use crate::{Asset, Color32, Configuration, I18n, ReadingInfo, ThemeEntry};
use crate::book::{Book, Colors, Line};
use crate::common::{get_theme, reading_info, txt_lines};
use crate::container::{BookContent, BookName, Container, load_book, load_container};
use crate::controller::{Controller, HighlightInfo, HighlightMode};
use crate::gui::dict::DictionaryManager;
use crate::gui::render::{measure_char_size, RenderContext};
use crate::gui::settings::SettingsData;
use crate::gui::view::GuiView;

mod render;
mod settings;
mod dict;
mod view;

const ICON_SIZE: Vec2 = Vec2 { x: 32.0, y: 32.0 };
const INLINE_ICON_SIZE: Vec2 = Vec2 { x: 16.0, y: 16.0 };
const APP_ICON_SIZE: u32 = 48;
const MIN_FONT_SIZE: u8 = 20;
const MAX_FONT_SIZE: u8 = 50;
const FONT_FILE_EXTENSIONS: [&str; 3] = ["ttf", "otf", "ttc"];
const MIN_SIDEBAR_WIDTH: f32 = 300.0;

const README_TEXT_FILENAME: &str = "readme";

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

fn load_icons() -> Result<HashMap<String, RetainedImage>>
{
	const ICONS_PREFIX: &str = "gui/image/";
	let mut map = HashMap::new();
	for file in Asset::iter() {
		if file.starts_with("gui/image/") && file.ends_with(".svg") {
			let content = Asset::get(file.as_ref()).unwrap().data;
			let retained_image = RetainedImage::from_svg_bytes(file.to_string(), &content).unwrap();
			let name = &file[ICONS_PREFIX.len()..];
			map.insert(name.to_string(), retained_image);
		}
	}
	Ok(map)
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

fn insert_font(fonts: &mut FontDefinitions, name: &str, font_data: FontData)
{
	fonts.font_data.insert(name.to_string(), font_data);

	fonts.families
		.entry(egui::FontFamily::Proportional)
		.or_default()
		.insert(0, name.to_string());

	fonts.families
		.entry(egui::FontFamily::Monospace)
		.or_default()
		.insert(0, name.to_string());
}

#[derive(PartialEq)]
enum SidebarList {
	Chapter(bool),
	Dictionary,
	Font,
}

enum AppStatus {
	Startup,
	Normal(String),
	Error(String, u64),
}

fn setup_fonts(ctx: &egui::Context, font_paths: &Vec<PathBuf>) -> Result<()>
{
	let mut fonts = FontDefinitions::default();
	if font_paths.is_empty() {
		let content = Asset::get("font/wqy-zenhei.ttc")
			.unwrap()
			.data
			.as_ref()
			.to_vec();
		insert_font(&mut fonts, "embedded", FontData::from_owned(content));
	} else {
		for path in font_paths {
			let mut file = OpenOptions::new().read(true).open(path)?;
			let mut buf = vec![];
			file.read_to_end(&mut buf)?;
			let filename = path.file_name().unwrap().to_str().unwrap();
			insert_font(&mut fonts, filename, FontData::from_owned(buf));
		}
	}
	ctx.set_fonts(fonts);
	Ok(())
}

enum GuiCommand {
	PageDown,
	PageUp,
	StepForward,
	StepBackward,
	TraceForward,
	TraceBackward,
	SearchForward,
	SearchBackward,
	// can not disable tab for navigate between view and search box
	// NextLink, PrevLink,
	TryGotoLink,
	GotoLink(usize, usize),
	ChapterBegin,
	ChapterEnd,
	NextChapter,
	PrevChapter,
	ClearHeightLight,
	CopyHeightLight,

	MouseDrag(Pos2, Pos2),
	MouseMove(Pos2),
	OpenDroppedFile(PathBuf),
}

enum DialogData {
	Setting(SettingsData),
}

struct DictionaryLookupData {
	words: Vec<String>,
	current_word: usize,
}

struct ReaderApp {
	configuration: Configuration,
	theme_entries: Vec<ThemeEntry>,
	i18n: I18n,
	images: HashMap<String, RetainedImage>,
	controller: Controller<Ui, GuiView>,

	status: AppStatus,
	current_toc: usize,
	popup_menu: Option<Pos2>,
	selected_text: String,
	sidebar: bool,
	sidebar_list: SidebarList,
	dialog: Option<DialogData>,
	input_search: bool,
	search_pattern: String,
	dropdown: bool,
	response_rect: Rect,
	dictionary: DictionaryManager,
	dictionary_lookup: DictionaryLookupData,

	view_rect: Rect,
	font_size: u8,
	default_font_measure: Vec2,
	colors: Colors,
}

impl ReaderApp {
	#[inline]
	fn image(&self, ctx: &egui::Context, name: &str) -> TextureId
	{
		let image = self.images.get(name).unwrap();
		image.texture_id(ctx)
	}

	#[inline]
	fn error(&mut self, error: String)
	{
		self.status = AppStatus::Error(error, ts());
	}

	#[inline]
	fn update_status(&mut self, status: String)
	{
		if let AppStatus::Error(_, start) = &self.status {
			if ts() - start < 5 {
				return;
			}
		}
		self.current_toc = self.controller.toc_index();
		self.status = AppStatus::Normal(status);
	}

	fn setup_sidebar(&mut self, ui: &mut Ui, width: f32)
	{
		egui::menu::bar(ui, |ui| {
			let chapter_text = self.i18n.msg("tab-chapter");
			let text = RichText::new(chapter_text.as_ref())
				.text_style(TextStyle::Heading);
			ui.selectable_value(
				&mut self.sidebar_list,
				SidebarList::Chapter(true),
				text);

			let dictionary_text = self.i18n.msg("tab-dictionary");
			let text = RichText::new(dictionary_text.as_ref())
				.text_style(TextStyle::Heading);
			ui.selectable_value(
				&mut self.sidebar_list,
				SidebarList::Dictionary,
				text);

			let font_text = self.i18n.msg("tab-font");
			let text = RichText::new(font_text.as_ref())
				.text_style(TextStyle::Heading);
			ui.selectable_value(
				&mut self.sidebar_list,
				SidebarList::Font,
				text);
		});
		ScrollArea::vertical().max_width(width).show(ui, |ui| {
			match self.sidebar_list {
				SidebarList::Chapter(init) => {
					let mut selected_book = None;
					let mut selected_toc = None;
					for (index, bn) in self.controller.container.inner_book_names().iter().enumerate() {
						let bookname = bn.name();
						if bookname == README_TEXT_FILENAME {
							break;
						}
						if index == self.controller.reading.inner_book {
							ui.heading(RichText::from(bookname).color(Color32::LIGHT_RED));
							if let Some(toc) = self.controller.book.toc_iterator() {
								for (title, value) in toc {
									let current = self.current_toc == value;
									let label = ui.selectable_label(current, title);
									if current && init {
										self.sidebar_list = SidebarList::Chapter(false);
										label.scroll_to_me(Some(Align::Center));
									}
									if label.clicked() {
										selected_toc = Some(value);
									}
								}
							}
						} else if ui.button(RichText::from(bookname).heading()).clicked() {
							selected_book = Some(index);
						}
					}
					if let Some(index) = selected_book {
						let new_reading = ReadingInfo::new(&self.controller.reading.filename)
							.with_inner_book(index);
						let msg = self.controller.switch_book(new_reading, ui);
						self.update_status(msg);
					} else if let Some(index) = selected_toc {
						if let Some(msg) = self.controller.goto_toc(index, ui) {
							self.update_status(msg);
						}
					}
				}
				SidebarList::Dictionary => {
					let lookup = &mut self.dictionary_lookup;
					let size = lookup.words.len();
					if size > 0 {
						if lookup.current_word >= size {
							lookup.current_word = size - 1;
						}
						let word = lookup.words
							.get(lookup.current_word).unwrap();
						self.dictionary.lookup_and_render(ui, &self.i18n,
							self.font_size as f32, word);
					}
				}
				SidebarList::Font => {
					let mut font_deleted = None;
					let font_remove_id = self.image(ui.ctx(), "remove.svg");
					ui.horizontal(|ui| {
						let font_add_id = self.image(ui.ctx(), "add.svg");
						if ImageButton::new(font_add_id, INLINE_ICON_SIZE).ui(ui).clicked() {
							let dialog = rfd::FileDialog::new()
								.add_filter(self.i18n.msg("font-file").as_ref(), &FONT_FILE_EXTENSIONS);
							if let Some(paths) = dialog.pick_files() {
								let mut new_fonts = self.configuration.gui.fonts.clone();
								'outer:
								for path in paths {
									for font in &new_fonts {
										if *font == path {
											continue 'outer;
										}
									}
									new_fonts.push(path)
								}
								if new_fonts.len() != self.configuration.gui.fonts.len() {
									match setup_fonts(ui.ctx(), &new_fonts) {
										Ok(_) => self.configuration.gui.fonts = new_fonts,
										Err(e) => {
											let error = self.i18n.args_msg("font-fail", vec![
												("error", e.to_string())
											]);
											self.error(error);
										}
									}
								}
							}
						}
						ui.label(self.i18n.msg("font-demo").as_ref());
					});
					for i in (0..self.configuration.gui.fonts.len()).rev() {
						let font = self.configuration.gui.fonts[i].to_str().unwrap();
						ui.horizontal(|ui| {
							if ImageButton::new(font_remove_id, INLINE_ICON_SIZE).ui(ui).clicked() {
								font_deleted = Some(i);
							}
							ui.label(font);
						});
					}
					if let Some(font_deleted) = font_deleted {
						self.configuration.gui.fonts.remove(font_deleted);
						if let Err(e) = setup_fonts(ui.ctx(), &self.configuration.gui.fonts) {
							let error = self.i18n.args_msg("font-fail", vec![
								("error", e.to_string())
							]);
							self.error(error);
						}
					}
				}
			}
		});
	}

	fn setup_input(&mut self, response: &Response, frame: &mut eframe::Frame,
		ui: &mut Ui) -> Result<bool>
	{
		let rect = &response.rect;
		if let Some(command) = response.ctx.input_mut(|input| {
			if input.consume_key(Modifiers::NONE, Key::Space)
				|| input.consume_key(Modifiers::NONE, Key::PageDown) {
				Some(GuiCommand::PageDown)
			} else if input.consume_key(Modifiers::SHIFT, Key::Space)
				|| input.consume_key(Modifiers::NONE, Key::PageUp) {
				Some(GuiCommand::PageUp)
			} else if input.consume_key(Modifiers::NONE, Key::ArrowDown) {
				Some(GuiCommand::StepForward)
			} else if input.consume_key(Modifiers::NONE, Key::ArrowUp) {
				Some(GuiCommand::StepBackward)
			} else if input.consume_key(Modifiers::NONE, Key::ArrowLeft) {
				Some(GuiCommand::TraceBackward)
			} else if input.consume_key(Modifiers::NONE, Key::ArrowRight) {
				Some(GuiCommand::TraceForward)
			} else if input.consume_key(Modifiers::NONE, Key::N) {
				Some(GuiCommand::SearchForward)
			} else if input.consume_key(Modifiers::SHIFT, Key::N) {
				Some(GuiCommand::SearchBackward)
				// } else if input.consume_key(Modifiers::SHIFT, Key::Tab) {
				// 	Some(GuiCommand::PrevLink)
				// } else if input.consume_key(Modifiers::NONE, Key::Tab) {
				// 	Some(GuiCommand::NextLink)
			} else if input.consume_key(Modifiers::NONE, Key::C) {
				if self.sidebar && matches!(self.sidebar_list, SidebarList::Chapter(_)) {
					self.sidebar = false;
				} else {
					self.sidebar = true;
					self.sidebar_list = SidebarList::Chapter(true);
				}
				None
			} else if input.consume_key(Modifiers::NONE, Key::D) {
				if self.sidebar && matches!(self.sidebar_list, SidebarList::Dictionary) {
					self.sidebar = false;
				} else {
					self.sidebar = true;
					self.sidebar_list = SidebarList::Dictionary;
				}
				None
			} else if input.consume_key(Modifiers::NONE, Key::Enter) {
				Some(GuiCommand::TryGotoLink)
			} else if input.consume_key(Modifiers::NONE, Key::Home) {
				if self.controller.reading.line != 0 || self.controller.reading.position != 0 {
					Some(GuiCommand::ChapterBegin)
				} else {
					None
				}
			} else if input.consume_key(Modifiers::NONE, Key::End) {
				Some(GuiCommand::ChapterEnd)
			} else if input.consume_key(Modifiers::CTRL, Key::D) {
				Some(GuiCommand::NextChapter)
			} else if input.consume_key(Modifiers::CTRL, Key::B) {
				Some(GuiCommand::PrevChapter)
			} else if input.consume_key(Modifiers::CTRL, Key::ArrowUp) {
				if self.configuration.gui.font_size < MAX_FONT_SIZE {
					self.configuration.gui.font_size += 2;
				}
				None
			} else if input.consume_key(Modifiers::CTRL, Key::ArrowDown) {
				if self.configuration.gui.font_size > MIN_FONT_SIZE {
					self.configuration.gui.font_size -= 2;
				}
				None
			} else if input.consume_key(Modifiers::NONE, Key::Escape) {
				if self.sidebar {
					self.sidebar = false;
					None
				} else if let Some(HighlightInfo { mode: HighlightMode::Selection(_), .. }) = self.controller.highlight {
					Some(GuiCommand::ClearHeightLight)
				} else {
					None
				}
			} else if input.consume_key(Modifiers::CTRL, Key::C) {
				if let Some(HighlightInfo { mode: HighlightMode::Selection(_), .. }) = self.controller.highlight {
					Some(GuiCommand::CopyHeightLight)
				} else {
					None
				}
			} else if input.consume_key(Modifiers::CTRL, Key::F) {
				self.input_search = true;
				None
			} else if let Some(DroppedFile { path: Some(path), .. }) = input.raw.dropped_files.first() {
				let path = path.clone();
				Some(GuiCommand::OpenDroppedFile(path))
			} else if let Some(pointer_pos) = input.pointer.interact_pos() {
				if rect.contains(pointer_pos) {
					if response.clicked() {
						if let Some((line, link_index)) = self.controller.render.link_resolve(pointer_pos, &self.controller.book.lines()) {
							Some(GuiCommand::GotoLink(line, link_index))
						} else {
							None
						}
					} else if input.scroll_delta.y != 0.0 {
						let delta = input.scroll_delta.y;
						// delta > 0.0 for scroll up
						if delta > 0.0 {
							Some(GuiCommand::StepBackward)
						} else {
							Some(GuiCommand::StepForward)
						}
					} else if input.zoom_delta() != 1.0 {
						if input.zoom_delta() > 1.0 {
							if self.configuration.gui.font_size < MAX_FONT_SIZE {
								self.configuration.gui.font_size += 2;
							}
						} else {
							if self.configuration.gui.font_size > MIN_FONT_SIZE {
								self.configuration.gui.font_size -= 2;
							}
						}
						None
					} else if response.secondary_clicked() {
						if let Some(HighlightInfo { mode: HighlightMode::Selection(_), .. }) = &self.controller.highlight {
							self.popup_menu = Some(pointer_pos);
						}
						None
					} else if input.pointer.primary_down() {
						if let Some(from_pos) = input.pointer.press_origin() {
							Some(GuiCommand::MouseDrag(from_pos, pointer_pos))
						} else {
							None
						}
					} else if input.pointer.primary_released() {
						if !self.selected_text.is_empty() {
							let lookup = &mut self.dictionary_lookup;
							lookup.words.clear();
							lookup.words.push(self.selected_text.clone());
							lookup.current_word = 0;
						}
						None
					} else {
						Some(GuiCommand::MouseMove(pointer_pos))
					}
				} else {
					None
				}
			} else {
				None
			}
		}) {
			match command {
				GuiCommand::PageDown => self.controller.next_page(ui)?,
				GuiCommand::PageUp => self.controller.prev_page(ui)?,
				GuiCommand::StepForward => self.controller.step_next(ui),
				GuiCommand::StepBackward => self.controller.step_prev(ui),
				GuiCommand::TraceForward => self.controller.goto_trace(false, ui)?,
				GuiCommand::TraceBackward => self.controller.goto_trace(true, ui)?,
				GuiCommand::SearchForward => self.controller.search_again(true, ui)?,
				GuiCommand::SearchBackward => self.controller.search_again(false, ui)?,
				// GuiCommand::NextLink => self.controller.switch_link_next(ui),
				// GuiCommand::PrevLink => self.controller.switch_link_prev(ui),
				GuiCommand::TryGotoLink => self.controller.try_goto_link(ui)?,
				GuiCommand::GotoLink(line, link_index) => if let Err(e) = self.controller.goto_link(line, link_index, ui) {
					self.error(e.to_string());
				} else {
					self.update_status(self.controller.status_msg());
				}
				GuiCommand::ChapterBegin => self.controller.redraw_at(0, 0, ui),
				GuiCommand::ChapterEnd => { self.controller.goto_end(ui); }
				GuiCommand::NextChapter => { self.controller.switch_chapter(true, ui)?; }
				GuiCommand::PrevChapter => { self.controller.switch_chapter(false, ui)?; }
				GuiCommand::MouseDrag(from_pos, pointer_pos) =>
					if let Some((from, to)) = self.controller.render.calc_selection(from_pos, pointer_pos, &self.view_rect) {
						self.selected_text = self.controller.select_text(from, to, ui);
					} else {
						self.selected_text.clear();
						self.controller.clear_highlight(ui);
					}
				GuiCommand::MouseMove(pointer_pos) => if let Some(_) = self.controller.render.link_resolve(pointer_pos, &self.controller.book.lines()) {
					ui.output_mut(|output| output.cursor_icon = CursorIcon::PointingHand);
				} else {
					ui.output_mut(|output| output.cursor_icon = CursorIcon::Default);
				},
				GuiCommand::ClearHeightLight => {
					self.selected_text.clear();
					self.controller.clear_highlight(ui);
				}
				GuiCommand::CopyHeightLight => ui.output_mut(|output| output.copied_text = self.selected_text.clone()),
				GuiCommand::OpenDroppedFile(path) => self.open_file(path, frame, ui),
			}
			Ok(true)
		} else {
			Ok(false)
		}
	}

	fn setup_toolbar(&mut self, frame: &mut eframe::Frame, ui: &mut Ui)
	{
		let sidebar = self.sidebar;
		let sidebar_id = self.image(ui.ctx(), if sidebar { "sidebar_off.svg" } else { "sidebar_on.svg" });
		if ImageButton::new(sidebar_id, ICON_SIZE).ui(ui).clicked() {
			self.sidebar = !sidebar;
			if self.sidebar && matches!(self.sidebar_list, SidebarList::Chapter(false)) {
				self.sidebar_list = SidebarList::Chapter(true);
			}
		}

		self.setup_history_button(frame, ui);

		let setting_id = self.image(ui.ctx(), "setting.svg");
		if ImageButton::new(setting_id, ICON_SIZE).ui(ui).clicked() {
			self.dialog = Some(DialogData::Setting(SettingsData::new(
				&self.theme_entries,
				&self.configuration.theme_name,
				&self.i18n,
				&self.configuration.gui.lang,
				&self.configuration.gui.dictionary_data_path,
			)));
		}

		match &mut self.dialog {
			Some(DialogData::Setting(settings_data)) =>
				if settings::show(ui, settings_data, &self.i18n) {
					let (update_context, redraw) = self.approve_settings();
					if update_context {
						self.update_context(ui);
					}
					if redraw {
						self.controller.redraw(ui);
					}
					self.dialog = None;
				}
			None => {}
		}

		let mut redraw = false;
		let mut update_context = false;
		let (render_type_id, render_type_tooltip) = if self.configuration.render_type == "han" {
			let id = self.image(ui.ctx(), "render_xi.svg");
			let tooltip = self.i18n.msg("render-xi");
			(id, tooltip)
		} else {
			let id = self.image(ui.ctx(), "render_han.svg");
			let tooltip = self.i18n.msg("render-han");
			(id, tooltip)
		};
		if ImageButton::new(render_type_id, ICON_SIZE)
			.ui(ui)
			.on_hover_text_at_pointer(render_type_tooltip)
			.clicked() {
			let render_type = if self.configuration.render_type == "han" {
				"xi"
			} else {
				"han"
			};
			self.configuration.render_type = render_type.to_owned();
			self.controller.render.reload_render(render_type);
			redraw = true;
		}

		let (custom_color_id, custom_color_tooltip) = if self.controller.reading.custom_color {
			let id = self.image(ui.ctx(), "custom_color_off.svg");
			let tooltip = self.i18n.msg("no-custom-color");
			(id, tooltip)
		} else {
			let id = self.image(ui.ctx(), "custom_color_on.svg");
			let tooltip = self.i18n.msg("with-custom-color");
			(id, tooltip)
		};
		if ImageButton::new(custom_color_id, ICON_SIZE)
			.ui(ui)
			.on_hover_text_at_pointer(custom_color_tooltip)
			.clicked() {
			self.controller.reading.custom_color = !self.controller.reading.custom_color;
			update_context = true;
			redraw = true;
		}
		if update_context {
			self.update_context(ui);
		}
		self.update_context(ui);
		if redraw {
			self.controller.redraw(ui);
		}

		let file_open_id = self.image(ui.ctx(), "file_open.svg");
		if ImageButton::new(file_open_id, ICON_SIZE).ui(ui).clicked() {
			let mut dialog = rfd::FileDialog::new();
			if self.controller.reading.filename != README_TEXT_FILENAME {
				let mut path = PathBuf::from(&self.controller.reading.filename);
				if path.pop() && path.is_dir() {
					dialog = dialog.set_directory(path);
				}
			}
			if let Some(path) = dialog.pick_file() {
				self.open_file(path, frame, ui);
			}
		}

		let search_id = self.image(ui.ctx(), "search.svg");
		ui.image(search_id, ICON_SIZE);
		let search_edit = ui.add(TextEdit::singleline(&mut self.search_pattern)
			.desired_width(100.0)
			.hint_text(self.i18n.msg("search-hint").as_ref())
			.id_source("search_text"));
		if self.input_search {
			if search_edit.ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter)) {
				self.do_search(ui);
			}
			if search_edit.clicked_elsewhere() {
				self.input_search = false;
			}
		}
		if search_edit.lost_focus() {
			self.input_search = false;
		}
		if search_edit.gained_focus() {
			self.input_search = true;
		}
		if self.input_search {
			search_edit.request_focus();
		};

		let status_msg = match &self.status {
			AppStatus::Startup => RichText::from("Starting...").color(Color32::GREEN),
			AppStatus::Normal(status) => RichText::from(status).color(Color32::BLUE),
			AppStatus::Error(error, _) => RichText::from(error).color(Color32::RED),
		};
		ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
			ui.label(status_msg);
		});
	}

	fn setup_history_button(&mut self, frame: &mut eframe::Frame, ui: &mut Ui)
	{
		let history_id = self.image(ui.ctx(), "history.svg");
		let history_popup = ui.make_persistent_id("history_popup");
		let history_button = ImageButton::new(history_id, ICON_SIZE).ui(ui);
		if history_button.clicked() {
			ui.memory_mut(|memory| memory.toggle_popup(history_popup));
		}
		self.dropdown = egui::popup::popup_below_widget(ui, history_popup, &history_button, |ui| {
			ui.set_max_width(400.0);
			let size = self.configuration.history.len();
			let start = if size < 20 {
				0
			} else {
				size - 20
			};
			for i in (start..size).rev() {
				let path_str = &self.configuration.history[i].filename;
				let path = PathBuf::from(path_str);
				if path.exists() {
					if let Some(file_name) = path.file_name() {
						if let Some(text) = file_name.to_str() {
							if ui.button(text)
								.on_hover_text_at_pointer(path.to_str().unwrap())
								.clicked() {
								self.open_file(path, frame, ui);
							}
						}
					}
				}
			}
		}).is_some();
	}

	fn approve_settings(&mut self) -> (bool, bool)
	{
		if let Some(DialogData::Setting(settings)) = &mut self.dialog {
			let mut redraw = false;
			let mut update_context = false;
			if self.configuration.theme_name != settings.theme_name {
				for theme in &self.theme_entries {
					if theme.0 == settings.theme_name {
						self.configuration.theme_name = settings.theme_name.clone();
						self.colors = convert_colors(&theme.1);
						update_context = true;
						redraw = true;
					}
				}
			}
			if self.configuration.gui.lang != settings.locale.locale {
				if let Ok(()) = self.i18n.set_locale(&settings.locale.locale) {
					self.configuration.gui.lang = settings.locale.locale.clone();
				}
			}

			if settings.dictionary_data_path.is_empty() {
				if self.configuration.gui.dictionary_data_path.is_some() {
					self.configuration.gui.dictionary_data_path = None;
					self.dictionary.reload(&self.configuration.gui.dictionary_data_path);
				}
			} else {
				if let Ok(dictionary_data_path) = PathBuf::from_str(&settings.dictionary_data_path) {
					let dictionary_data_path = Some(dictionary_data_path);
					if self.configuration.gui.dictionary_data_path != dictionary_data_path {
						self.configuration.gui.dictionary_data_path = dictionary_data_path;
						self.dictionary.reload(&self.configuration.gui.dictionary_data_path);
					}
				}
			}
			(update_context, redraw)
		} else {
			(false, false)
		}
	}

	fn do_search(&mut self, ui: &mut Ui)
	{
		if let Err(e) = self.controller.search(&self.search_pattern, ui) {
			self.error(e.to_string());
		} else {
			self.update_status(self.controller.status_msg());
		}
		self.input_search = false;
	}

	#[inline]
	fn update_context(&self, ui: &mut Ui)
	{
		let context = RenderContext {
			colors: self.colors.clone(),
			font_size: self.font_size,
			default_font_measure: self.default_font_measure,
			custom_color: self.controller.reading.custom_color,
			rect: self.view_rect,
			leading_space: 0.0,
			max_page_size: 0.0,
		};
		ui.data_mut(|data| data.insert_temp(render_context_id(), context));
	}

	fn open_file(&mut self, path: PathBuf, frame: &mut eframe::Frame,
		ui: &mut Ui)
	{
		if let Ok(absolute_path) = path.canonicalize() {
			if let Some(filepath) = absolute_path.to_str() {
				if filepath != self.controller.reading.filename {
					let reading_now = self.controller.reading.clone();
					let (history, new_reading) = reading_info(&mut self.configuration.history, filepath);
					let history_entry = if history { Some(new_reading.clone()) } else { None };
					match self.controller.switch_container(new_reading, ui) {
						Ok(msg) => {
							self.configuration.history.push(reading_now);
							update_title(frame, &self.controller.reading.filename);
							self.update_status(msg)
						}
						Err(e) => {
							if let Some(history_entry) = history_entry {
								self.configuration.history.push(history_entry);
							}
							self.error(e.to_string())
						}
					}
				}
			}
		}
	}
}

impl eframe::App for ReaderApp {
	fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
		egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
			egui::menu::bar(ui, |ui| {
				self.setup_toolbar(frame, ui);
			});
		});

		if self.sidebar {
			let width = ctx.available_rect().width();
			let mut max = width / 2.0;
			if max < MIN_SIDEBAR_WIDTH {
				max = MIN_SIDEBAR_WIDTH;
			}
			let mut min = width / 4.0;
			if min < MIN_SIDEBAR_WIDTH {
				min = MIN_SIDEBAR_WIDTH;
			}
			egui::SidePanel::left("sidebar")
				.default_width(min)
				.width_range(min..=max)
				.show(ctx, |ui| {
					self.setup_sidebar(ui, ui.available_width());
				});
		}

		egui::CentralPanel::default().frame(Frame::default().fill(self.colors.background)).show(ctx, |ui| {
			if matches!(self.status, AppStatus::Startup) {
				self.update_status(self.controller.status_msg());
			}
			if self.font_size != self.configuration.gui.font_size {
				self.default_font_measure = measure_char_size(ui, '漢', self.configuration.gui.font_size as f32);
				self.font_size = self.configuration.gui.font_size;
				self.update_context(ui);
				self.controller.redraw(ui);
			}
			let size = ui.available_size();
			let response = ui.allocate_response(size, Sense::click_and_drag());
			let rect = &response.rect;
			if rect.min != self.response_rect.min || rect.max != self.response_rect.max {
				self.response_rect = rect.clone();
				let margin = self.default_font_measure.y / 2.0;
				self.view_rect = Rect::from_min_max(
					Pos2::new(rect.min.x + margin, rect.min.y + margin),
					Pos2::new(rect.max.x - margin, rect.max.y - margin));
				self.update_context(ui);
				self.controller.redraw(ui);
			}
			if !self.sidebar && !self.input_search && !self.dropdown && self.dialog.is_none() && self.popup_menu.is_none() {
				response.request_focus();
			}
			if let Some(mut pos) = self.popup_menu {
				if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
					self.popup_menu = None;
				} else {
					let text_view_popup = ui.make_persistent_id("text_view_popup");
					let popup_response = Area::new(text_view_popup)
						.order(Order::Foreground)
						.fixed_pos(pos)
						.drag_bounds(Rect::EVERYTHING)
						.show(ctx, |ui| {
							Frame::popup(&ctx.style())
								.show(ui, |ui| {
									let texture_id = self.image(ctx, "copy.svg");
									let text = self.i18n.msg("copy-content");
									if Button::image_and_text(texture_id, ICON_SIZE, text).ui(ui).clicked() {
										ui.output_mut(|output| output.copied_text = self.selected_text.clone());
										self.popup_menu = None;
									}
									let texture_id = self.image(ctx, "dict.svg");
									let text = self.i18n.msg("lookup-dictionary");
									if Button::image_and_text(texture_id, ICON_SIZE, text).ui(ui).clicked() {
										self.sidebar = true;
										self.sidebar_list = SidebarList::Dictionary;
										self.popup_menu = None;
									}
									// let texture_id = self.image(ctx, "bookmark.svg");
									// Button::image_and_text(texture_id, ICON_SIZE, "增加书签").ui(ui);
								}).inner
						}).response;
					let repos = if popup_response.rect.max.x > rect.max.x {
						pos.x -= popup_response.rect.max.x - rect.max.x;
						if popup_response.rect.max.y > rect.max.y {
							pos.y -= popup_response.rect.max.y - rect.max.y;
						}
						true
					} else if popup_response.rect.max.y > rect.max.y {
						pos.y -= popup_response.rect.max.y - rect.max.y;
						true
					} else {
						false
					};
					if repos {
						self.popup_menu = Some(pos);
					}
					if response.clicked() || response.clicked_elsewhere() {
						self.popup_menu = None;
					}
				}
			} else if !self.input_search && !self.dropdown && self.dialog.is_none() {
				match self.setup_input(&response, frame, ui) {
					Ok(action) => if action {
						self.update_status(self.controller.status_msg());
					}
					Err(e) => self.error(e.to_string()),
				}
			}

			ui.set_clip_rect(rect.clone());
			self.controller.render.draw(ui);
			response
		});
	}

	fn on_exit(&mut self, _gl: Option<&Context>)
	{
		if self.controller.reading.filename != README_TEXT_FILENAME {
			self.configuration.current = Some(self.controller.reading.filename.clone());
			self.configuration.history.push(self.controller.reading.clone());
		}
		if let Err(e) = self.configuration.save() {
			println!("Failed save configuration: {}", e.to_string());
		}
	}
}

fn app_icon() -> Option<IconData>
{
	let bytes = Asset::get("gui/icon.png").unwrap().data;
	let image = load_image("icon.png", &bytes)?;
	let icon_image = image.resize(48, 48, FilterType::Nearest);
	let image_buffer = icon_image.to_rgba8();
	let pixels = image_buffer.as_flat_samples().as_slice().to_vec();
	Some(IconData {
		rgba: pixels,
		width: APP_ICON_SIZE,
		height: APP_ICON_SIZE,
	})
}

pub(self) fn load_image(name: &str, bytes: &[u8]) -> Option<DynamicImage>
{
	let cursor = Cursor::new(bytes);
	let reader = BufReader::new(cursor);
	let format = match ImageFormat::from_path(name) {
		Ok(f) => f,
		Err(_) => return None,
	};
	match image::load(reader, format) {
		Ok(image) => Some(image),
		Err(_) => None,
	}
}

pub fn start(mut configuration: Configuration, theme_entries: Vec<ThemeEntry>,
	i18n: I18n) -> Result<()>
{
	let reading = if let Some(current) = &configuration.current {
		Some(reading_info(&mut configuration.history, current).1)
	} else {
		None
	};
	let colors = convert_colors(get_theme(&configuration.theme_name, &theme_entries)?);
	let render = Box::new(GuiView::new(&configuration.render_type));
	let images = load_icons()?;
	let dictionary = DictionaryManager::from(&configuration.gui.dictionary_data_path);
	let dictionary_lookup = DictionaryLookupData { words: vec![], current_word: 0 };

	let container_manager = Default::default();
	let (container, book, reading, title) = if let Some(mut reading) = reading {
		let mut container = load_container(&container_manager, &reading)?;
		let book = load_book(&container_manager, &mut container, &mut reading)?;
		let title = reading.filename.clone();
		(container, book, reading, title)
	} else {
		let readme = i18n.msg("readme");
		let container: Box<dyn Container> = Box::new(ReadmeContainer::new(readme.as_ref()));
		let book: Box<dyn Book> = Box::new(ReadmeBook::new(readme.as_ref()));
		(container, book, ReadingInfo::new(README_TEXT_FILENAME), "The e-book reader".to_string())
	};
	let controller = Controller::from_data(reading, container_manager, container, book, render)?;

	let icon_data = app_icon();

	let options = eframe::NativeOptions {
		drag_and_drop_support: true,
		maximized: true,
		default_theme: eframe::Theme::Light,
		icon_data,
		..Default::default()
	};
	if let Err(err) = eframe::run_native(
		&title,
		options,
		Box::new(move |cc| {
			if let Err(e) = setup_fonts(&cc.egui_ctx, &configuration.gui.fonts) {
				println!("Failed setup fonts: {}", e.to_string());
			}
			let app = ReaderApp {
				configuration,
				theme_entries,
				i18n,
				images,
				controller,
				dictionary,
				dictionary_lookup,

				status: AppStatus::Startup,
				current_toc: 0,
				popup_menu: None,
				selected_text: String::new(),
				sidebar: false,
				sidebar_list: SidebarList::Chapter(true),
				dialog: None,
				input_search: false,
				search_pattern: String::new(),
				dropdown: false,
				response_rect: Rect::NOTHING,

				view_rect: Rect::NOTHING,
				font_size: 0,
				default_font_measure: Default::default(),
				colors,
			};
			Box::new(app)
		}),
	) {
		bail!("{}", err.to_string())
	} else {
		Ok(())
	}
}

#[inline]
fn render_context_id() -> Id
{
	Id::new("render_context")
}

#[inline]
pub(self) fn get_render_context(ui: &mut Ui) -> RenderContext
{
	ui.data_mut(|data| data.get_temp(render_context_id()).expect("context not set"))
}

#[inline]
fn update_title(frame: &mut eframe::Frame, title: &str)
{
	if title != README_TEXT_FILENAME {
		frame.set_window_title(title);
	}
}

#[inline]
fn ts() -> u64
{
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.expect("Time went backwards")
		.as_secs()
}
