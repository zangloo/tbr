use std::ops::Range;
use eframe::egui::{Align2, FontFamily, FontId, Rect, Rounding, Stroke, Ui};
use eframe::emath::{Pos2, Vec2};
use eframe::epaint::Color32;

use crate::book::{Colors, Line, TextStyle};
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

pub(super) trait GuiRender: Render<Ui> {
	fn reset_render_context(&self, render_context: &mut RenderContext);
	fn create_render_line(&self, default_font_measure: &Vec2) -> RenderLine;
	fn wrap_line(&self, text: &Line, line: usize, start_offset: usize, end_offset: usize, highlight: &Option<HighlightInfo>, ui: &mut Ui, context: &mut RenderContext) -> Vec<RenderLine>;
	fn draw_style(&self, draw_text: &RenderLine, ui: &mut Ui);

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
				return if reading_line == index {
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
			}
			let wrapped_lines = self.wrap_line(&line, index, offset, line.len(), highlight, ui, &mut context);
			offset = 0;
			for wrapped_line in wrapped_lines {
				drawn_size += wrapped_line.draw_size + wrapped_line.line_space;
				if drawn_size > context.max_page_size {
					let next = if let Some(char) = wrapped_line.chars.first() {
						Some(Position::new(index, char.offset))
					} else {
						Some(Position::new(index, 0))
					};
					render_lines.push(wrapped_line);
					put_render_lines(ui, render_lines);
					return next;
				}
				render_lines.push(wrapped_line);
			}
		}
		put_render_lines(ui, render_lines);
		None
	}

	fn draw(&self, render_lines: &Vec<RenderLine>, ui: &mut Ui)
	{
		for render_line in render_lines {
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
				drawn_size += wrapped_line.draw_size + wrapped_line.line_space;
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
pub(super) fn paint_char(ui: &Ui, char: char, font_size: f32, position: &Pos2, align: Align2, color: Color32) -> Rect {
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
