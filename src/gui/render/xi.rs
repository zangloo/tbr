use std::collections::HashMap;
use std::iter::Enumerate;
use std::ops::Range;
use std::vec::IntoIter;
use gtk4::cairo::Context as CairoContext;
use gtk4::pango::Layout as PangoContext;

use crate::book::{Book, CharStyle, Line};
use crate::color::Color32;
use crate::common::with_leading;
use crate::controller::HighlightInfo;
use crate::gui::math::{Pos2, Rect, Vec2, vec2};
use crate::gui::render::{RenderContext, RenderLine, GuiRender, scale_font_size, RenderChar, update_for_highlight, ImageDrawingData, PointerPosition, TextDecoration, RenderCell, CharCell, hline, vline, CharDrawData};

pub(super) struct GuiXiRender {
	images: HashMap<String, ImageDrawingData>,
	baseline: f32,
	outline_draw_cache: HashMap<u64, CharDrawData>,
}

impl GuiXiRender
{
	pub fn new() -> Self
	{
		GuiXiRender { images: HashMap::new(), baseline: 0.0, outline_draw_cache: HashMap::new() }
	}
}

impl GuiRender for GuiXiRender
{
	#[inline(always)]
	fn render_han(&self) -> bool {
		false
	}

	#[inline]
	fn reset_baseline(&mut self, render_context: &RenderContext)
	{
		self.baseline = render_context.render_rect.min.y;
	}

	#[inline]
	fn reset_render_context(&mut self, render_context: &mut RenderContext)
	{
		render_context.max_page_size = render_context.render_rect.height();
		render_context.leading_space = render_context.default_font_measure.x
			* render_context.leading_chars as f32;
	}

	#[inline]
	fn create_render_line(&self, line: usize, render_context: &RenderContext)
		-> RenderLine
	{
		let height = render_context.default_font_measure.y;
		let space = height / 2.0;
		RenderLine::new(line, height, space)
	}

	#[inline]
	fn update_baseline_for_delta(&mut self, delta: f32)
	{
		self.baseline += delta
	}

	fn wrap_line(&mut self, book: &dyn Book, text: &Line, line: usize,
		start_offset: usize, end_offset: usize, highlight: &Option<HighlightInfo>,
		pango: &PangoContext, context: &mut RenderContext) -> Vec<RenderLine>
	{
		// align chars and calculate line size and space,
		// and reset context.line_base
		fn push_line(draw_lines: &mut Vec<RenderLine>,
			mut draw_chars: Vec<(RenderChar, CharStyle)>,
			line: usize, context: &RenderContext, mut baseline: f32) -> f32
		{
			let mut line_size = 0.0;
			let mut line_space = 0.0;
			let default_size = context.default_font_measure.y;
			for (dc, _) in &draw_chars {
				let this_height = dc.rect.height();
				if this_height > line_size {
					line_size = this_height;
					if matches!(dc.cell, RenderCell::Image(_)) {
						let default_space = default_size / 2.0;
						if line_space < default_space {
							line_space = default_space;
						}
					} else {
						if line_size < default_size {
							line_size = default_size;
						}
						line_space = line_size / 2.0
					}
				}
			}
			let bottom = baseline + line_size;
			baseline = baseline + line_size + line_space;
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
			let mut render_line = RenderLine::new(line, line_size, line_space);
			setup_decorations(draw_chars, &mut render_line, context);
			draw_lines.push(render_line);
			baseline
		}
		let (end_offset, wrapped_empty_lines) = self.prepare_wrap(text, line, start_offset, end_offset, context);
		if let Some(wrapped_empty_lines) = wrapped_empty_lines {
			return wrapped_empty_lines;
		}
		let mut draw_lines = vec![];
		let mut draw_chars = vec![];
		let mut break_position = None;

		let mut left = context.render_rect.min.x;
		let max_left = context.render_rect.max.x;
		for i in start_offset..end_offset {
			let char_style = text.char_style_at(i, context.custom_color, &context.colors);
			let (cell, mut rect, is_blank_char, can_break) = if let Some((path, size)) = self.with_image(&char_style, book, &context.render_rect) {
				let bottom = self.baseline + size.y;
				let right = left + size.x;
				let rect = Rect::from_min_max(
					Pos2::new(left, self.baseline),
					Pos2::new(right, bottom),
				);
				(RenderCell::Image(path), rect, false, true)
			} else {
				if i == 0 && with_leading(text) {
					left += context.leading_space;
				}
				let char = text.char_at(i).unwrap();
				let font_size = scale_font_size(context.font_size, char_style.font_scale);
				let (cell_size, _draw_size, _draw_offset) = self.measure_char(
					pango,
					char,
					font_size,
					context);

				let mut rect = Rect::new(left, self.baseline, cell_size.x, cell_size.y);
				let color = char_style.color.clone();
				let background = update_for_highlight(line, i, char_style.background.clone(), &context.colors, highlight);
				let cell_offset = if let Some(range) = &char_style.border {
					let draw_width = cell_size.x;
					let padding = draw_width / 4.0;
					if range.len() == 1 {
						rect.max.x += padding * 2.0;
						Vec2::new(padding, 0.0)
					} else if i == range.start {
						rect.max.x += padding;
						Vec2::new(padding, 0.0)
					} else if i == range.end - 1 {
						rect.max.x += padding;
						Vec2::ZERO
					} else {
						Vec2::ZERO
					}
				} else {
					Vec2::ZERO
				};
				let blank_char = char == ' ' || char == '\t';
				let cell = CharCell {
					char: if blank_char { ' ' } else { char },
					font_size,
					color,
					background,
					cell_offset,
					cell_size,
				};
				(RenderCell::Char(cell), rect, blank_char, blank_char || !char.is_ascii_alphanumeric())
			};
			let draw_height = rect.height();
			let draw_width = rect.width();

			if left + draw_width > max_left {
				left = context.render_rect.min.x;
				// for unicode, can_break, or prev break not exists, or breaking conent too long
				if can_break || break_position.is_none()
					|| draw_chars.len() > break_position.unwrap() + 20
					|| break_position.unwrap() >= draw_chars.len() {
					self.baseline = push_line(&mut draw_lines, draw_chars, line, context, self.baseline);
					draw_chars = vec![];
					break_position = None;
					// for break char, will not print it any more
					// skip it for line break
					if is_blank_char {
						continue;
					}
					rect = Rect {
						min: Pos2::new(left, self.baseline),
						max: Pos2::new(left + draw_width, draw_height + self.baseline),
					};
				} else {
					let break_draw_chars = if let Some(break_position) = break_position {
						draw_chars.drain(break_position..).collect()
					} else {
						vec![]
					};
					self.baseline = push_line(&mut draw_lines, draw_chars, line, context, self.baseline);
					draw_chars = break_draw_chars;
					for (draw_char, _) in &mut draw_chars {
						let w = draw_char.rect.width();
						let h = draw_char.rect.height();
						draw_char.rect = Rect {
							min: Pos2::new(left, self.baseline),
							max: Pos2::new(left + w, self.baseline + h),
						};
						left += w;
					}
					rect = Rect {
						min: Pos2::new(left, self.baseline),
						max: Pos2::new(left + draw_width, draw_height + self.baseline),
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
			self.baseline = push_line(&mut draw_lines, draw_chars, line,
				context, self.baseline);
		}
		return draw_lines;
	}

	fn draw_decoration(&self, decoration: &TextDecoration, cairo: &CairoContext)
	{
		#[inline]
		pub(self) fn border(cairo: &CairoContext, left: f32, right: f32, top: f32,
			bottom: f32, with_start: bool, with_end: bool, stroke_width: f32,
			color: &Color32)
		{
			hline(cairo, left, right, top, stroke_width, color);
			hline(cairo, left, right, bottom, stroke_width, color);
			if with_start {
				vline(cairo, left, top, bottom, stroke_width, color);
			}
			if with_end {
				vline(cairo, right, top, bottom, stroke_width, color);
			}
		}
		match decoration {
			TextDecoration::Border { rect, stroke_width, start, end, color } => {
				border(cairo, rect.min.x, rect.max.x, rect.min.y, rect.max.y, *start, *end, *stroke_width, color);
			}
			TextDecoration::UnderLine { pos2, length, stroke_width, color, .. } => {
				hline(cairo, pos2.x, pos2.x + length, pos2.y, *stroke_width, color);
			}
		}
	}

	fn image_cache(&mut self) -> &mut HashMap<String, ImageDrawingData> {
		&mut self.images
	}

	fn pointer_pos(&self, pointer_pos: &Pos2, render_lines: &Vec<RenderLine>,
		rect: &Rect) -> (PointerPosition, PointerPosition)
	{
		let y = pointer_pos.y;
		let mut line_base = rect.top();
		if y < line_base {
			return (PointerPosition::Head, PointerPosition::Head);
		}
		for i in 0..render_lines.len() {
			let render_line = &render_lines[i];
			let bottom = line_base + render_line.line_size + render_line.line_space;
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

	#[inline]
	fn cache(&self) -> &HashMap<u64, CharDrawData>
	{
		&self.outline_draw_cache
	}

	#[inline]
	fn cache_mut(&mut self) -> &mut HashMap<u64, CharDrawData>
	{
		&mut self.outline_draw_cache
	}

	fn drawn_size(&self, context: &mut RenderContext) -> Vec2
	{
		let height = self.baseline - context.render_rect.min.y
			+ context.default_font_measure.y / 2.;
		vec2(context.render_rect.width(), height)
	}
}

fn setup_decorations(draw_chars: Vec<(RenderChar, CharStyle)>,
	render_line: &mut RenderLine, context: &RenderContext)
{
	#[inline]
	fn setup_underline(mut draw_char: RenderChar, range: &Range<usize>, render_line: &mut RenderLine,
		index: usize, len: usize, iter: &mut Enumerate<IntoIter<(RenderChar, CharStyle)>>, context: &RenderContext) -> TextDecoration {
		let rect = &draw_char.rect;
		let min = &rect.min;
		let left = min.x;
		let offset = draw_char.offset;
		let (color, padding) = match &draw_char.cell {
			RenderCell::Image(_) => (context.colors.color.clone(), 0.0),
			RenderCell::Char(CharCell { color, cell_size, .. }) => (color.clone(), cell_size.x / 4.0),
		};
		let margin = padding / 2.0;
		let draw_left = if offset == range.start {
			left + margin
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
			max.x - margin
		} else {
			max.x
		};
		let draw_bottom = max.y + margin;
		render_line.chars.push(draw_char);
		TextDecoration::UnderLine {
			pos2: Pos2 { x: draw_left, y: draw_bottom },
			length: draw_right - draw_left,
			stroke_width: margin / 2.0,
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
			let (color, padding) = match &draw_char.cell {
				RenderCell::Image(_) => (context.colors.color.clone(), 0.0),
				RenderCell::Char(CharCell { color, cell_size, .. }) => (color.clone(), cell_size.x / 4.0),
			};
			let margin = padding / 2.0;
			let mut top = min.y;
			let (start, border_left) = if offset == range.start {
				(true, left + margin)
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
			let border_right = max.x - margin;
			let border_top = top - margin;
			let border_bottom = max.y + margin;
			render_line.chars.push(draw_char);
			render_line.add_decoration(TextDecoration::Border {
				rect: Rect {
					min: Pos2 { x: border_left, y: border_top },
					max: Pos2 { x: border_right, y: border_bottom },
				},
				stroke_width: margin / 2.0,
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
