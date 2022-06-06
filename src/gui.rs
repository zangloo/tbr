mod render;

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{BufReader, Cursor, Read};
use std::ops::Index;
use std::path::PathBuf;
use anyhow::Result;
use cursive::theme::{BaseColor, Color, PaletteColor, Theme};
use eframe::{egui, IconData};
use eframe::egui::{Button, Color32, FontData, FontDefinitions, Frame, Id, ImageButton, PointerButton, Pos2, Rect, Response, Sense, TextureId, Ui, Vec2, Widget};
use eframe::glow::Context;
use egui::{ComboBox, Key, Modifiers, RichText, ScrollArea, TextEdit};
use egui_extras::RetainedImage;
use image::{DynamicImage, ImageFormat};
use image::imageops::FilterType;

use crate::{Asset, Configuration, ReadingInfo, ThemeEntry};
use crate::book::{Book, Colors, Line, TextStyle};
use crate::common::{get_theme, reading_info, txt_lines};
use crate::container::{BookContent, BookName, Container, load_book, load_container};
use crate::controller::Controller;
use crate::gui::render::{create_render, GuiRender, measure_char_size, RenderContext, RenderLine};

const ICON_SIZE: Vec2 = Vec2 { x: 32.0, y: 32.0 };
const APP_ICON_SIZE: u32 = 48;
const MIN_FONT_SIZE: u8 = 20;
const MAX_FONT_SIZE: u8 = 50;

const README_TEXT_FILENAME: &str = "readme";
const README_TEXT: &str = "
The terminal and gui e-book reader
";

struct ReadmeContainer {
	book_names: Vec<BookName>,
}

impl ReadmeContainer {
	#[inline]
	fn new() -> Self
	{
		ReadmeContainer { book_names: vec![BookName::new(README_TEXT_FILENAME.to_string(), 0)] }
	}
}

impl Container for ReadmeContainer {
	#[inline]
	fn inner_book_names(&self) -> &Vec<BookName> {
		&self.book_names
	}

	#[inline]
	fn book_content(&mut self, _inner_index: usize) -> Result<BookContent> {
		Ok(BookContent::Buf(README_TEXT.as_bytes().to_vec()))
	}
}

struct ReadmeBook {
	lines: Vec<Line>,
}

impl ReadmeBook
{
	#[inline]
	fn new() -> Self
	{
		ReadmeBook { lines: txt_lines(README_TEXT) }
	}
}

impl Book for ReadmeBook
{
	#[inline]
	fn lines(&self) -> &Vec<Line> {
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
	fn convert_base(base_color: &BaseColor) -> Color32 {
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
	fn convert(color: &Color) -> Color32 {
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

fn insert_font(fonts: &mut FontDefinitions, name: &str, font_data: FontData) {
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

enum SidebarList {
	Chapter,
	History,
}

enum AppStatus {
	Startup,
	Normal(String),
	Error(String),
}

fn setup_fonts(ctx: &egui::Context, font_paths: &HashSet<PathBuf>) -> Result<()> {
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

struct ReaderApp {
	configuration: Configuration,
	theme_entries: Vec<ThemeEntry>,
	images: HashMap<String, RetainedImage>,
	controller: Controller<Ui, dyn GuiRender>,

	status: AppStatus,
	current_toc: usize,
	popup_menu: Option<Pos2>,
	sidebar: bool,
	sidebar_list: SidebarList,
	dropdown: bool,
	response_rect: Rect,

	view_rect: Rect,
	font_size: u8,
	default_font_measure: Vec2,
	colors: Colors,
	render_lines: Vec<RenderLine>,
}

impl ReaderApp {
	#[inline]
	fn image(&self, ctx: &egui::Context, name: &str) -> TextureId
	{
		let image = self.images.get(name).unwrap();
		image.texture_id(ctx)
	}

	#[inline]
	fn result(&mut self, result: Result<String>)
	{
		match result {
			Ok(msg) => self.update_status(msg),
			Err(e) => self.error(e.to_string()),
		}
	}

	#[inline]
	fn error(&mut self, error: String)
	{
		self.status = AppStatus::Error(error);
	}

	#[inline]
	fn update_status(&mut self, status: String)
	{
		self.current_toc = self.controller.book.toc_index();
		self.status = AppStatus::Normal(status);
	}

	fn setup_popup(&mut self, ui: &mut Ui, response: &Response) {
		let ctx = ui.ctx();
		let text_view_popup = ui.make_persistent_id("text_view_popup");
		if response.clicked_by(PointerButton::Secondary) {
			self.popup_menu = ctx
				.input()
				.pointer
				.hover_pos();
		}
		if self.popup_menu.is_some() {
			egui::popup::show_tooltip_at(ui.ctx(), text_view_popup, self.popup_menu, |ui| {
				let texture_id = self.image(ctx, "copy.svg");
				Button::image_and_text(texture_id, ICON_SIZE, "复制内容").ui(ui);
				let texture_id = self.image(ctx, "dict.svg");
				Button::image_and_text(texture_id, ICON_SIZE, "查阅字典").ui(ui);
				let texture_id = self.image(ctx, "bookmark.svg");
				Button::image_and_text(texture_id, ICON_SIZE, "增加书签").ui(ui);
			});
			if response.clicked() || response.clicked_elsewhere() {
				self.popup_menu = None;
			}
		}
	}

	#[inline]
	fn put_render_context(&self, ui: &mut Ui)
	{
		let context = RenderContext {
			colors: self.colors.clone(),
			font_size: self.font_size,
			default_font_measure: self.default_font_measure,
			rect: self.view_rect,
			leading_space: 0.0,
			max_page_size: 0.0,
			line_base: 0.0,
		};
		prepare_redraw(ui, context);
	}

	fn setup_input(&mut self, rect: &Rect, ui: &mut Ui) -> Result<bool>
	{
		let mut input = ui.input_mut();
		let action = if input.consume_key(Modifiers::NONE, Key::Space)
			|| input.consume_key(Modifiers::NONE, Key::PageDown) {
			drop(input);
			self.put_render_context(ui);
			self.controller.next_page(ui)?;
			true
		} else if input.consume_key(Modifiers::NONE, Key::PageUp) {
			drop(input);
			self.put_render_context(ui);
			self.controller.prev_page(ui)?;
			true
		} else if input.consume_key(Modifiers::NONE, Key::ArrowDown) {
			drop(input);
			self.put_render_context(ui);
			self.controller.step_next(ui);
			true
		} else if input.consume_key(Modifiers::NONE, Key::ArrowUp) {
			drop(input);
			self.put_render_context(ui);
			self.controller.step_prev(ui);
			true
		} else if input.consume_key(Modifiers::NONE, Key::ArrowLeft) {
			drop(input);
			self.put_render_context(ui);
			self.controller.goto_trace(true, ui)?;
			true
		} else if input.consume_key(Modifiers::NONE, Key::ArrowRight) {
			drop(input);
			self.put_render_context(ui);
			self.controller.goto_trace(false, ui)?;
			true
		} else if input.consume_key(Modifiers::NONE, Key::N) {
			drop(input);
			self.put_render_context(ui);
			self.controller.search_again(true, ui)?;
			true
		} else if input.consume_key(Modifiers::SHIFT, Key::N) {
			drop(input);
			self.put_render_context(ui);
			self.controller.search_again(false, ui)?;
			true
		} else if input.consume_key(Modifiers::SHIFT, Key::Tab) {
			drop(input);
			self.put_render_context(ui);
			self.controller.switch_link_prev(ui);
			true
		} else if input.consume_key(Modifiers::NONE, Key::Tab) {
			drop(input);
			self.put_render_context(ui);
			self.controller.switch_link_next(ui);
			true
		} else if input.consume_key(Modifiers::NONE, Key::Enter) {
			drop(input);
			self.put_render_context(ui);
			self.controller.try_goto_link(ui)?;
			true
		} else if input.consume_key(Modifiers::NONE, Key::Home) {
			drop(input);
			if self.controller.reading.line != 0 || self.controller.reading.position != 0 {
				self.put_render_context(ui);
				self.controller.redraw_at(0, 0, ui);
				true
			} else {
				false
			}
		} else if input.consume_key(Modifiers::NONE, Key::End) {
			drop(input);
			self.put_render_context(ui);
			self.controller.goto_end(ui);
			true
		} else if input.consume_key(Modifiers::CTRL, Key::D) {
			drop(input);
			self.put_render_context(ui);
			self.controller.switch_chapter(true, ui)?;
			true
		} else if input.consume_key(Modifiers::CTRL, Key::B) {
			drop(input);
			self.put_render_context(ui);
			self.controller.switch_chapter(false, ui)?;
			true
		} else if input.consume_key(Modifiers::NONE, Key::Escape) {
			self.sidebar = false;
			false
		} else if let Some(pos) = input.pointer.interact_pos() {
			if rect.contains(pos) {
				if input.scroll_delta.y != 0.0 {
					let delta = input.scroll_delta.y;
					drop(input);
					// delta > 0.0 for scroll up
					if delta > 0.0 {
						self.put_render_context(ui);
						self.controller.step_prev(ui);
					} else {
						self.put_render_context(ui);
						self.controller.step_next(ui);
					}
					true
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
					false
				} else {
					false
				}
			} else {
				false
			}
		} else {
			false
		};
		Ok(action)
	}

	fn click_event(&mut self, click_position: Pos2, ui: &mut Ui) -> Result<bool>
	{
		for line in &self.render_lines {
			if let Some(dc) = line.char_at_pos(click_position) {
				if let Some((TextStyle::Link(_), _)) = dc.style {
					if let Some(link_index) = self.controller.book.lines()[dc.line].link_iter(true, |link| {
						if link.range.contains(&dc.offset) {
							(true, Some(link.index))
						} else {
							(false, None)
						}
					}) {
						self.put_render_context(ui);
						self.controller.goto_link(dc.line, link_index, ui)?;
						return Ok(true);
					}
				}
			}
		}
		Ok(false)
	}

	fn setup_toolbar(&mut self, frame: &mut eframe::Frame, ui: &mut Ui) -> bool
	{
		let sidebar = self.sidebar;
		let sidebar_id = self.image(ui.ctx(), if sidebar { "sidebar_off.svg" } else { "sidebar_on.svg" });
		if ImageButton::new(sidebar_id, ICON_SIZE).ui(ui).clicked() {
			self.sidebar = !sidebar;
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
				if let Some(filepath) = path.to_str() {
					self.put_render_context(ui);
					let result = self.controller.switch_container(ReadingInfo::new(filepath), ui);
					self.result(result);
					update_title(frame, &self.controller.reading.filename);
				}
			}
		}

		let theme_dropdown = self.setup_theme_button(ui);

		// setup render dropdown
		let mut selected_text = if self.configuration.render_type == "han" { "漢" } else { "En" };
		let mut selected_render = None;
		let render_dropdown = ComboBox::from_label("")
			.selected_text(selected_text.to_string())
			.show_ui(ui, |ui| {
				if ui.selectable_value(&mut selected_text, "han", "漢").clicked() {
					selected_render = Some("han");
				};
				if ui.selectable_value(&mut selected_text, "xi", "En").clicked() {
					selected_render = Some("xi");
				};
			}).inner.is_some();
		if let Some(render_type) = selected_render {
			if render_type != &self.configuration.render_type {
				self.configuration.render_type = render_type.to_string();
				self.controller.render = create_render(render_type);
				self.put_render_context(ui);
				self.controller.redraw(ui);
			}
		}

		let search_id = self.image(ui.ctx(), "search.svg");
		ui.image(search_id, ICON_SIZE);
		let search_edit = ui.add(TextEdit::singleline(&mut self.configuration.search_pattern));
		let searching = search_edit.has_focus();
		if search_edit.changed() {
			self.put_render_context(ui);
			if let Err(e) = self.controller.search(&self.configuration.search_pattern, ui) {
				self.error(e.to_string());
			} else {
				self.update_status(self.controller.status_msg());
			}
		}

		let status_msg = match &self.status {
			AppStatus::Startup => RichText::from("Starting...").color(Color32::GREEN),
			AppStatus::Normal(status) => RichText::from(status).color(Color32::BLUE),
			AppStatus::Error(error) => RichText::from(error).color(Color32::RED),
		};
		ui.label(status_msg);

		theme_dropdown || render_dropdown || searching
	}

	fn setup_theme_button(&mut self, ui: &mut Ui) -> bool
	{
		let themes_id = self.image(ui.ctx(), "themes.svg");
		let themes_popup = ui.make_persistent_id("themes_popup");
		let theme_button = ImageButton::new(themes_id, ICON_SIZE).ui(ui);
		if theme_button.clicked() {
			ui.memory().toggle_popup(themes_popup);
		}
		egui::popup::popup_below_widget(ui, themes_popup, &theme_button, |ui| {
			ui.set_min_width(200.0);
			for entry in &self.theme_entries {
				if ui.button(entry.0.clone()).clicked() {
					self.colors = convert_colors(&entry.1);
					self.put_render_context(ui);
					self.controller.redraw(ui);
				}
			}
		}).is_some()
	}
}

impl eframe::App for ReaderApp {
	fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
		egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
			egui::menu::bar(ui, |ui| {
				self.dropdown = self.setup_toolbar(frame, ui);
			});
		});

		if self.sidebar {
			let width = ctx.available_rect().width() / 4.0;
			egui::SidePanel::left("sidebar").max_width(width).show(ctx, |ui| {
				egui::menu::bar(ui, |ui| {
					let (chapter_icon, history_icon) = match self.sidebar_list {
						SidebarList::Chapter => ("chapter_on.svg", "history_off.svg"),
						SidebarList::History => ("chapter_off.svg", "history_on.svg"),
					};
					let chapter_id = self.image(ui.ctx(), chapter_icon);
					let chapter_button = ImageButton::new(chapter_id, ICON_SIZE).ui(ui);
					if chapter_button.clicked() {
						self.sidebar_list = SidebarList::Chapter;
					}
					let history_id = self.image(ui.ctx(), history_icon);
					let history_button = ImageButton::new(history_id, ICON_SIZE).ui(ui);
					if history_button.clicked() {
						self.sidebar_list = SidebarList::History;
					}
				});
				ScrollArea::new([false, true]).max_width(width).show(ui, |ui| {
					match self.sidebar_list {
						SidebarList::Chapter => {
							let mut selected_book = None;
							let mut selected_toc = None;
							for (index, bn) in self.controller.container.inner_book_names().iter().enumerate() {
								let bookname = bn.name();
								if bookname == README_TEXT_FILENAME {
									break;
								}
								if index == self.controller.reading.inner_book {
									ui.heading(RichText::from(bookname).color(Color32::LIGHT_RED));
									if let Some(toc) = self.controller.book.toc_list() {
										for entry in toc {
											if self.current_toc == entry.value {
												ui.label(RichText::from(entry.title)
													.background_color(Color32::WHITE).
													color(Color32::BLUE));
											} else if ui.button(RichText::from(entry.title)
												.background_color(Color32::WHITE)).clicked() {
												selected_toc = Some(entry.value);
											}
										}
									}
								} else if ui.button(RichText::from(bookname).heading()).clicked() {
									selected_book = Some(index);
								}
							}
							if let Some(index) = selected_book {
								self.put_render_context(ui);
								let new_reading = ReadingInfo::new(&self.controller.reading.filename)
									.with_inner_book(index);
								let msg = self.controller.switch_book(new_reading, ui);
								self.update_status(msg);
							} else if let Some(index) = selected_toc {
								self.put_render_context(ui);
								if let Some(msg) = self.controller.goto_toc(index, ui) {
									self.update_status(msg);
								}
							}
						}
						SidebarList::History => {
							if self.controller.reading.filename != README_TEXT_FILENAME {
								let mut selected = None;
								for i in (0..self.configuration.history.len()).rev() {
									let reading = &self.configuration.history[i];
									if ui.button(&reading.filename).clicked() {
										selected = Some(i)
									}
								}
								if let Some(selected) = selected {
									let new_one = self.configuration.history.remove(selected);
									let reading_now = self.controller.reading.clone();
									self.configuration.history.push(reading_now);
									let result = self.controller.switch_container(new_one, ui);
									self.result(result);
									update_title(frame, &self.controller.reading.filename);
								}
							}
						}
					}
				})
			});
		}

		egui::CentralPanel::default().frame(Frame::default().fill(self.colors.background)).show(ctx, |ui| {
			if matches!(self.status, AppStatus::Startup) {
				self.update_status(self.controller.status_msg());
			}
			if self.font_size != self.configuration.gui.font_size {
				self.default_font_measure = measure_char_size(ui, '漢', self.configuration.gui.font_size as f32);
				self.font_size = self.configuration.gui.font_size;
				self.put_render_context(ui);
				self.controller.redraw(ui);
			}
			if self.sidebar {}
			let size = ui.available_size();
			let response = ui.allocate_response(size, Sense::click_and_drag());
			// self.setup_popup(ui, &response);
			let rect = &response.rect;
			if rect.min != self.response_rect.min || rect.max != self.response_rect.max {
				self.response_rect = rect.clone();
				let margin = self.default_font_measure.y / 2.0;
				self.view_rect = Rect::from_min_max(
					Pos2::new(rect.min.x + margin, rect.min.y + margin),
					Pos2::new(rect.max.x - margin, rect.max.y - margin));
				let context = RenderContext {
					colors: self.colors.clone(),
					font_size: self.font_size,
					default_font_measure: self.default_font_measure,
					rect: self.view_rect,
					leading_space: 0.0,
					max_page_size: 0.0,
					line_base: 0.0,
				};
				prepare_redraw(ui, context);
				self.controller.redraw(ui);
			}
			if self.popup_menu.is_none() && !self.dropdown {
				response.request_focus();
				if response.clicked() {
					let input = ctx.input();
					if let Some(click_position) = input.pointer.interact_pos() {
						drop(input);
						match self.click_event(click_position, ui) {
							Ok(action) => if action {
								self.update_status(self.controller.status_msg());
							}
							Err(e) => self.error(e.to_string()),
						}
					}
				}
				match self.setup_input(&rect, ui) {
					Ok(action) => if action {
						self.update_status(self.controller.status_msg());
					}
					Err(e) => self.error(e.to_string()),
				}
			}

			if let Some(lines) = take_render_lines(ui) {
				self.render_lines = lines;
			}
			ui.set_clip_rect(rect.clone());
			self.controller.render.draw(&self.render_lines, &mut self.controller.book, ui);
			response
		});
	}

	fn on_exit(&mut self, _gl: &Context) {
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

pub fn start(mut configuration: Configuration, theme_entries: Vec<ThemeEntry>) -> Result<()>
{
	let reading = if let Some(current) = &configuration.current {
		Some(reading_info(&mut configuration.history, current))
	} else {
		None
	};
	let colors = convert_colors(get_theme(&configuration.theme_name, &theme_entries)?);
	let render = create_render(&configuration.render_type);
	let images = load_icons()?;

	let container_manager = Default::default();
	let (container, book, reading, title) = if let Some(mut reading) = reading {
		let mut container = load_container(&container_manager, &reading)?;
		let book = load_book(&container_manager, &mut container, &mut reading)?;
		let title = reading.filename.clone();
		(container, book, reading, title)
	} else {
		let container: Box<dyn Container> = Box::new(ReadmeContainer::new());
		let book: Box<dyn Book> = Box::new(ReadmeBook::new());
		(container, book, ReadingInfo::new(README_TEXT_FILENAME), "The e-book reader".to_string())
	};
	let controller = Controller::from_data(reading, &configuration.search_pattern, container_manager, container, book, render)?;

	let icon_data = app_icon();

	let options = eframe::NativeOptions {
		drag_and_drop_support: true,
		icon_data,
		..Default::default()
	};
	eframe::run_native(
		&title,
		options,
		Box::new(move |cc| {
			if let Err(e) = setup_fonts(&cc.egui_ctx, &configuration.gui.fonts) {
				println!("Failed setup fonts: {}", e.to_string());
			}
			let app = ReaderApp {
				configuration,
				theme_entries,
				images,
				controller,

				status: AppStatus::Startup,
				current_toc: 0,
				popup_menu: None,
				dropdown: false,
				sidebar: false,
				sidebar_list: SidebarList::Chapter,
				response_rect: Rect::NOTHING,

				view_rect: Rect::NOTHING,
				font_size: 0,
				default_font_measure: Default::default(),
				colors,
				render_lines: vec![],
			};
			Box::new(app)
		}),
	);
}

#[inline]
fn render_context_id() -> Id
{
	Id::new("render_context")
}

#[inline]
pub(self) fn prepare_redraw(ui: &mut Ui, render_context: RenderContext)
{
	ui.data().insert_temp(render_context_id(), render_context);
	ui.set_clip_rect(Rect::NOTHING);
}

#[inline]
pub(self) fn get_render_context(ui: &mut Ui) -> RenderContext
{
	ui.data().get_temp(render_context_id()).expect("context not set")
}

#[inline]
fn render_lines_id() -> Id
{
	Id::new("render_lines")
}

#[inline]
pub(self) fn put_render_lines(ui: &mut Ui, render_lines: Vec<RenderLine>)
{
	ui.data().insert_temp(render_lines_id(), render_lines)
}

#[inline]
fn update_title(frame: &mut eframe::Frame, title: &str)
{
	if title != README_TEXT_FILENAME {
		frame.set_window_title(title);
	}
}

#[inline]
fn take_render_lines(ui: &mut Ui) -> Option<Vec<RenderLine>>
{
	let id = render_lines_id();
	let mut data = ui.data();
	if let Some(lines) = data.get_temp(id) {
		data.remove::<Vec<RenderLine>>(id);
		Some(lines)
	} else {
		None
	}
}
