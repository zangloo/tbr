use std::collections::HashMap;
use eframe::egui::{Align2, Color32, Pos2, Rect, Ui, Vec2};

use crate::book::{Line, StylePosition, TextStyle};
use crate::common::{HAN_RENDER_CHARS_PAIRS, with_leading};
use crate::controller::{HighlightInfo, Render};
use crate::gui::render::{RenderChar, RenderContext, RenderLine, GuiRender, paint_char, scale_font_size};
use crate::Position;

pub(super) struct GuiHanRender {
	chars_map: HashMap<char, char>,
}

impl GuiHanRender
{
	pub fn new() -> Self
	{
		GuiHanRender
		{
			chars_map: HAN_RENDER_CHARS_PAIRS.into_iter().collect(),
		}
	}

	fn map_char(&self, ch: char) -> char
	{
		*self.chars_map.get(&ch).unwrap_or(&ch)
	}
}

impl Render<Ui> for GuiHanRender {
	fn redraw(&mut self, lines: &Vec<Line>, line: usize, offset: usize, highlight: &Option<HighlightInfo>, ui: &mut Ui) -> Option<Position> {
		self.gui_redraw(lines, line, offset, highlight, ui)
	}

	fn prev(&mut self, lines: &Vec<Line>, line: usize, offset: usize, ui: &mut Ui) -> Position {
		todo!()
	}

	fn next_line(&mut self, lines: &Vec<Line>, line: usize, offset: usize, ui: &mut Ui) -> Position {
		todo!()
	}

	fn prev_line(&mut self, lines: &Vec<Line>, line: usize, offset: usize, ui: &mut Ui) -> Position {
		todo!()
	}

	fn setup_highlight(&mut self, lines: &Vec<Line>, line: usize, start: usize, ui: &mut Ui) -> Position {
		todo!()
	}
}

impl GuiRender for GuiHanRender
{
	#[inline]
	fn reset_render_context(&self, render_context: &mut RenderContext)
	{
		render_context.max_page_size = render_context.rect.width();
		render_context.line_base = render_context.rect.max.x;
		render_context.leading_space = render_context.default_font_measure.y * 2.0;
		render_context.render_lines.clear();
	}

	#[inline]
	fn create_render_line(&self, default_char_size: &Vec2) -> RenderLine
	{
		let width = default_char_size.x;
		let space = width / 2.0;
		RenderLine::new(width, space)
	}

	fn wrap_line(&self, text: &Line, line: usize, start_offset: usize, end_offset: usize, ui: &mut Ui, draw_context: &mut RenderContext) -> Vec<RenderLine>
	{
		let mut draw_lines = vec![];
		let mut draw_line = self.create_render_line(&draw_context.default_font_measure);
		if start_offset == end_offset {
			draw_lines.push(draw_line);
			return draw_lines;
		}
		let mut top = draw_context.rect.min.y;
		let max_top = draw_context.rect.max.y;

		for i in start_offset..end_offset {
			if i == 0 && with_leading(text) {
				top = draw_context.rect.min.y + draw_context.leading_space;
			}
			let char = text.char_at(i).unwrap();
			let char = self.map_char(char);
			let char_style = text.char_style_at(i, &draw_context.colors);
			let font_size = scale_font_size(draw_context.font_size, char_style.font_scale);
			let mut rect = paint_char(
				ui,
				char,
				font_size,
				&Pos2::new(draw_context.line_base, top),
				Align2::RIGHT_TOP,
				Color32::BLACK);

			let mut draw_height = rect.height();
			let (draw_offset, style) = if let Some(position) = char_style.border {
				match position {
					StylePosition::Start => {
						let space = draw_height / 2.0;
						draw_height += space;
						rect.max.y += space;
						(Pos2::new(0.0, space), Some((TextStyle::Border, position)))
					}
					StylePosition::Middle | StylePosition::Single => {
						(Pos2::ZERO, Some((TextStyle::Border, position)))
					}
					StylePosition::End => {
						let space = draw_height / 2.0;
						draw_height += space;
						rect.max.y += space;
						(Pos2::ZERO, Some((TextStyle::Border, position)))
					}
				}
			} else if let Some((line, position)) = char_style.line {
				(Pos2::ZERO, Some((TextStyle::Line(line), position)))
			} else {
				(Pos2::ZERO, None)
			};
			let draw_width = rect.width();
			if top + draw_height > max_top {
				let line_delta = draw_line.draw_size + draw_line.line_space;
				draw_context.line_base -= line_delta;
				draw_lines.push(draw_line);
				draw_line = self.create_render_line(&draw_context.default_font_measure);
				rect = Rect {
					min: Pos2::new(rect.min.x - line_delta, rect.min.y - top + draw_context.rect.min.y),
					max: Pos2::new(rect.max.x - line_delta, rect.max.y - top + draw_context.rect.min.y),
				}
			}
			if draw_width > draw_line.draw_size {
				draw_line.draw_size = draw_width;
				draw_line.line_space = draw_width / 2.0;
			}
			let dc = RenderChar {
				char,
				font_size,
				color: char_style.color,
				background: char_style.background,
				style,
				line,
				offset: i,
				rect,
				draw_offset,
			};
			draw_line.chars.push(dc);
			top = rect.max.y;
		}
		if draw_line.chars.len() > 0 {
			draw_context.line_base -= draw_line.draw_size + draw_line.line_space;
			draw_lines.push(draw_line);
		}
		return draw_lines;
	}

	fn draw_style(&self, text: &Line, draw_text: &RenderLine, ui: &mut Ui)
	{}
}