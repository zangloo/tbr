use std::collections::hash_map::Entry;
use std::collections::HashMap;
use eframe::egui::{Align2, FontFamily, FontId, Rect, Rounding, Stroke, Ui};
use eframe::emath::{Pos2, Vec2};
use eframe::epaint::Color32;
use egui::{ColorImage, Mesh, Shape, TextureFilter, TextureHandle, TextureOptions};
use image::imageops::FilterType;

use crate::book::{Book, CharStyle, Colors, Line};
use crate::common::Position;
use crate::controller::{HighlightInfo, HighlightMode};
use crate::gui::render::han::GuiHanRender;
use crate::gui::render::xi::GuiXiRender;
use crate::gui::load_image;

mod han;
mod xi;

#[derive(Clone)]
pub(super) enum TextDecoration {
	// rect, stroke width, is first, is last, color
	Border {
		rect: Rect,
		stroke_width: f32,
		start: bool,
		end: bool,
		color: Color32,
	},
	// start(x,y), length,stroke width, is first, color
	UnderLine {
		pos2: Pos2,
		length: f32,
		stroke_width: f32,
		color: Color32,
	},
}

#[derive(Clone)]
pub(super) struct CharCell {
	pub char: char,
	pub font_size: f32,
	pub color: Color32,
	pub background: Option<Color32>,
	pub draw_offset: Vec2,
	pub char_size: Vec2,
}

#[derive(Clone)]
pub(super) enum RenderCell {
	Char(CharCell),
	Image(String),
}

#[derive(Clone)]
pub(super) struct RenderChar {
	pub cell: RenderCell,
	pub offset: usize,
	pub rect: Rect,
}

#[derive(Clone)]
pub(super) struct RenderLine {
	pub(super) chars: Vec<RenderChar>,
	pub(super) line: usize,
	draw_size: f32,
	line_space: f32,
	decorations: Vec<TextDecoration>,
}

impl RenderLine
{
	fn new(line: usize, draw_size: f32, line_space: f32) -> Self
	{
		RenderLine {
			chars: vec![],
			line,
			draw_size,
			line_space,
			decorations: vec![],
		}
	}

	pub(super) fn char_at_pos(&self, pos: Pos2) -> Option<&RenderChar>
	{
		for dc in &self.chars {
			if dc.rect.contains(pos) {
				return Some(dc);
			}
		}
		None
	}

	pub fn add_decoration(&mut self, decoration: TextDecoration)
	{
		self.decorations.push(decoration)
	}
}

#[derive(Clone)]
pub(super) struct RenderContext
{
	pub colors: Colors,
	// font size in configuration
	pub font_size: u8,
	// default single char size
	pub default_font_measure: Vec2,

	// use book custom color
	pub custom_color: bool,

	pub view_port: Rect,
	pub render_rect: Rect,
	pub leading_chars: usize,
	pub leading_space: f32,
	// for calculate chars in single line
	pub max_page_size: f32,
}

pub(super) struct ImageDrawingData {
	view_rect: Rect,
	image_size: Pos2,
	texture: TextureHandle,
}

pub(super) enum PointerPosition {
	Head,
	Exact(usize),
	Tail,
}

pub(super) trait GuiRender {
	fn reset_baseline(&mut self, render_context: &RenderContext);
	fn reset_render_context(&mut self, render_context: &mut RenderContext);
	fn create_render_line(&self, line: usize, render_context: &RenderContext)
		-> RenderLine;
	fn update_baseline_for_delta(&mut self, delta: f32);
	fn wrap_line(&mut self, book: &dyn Book, text: &Line, line: usize,
		start_offset: usize, end_offset: usize, highlight: &Option<HighlightInfo>,
		ui: &mut Ui, context: &RenderContext) -> Vec<RenderLine>;
	fn draw_decoration(&self, decoration: &TextDecoration, ui: &mut Ui);
	fn image_cache(&mut self) -> &mut HashMap<String, ImageDrawingData>;
	// return (line, offset) position
	fn pointer_pos(&self, pointer_pos: &Pos2, render_lines: &Vec<RenderLine>,
		rect: &Rect) -> (PointerPosition, PointerPosition);
	fn measure_lines_size(&mut self, book: &dyn Book, ui: &mut Ui,
		context: &mut RenderContext) -> Rect;

	#[inline]
	fn prepare_wrap(&mut self, text: &Line, line: usize, start_offset: usize,
		end_offset: usize, context: &RenderContext)
		-> (usize, Option<Vec<RenderLine>>)
	{
		let end_offset = if end_offset > text.len() {
			text.len()
		} else {
			end_offset
		};
		if start_offset == end_offset {
			let draw_line = self.create_render_line(line, context);
			let line_delta = draw_line.draw_size + draw_line.line_space;
			self.update_baseline_for_delta(line_delta);
			(end_offset, Some(vec![draw_line]))
		} else {
			(end_offset, None)
		}
	}

	fn gui_redraw(&mut self, book: &dyn Book, lines: &Vec<Line>,
		reading_line: usize, reading_offset: usize,
		highlight: &Option<HighlightInfo>, ui: &mut Ui,
		render_lines: &mut Vec<RenderLine>, context: &RenderContext)
		-> Option<Position>
	{
		render_lines.clear();
		self.reset_baseline(context);
		ui.set_clip_rect(Rect::NOTHING);

		let mut drawn_size = 0.0;
		let mut offset = reading_offset;
		for index in reading_line..lines.len() {
			let line = &lines[index];
			let wrapped_lines = self.wrap_line(book, &line, index, offset, line.len(), highlight, ui, context);
			offset = 0;
			for wrapped_line in wrapped_lines {
				drawn_size += wrapped_line.draw_size;
				if drawn_size > context.max_page_size {
					let next = if let Some(char) = wrapped_line.chars.first() {
						Some(Position::new(index, char.offset))
					} else {
						Some(Position::new(index, 0))
					};
					return next;
				}
				drawn_size += wrapped_line.line_space;
				render_lines.push(wrapped_line);
			}
		}
		None
	}

	fn draw(&mut self, render_lines: &Vec<RenderLine>, ui: &mut Ui)
	{
		for render_line in render_lines {
			for dc in &render_line.chars {
				match &dc.cell {
					RenderCell::Image(name) => {
						self.draw_image(name, &dc.rect, ui);
					}
					RenderCell::Char(cell) => {
						if let Some(bg) = cell.background {
							let min = dc.rect.min + cell.draw_offset;
							let max = min + cell.char_size;
							let rect = Rect::from_min_max(min, max);
							ui.painter().rect(rect, Rounding::none(), bg, Stroke::default());
						}
						let draw_position = Pos2::new(dc.rect.min.x + cell.draw_offset.x, dc.rect.min.y + cell.draw_offset.y);
						paint_char(ui, cell.char, cell.font_size, &draw_position, Align2::LEFT_TOP, cell.color);
					}
				}
			}
			for decoration in &render_line.decorations {
				self.draw_decoration(decoration, ui);
			}
		}
	}

	fn gui_prev_page(&mut self, book: &dyn Book, lines: &Vec<Line>,
		reading_line: usize, offset: usize, ui: &mut Ui, context: &RenderContext) -> Position
	{
		ui.set_clip_rect(Rect::NOTHING);

		let (reading_line, mut offset) = if offset == 0 {
			(reading_line - 1, usize::MAX)
		} else {
			(reading_line, offset)
		};

		let mut drawn_size = 0.0;
		for index in (0..=reading_line).rev() {
			let line = &lines[index];
			let wrapped_lines = self.wrap_line(book, &line, index, 0, offset, &None, ui, context);
			offset = usize::MAX;
			for wrapped_line in wrapped_lines.iter().rev() {
				drawn_size += wrapped_line.draw_size;
				if drawn_size > context.max_page_size {
					return if let Some(char) = wrapped_line.chars.last() {
						let offset = char.offset + 1;
						if offset >= line.len() {
							Position::new(index + 1, 0)
						} else {
							Position::new(index, offset)
						}
					} else {
						Position::new(index + 1, 0)
					};
				}
				drawn_size += wrapped_line.line_space;
			}
		}
		Position::new(0, 0)
	}

	fn gui_next_line(&mut self, book: &dyn Book, lines: &Vec<Line>,
		line: usize, offset: usize, ui: &mut Ui, context: &RenderContext)
		-> Position
	{
		ui.set_clip_rect(Rect::NOTHING);

		let wrapped_lines = self.wrap_line(book, &lines[line], line, offset, usize::MAX, &None, ui, context);
		if wrapped_lines.len() > 1 {
			if let Some(next_line_char) = wrapped_lines[1].chars.first() {
				Position::new(line, next_line_char.offset)
			} else {
				Position::new(line + 1, 0)
			}
		} else {
			Position::new(line + 1, 0)
		}
	}

	fn gui_prev_line(&mut self, book: &dyn Book, lines: &Vec<Line>,
		line: usize, offset: usize, ui: &mut Ui, context: &RenderContext)
		-> Position
	{
		ui.set_clip_rect(Rect::NOTHING);

		let (line, offset) = if offset == 0 {
			if line == 0 {
				return Position::new(0, 0);
			}
			(line - 1, usize::MAX)
		} else {
			(line, offset)
		};
		let text = &lines[line];
		let wrapped_lines = self.wrap_line(book, text, line, 0, offset, &None, ui, context);
		if let Some(last_line) = wrapped_lines.last() {
			if let Some(first_char) = last_line.chars.first() {
				Position::new(line, first_char.offset)
			} else {
				Position::new(line, 0)
			}
		} else {
			Position::new(line, 0)
		}
	}

	fn gui_setup_highlight(&mut self, book: &dyn Book, lines: &Vec<Line>,
		line: usize, start: usize, ui: &mut Ui, context: &RenderContext)
		-> Position
	{
		ui.set_clip_rect(Rect::NOTHING);

		let text = &lines[line];
		let wrapped_lines = self.wrap_line(book, text, line, 0, start + 1, &None, ui, context);
		if let Some(last_line) = wrapped_lines.last() {
			if let Some(first_char) = last_line.chars.first() {
				Position::new(line, first_char.offset)
			} else {
				Position::new(line, 0)
			}
		} else {
			Position::new(line, 0)
		}
	}

	fn with_image(&mut self, char_style: &CharStyle, full_screen_if_image: bool,
		book: &dyn Book, view_rect: &Rect, ui: &mut Ui) -> Option<(String, Pos2)>
	{
		if let Some(href) = &char_style.image {
			if let Some((path, bytes)) = book.image(href) {
				let cache = self.image_cache();
				let mut image_data = match cache.entry(path.clone()) {
					Entry::Occupied(o) => o.into_mut(),
					Entry::Vacant(v) => if let Some(data) = load_image_and_resize(view_rect, full_screen_if_image, bytes, &path, ui) {
						v.insert(data)
					} else {
						return None;
					}
				};

				if *view_rect != image_data.view_rect {
					if let Some(new_image_data) = load_image_and_resize(view_rect, full_screen_if_image, bytes, &path, ui) {
						cache.insert(path.clone(), new_image_data);
						image_data = cache.get_mut(&path).unwrap();
					} else {
						return None;
					}
				}

				Some((path, image_data.image_size))
			} else {
				None
			}
		} else {
			None
		}
	}

	fn draw_image(&mut self, name: &str, rect: &Rect, ui: &mut Ui)
	{
		if let Some(image_data) = self.image_cache().get(name) {
			let mut mesh = Mesh::with_texture(image_data.texture.id());
			let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
			mesh.add_rect_with_uv(*rect, uv, Color32::WHITE);
			ui.painter().add(Shape::mesh(mesh));
		}
	}
}

fn load_image_and_resize(view_rect: &Rect, full_screen: bool, bytes: &[u8], name: &str, ui: &mut Ui) -> Option<ImageDrawingData>
{
	let image = load_image(name, bytes)?;
	let width = view_rect.width() as u32;
	let height = view_rect.height() as u32;
	let image = if full_screen || image.width() > width || image.height() > height {
		image.resize(width, height, FilterType::Nearest)
	} else {
		image
	};
	let draw_width = image.width();
	let draw_height = image.height();
	let image_buffer = image.to_rgba8();
	let pixels = image_buffer.as_flat_samples();
	let color_image = ColorImage::from_rgba_unmultiplied(
		[draw_width as usize, draw_height as usize],
		pixels.as_slice(),
	);
	let texture = ui.ctx().load_texture(name, color_image, TextureOptions {
		magnification: TextureFilter::Linear,
		minification: TextureFilter::Linear,
	});
	Some(ImageDrawingData {
		view_rect: *view_rect,
		image_size: Pos2::new(draw_width as f32, draw_height as f32),
		texture,
	})
}

#[inline]
pub(self) fn update_for_highlight(render_line: usize, offset: usize, background: Option<Color32>, colors: &Colors, highlight: &Option<HighlightInfo>) -> Option<Color32>
{
	match highlight {
		Some(HighlightInfo { mode: HighlightMode::Search, line, start, end })
		| Some(HighlightInfo { mode: HighlightMode::Link(_), line, start, end })
		if *line == render_line && *start <= offset && *end > offset
		=> Some(colors.highlight_background),

		Some(HighlightInfo { mode: HighlightMode::Selection(line2), line, start, end })
		if (*line == render_line && *line2 == render_line && *start <= offset && *end > offset)
			|| (*line == render_line && *line2 > render_line && *start <= offset)
			|| (*line < render_line && *line2 == render_line && *end > offset)
			|| (*line < render_line && *line2 > render_line)
		=> {
			Some(colors.highlight_background)
		}

		_ => background,
	}
}

pub(super) fn measure_char_size(ui: &mut Ui, char: char, font_size: f32) -> Vec2 {
	let old_clip_rect = ui.clip_rect();
	ui.set_clip_rect(Rect::NOTHING);
	let rect = paint_char(ui, char, font_size, &Pos2::ZERO, Align2::LEFT_TOP, Color32::BLACK);
	ui.set_clip_rect(old_clip_rect);
	rect.size()
}

#[inline]
pub(super) fn paint_char(ui: &Ui, char: char, font_size: f32, position: &Pos2, align: Align2, color: Color32) -> Rect
{
	let rect = ui.painter().text(
		*position,
		align,
		char,
		FontId::new(font_size, FontFamily::Proportional),
		color);
	rect
}

#[inline]
pub(super) fn scale_font_size(font_size: u8, scale: f32) -> f32
{
	let scaled_size = font_size as f32 * scale;
	if scaled_size < 9.0 {
		9.0
	} else {
		scaled_size
	}
}

pub(super) fn create_render(render_type: &str) -> Box<dyn GuiRender>
{
	if render_type == "han" {
		Box::new(GuiHanRender::new())
	} else {
		Box::new(GuiXiRender::new())
	}
}
