#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod render;

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Read;
use std::ops::Index;
use std::path::PathBuf;
use anyhow::Result;
use cursive::theme::{BaseColor, Color, PaletteColor, Theme};
use eframe::egui;
use eframe::egui::{Button, Color32, FontData, FontDefinitions, Frame, Id, ImageButton, PointerButton, Pos2, Rect, Response, Sense, TextureId, Ui, Vec2, Widget};
use eframe::emath::vec2;
use eframe::glow::Context;
use egui::{Key, Modifiers};
use egui_extras::RetainedImage;

use crate::{Asset, Configuration, ReadingInfo, ThemeEntry};
use crate::book::{Book, Colors, Line, TextStyle};
use crate::common::{get_theme, reading_info, txt_lines};
use crate::container::{BookContent, BookName, Container, load_book, load_container};
use crate::controller::Controller;
use crate::gui::render::{create_render, GuiRender, measure_char_size, RenderContext, RenderLine};

const ICON_SIZE: f32 = 32.0;
const README_TEXT_FILENAME: &str = "readme";
const README_TEXT: &str = "
可在上方工具栏，打开需要阅读的书籍。
在文字上点击右键，可以选择增加书签，复制内容，查阅字典等功能
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

const EMBEDDED_RESOURCE_PREFIX: &str = "embedded://";

fn setup_fonts(ctx: &egui::Context, font_paths: &HashSet<PathBuf>) -> Result<()> {
	let mut fonts = FontDefinitions::default();
	for path in font_paths {
		let str = path.to_str().unwrap();
		let (filename, content) = if str.starts_with(EMBEDDED_RESOURCE_PREFIX) {
			let filename = path.file_name().unwrap().to_str().unwrap();
			let content = Asset::get(&str[EMBEDDED_RESOURCE_PREFIX.len()..])
				.unwrap()
				.data
				.as_ref()
				.to_vec();
			(filename, content)
		} else {
			let mut file = OpenOptions::new().read(true).open(path)?;
			let mut buf = vec![];
			file.read_to_end(&mut buf)?;
			let filename = path.file_name().unwrap().to_str().unwrap();
			(filename, buf)
		};
		insert_font(&mut fonts, filename, FontData::from_owned(content));
	}
	ctx.set_fonts(fonts);
	Ok(())
}

struct ReaderApp {
	configuration: Configuration,
	theme_entries: Vec<ThemeEntry>,
	images: HashMap<String, RetainedImage>,
	controller: Controller<Ui, dyn GuiRender>,

	popup: Option<Pos2>,
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

	fn setup_popup(&mut self, ui: &mut Ui, response: &mut Response) {
		let ctx = ui.ctx();
		let text_view_popup = ui.make_persistent_id("text_view_popup");
		if response.clicked_by(PointerButton::Secondary) {
			self.popup = ctx
				.input()
				.pointer
				.hover_pos();
		}
		if self.popup.is_some() {
			egui::popup::show_tooltip_at(ui.ctx(), text_view_popup, self.popup, |ui| {
				let texture_id = self.image(ctx, "copy.svg");
				Button::image_and_text(texture_id, vec2(ICON_SIZE, ICON_SIZE), "复制内容").ui(ui);
				let texture_id = self.image(ctx, "dict.svg");
				Button::image_and_text(texture_id, vec2(ICON_SIZE, ICON_SIZE), "查阅字典").ui(ui);
				let texture_id = self.image(ctx, "bookmark.svg");
				Button::image_and_text(texture_id, vec2(ICON_SIZE, ICON_SIZE), "增加书签").ui(ui);
			});
		}
		if response.clicked() || response.clicked_elsewhere() {
			self.popup = None;
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
		put_render_context(ui, context);
	}

	fn setup_keys(&mut self, ui: &mut Ui) -> Result<()>
	{
		let mut input = ui.input_mut();
		if input.consume_key(Modifiers::NONE, Key::Space)
			|| input.consume_key(Modifiers::NONE, Key::PageDown) {
			drop(input);
			self.put_render_context(ui);
			self.controller.next_page(ui)?;
		} else if input.consume_key(Modifiers::NONE, Key::PageUp) {
			drop(input);
			self.put_render_context(ui);
			self.controller.prev_page(ui)?;
		} else if input.consume_key(Modifiers::NONE, Key::ArrowDown) {
			drop(input);
			self.put_render_context(ui);
			self.controller.step_next(ui);
		} else if input.consume_key(Modifiers::NONE, Key::ArrowUp) {
			drop(input);
			self.put_render_context(ui);
			self.controller.step_prev(ui);
		} else if input.consume_key(Modifiers::NONE, Key::ArrowLeft) {
			drop(input);
			self.put_render_context(ui);
			self.controller.goto_trace(true, ui)?;
		} else if input.consume_key(Modifiers::NONE, Key::ArrowRight) {
			drop(input);
			self.put_render_context(ui);
			self.controller.goto_trace(false, ui)?;
		} else if input.consume_key(Modifiers::NONE, Key::N) {
			drop(input);
			self.put_render_context(ui);
			self.controller.search_again(true, ui)?;
		} else if input.consume_key(Modifiers::SHIFT, Key::N) {
			drop(input);
			self.put_render_context(ui);
			self.controller.search_again(false, ui)?;
		} else if input.consume_key(Modifiers::SHIFT, Key::Tab) {
			drop(input);
			self.put_render_context(ui);
			self.controller.switch_link_prev(ui);
		} else if input.consume_key(Modifiers::NONE, Key::Tab) {
			drop(input);
			self.put_render_context(ui);
			self.controller.switch_link_next(ui);
		} else if input.consume_key(Modifiers::NONE, Key::Enter) {
			drop(input);
			self.put_render_context(ui);
			self.controller.try_goto_link(ui)?;
		} else if input.consume_key(Modifiers::NONE, Key::Home) {
			drop(input);
			self.put_render_context(ui);
			self.controller.redraw_at(0, 0, ui);
		} else if input.consume_key(Modifiers::NONE, Key::End) {
			drop(input);
			self.put_render_context(ui);
			self.controller.goto_end(ui);
		} else if input.consume_key(Modifiers::CTRL, Key::D) {
			drop(input);
			self.put_render_context(ui);
			self.controller.switch_chapter(true, ui)?;
		} else if input.consume_key(Modifiers::CTRL, Key::B) {
			drop(input);
			self.put_render_context(ui);
			self.controller.switch_chapter(false, ui)?;
		}
		Ok(())
	}

	fn click_event(&mut self, click_position: Pos2, ui: &mut Ui) -> Result<()>
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
						return Ok(());
					}
				}
			}
		}
		Ok(())
	}
}

impl eframe::App for ReaderApp {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
			egui::menu::bar(ui, |ui| {
				let texture_id = self.image(ctx, "file_open.svg");
				if ImageButton::new(texture_id, vec2(32.0, 32.0)).ui(ui).clicked() {
					if let Some(path) = rfd::FileDialog::new().pick_file() {
						println!("open: {}", path.display().to_string());
					}
				}
			});
		});
		egui::CentralPanel::default().frame(Frame::default().fill(self.colors.background)).show(ctx, |ui| {
			if self.font_size != self.configuration.gui.font_size {
				self.default_font_measure = measure_char_size(ui, '漢', self.configuration.gui.font_size as f32);
				self.font_size = self.configuration.gui.font_size;
			}
			let size = ui.available_size();
			let mut response = ui.allocate_response(size, Sense::click_and_drag());
			self.setup_popup(ui, &mut response);
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
				put_render_context(ui, context);
				ui.set_clip_rect(Rect::NOTHING);
				self.controller.redraw(ui);
			}
			ui.set_clip_rect(rect.clone());

			if self.popup.is_none() {
				response.request_focus();
				if response.clicked() {
					let input = ctx.input();
					if let Some(click_position) = input.pointer.interact_pos() {
						drop(input);
						if let Err(e) = self.click_event(click_position, ui) {
							println!("{}", e.to_string());
						}
					}
				}
				if let Err(e) = self.setup_keys(ui) {
					println!("{}", e.to_string());
				}
			}

			if let Some(lines) = take_render_lines(ui) {
				self.render_lines = lines;
			}
			self.controller.render.draw(&self.render_lines, ui);
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
	let (container, book, reading) = if let Some(mut reading) = reading {
		let mut container = load_container(&container_manager, &reading)?;
		let book = load_book(&container_manager, &mut container, &mut reading)?;
		(container, book, reading)
	} else {
		let container: Box<dyn Container> = Box::new(ReadmeContainer::new());
		let book: Box<dyn Book> = Box::new(ReadmeBook::new());
		(container, book, ReadingInfo::new(README_TEXT_FILENAME))
	};
	let controller = Controller::from_data(reading, &configuration.search_pattern, container_manager, container, book, render)?;

	let options = eframe::NativeOptions {
		drag_and_drop_support: true,
		..Default::default()
	};
	eframe::run_native(
		"The ebook reader",
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

				popup: None,
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
pub(self) fn put_render_context(ui: &mut Ui, render_context: RenderContext)
{
	ui.data().insert_temp(render_context_id(), render_context)
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
