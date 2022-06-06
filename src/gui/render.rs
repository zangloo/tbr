use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io::{BufReader, Cursor};
use std::ops::Range;
use eframe::egui::{Align2, FontFamily, FontId, Rect, Rounding, Stroke, Ui};
use eframe::emath::{Pos2, Vec2};
use eframe::epaint::Color32;
use egui::{ColorImage, Mesh, Shape, TextureHandle};
use image::ImageFormat;
use image::imageops::FilterType;

use crate::book::{Book, Colors, Line, TextStyle};
use crate::common::Position;
use crate::controller::{HighlightInfo, Render};
use crate::gui::render::han::GuiHanRender;
use crate::gui::render::xi::GuiXiRender;
use crate::gui::{put_render_lines, get_render_context};

mod han;
mod xi;

#[derive(Clone)]
pub(super) struct RenderChar {
	pub char: char,
	pub font_size: f32,
	pub color: Color32,
	pub background: Option<Color32>,
	pub style: Option<(TextStyle, Range<usize>)>,

	pub line: usize,
	pub offset: usize,
	pub rect: Rect,
	pub draw_offset: Pos2,
}

#[derive(Clone)]
pub(super) struct RenderLine {
	chars: Vec<RenderChar>,
	draw_size: f32,
	line_space: f32,
}

impl RenderLine
{
	fn new(draw_size: f32, line_space: f32) -> Self
	{
		RenderLine { chars: vec![], draw_size, line_space }
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
}

#[derive(Clone)]
pub(super) struct RenderContext
{
	pub colors: Colors,
	// font size in configuration
	pub font_size: u8,
	// default single char size
	pub default_font_measure: Vec2,

	// draw rect
	pub rect: Rect,
	pub leading_space: f32,
	// for calculate chars in single line
	pub max_page_size: f32,
	// current line base
	pub line_base: f32,
}

pub(super) struct ImageDrawingData {
	rect: Rect,
	draw_rect: Rect,
	texture: TextureHandle,
}

pub(super) trait GuiRender: Render<Ui> {
	fn reset_render_context(&self, render_context: &mut RenderContext);
	fn create_render_line(&self, default_font_measure: &Vec2) -> RenderLine;
	fn update_base_line_for_delta(&self, context: &mut RenderContext, delta: f32);
	fn wrap_line(&self, text: &Line, line: usize, start_offset: usize, end_offset: usize, highlight: &Option<HighlightInfo>, ui: &mut Ui, context: &mut RenderContext) -> Vec<RenderLine>;
	fn draw_style(&self, draw_text: &RenderLine, ui: &mut Ui);
	fn image_cache(&mut self) -> &mut HashMap<String, ImageDrawingData>;

	#[inline]
	fn prepare_wrap(&self, text: &Line, start_offset: usize, end_offset: usize, context: &mut RenderContext) -> (usize, Option<Vec<RenderLine>>)
	{
		let end_offset = if end_offset > text.len() {
			text.len()
		} else {
			end_offset
		};
		if start_offset == end_offset {
			let draw_line = self.create_render_line(&context.default_font_measure);
			let line_delta = draw_line.draw_size + draw_line.line_space;
			self.update_base_line_for_delta(context, line_delta);
			(end_offset, Some(vec![draw_line]))
		} else {
			(end_offset, None)
		}
	}

	fn gui_redraw(&self, lines: &Vec<Line>, reading_line: usize, reading_offset: usize,
		highlight: &Option<HighlightInfo>, ui: &mut Ui) -> Option<Position>
	{
		// load context and init for rendering
		let mut context = get_render_context(ui);
		self.reset_render_context(&mut context);
		let mut render_lines = vec![];

		let mut drawn_size = 0.0;
		let mut offset = reading_offset;
		for index in reading_line..lines.len() {
			let line = &lines[index];
			if let Some((target, offset)) = line.with_image() {
				let next = if reading_line == index {
					let mut draw_line = self.create_render_line(&context.default_font_measure);
					draw_line.chars.push(RenderChar {
						char: 'I',
						font_size: 1.0,
						color: context.colors.color,
						background: None,
						style: Some((TextStyle::Image(target.to_string()), offset..offset + 1)),

						line: index,
						offset,
						rect: context.rect.clone(),
						draw_offset: Pos2::ZERO,
					});
					render_lines.push(draw_line);
					let next_line = index + 1;
					if next_line < lines.len() {
						Some(Position::new(next_line, 0))
					} else {
						None
					}
				} else {
					Some(Position::new(index, 0))
				};
				put_render_lines(ui, render_lines);
				return next;
			}
			let wrapped_lines = self.wrap_line(&line, index, offset, line.len(), highlight, ui, &mut context);
			offset = 0;
			for wrapped_line in wrapped_lines {
				drawn_size += wrapped_line.draw_size;
				if drawn_size > context.max_page_size {
					let next = if let Some(char) = wrapped_line.chars.first() {
						Some(Position::new(index, char.offset))
					} else {
						Some(Position::new(index, 0))
					};
					put_render_lines(ui, render_lines);
					return next;
				}
				drawn_size += wrapped_line.line_space;
				render_lines.push(wrapped_line);
			}
		}
		put_render_lines(ui, render_lines);
		None
	}

	fn draw(&mut self, render_lines: &Vec<RenderLine>, book: &mut Box<dyn Book>, ui: &mut Ui)
	{
		for render_line in render_lines {
			// for now, image will always take a line and is the first char
			if let Some(RenderChar { style: Some((TextStyle::Image(href), _)), rect, .. }) = render_line.chars.first() {
				if let Some(bytes) = book.image(href) {
					self.draw_image(href, bytes, &rect, ui);
				}
			}
			for dc in &render_line.chars {
				if let Some(bg) = dc.background {
					ui.painter().rect(dc.rect.clone(), Rounding::none(), bg, Stroke::default());
				}
				let draw_position = Pos2::new(dc.rect.min.x + dc.draw_offset.x, dc.rect.min.y + dc.draw_offset.y);
				paint_char(ui, dc.char, dc.font_size, &draw_position, Align2::LEFT_TOP, dc.color);
			}
			self.draw_style(render_line, ui);
		}
	}

	fn gui_prev_page(&mut self, lines: &Vec<Line>, reading_line: usize, offset: usize, ui: &mut Ui) -> Position
	{
		// load context and init for rendering
		let mut context = get_render_context(ui);
		self.reset_render_context(&mut context);

		let (reading_line, mut offset) = if offset == 0 {
			(reading_line - 1, usize::MAX)
		} else {
			(reading_line, offset)
		};

		let mut drawn_size = 0.0;
		for index in (0..=reading_line).rev() {
			let line = &lines[index];
			if line.with_image().is_some() {
				return if reading_line == index {
					Position::new(index, 0)
				} else {
					Position::new(index + 1, 0)
				};
			}
			let wrapped_lines = self.wrap_line(&line, index, 0, offset, &None, ui, &mut context);
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

	fn gui_next_line(&mut self, lines: &Vec<Line>, line: usize, offset: usize, ui: &mut Ui) -> Position
	{
		// load context and init for rendering
		let mut context = get_render_context(ui);
		self.reset_render_context(&mut context);

		let wrapped_lines = self.wrap_line(&lines[line], line, offset, usize::MAX, &None, ui, &mut context);
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

	fn gui_prev_line(&mut self, lines: &Vec<Line>, line: usize, offset: usize, ui: &mut Ui) -> Position
	{
		// load context and init for rendering
		let mut context = get_render_context(ui);
		self.reset_render_context(&mut context);

		let (line, offset) = if offset == 0 {
			if line == 0 {
				return Position::new(0, 0);
			}
			(line - 1, usize::MAX)
		} else {
			(line, offset)
		};
		let text = &lines[line];
		let wrapped_lines = self.wrap_line(text, line, 0, offset, &None, ui, &mut context);
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

	fn gui_setup_highlight(&mut self, lines: &Vec<Line>, line: usize, start: usize, ui: &mut Ui) -> Position
	{
		// load context and init for rendering
		let mut context = get_render_context(ui);
		self.reset_render_context(&mut context);

		let text = &lines[line];
		let wrapped_lines = self.wrap_line(text, line, 0, start + 1, &None, ui, &mut context);
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

	fn draw_image(&mut self, name: &str, bytes: &Vec<u8>, rect: &Rect, ui: &mut Ui)
	{
		fn load_image(rect: &Rect, bytes: &Vec<u8>, name: &str, ui: &mut Ui) -> Option<ImageDrawingData>
		{
			let cursor = Cursor::new(bytes);
			let reader = BufReader::new(cursor);
			let format = match ImageFormat::from_path(name) {
				Ok(f) => f,
				Err(_) => return None,
			};
			let image = match image::load(reader, format) {
				Ok(i) => i,
				Err(_) => return None,
			};

			let width = rect.width() as u32;
			let height = rect.height() as u32;
			let image = image.resize(width, height, FilterType::Nearest);
			let draw_width = image.width();
			let draw_height = image.height();
			let image_buffer = image.to_rgba8();
			let pixels = image_buffer.as_flat_samples();
			let color_image = ColorImage::from_rgba_unmultiplied(
				[draw_width as usize, draw_height as usize],
				pixels.as_slice(),
			);
			let x_margin = ((width - draw_width) / 2) as f32;
			let y_margin = ((height - draw_height) / 2) as f32;
			let draw_rect = Rect::from_min_size(
				Pos2::new(rect.min.x + x_margin, rect.min.y + y_margin),
				Vec2::new(draw_width as f32, draw_height as f32),
			);
			let texture = ui.ctx().load_texture(name, color_image);
			Some(ImageDrawingData {
				rect: *rect,
				draw_rect,
				texture,
			})
		}
		let cache = self.image_cache();
		let mut image_data = match cache.entry(name.to_string()) {
			Entry::Occupied(o) => o.into_mut(),
			Entry::Vacant(v) => if let Some(data) = load_image(rect, bytes, name, ui) {
				v.insert(data)
			} else {
				return;
			}
		};

		if *rect != image_data.rect {
			if let Some(new_image_data) = load_image(rect, bytes, name, ui) {
				cache.insert(name.to_string(), new_image_data);
				image_data = cache.get_mut(name).unwrap();
			} else {
				return;
			}
		}
		let mut mesh = Mesh::with_texture(image_data.texture.id());
		let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
		mesh.add_rect_with_uv(image_data.draw_rect, uv, Color32::WHITE);
		ui.painter().add(Shape::mesh(mesh));
	}
}

#[inline]
pub(self) fn update_for_highlight(line: usize, offset: usize, background: Option<Color32>, colors: &Colors, highlight: &Option<HighlightInfo>) -> Option<Color32>
{
	if let Some(highlight) = highlight {
		if highlight.line == line && highlight.start <= offset && highlight.end > offset {
			Some(colors.highlight_background)
		} else {
			background
		}
	} else {
		background
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
	return font_size as f32 * scale;
}

pub(super) fn create_render(render_type: &str) -> Box<dyn GuiRender>
{
	if render_type == "han" {
		Box::new(GuiHanRender::new())
	} else {
		Box::new(GuiXiRender::new())
	}
}

#[inline]
pub(self) fn stroke_width_for_space(space: f32) -> f32 {
	space / 4.0
}
