use std::collections::HashMap;
use std::ops::RangeInclusive;
use eframe::egui::Ui;
use eframe::emath::Align2;
use egui::{Color32, Pos2, Rect, Stroke};

use crate::book::{Book, IMAGE_CHAR, Line, TextStyle};
use crate::common::with_leading;
use crate::controller::{HighlightInfo, Render};
use crate::gui::render::{RenderContext, RenderLine, GuiRender, scale_font_size, paint_char, RenderChar, update_for_highlight, stroke_width_for_space, ImageDrawingData, PointerPosition};
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
		fn push_line(draw_lines: &mut Vec<RenderLine>, mut draw_line: RenderLine, context: &mut RenderContext)
		{
			let mut draw_size = 0.0;
			let mut line_space = 0.0;
			for dc in &draw_line.chars {
				let this_height = dc.rect.height();
				if this_height > draw_size {
					draw_size = this_height;
					if dc.char != IMAGE_CHAR {
						line_space = draw_size / 2.0
					}
				}
			}
			draw_line.draw_size = draw_size;
			draw_line.line_space = line_space;
			let bottom = context.line_base + draw_line.draw_size;
			context.line_base = context.line_base + draw_line.draw_size + draw_line.line_space;
			// align to bottom
			for dc in &mut draw_line.chars {
				let rect = &mut dc.rect;
				let max = &mut rect.max;
				let delta = bottom - max.y;
				if delta != 0.0 {
					max.y += delta;
					rect.min.y += delta;
				}
			}
			draw_lines.push(draw_line);
		}
		let (end_offset, wrapped_empty_lines) = self.prepare_wrap(text, line, start_offset, end_offset, context);
		if let Some(wrapped_empty_lines) = wrapped_empty_lines {
			return wrapped_empty_lines;
		}
		let mut draw_lines = vec![];
		let mut draw_line = self.create_render_line(line, context);
		let mut break_position = None;

		let mut left = context.rect.min.x;
		let max_left = context.rect.max.x;
		for i in start_offset..end_offset {
			let char_style = text.char_style_at(i, &context.colors);
			let (char, mut rect, font_size, draw_offset, style) = if let Some((path, size)) = self.with_image(&char_style, book, &context.rect, ui) {
				let bottom = context.line_base + size.y;
				let right = left + size.x;
				let rect = Rect::from_min_max(
					Pos2::new(left, context.line_base),
					Pos2::new(right, bottom),
				);
				let style = Some((TextStyle::Image(path), i..i + 1));
				(IMAGE_CHAR, rect, context.font_size as f32, Pos2::ZERO, style)
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

				let draw_width = rect.width();
				let (draw_offset, style) = if let Some(range) = char_style.border {
					if range.len() == 1 {
						let space = draw_width / 2.0;
						rect.max.x += draw_width;
						(Pos2::new(space, 0.0), Some((TextStyle::Border, range.clone())))
					} else if i == range.start {
						let space = draw_width / 2.0;
						rect.max.x += space;
						(Pos2::new(space, 0.0), Some((TextStyle::Border, range.clone())))
					} else if i == range.end - 1 {
						let space = draw_width / 2.0;
						rect.max.x += space;
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
			let draw_height = rect.height();
			let draw_width = rect.width();

			let can_break = char == ' ' || char == '\t';
			if left + draw_width > max_left {
				left = context.rect.min.x;
				// for unicode, can_break, or prev break not exists, or breaking conent too long
				if !char.is_ascii_alphanumeric() || can_break || break_position.is_none()
					|| draw_line.chars.len() > break_position.unwrap() + 20
					|| break_position.unwrap() >= draw_line.chars.len() {
					push_line(&mut draw_lines, draw_line, context);
					draw_line = self.create_render_line(line, context);
					rect = Rect {
						min: Pos2::new(left, context.line_base),
						max: Pos2::new(left + draw_width, draw_height + context.line_base),
					};
					break_position = None;
					// for break char, will not print it any more
					// skip it for line break
					if can_break {
						continue;
					}
				} else {
					let mut break_draw_line = self.create_render_line(line, context);
					if let Some(break_position) = break_position {
						let break_chars = draw_line.chars.drain(break_position..).collect();
						break_draw_line.chars = break_chars
					}
					push_line(&mut draw_lines, draw_line, context);
					draw_line = break_draw_line;
					for draw_char in &mut draw_line.chars {
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
						max: Pos2::new(left + draw_width, context.line_base + draw_height),
					}
				}
			}
			let color = char_style.color;
			let background = update_for_highlight(line, i, char_style.background, &context.colors, highlight);
			left += draw_width;
			if can_break {
				let blank_char = RenderChar {
					char: ' ',
					font_size,
					color,
					background,
					style,
					line,
					offset: i,
					rect,
					draw_offset,
				};
				draw_line.chars.push(blank_char);
				break_position = Some(draw_line.chars.len());
			} else {
				let blank_char = RenderChar {
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
				draw_line.chars.push(blank_char);
			}
		}
		if draw_line.chars.len() > 0 {
			push_line(&mut draw_lines, draw_line, context);
		}
		return draw_lines;
	}

	fn draw_style(&self, draw_text: &RenderLine, ui: &mut Ui)
	{
		#[inline]
		pub(self) fn underline(ui: &mut Ui, bottom: f32, left: f32, right: f32, stroke_width: f32, color: Color32) {
			let stroke = Stroke::new(stroke_width, color);
			ui.painter().hline(RangeInclusive::new(left, right), bottom, stroke);
		}

		#[inline]
		pub(self) fn border(ui: &mut Ui, left: f32, right: f32, top: f32, bottom: f32, with_start: bool, with_end: bool, stroke_width: f32, color: Color32) {
			let stroke = Stroke::new(stroke_width, color);
			ui.painter().hline(RangeInclusive::new(left, right), top, stroke);
			ui.painter().hline(RangeInclusive::new(left, right), bottom, stroke);
			if with_start {
				ui.painter().vline(left, RangeInclusive::new(top, bottom), stroke);
			}
			if with_end {
				ui.painter().vline(right, RangeInclusive::new(top, bottom), stroke);
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
				let bottom = rect.bottom();
				let right = rect.right();
				if range.len() == 1 {
					match style {
						TextStyle::Line(_)
						| TextStyle::Link(_) => {
							let width = right - left;
							let space = width / 4.0;
							let stroke_width = stroke_width_for_space(space);
							underline(ui, bottom + space, left + space, right - space, stroke_width, draw_char.color);
						}
						TextStyle::Border => {
							let space = draw_char.draw_offset.x / 2.0;
							let stroke_width = stroke_width_for_space(space);
							border(ui, rect.left() + space, right - space, rect.top() - space, bottom + space, true, true, stroke_width, draw_char.color);
						}
						TextStyle::FontSize { .. }
						| TextStyle::Image(_) => {}
					}
				} else if offset == range.end - 1 {
					match style {
						TextStyle::Line(_)
						| TextStyle::Link(_) => {
							let width = right - left;
							let space = width / 4.0;
							let stroke_width = stroke_width_for_space(space);
							underline(ui, bottom + space, left, right - space, stroke_width, draw_char.color);
						}
						TextStyle::Border => {
							let space = (right - left) / 6.0;
							let stroke_width = stroke_width_for_space(space);
							border(ui, left, right - space, rect.top() - space, bottom + space, false, true, stroke_width, draw_char.color);
						}
						TextStyle::FontSize { .. }
						| TextStyle::Image(_) => {}
					}
				} else {
					let with_start = offset == range.start;
					i += 1;
					if i < len {
						let (draw_left, color, mut draw_bottom, mut draw_top, mut stroke_width, mut space, style) = match style {
							TextStyle::Line(_)
							| TextStyle::Link(_) => {
								let width = right - left;
								let space = width / 4.0;
								let stroke_width = stroke_width_for_space(space);
								if with_start {
									(left + space, draw_char.color, bottom + space, rect.top(), stroke_width, space, style.clone())
								} else {
									(left, draw_char.color, bottom + space, rect.top(), stroke_width, space, style.clone())
								}
							}
							TextStyle::Border => {
								let width = right - left;
								let space = if with_start {
									width / 6.0
								} else {
									width / 4.0
								};
								let stroke_width = stroke_width_for_space(space);
								if with_start {
									(left + space, draw_char.color, bottom + space, rect.top() - space, stroke_width, space, TextStyle::Border)
								} else {
									(left, draw_char.color, bottom + space, rect.top() - space, stroke_width, space, TextStyle::Border)
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
							let width = this_rect.width();
							let this_space = width / 4.0;
							if this_space > space {
								draw_top = this_rect.top() - this_space;
								draw_bottom = this_rect.bottom() + this_space;
								space = this_space;
								stroke_width = stroke_width_for_space(space);
							}
							i += 1;
						}
						let draw_char = &chars[i];
						let last_rect = draw_char.rect;
						let last_space = match style {
							TextStyle::Line(_) | TextStyle::Link(_) => last_rect.width() / 4.0,
							TextStyle::Border => last_rect.width() / 6.0,
							_ => { panic!("internal error"); }
						};
						if last_space > space {
							draw_top = last_rect.top() - last_space;
							draw_bottom = last_rect.bottom() + last_space;
							space = last_space;
							stroke_width = stroke_width_for_space(space);
						}
						let draw_right = if end {
							last_rect.right() - last_space
						} else {
							last_rect.right()
						};
						match style {
							TextStyle::Line(_) | TextStyle::Link(_) => underline(ui, draw_bottom, draw_left, draw_right, stroke_width, color),
							TextStyle::Border => border(ui, draw_left, draw_right, draw_top, draw_bottom, with_start, end, stroke_width, color),
							_ => { panic!("internal error"); }
						};
					} else {
						let color = draw_char.color;
						let width = right - left;
						match style {
							TextStyle::Line(_) | TextStyle::Link(_) => {
								let space = width / 4.0;
								let stroke_width = stroke_width_for_space(space);
								underline(ui, bottom, left + space, right, stroke_width, color)
							}
							TextStyle::Border => {
								let space = if with_start {
									width / 6.0
								} else {
									width / 4.0
								};
								let stroke_width = stroke_width_for_space(space);
								border(ui, left + space, right, rect.top() - space, bottom + space, with_start, false, stroke_width, color)
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