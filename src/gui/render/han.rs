use std::collections::HashMap;
use eframe::egui::{Align2, Color32, Pos2, Rect, Ui, Vec2};
use crate::book::{Line, StylePosition};
use crate::common::{HAN_RENDER_CHARS_PAIRS, with_leading};
use crate::gui::Colors;
use crate::gui::render::{DrawChar, DrawContext, DrawLine, GuiRender, paint_char, scale_font_size};

pub(crate) struct GuiHanRender {
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

impl GuiRender for GuiHanRender
{
	#[inline]
	fn create_draw_context<'a>(&self, ui: &'a Ui, rect: &'a Rect, colors: &'a Colors, font_size: u8, default_char_size: &Vec2) -> DrawContext<'a>
	{
		let max_page_size = rect.width();
		let baseline = rect.max.x;
		let leading_space = default_char_size.y * 2.0;
		DrawContext::new(ui, rect, colors, max_page_size, baseline, font_size, default_char_size, leading_space)
	}

	#[inline]
	fn create_draw_line(&self, default_char_size: &Vec2) -> DrawLine
	{
		let width = default_char_size.x;
		let space = width / 2.0;
		DrawLine::new(width, space)
	}

	fn wrap_line(&self, text: &Line, line: usize, start_offset: usize, end_offset: usize, draw_context: &mut DrawContext) -> Vec<DrawLine>
	{
		let mut draw_lines = vec![];
		let mut draw_line = self.create_draw_line(&draw_context.default_char_size);
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
			let char_scale = text.char_scale_at(i);
			let font_size = scale_font_size(draw_context.font_size, char_scale);
			let mut rect = paint_char(
				draw_context.ui,
				char,
				font_size,
				&Pos2::new(draw_context.line_base, top),
				Align2::RIGHT_TOP,
				Color32::BLACK);

			let mut draw_height = rect.height();
			let (draw_offset, style) = if let Some((style, position)) = text.char_style_at(i) {
				match position {
					StylePosition::Start => {
						let space = draw_height / 2.0;
						draw_height += space;
						rect.max.y += space;
						(Pos2::new(0.0, space), Some((style, position)))
					}
					StylePosition::Middle | StylePosition::Single => {
						(Pos2::ZERO, Some((style, position)))
					}
					StylePosition::End => {
						let space = draw_height / 2.0;
						draw_height += space;
						rect.max.y += space;
						(Pos2::ZERO, Some((style, position)))
					}
				}
			} else {
				(Pos2::ZERO, None)
			};
			let draw_width = rect.width();
			if top + draw_height > max_top {
				let line_delta = draw_line.draw_size + draw_line.line_space;
				draw_context.line_base -= line_delta;
				draw_lines.push(draw_line);
				draw_line = self.create_draw_line(&draw_context.default_char_size);
				rect = Rect {
					min: Pos2::new(rect.min.x - line_delta, rect.min.y - top + draw_context.rect.min.y),
					max: Pos2::new(rect.max.x - line_delta, rect.max.y - top + draw_context.rect.min.y),
				}
			}
			if draw_width > draw_line.draw_size {
				draw_line.draw_size = draw_width;
				draw_line.line_space = draw_width / 2.0;
			}
			let dc = DrawChar {
				char,
				font_size,
				color: draw_context.colors.color,
				background: None,
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

	fn draw_style(&self, text: &Line, draw_text: &DrawLine, ui: &mut Ui)
	{}
}