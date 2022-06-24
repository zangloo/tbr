use std::collections::HashMap;
use std::ops::RangeInclusive;
use eframe::egui::{Align2, Color32, Pos2, Rect, Stroke, Ui};

use crate::book::{Book, Line, TextStyle, IMAGE_CHAR};
use crate::common::{HAN_RENDER_CHARS_PAIRS, with_leading};
use crate::controller::{HighlightInfo, Render};
use crate::gui::render::{RenderChar, RenderContext, RenderLine, GuiRender, paint_char, scale_font_size, update_for_highlight, stroke_width_for_space, ImageDrawingData, PointerPosition};
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
		let mut draw_line = self.create_render_line(line, context);
		let mut top = context.rect.min.y;
		let max_top = context.rect.max.y;

		for i in start_offset..end_offset {
			let char_style = text.char_style_at(i, &context.colors);
			let (char, mut rect, font_size, draw_offset, style) = if let Some((path, size)) = self.with_image(&char_style, book, &context.rect, ui) {
				let left = context.line_base - size.x;
				let bottom = top + size.y;
				let rect = Rect::from_min_max(
					Pos2::new(left, top),
					Pos2::new(context.line_base, bottom),
				);
				let style = Some((TextStyle::Image(path), i..i + 1));
				(IMAGE_CHAR, rect, context.font_size as f32, Pos2::ZERO, style)
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
				let draw_height = rect.height();
				let (draw_offset, style) = if let Some(range) = char_style.border {
					if range.len() == 1 {
						let space = draw_height / 2.0;
						rect.max.y += draw_height;
						(Pos2::new(0.0, space), Some((TextStyle::Border, range.clone())))
					} else if i == range.start {
						let space = draw_height / 2.0;
						rect.max.y += space;
						(Pos2::new(0.0, space), Some((TextStyle::Border, range.clone())))
					} else if i == range.end - 1 {
						let space = draw_height / 2.0;
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
				(char, rect, font_size, draw_offset, style)
			};
			let draw_width = rect.width();
			if top + rect.height() > max_top {
				let line_delta = draw_line.draw_size + draw_line.line_space;
				context.line_base -= line_delta;
				draw_lines.push(draw_line);
				draw_line = self.create_render_line(line, context);
				rect = Rect {
					min: Pos2::new(rect.min.x - line_delta, rect.min.y - top + context.rect.min.y),
					max: Pos2::new(rect.max.x - line_delta, rect.max.y - top + context.rect.min.y),
				}
			}
			if draw_width > draw_line.draw_size {
				draw_line.draw_size = draw_width;
				if char != IMAGE_CHAR {
					draw_line.line_space = draw_width / 2.0;
				}
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
		fn draw_properties(rect: &Rect, start: bool, end: bool) -> (f32, f32, f32)
		{
			let line_margin = line_margin(rect);
			let char_margin = char_margin(rect, start, end);
			let stroke_width = stroke_width_for_space(line_margin);
			(line_margin, char_margin, stroke_width)
		}
		#[inline]
		fn char_margin(rect: &Rect, start: bool, end: bool) -> f32 {
			if start {
				if end {
					rect.height() / 8.0
				} else {
					rect.height() / 6.0
				}
			} else if end {
				rect.height() / 6.0
			} else {
				rect.height() / 4.0
			}
		}
		#[inline]
		fn line_margin(rect: &Rect) -> f32 {
			rect.width() / 4.0
		}
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
							let (line_margin, char_margin, stroke_width) = draw_properties(rect, true, true);
							underline(ui, left - line_margin, top + char_margin, bottom - char_margin, stroke_width, draw_char.color);
						}
						TextStyle::Border => {
							let (line_margin, char_margin, stroke_width) = draw_properties(rect, true, true);
							border(ui, left - line_margin, rect.right() + line_margin, top + char_margin, bottom - char_margin, true, true, stroke_width, draw_char.color);
						}
						TextStyle::FontSize { .. }
						| TextStyle::Image(_) => {}
					}
				} else if offset == range.end - 1 {
					match style {
						TextStyle::Line(_)
						| TextStyle::Link(_) => {
							let (line_margin, char_margin, stroke_width) = draw_properties(rect, false, true);
							underline(ui, left - line_margin, top + char_margin, bottom - char_margin, stroke_width, draw_char.color);
						}
						TextStyle::Border => {
							let (line_margin, char_margin, stroke_width) = draw_properties(rect, false, true);
							border(ui, left - line_margin, rect.right() + line_margin, top + char_margin, bottom - char_margin, false, true, stroke_width, draw_char.color);
						}
						TextStyle::FontSize { .. }
						| TextStyle::Image(_) => {}
					}
				} else {
					let start = offset == range.start;
					i += 1;
					if i < len {
						let mut left = rect.left();
						let (draw_top, color, style, mut line_margin, mut char_margin, mut stroke_width) = match style {
							TextStyle::Line(_)
							| TextStyle::Link(_)
							| TextStyle::Border => {
								let (line_margin, char_margin, stroke_width) = draw_properties(rect, start, false);
								if start {
									(top + char_margin, draw_char.color, style.clone(), line_margin, char_margin, stroke_width)
								} else {
									(top, draw_char.color, style.clone(), line_margin, char_margin, stroke_width)
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
							let this_rect = &draw_char.rect;
							let this_left = this_rect.left();
							if this_left < left {
								left = this_left;
								(line_margin, char_margin, stroke_width) = draw_properties(this_rect, false, false);
							}
							i += 1;
						}
						let draw_char = &chars[i];
						let last_rect = &draw_char.rect;
						let last_left = last_rect.left();
						if last_left < left {
							left = last_left;
							(line_margin, char_margin, stroke_width) = draw_properties(last_rect, false, true);
						}
						let draw_bottom = if end {
							last_rect.bottom() - char_margin
						} else {
							last_rect.bottom()
						};
						let draw_left = left - line_margin;
						match style {
							TextStyle::Line(_) | TextStyle::Link(_) => underline(ui, draw_left, draw_top, draw_bottom, stroke_width, color),
							TextStyle::Border => border(ui, draw_left, last_rect.right() + line_margin, draw_top, draw_bottom, start, end, stroke_width, color),
							_ => { panic!("internal error"); }
						};
					} else {
						let color = draw_char.color;
						match style {
							TextStyle::Line(_) | TextStyle::Link(_) => {
								let (line_margin, char_margin, stroke_width) = draw_properties(rect, start, false);
								underline(ui, left - line_margin, top + char_margin, bottom, stroke_width, color)
							}
							TextStyle::Border => {
								let (line_margin, char_margin, stroke_width) = draw_properties(rect, start, false);
								border(ui, left - line_margin, rect.right() + line_margin, top + char_margin, bottom, start, false, stroke_width, color)
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
