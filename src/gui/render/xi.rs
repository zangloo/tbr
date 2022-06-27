use std::collections::HashMap;
use std::iter::Enumerate;
use std::ops::Range;
use std::vec::IntoIter;
use eframe::egui::Ui;
use eframe::emath::Align2;
use egui::{Color32, Pos2, Rect, Stroke};

use crate::book::{Book, CharStyle, Line};
use crate::common::with_leading;
use crate::controller::{HighlightInfo, Render};
use crate::gui::render::{RenderContext, RenderLine, GuiRender, scale_font_size, paint_char, RenderChar, update_for_highlight, ImageDrawingData, PointerPosition, TextDecoration, RenderCell, CharCell};
use crate::Position;

pub(super) struct GuiXiRender {
	images: HashMap<String, ImageDrawingData>,
}

impl GuiXiRender
{
	pub fn new() -> Self
	{
		GuiXiRender { images: HashMap::new() }
	}
}

impl Render<Ui> for GuiXiRender
{
	#[inline]
	fn redraw(&mut self, book: &Box<dyn Book>, lines: &Vec<Line>, line: usize, offset: usize, highlight: &Option<HighlightInfo>, ui: &mut Ui) -> Option<Position>
	{
		self.gui_redraw(book, lines, line, offset, highlight, ui)
	}

	#[inline]
	fn prev_page(&mut self, book: &Box<dyn Book>, lines: &Vec<Line>, line: usize, offset: usize, ui: &mut Ui) -> Position
	{
		self.gui_prev_page(book, lines, line, offset, ui)
	}

	#[inline]
	fn next_line(&mut self, book: &Box<dyn Book>, lines: &Vec<Line>, line: usize, offset: usize, ui: &mut Ui) -> Position
	{
		self.gui_next_line(book, lines, line, offset, ui)
	}

	#[inline]
	fn prev_line(&mut self, book: &Box<dyn Book>, lines: &Vec<Line>, line: usize, offset: usize, ui: &mut Ui) -> Position
	{
		self.gui_prev_line(book, lines, line, offset, ui)
	}

	#[inline]
	fn setup_highlight(&mut self, book: &Box<dyn Book>, lines: &Vec<Line>, line: usize, start: usize, ui: &mut Ui) -> Position
	{
		self.gui_setup_highlight(book, lines, line, start, ui)
	}
}

impl GuiRender for GuiXiRender
{
	#[inline]
	fn reset_render_context(&self, render_context: &mut RenderContext)
	{
		render_context.max_page_size = render_context.rect.height();
		render_context.line_base = render_context.rect.min.y;
		render_context.leading_space = render_context.default_font_measure.x * 2.0;
	}

	#[inline]
	fn create_render_line(&self, line: usize, render_context: &RenderContext) -> RenderLine
	{
		let height = render_context.default_font_measure.y;
		let space = height / 2.0;
		RenderLine::new(line, height, space)
	}

	#[inline]
	fn update_base_line_for_delta(&self, context: &mut RenderContext, delta: f32)
	{
		context.line_base += delta
	}

	fn wrap_line(&mut self, book: &Box<dyn Book>, text: &Line, line: usize, start_offset: usize, end_offset: usize, highlight: &Option<HighlightInfo>, ui: &mut Ui, context: &mut RenderContext) -> Vec<RenderLine>
	{
		#[inline]
		// align chars and calculate line size and space, and reset context.line_base
		fn push_line(draw_lines: &mut Vec<RenderLine>, mut draw_chars: Vec<(RenderChar, CharStyle)>, line: usize, context: &mut RenderContext)
		{
			let mut draw_size = 0.0;
			let mut line_space = 0.0;
			for (dc, _) in &draw_chars {
				let this_height = dc.rect.height();
				if this_height > draw_size {
					draw_size = this_height;
					if !matches!(dc.cell, RenderCell::Image(_)) {
						line_space = draw_size / 2.0
					}
				}
			}
			let bottom = context.line_base + draw_size;
			context.line_base = context.line_base + draw_size + line_space;
			// align to bottom
			for (dc, _) in &mut draw_chars {
				let rect = &mut dc.rect;
				let max = &mut rect.max;
				let delta = bottom - max.y;
				if delta != 0.0 {
					max.y += delta;
					rect.min.y += delta;
				}
			}
			let mut render_line = RenderLine::new(line, draw_size, line_space);
			setup_decorations(draw_chars, &mut render_line, context);
			draw_lines.push(render_line);
		}
		let (end_offset, wrapped_empty_lines) = self.prepare_wrap(text, line, start_offset, end_offset, context);
		if let Some(wrapped_empty_lines) = wrapped_empty_lines {
			return wrapped_empty_lines;
		}
		let mut draw_lines = vec![];
		let mut draw_chars = vec![];
		let mut break_position = None;

		let mut left = context.rect.min.x;
		let max_left = context.rect.max.x;
		for i in start_offset..end_offset {
			let char_style = text.char_style_at(i, context.custom_color, &context.colors);
			let (cell, mut rect, is_blank_char, can_break) = if let Some((path, size)) = self.with_image(&char_style, book, &context.rect, ui) {
				let bottom = context.line_base + size.y;
				let right = left + size.x;
				let rect = Rect::from_min_max(
					Pos2::new(left, context.line_base),
					Pos2::new(right, bottom),
				);
				(RenderCell::Image(path), rect, false, true)
			} else {
				if i == 0 && with_leading(text) {
					left += context.leading_space;
				}
				let char = text.char_at(i).unwrap();
				let font_size = scale_font_size(context.font_size, char_style.font_scale);
				let mut rect = paint_char(
					ui,
					char,
					font_size,
					&Pos2::new(left, context.line_base),
					Align2::LEFT_TOP,
					Color32::BLACK);

				let color = char_style.color;
				let char_size = Pos2::new(rect.width(), rect.height());
				let background = update_for_highlight(line, i, char_style.background, &context.colors, highlight);
				let draw_offset = if let Some(range) = &char_style.border {
					let draw_width = char_size.x;
					let padding = draw_width / 8.0;
					if range.len() == 1 {
						rect.max.x += padding * 2.0;
						Pos2::new(padding, 0.0)
					} else if i == range.start {
						rect.max.x += padding;
						Pos2::new(padding, 0.0)
					} else if i == range.end - 1 {
						rect.max.x += padding;
						Pos2::ZERO
					} else {
						Pos2::ZERO
					}
				} else {
					Pos2::ZERO
				};
				let blank_char = char == ' ' || char == '\t';
				let cell = CharCell {
					char: if blank_char { ' ' } else { char },
					font_size,
					color,
					background,
					draw_offset,
					char_size,
				};
				(RenderCell::Char(cell), rect, blank_char, blank_char || !char.is_ascii_alphanumeric())
			};
			let draw_height = rect.height();
			let draw_width = rect.width();

			if left + draw_width > max_left {
				left = context.rect.min.x;
				// for unicode, can_break, or prev break not exists, or breaking conent too long
				if can_break || break_position.is_none()
					|| draw_chars.len() > break_position.unwrap() + 20
					|| break_position.unwrap() >= draw_chars.len() {
					push_line(&mut draw_lines, draw_chars, line, context);
					draw_chars = vec![];
					break_position = None;
					// for break char, will not print it any more
					// skip it for line break
					if is_blank_char {
						continue;
					}
					rect = Rect {
						min: Pos2::new(left, context.line_base),
						max: Pos2::new(left + draw_width, draw_height + context.line_base),
					};
				} else {
					let break_draw_chars = if let Some(break_position) = break_position {
						draw_chars.drain(break_position..).collect()
					} else {
						vec![]
					};
					push_line(&mut draw_lines, draw_chars, line, context);
					draw_chars = break_draw_chars;
					for (draw_char, _) in &mut draw_chars {
						let w = draw_char.rect.width();
						let h = draw_char.rect.height();
						draw_char.rect = Rect {
							min: Pos2::new(left, context.line_base),
							max: Pos2::new(left + w, context.line_base + h),
						};
						left += w;
					}
					rect = Rect {
						min: Pos2::new(left, context.line_base),
						max: Pos2::new(left + draw_width, draw_height + context.line_base),
					}
				}
			}
			left += draw_width;
			draw_chars.push((RenderChar {
				cell,
				offset: i,
				rect,
			}, char_style));
			if is_blank_char {
				break_position = Some(draw_chars.len());
			}
		}
		if draw_chars.len() > 0 {
			push_line(&mut draw_lines, draw_chars, line, context);
		}
		return draw_lines;
	}

	fn draw_decoration(&self, decoration: &TextDecoration, ui: &mut Ui)
	{
		#[inline]
		pub(self) fn underline(ui: &mut Ui, bottom: f32, left: f32, right: f32, stroke_width: f32, color: Color32) {
			let stroke = Stroke::new(stroke_width, color);
			ui.painter().hline(left..=right, bottom, stroke);
		}

		#[inline]
		pub(self) fn border(ui: &mut Ui, left: f32, right: f32, top: f32, bottom: f32, with_start: bool, with_end: bool, stroke_width: f32, color: Color32) {
			let stroke = Stroke::new(stroke_width, color);
			ui.painter().hline(left..=right, top, stroke);
			ui.painter().hline(left..=right, bottom, stroke);
			if with_start {
				ui.painter().vline(left, top..=bottom, stroke);
			}
			if with_end {
				ui.painter().vline(right, top..=bottom, stroke);
			}
		}
		match decoration {
			TextDecoration::Border { rect, stroke_width, start, end, color } => {
				border(ui, rect.min.x, rect.max.x, rect.min.y, rect.max.y, *start, *end, *stroke_width, *color);
			}
			TextDecoration::UnderLine { pos2, length, stroke_width, color, .. } => {
				underline(ui, pos2.y, pos2.x, pos2.x + length, *stroke_width, *color);
			}
		}
	}

	fn image_cache(&mut self) -> &mut HashMap<String, ImageDrawingData> {
		&mut self.images
	}

	fn pointer_pos(&self, pointer_pos: &Pos2, render_lines: &Vec<RenderLine>, rect: &Rect) -> (PointerPosition, PointerPosition)
	{
		let y = pointer_pos.y;
		let mut line_base = rect.top();
		if y < line_base {
			return (PointerPosition::Head, PointerPosition::Head);
		}
		for i in 0..render_lines.len() {
			let render_line = &render_lines[i];
			let bottom = line_base + render_line.draw_size + render_line.line_space;
			if y >= line_base && y < bottom {
				let x = pointer_pos.x;
				if x <= rect.left() {
					return (PointerPosition::Exact(i), PointerPosition::Head);
				}
				for (j, dc) in render_line.chars.iter().enumerate() {
					if x > dc.rect.left() && x <= dc.rect.right() {
						return (PointerPosition::Exact(i), PointerPosition::Exact(j));
					}
				}
				return (PointerPosition::Exact(i), PointerPosition::Tail);
			}
			line_base = bottom;
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
		let left = min.x;
		let offset = draw_char.offset;
		let (color, padding) = match draw_char.cell {
			RenderCell::Image(_) => (context.colors.color, 0.0),
			RenderCell::Char(CharCell { color, char_size, .. }) => (color, char_size.x / 8.0),
		};
		let draw_left = if offset == range.start {
			left + padding
		} else {
			left
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
				render_line.chars.push(e.1.0);
			}
			let e = iter.next().unwrap();
			draw_char = e.1.0;
		}
		let max = draw_char.rect.max;
		let draw_right = if end {
			max.x - padding
		} else {
			max.x
		};
		let draw_bottom = max.y + padding;
		render_line.chars.push(draw_char);
		TextDecoration::UnderLine {
			pos2: Pos2 { x: draw_left, y: draw_bottom },
			length: draw_right - draw_left,
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
			let left = min.x;
			let offset = draw_char.offset;
			let (color, padding) = match draw_char.cell {
				RenderCell::Image(_) => (context.colors.color, 0.0),
				RenderCell::Char(CharCell { color, char_size, .. }) => (color, char_size.x / 8.0),
			};
			let mut top = min.y;
			let (start, border_left) = if offset == range.start {
				(true, left)
			} else {
				(false, left)
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
					let new_top = e.1.0.rect.top();
					if top > new_top {
						top = new_top;
					}
					render_line.chars.push(e.1.0);
				}
				let e = iter.next().unwrap();
				let new_top = e.1.0.rect.top();
				if top > new_top {
					top = new_top;
				}
				draw_char = e.1.0;
			}
			let max = &draw_char.rect.max;
			let border_right = max.x;
			let border_top = top - padding;
			let border_bottom = max.y + padding;
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
			render_line.add_decoration(decoration)
		} else if let Some((_, range)) = char_style.link {
			let decoration = setup_underline(draw_char, &range, render_line, index, len, &mut iter, context);
			render_line.add_decoration(decoration)
		} else {
			render_line.chars.push(draw_char);
		}
	}
}
