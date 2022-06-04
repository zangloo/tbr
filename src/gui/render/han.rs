use std::collections::HashMap;
use std::ops::RangeInclusive;
use eframe::egui::{Align2, Color32, Pos2, Rect, Stroke, Ui, Vec2};

use crate::book::{Line, TextStyle};
use crate::common::{HAN_RENDER_CHARS_PAIRS, with_leading};
use crate::controller::{HighlightInfo, Render};
use crate::gui::render::{RenderChar, RenderContext, RenderLine, GuiRender, paint_char, scale_font_size, update_for_highlight, stroke_width_for_space};
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
	#[inline]
	fn redraw(&mut self, lines: &Vec<Line>, line: usize, offset: usize, highlight: &Option<HighlightInfo>, ui: &mut Ui) -> Option<Position> {
		self.gui_redraw(lines, line, offset, highlight, ui)
	}

	#[inline]
	fn prev_page(&mut self, lines: &Vec<Line>, line: usize, offset: usize, ui: &mut Ui) -> Position {
		self.gui_prev_page(lines, line, offset, ui)
	}

	#[inline]
	fn next_line(&mut self, lines: &Vec<Line>, line: usize, offset: usize, ui: &mut Ui) -> Position {
		self.gui_next_line(lines, line, offset, ui)
	}

	#[inline]
	fn prev_line(&mut self, lines: &Vec<Line>, line: usize, offset: usize, ui: &mut Ui) -> Position {
		self.gui_prev_line(lines, line, offset, ui)
	}

	#[inline]
	fn setup_highlight(&mut self, lines: &Vec<Line>, line: usize, start: usize, ui: &mut Ui) -> Position {
		self.gui_setup_highlight(lines, line, start, ui)
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
	}

	#[inline]
	fn create_render_line(&self, default_char_size: &Vec2) -> RenderLine
	{
		let width = default_char_size.x;
		let space = width / 2.0;
		RenderLine::new(width, space)
	}

	fn wrap_line(&self, text: &Line, line: usize, start_offset: usize, end_offset: usize, highlight: &Option<HighlightInfo>, ui: &mut Ui, context: &mut RenderContext) -> Vec<RenderLine>
	{
		let mut draw_lines = vec![];
		let mut draw_line = self.create_render_line(&context.default_font_measure);
		let end_offset = if end_offset > text.len() {
			text.len()
		} else {
			end_offset
		};
		if start_offset == end_offset {
			draw_lines.push(draw_line);
			return draw_lines;
		}
		let mut top = context.rect.min.y;
		let max_top = context.rect.max.y;

		for i in start_offset..end_offset {
			if i == 0 && with_leading(text) {
				top = context.rect.min.y + context.leading_space;
			}
			let char = text.char_at(i).unwrap();
			let char = self.map_char(char);
			let char_style = text.char_style_at(i, &context.colors);
			let font_size = scale_font_size(context.font_size, char_style.font_scale);
			let mut rect = paint_char(
				ui,
				char,
				font_size,
				&Pos2::new(context.line_base, top),
				Align2::RIGHT_TOP,
				Color32::BLACK);

			let mut draw_height = rect.height();
			let (draw_offset, style) = if let Some(range) = char_style.border {
				if range.len() == 1 {
					let space = draw_height / 2.0;
					draw_height += draw_height;
					(Pos2::new(0.0, space), Some((TextStyle::Border, range.clone())))
				} else if i == range.start {
					let space = draw_height / 2.0;
					draw_height += space;
					rect.max.y += space;
					(Pos2::new(0.0, space), Some((TextStyle::Border, range.clone())))
				} else if i == range.end - 1 {
					let space = draw_height / 2.0;
					draw_height += space;
					rect.max.y += space;
					(Pos2::ZERO, Some((TextStyle::Border, range.clone())))
				} else {
					(Pos2::ZERO, Some((TextStyle::Border, range.clone())))
				}
			} else if let Some((line, range)) = char_style.line {
				(Pos2::ZERO, Some((TextStyle::Line(line), range.clone())))
			} else if let Some((target, range)) = char_style.link {
				(Pos2::ZERO, Some((TextStyle::Link(target), range.clone())))
			} else {
				(Pos2::ZERO, None)
			};
			let draw_width = rect.width();
			if top + draw_height > max_top {
				let line_delta = draw_line.draw_size + draw_line.line_space;
				context.line_base -= line_delta;
				draw_lines.push(draw_line);
				draw_line = self.create_render_line(&context.default_font_measure);
				rect = Rect {
					min: Pos2::new(rect.min.x - line_delta, rect.min.y - top + context.rect.min.y),
					max: Pos2::new(rect.max.x - line_delta, rect.max.y - top + context.rect.min.y),
				}
			}
			if draw_width > draw_line.draw_size {
				draw_line.draw_size = draw_width;
				draw_line.line_space = draw_width / 2.0;
			}
			let color = char_style.color;
			let background = update_for_highlight(line, i, char_style.background, &context.colors, highlight);
			let dc = RenderChar {
				char,
				font_size,
				color,
				background,
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
			context.line_base -= draw_line.draw_size + draw_line.line_space;
			draw_lines.push(draw_line);
		}
		return draw_lines;
	}

	fn draw_style(&self, draw_text: &RenderLine, ui: &mut Ui)
	{
		#[inline]
		fn underline(ui: &mut Ui, left: f32, top: f32, bottom: f32, stroke_width: f32, color: Color32) {
			let stroke = Stroke::new(stroke_width, color);
			ui.painter().vline(left, RangeInclusive::new(top, bottom), stroke);
		}

		#[inline]
		fn border(ui: &mut Ui, left: f32, right: f32, top: f32, bottom: f32, start: bool, end: bool, stroke_width: f32, color: Color32) {
			let stroke = Stroke::new(stroke_width, color);
			ui.painter().vline(left, RangeInclusive::new(top, bottom), stroke);
			ui.painter().vline(right, RangeInclusive::new(top, bottom), stroke);
			if start {
				ui.painter().hline(RangeInclusive::new(left, right), top, stroke);
			}
			if end {
				ui.painter().hline(RangeInclusive::new(left, right), bottom, stroke);
			}
		}

		let mut i = 0;
		let chars = &draw_text.chars;
		let len = chars.len();
		while i < len {
			let draw_char = &chars[i];
			if let Some((style, range)) = &draw_char.style {
				let offset = draw_char.offset;
				let rect = &draw_char.rect;
				let left = rect.left();
				let top = rect.top();
				let bottom = rect.bottom();
				if range.len() == 1 {
					match style {
						TextStyle::Line(_)
						| TextStyle::Link(_) => {
							let height = bottom - top;
							let space = height / 4.0;
							let stroke_width = stroke_width_for_space(space);
							underline(ui, left - space, top + space, bottom - space, stroke_width, draw_char.color);
						}
						TextStyle::Border => {
							let space = draw_char.draw_offset.y / 2.0;
							let stroke_width = stroke_width_for_space(space);
							border(ui, left - space, rect.right() + space, top + space, bottom - space, true, true, stroke_width, draw_char.color);
						}
						TextStyle::FontSize { .. }
						| TextStyle::Image(_) => {}
					}
				} else if offset == range.end - 1 {
					match style {
						TextStyle::Line(_)
						| TextStyle::Link(_) => {
							let height = bottom - top;
							let space = height / 4.0;
							let stroke_width = stroke_width_for_space(space);
							underline(ui, left - space, top, bottom - space, stroke_width, draw_char.color);
						}
						TextStyle::Border => {
							let space = (bottom - top) / 6.0;
							let stroke_width = stroke_width_for_space(space);
							border(ui, left - space, rect.right() + space, top, bottom - space, false, true, stroke_width, draw_char.color);
						}
						TextStyle::FontSize { .. }
						| TextStyle::Image(_) => {}
					}
				} else {
					let start = offset == range.start;
					i += 1;
					if i < len {
						let (draw_top, color, mut draw_left, mut stroke_width, mut space, style) = match style {
							TextStyle::Line(_)
							| TextStyle::Link(_) => {
								let height = bottom - top;
								let space = height / 4.0;
								let stroke_width = stroke_width_for_space(space);
								if start {
									(top + space, draw_char.color, left - space, stroke_width, space, style.clone())
								} else {
									(top, draw_char.color, left - space, stroke_width, space, style.clone())
								}
							}
							TextStyle::Border => {
								let space = if start {
									(bottom - top) / 6.0
								} else {
									(bottom - top) / 4.0
								};
								let stroke_width = stroke_width_for_space(space);
								if start {
									(top + space, draw_char.color, left - space, stroke_width, space, TextStyle::Border)
								} else {
									(top, draw_char.color, left - space, stroke_width, space, TextStyle::Border)
								}
							}
							TextStyle::FontSize { .. }
							| TextStyle::Image(_) => {
								continue;
							}
						};
						let draw_char_left = len - i;
						let style_char_left = range.end - offset - 1;
						let (char_left, end) = if draw_char_left >= style_char_left {
							(style_char_left, true)
						} else {
							(draw_char_left, false)
						};
						let stop = char_left + i;
						while i < stop - 1 {
							let draw_char = &chars[i];
							let this_rect = draw_char.rect;
							let height = this_rect.height();
							let this_space = height / 4.0;
							let this_left = this_rect.left() - this_space;
							if this_left < draw_left {
								draw_left = this_left;
								space = this_space;
								stroke_width = stroke_width_for_space(space);
							}
							i += 1;
						}
						let draw_char = &chars[i];
						let last_rect = draw_char.rect;
						let this_space = match style {
							TextStyle::Line(_) | TextStyle::Link(_) => last_rect.height() / 4.0,
							TextStyle::Border => last_rect.height() / 6.0,
							_ => { panic!("internal error"); }
						};
						let this_left = last_rect.left() - this_space;
						if this_left < draw_left {
							draw_left = this_left;
							space = this_space;
							stroke_width = stroke_width_for_space(space);
						}
						let bottom = if end {
							last_rect.bottom() - this_space
						} else {
							last_rect.bottom()
						};
						match style {
							TextStyle::Line(_) | TextStyle::Link(_) => underline(ui, draw_left, draw_top, bottom, stroke_width, color),
							TextStyle::Border => border(ui, draw_left, last_rect.right() + space, draw_top, bottom, start, end, stroke_width, color),
							_ => { panic!("internal error"); }
						};
					} else {
						let color = draw_char.color;
						let height = bottom - top;
						match style {
							TextStyle::Line(_) | TextStyle::Link(_) => {
								let space = height / 4.0;
								let stroke_width = stroke_width_for_space(space);
								underline(ui, left - space, top + space, bottom, stroke_width, color)
							}
							TextStyle::Border => {
								let space = if start {
									height / 6.0
								} else {
									height / 4.0
								};
								let stroke_width = stroke_width_for_space(space);
								border(ui, left - space, rect.right() + space, top + space, bottom, start, false, stroke_width, color)
							}
							_ => { panic!("internal error"); }
						};
						break;
					}
				}
			}
			i += 1;
		}
	}
}