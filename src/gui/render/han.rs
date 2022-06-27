use std::collections::HashMap;
use std::iter::Enumerate;
use std::ops::Range;
use std::vec::IntoIter;
use eframe::egui::{Align2, Color32, Pos2, Rect, Ui};
use egui::Stroke;

use crate::book::{Book, CharStyle, Line};
use crate::common::{HAN_RENDER_CHARS_PAIRS, with_leading};
use crate::controller::{HighlightInfo, Render};
use crate::gui::render::{RenderChar, RenderContext, RenderLine, GuiRender, paint_char, scale_font_size, update_for_highlight, ImageDrawingData, PointerPosition, RenderCell, CharCell, TextDecoration};
use crate::Position;

pub(super) struct GuiHanRender {
	chars_map: HashMap<char, char>,
	images: HashMap<String, ImageDrawingData>,
}

impl GuiHanRender
{
	pub fn new() -> Self
	{
		GuiHanRender
		{
			chars_map: HAN_RENDER_CHARS_PAIRS.into_iter().collect(),
			images: HashMap::new(),
		}
	}

	fn map_char(&self, ch: char) -> char
	{
		*self.chars_map.get(&ch).unwrap_or(&ch)
	}
}

impl Render<Ui> for GuiHanRender {
	#[inline]
	fn redraw(&mut self, book: &Box<dyn Book>, lines: &Vec<Line>, line: usize, offset: usize, highlight: &Option<HighlightInfo>, ui: &mut Ui) -> Option<Position> {
		self.gui_redraw(book, lines, line, offset, highlight, ui)
	}

	#[inline]
	fn prev_page(&mut self, book: &Box<dyn Book>, lines: &Vec<Line>, line: usize, offset: usize, ui: &mut Ui) -> Position {
		self.gui_prev_page(book, lines, line, offset, ui)
	}

	#[inline]
	fn next_line(&mut self, book: &Box<dyn Book>, lines: &Vec<Line>, line: usize, offset: usize, ui: &mut Ui) -> Position {
		self.gui_next_line(book, lines, line, offset, ui)
	}

	#[inline]
	fn prev_line(&mut self, book: &Box<dyn Book>, lines: &Vec<Line>, line: usize, offset: usize, ui: &mut Ui) -> Position {
		self.gui_prev_line(book, lines, line, offset, ui)
	}

	#[inline]
	fn setup_highlight(&mut self, book: &Box<dyn Book>, lines: &Vec<Line>, line: usize, start: usize, ui: &mut Ui) -> Position {
		self.gui_setup_highlight(book, lines, line, start, ui)
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
	fn create_render_line(&self, line: usize, render_context: &RenderContext) -> RenderLine
	{
		let width = render_context.default_font_measure.x;
		let space = width / 2.0;
		RenderLine::new(line, width, space)
	}

	#[inline]
	fn update_base_line_for_delta(&self, context: &mut RenderContext, delta: f32)
	{
		context.line_base -= delta
	}

	fn wrap_line(&mut self, book: &Box<dyn Book>, text: &Line, line: usize, start_offset: usize, end_offset: usize, highlight: &Option<HighlightInfo>, ui: &mut Ui, context: &mut RenderContext) -> Vec<RenderLine>
	{
		let (end_offset, wrapped_empty_lines) = self.prepare_wrap(text, line, start_offset, end_offset, context);
		if let Some(wrapped_empty_lines) = wrapped_empty_lines {
			return wrapped_empty_lines;
		}
		let mut draw_lines = vec![];
		let mut draw_chars = vec![];
		let mut top = context.rect.min.y;
		let max_top = context.rect.max.y;
		let mut draw_size = 0.0;
		let mut line_space = 0.0;

		for i in start_offset..end_offset {
			let char_style = text.char_style_at(i, &context.colors);
			let (cell, mut rect) = if let Some((path, size)) = self.with_image(&char_style, book, &context.rect, ui) {
				let left = context.line_base - size.x;
				let bottom = top + size.y;
				let rect = Rect::from_min_max(
					Pos2::new(left, top),
					Pos2::new(context.line_base, bottom),
				);
				(RenderCell::Image(path), rect)
			} else {
				if i == 0 && with_leading(text) {
					top = context.rect.min.y + context.leading_space;
				}
				let char = text.char_at(i).unwrap();
				let char = self.map_char(char);
				let font_size = scale_font_size(context.font_size, char_style.font_scale);
				let mut rect = paint_char(
					ui,
					char,
					font_size,
					&Pos2::new(context.line_base, top),
					Align2::RIGHT_TOP,
					Color32::BLACK);
				let color = char_style.color;
				let char_size = Pos2::new(rect.width(), rect.height());
				let draw_offset = if let Some(range) = &char_style.border {
					let draw_height = rect.height();
					let padding = draw_height / 8.0;
					let max = &mut rect.max;
					if range.len() == 1 {
						max.y += padding * 2.0;
						Pos2::new(0.0, padding)
					} else if i == range.start {
						max.y += padding;
						Pos2::new(0.0, padding)
					} else if i == range.end - 1 {
						max.y += padding;
						Pos2::ZERO
					} else {
						Pos2::ZERO
					}
				} else {
					Pos2::ZERO
				};
				let background = update_for_highlight(line, i, char_style.background, &context.colors, highlight);
				let cell = CharCell {
					char,
					font_size,
					color,
					background,
					draw_offset,
					char_size,
				};
				(RenderCell::Char(cell), rect)
			};
			if top + rect.height() > max_top {
				let mut render_line = RenderLine::new(line, draw_size, line_space);
				draw_size = 0.0;
				line_space = 0.0;
				setup_decorations(draw_chars, &mut render_line, context);
				context.line_base -= render_line.draw_size + render_line.line_space;
				let line_delta = render_line.draw_size + render_line.line_space;
				draw_lines.push(render_line);
				draw_chars = vec![];
				// the char wrapped to new line, so update positions
				let y_delta = top - context.rect.min.y;
				rect = Rect {
					min: Pos2::new(rect.min.x - line_delta, rect.min.y - y_delta),
					max: Pos2::new(rect.max.x - line_delta, rect.max.y - y_delta),
				};
			}
			if draw_size < rect.width() {
				draw_size = rect.width();
				if !matches!(cell, RenderCell::Image(_)) {
					line_space = draw_size / 2.0
				}
			}
			let dc = RenderChar {
				cell,
				offset: i,
				rect,
			};
			draw_chars.push((dc, char_style));
			top = rect.max.y;
		}
		if draw_chars.len() > 0 {
			let mut render_line = RenderLine::new(line, draw_size, line_space);
			setup_decorations(draw_chars, &mut render_line, context);
			context.line_base -= render_line.draw_size + render_line.line_space;
			draw_lines.push(render_line);
		}
		return draw_lines;
	}

	fn draw_decoration(&self, decoration: &TextDecoration, ui: &mut Ui) {
		#[inline]
		fn underline(ui: &mut Ui, left: f32, top: f32, bottom: f32, stroke_width: f32, color: Color32) {
			let stroke = Stroke::new(stroke_width, color);
			ui.painter().vline(left, top..=bottom, stroke);
		}

		#[inline]
		fn border(ui: &mut Ui, left: f32, right: f32, top: f32, bottom: f32, start: bool, end: bool, stroke_width: f32, color: Color32) {
			let stroke = Stroke::new(stroke_width, color);
			ui.painter().vline(left, top..=bottom, stroke);
			ui.painter().vline(right, top..=bottom, stroke);
			if start {
				ui.painter().hline(left..=right, top, stroke);
			}
			if end {
				ui.painter().hline(left..=right, bottom, stroke);
			}
		}
		match decoration {
			TextDecoration::Border { rect, stroke_width, start, end, color } => {
				border(ui, rect.min.x, rect.max.x, rect.min.y, rect.max.y, *start, *end, *stroke_width, *color);
			}
			TextDecoration::UnderLine { pos2, length, stroke_width, color, .. } => {
				underline(ui, pos2.x, pos2.y, pos2.y + length, *stroke_width, *color);
			}
		}
	}

	fn image_cache(&mut self) -> &mut HashMap<String, ImageDrawingData> {
		&mut self.images
	}

	fn pointer_pos(&self, pointer_pos: &Pos2, render_lines: &Vec<RenderLine>, rect: &Rect) -> (PointerPosition, PointerPosition)
	{
		let x = pointer_pos.x;
		let mut line_base = rect.right();
		if x > line_base {
			return (PointerPosition::Head, PointerPosition::Head);
		}
		for i in 0..render_lines.len() {
			let render_line = &render_lines[i];
			let left = line_base - render_line.draw_size - render_line.line_space;
			if x <= line_base && x > left {
				let y = pointer_pos.y;
				if y <= rect.top() {
					return (PointerPosition::Exact(i), PointerPosition::Head);
				}
				for (j, dc) in render_line.chars.iter().enumerate() {
					if y > dc.rect.top() && y <= dc.rect.bottom() {
						return (PointerPosition::Exact(i), PointerPosition::Exact(j));
					}
				}
				return (PointerPosition::Exact(i), PointerPosition::Tail);
			}
			line_base = left;
		}
		(PointerPosition::Tail, PointerPosition::Tail)
	}
}

fn setup_decorations(draw_chars: Vec<(RenderChar, CharStyle)>, render_line: &mut RenderLine, context: &RenderContext)
{
	#[inline]
	fn setup_underline(mut draw_char: RenderChar, range: &Range<usize>, render_line: &mut RenderLine,
		index: usize, len: usize, iter: &mut Enumerate<IntoIter<(RenderChar, CharStyle)>>, context: &RenderContext) -> TextDecoration {
		let rect = &draw_char.rect;
		let min = &rect.min;
		let mut left = min.x;
		let top = min.y;
		let offset = draw_char.offset;
		let (color, padding) = match draw_char.cell {
			RenderCell::Image(_) => (context.colors.color, 0.0),
			RenderCell::Char(CharCell { color, char_size, .. }) => (color, char_size.y / 8.0),
		};
		let draw_top = if offset == range.start {
			top + padding
		} else {
			top
		};
		let style_left = range.end - offset - 1;
		let chars_left = len - index - 1;
		let (left_count, end) = if style_left <= chars_left {
			(style_left, true)
		} else {
			(chars_left, false)
		};
		if left_count > 0 {
			render_line.chars.push(draw_char);
			for _ in 1..left_count {
				let e = iter.next().unwrap();
				if left > e.1.0.rect.left() {
					left = e.1.0.rect.left()
				}
				render_line.chars.push(e.1.0);
			}
			let e = iter.next().unwrap();
			if left > e.1.0.rect.left() {
				left = e.1.0.rect.left()
			}
			draw_char = e.1.0;
		}
		let draw_bottom = if end {
			draw_char.rect.bottom() - padding
		} else {
			draw_char.rect.bottom()
		};
		let draw_left = left - padding;
		render_line.chars.push(draw_char);
		TextDecoration::UnderLine {
			pos2: Pos2 { x: draw_left, y: draw_top },
			length: draw_bottom - draw_top,
			stroke_width: padding / 2.0,
			color,
		}
	}
	let len = draw_chars.len();
	let mut iter = draw_chars.into_iter().enumerate();
	while let Some((index, (mut draw_char, char_style))) = iter.next() {
		if let Some(range) = char_style.border {
			let rect = &draw_char.rect;
			let min = &rect.min;
			let top = min.y;
			let offset = draw_char.offset;
			let (color, padding) = match draw_char.cell {
				RenderCell::Image(_) => (context.colors.color, 0.0),
				RenderCell::Char(CharCell { color, char_size, .. }) => (color, char_size.y / 8.0),
			};
			let mut left = min.x;
			let (start, border_top) = if offset == range.start {
				(true, top)
			} else {
				(false, top)
			};
			let style_left = range.end - offset - 1;
			let chars_left = len - index - 1;
			let (left_count, end) = if style_left <= chars_left {
				(style_left, true)
			} else {
				(chars_left, false)
			};
			if left_count > 0 {
				render_line.chars.push(draw_char);
				for _ in 1..left_count {
					let e = iter.next().unwrap();
					let new_left = e.1.0.rect.left();
					if left > new_left {
						left = new_left;
					}
					render_line.chars.push(e.1.0);
				}
				let e = iter.next().unwrap();
				let new_left = e.1.0.rect.left();
				if left > new_left {
					left = new_left;
				}
				draw_char = e.1.0;
			}
			let max = &draw_char.rect.max;
			let border_bottom = max.y;
			let border_left = left - padding;
			let border_right = max.x + padding;
			render_line.chars.push(draw_char);
			render_line.add_decoration(TextDecoration::Border {
				rect: Rect {
					min: Pos2 { x: border_left, y: border_top },
					max: Pos2 { x: border_right, y: border_bottom },
				},
				stroke_width: padding / 2.0,
				start,
				end,
				color,
			});
		} else if let Some((_, range)) = char_style.line {
			let decoration = setup_underline(draw_char, &range, render_line, index, len, &mut iter, context);
			render_line.add_decoration(decoration);
		} else if let Some((_, range)) = char_style.link {
			let decoration = setup_underline(draw_char, &range, render_line, index, len, &mut iter, context);
			render_line.add_decoration(decoration);
		} else {
			render_line.chars.push(draw_char);
		}
	}
}
