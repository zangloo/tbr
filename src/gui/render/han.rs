use std::collections::{HashMap, HashSet};
use std::iter::Enumerate;
use std::ops::Range;
use std::vec::IntoIter;
use gtk4::cairo::Context as CairoContext;
use gtk4::pango::Layout as PangoContext;

use crate::book::{Book, CharStyle, Line};
use crate::color::Color32;
use crate::common::{HAN_COMPACT_CHARS, HAN_RENDER_CHARS_PAIRS, with_leading};
use crate::controller::HighlightInfo;
use crate::gui::math::{Pos2, pos2, Rect, vec2};
use crate::gui::render::{RenderChar, RenderContext, RenderLine, GuiRender, scale_font_size, update_for_highlight, ImageDrawingData, PointerPosition, RenderCell, CharCell, TextDecoration, vline, hline, CharDrawData, GuiViewScrollDirection, GuiViewScrollSizing};

pub(super) struct GuiHanRender {
	chars_map: HashMap<char, char>,
	compact_chars: HashSet<char>,
	images: HashMap<String, ImageDrawingData>,
	baseline: f32,
	outline_draw_cache: HashMap<u64, CharDrawData>,
}

impl GuiHanRender
{
	pub fn new() -> Self
	{
		GuiHanRender
		{
			chars_map: HAN_RENDER_CHARS_PAIRS.into_iter().collect(),
			compact_chars: HAN_COMPACT_CHARS.into_iter().collect(),
			images: HashMap::new(),
			baseline: 0.0,
			outline_draw_cache: HashMap::new(),
		}
	}

	fn map_char(&self, ch: char) -> char
	{
		*self.chars_map.get(&ch).unwrap_or(&ch)
	}
}

impl GuiRender for GuiHanRender
{
	#[inline(always)]
	fn render_han(&self) -> bool {
		true
	}

	#[inline]
	fn reset_baseline(&mut self, render_context: &RenderContext)
	{
		self.baseline = render_context.render_rect.max.x;
	}

	#[inline]
	fn reset_render_context(&mut self, render_context: &mut RenderContext)
	{
		render_context.max_page_size = render_context.render_rect.width();
		render_context.leading_space = render_context.default_font_measure.y
			* render_context.leading_chars as f32;
	}

	#[inline]
	fn create_render_line(&self, line: usize, render_context: &RenderContext)
		-> RenderLine
	{
		let width = render_context.default_font_measure.x;
		let space = width / 2.0;
		RenderLine::new(line, width, space)
	}

	#[inline]
	fn update_baseline_for_delta(&mut self, delta: f32)
	{
		self.baseline -= delta
	}

	fn wrap_line(&mut self, book: &dyn Book, text: &Line, line: usize,
		start_offset: usize, end_offset: usize, highlight: &Option<HighlightInfo>,
		pango: &PangoContext, context: &mut RenderContext) -> Vec<RenderLine>
	{
		let (end_offset, wrapped_empty_lines) = self.prepare_wrap(text, line, start_offset, end_offset, context);
		if let Some(wrapped_empty_lines) = wrapped_empty_lines {
			return wrapped_empty_lines;
		}
		let mut draw_lines = vec![];
		let mut draw_chars = vec![];
		let mut top = context.render_rect.min.y;
		let max_top = context.render_rect.max.y;
		let mut line_size = 0.0;
		let mut line_space = 0.0;
		let default_size = context.default_font_measure.x;

		for i in start_offset..end_offset {
			let char_style = text.char_style_at(i, context.custom_color, &context.colors);
			let (cell, mut rect) = if let Some((path, size)) = self.with_image(&char_style, book, &context.render_rect) {
				let left = self.baseline - size.x;
				let bottom = top + size.y;
				let rect = Rect::from_min_max(
					Pos2::new(left, top),
					Pos2::new(self.baseline, bottom),
				);
				(RenderCell::Image(path), rect)
			} else {
				if i == 0 && with_leading(text) {
					top = context.render_rect.min.y + context.leading_space;
				}
				let char = text.char_at(i).unwrap();
				let char = self.map_char(char);
				let font_size = scale_font_size(context.font_size, char_style.font_scale);
				let (size, draw_size, draw_offset) = self.measure_char(
					pango,
					char,
					font_size,
					context);
				let (char_height, y_offset) = if self.compact_chars.contains(&char) {
					(draw_size.y * 2., -draw_offset.y + (draw_size.y / 2.))
				} else {
					(size.y, 0.)
				};
				let mut cell_offset = vec2(-draw_offset.x, y_offset);
				let cell_size = vec2(draw_size.x, char_height);
				let color = char_style.color.clone();
				let mut rect = Rect::new(self.baseline - cell_size.x, top, cell_size.x, cell_size.y);
				if let Some(range) = &char_style.border {
					let padding = cell_size.y / 4.0;
					let max = &mut rect.max;
					if range.len() == 1 {
						max.y += padding * 2.0;
						cell_offset.y += padding;
					} else if i == range.start {
						max.y += padding;
						cell_offset.y += padding;
					} else if i == range.end - 1 {
						max.y += padding;
					}
				}

				let background = update_for_highlight(line, i, char_style.background.clone(), &context.colors, highlight);
				let cell = CharCell {
					char,
					font_size,
					color,
					background,
					cell_offset,
					cell_size,
				};
				(RenderCell::Char(cell), rect)
			};
			if top + rect.height() > max_top {
				let mut render_line = RenderLine::new(line, line_size, line_space);
				line_size = 0.0;
				line_space = 0.0;
				setup_decorations(draw_chars, &mut render_line, context);
				self.baseline -= render_line.line_size + render_line.line_space;
				let line_delta = render_line.line_size + render_line.line_space;
				draw_lines.push(render_line);
				draw_chars = vec![];
				// the char wrapped to new line, so update positions
				let y_delta = top - context.render_rect.min.y;
				rect = Rect {
					min: Pos2::new(rect.min.x - line_delta, rect.min.y - y_delta),
					max: Pos2::new(rect.max.x - line_delta, rect.max.y - y_delta),
				};
			}
			let rect_width = rect.width();
			if line_size < rect_width {
				line_size = rect_width;
				if matches!(cell, RenderCell::Image(_)) {
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
			top = rect.max.y;
			let dc = RenderChar {
				cell,
				offset: i,
				rect,
			};
			draw_chars.push((dc, char_style));
		}
		if draw_chars.len() > 0 {
			let mut render_line = RenderLine::new(line, line_size, line_space);
			setup_decorations(draw_chars, &mut render_line, context);
			self.baseline -= render_line.line_size + render_line.line_space;
			draw_lines.push(render_line);
		}
		return draw_lines;
	}

	fn draw_decoration(&self, decoration: &TextDecoration, cairo: &CairoContext) {
		#[inline]
		fn border(cairo: &CairoContext, left: f32, right: f32, top: f32, bottom: f32, start: bool, end: bool, stroke_width: f32, color: Color32) {
			vline(cairo, left, top, bottom, stroke_width, &color);
			vline(cairo, right, top, bottom, stroke_width, &color);
			if start {
				hline(cairo, left, right, top, stroke_width, &color);
			}
			if end {
				hline(cairo, left, right, bottom, stroke_width, &color);
			}
		}
		match decoration {
			TextDecoration::Border { rect, stroke_width, start, end, color } => {
				border(cairo, rect.min.x, rect.max.x, rect.min.y, rect.max.y, *start, *end, *stroke_width, color.clone());
			}
			TextDecoration::UnderLine { pos2, length, stroke_width, color, .. } => {
				vline(cairo, pos2.x, pos2.y, pos2.y + length, *stroke_width, color);
			}
		}
	}

	fn image_cache(&self) -> &HashMap<String, ImageDrawingData> {
		&self.images
	}

	fn image_cache_mut(&mut self) -> &mut HashMap<String, ImageDrawingData> {
		&mut self.images
	}

	fn pointer_pos(&self, pointer_pos: &Pos2, render_lines: &Vec<RenderLine>,
		rect: &Rect) -> (PointerPosition, PointerPosition)
	{
		let x = pointer_pos.x;
		let mut line_base = rect.right();
		if x > line_base {
			return (PointerPosition::Head, PointerPosition::Head);
		}
		for i in 0..render_lines.len() {
			let render_line = &render_lines[i];
			let left = line_base - render_line.line_size - render_line.line_space;
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

	#[inline]
	fn scroll_size(&self, context: &mut RenderContext) -> GuiViewScrollSizing
	{
		let width = context.render_rect.max.x - self.baseline
			+ context.default_font_measure.x / 2.;
		GuiViewScrollSizing {
			direction: GuiViewScrollDirection::Horizontal,
			init_scroll_value: width,
			full_size: width,
			step_size: context.default_font_measure.x,
			page_size: context.render_rect.width(),
		}
	}

	fn visible_scrolling<'a>(&self, position: f32, size: f32, render_rect: &Rect,
		render_lines: &'a [RenderLine]) -> (Pos2, &'a [RenderLine])
	{
		let mut start = 0;
		let mut end = None;
		let right = render_rect.max.x;
		let left = render_rect.min.x;
		let width = right - left;
		let offset = size - width - position;
		let mut line_left = offset + width;
		for (index, line) in render_lines.iter().enumerate() {
			if line_left > right {
				start = index;
			}
			line_left -= line.size();
			if line_left < left {
				end = Some(index + 1);
				break;
			}
		}
		let end = end.unwrap_or_else(|| render_lines.len());
		(pos2(offset, 0.), &render_lines[start..end])
	}

	#[inline]
	fn translate_mouse_pos(&self, mouse_pos: &mut Pos2, render_rect: &Rect,
		scroll_value: f32, scroll_size: f32)
	{
		let width = render_rect.width();
		mouse_pos.x -= scroll_size - width - scroll_value;
	}
}

fn setup_decorations(mut draw_chars: Vec<(RenderChar, CharStyle)>,
	render_line: &mut RenderLine, context: &RenderContext)
{
	#[inline]
	fn setup_underline(mut draw_char: RenderChar, range: &Range<usize>, render_line: &mut RenderLine,
		index: usize, len: usize, iter: &mut Enumerate<IntoIter<(RenderChar, CharStyle)>>, context: &RenderContext) -> TextDecoration {
		let rect = &draw_char.rect;
		let min = &rect.min;
		let mut left = min.x;
		let top = min.y;
		let offset = draw_char.offset;
		let (color, padding) = match &draw_char.cell {
			RenderCell::Image(_) => (context.colors.color.clone(), 0.0),
			RenderCell::Char(CharCell { color, cell_size, .. }) => (color.clone(), cell_size.y / 4.0),
		};
		let margin = padding / 2.0;
		let draw_top = if offset == range.start {
			top + margin
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
			draw_char.rect.bottom() - margin
		} else {
			draw_char.rect.bottom()
		};
		let draw_left = left - margin;
		render_line.chars.push(draw_char);
		TextDecoration::UnderLine {
			pos2: Pos2 { x: draw_left, y: draw_top },
			length: draw_bottom - draw_top,
			stroke_width: margin / 2.0,
			color,
		}
	}

	// align chars
	let line_size = render_line.line_size;
	for (ref mut char, _) in &mut draw_chars {
		let rect = &mut char.rect;
		let width = rect.width();
		if width < line_size {
			let delta = (line_size - width) / 2.;
			rect.min.x -= delta;
			rect.max.x -= delta;
		}
	}

	// do setup decorations
	let len = draw_chars.len();
	let mut iter = draw_chars.into_iter().enumerate();
	while let Some((index, (mut draw_char, char_style))) = iter.next() {
		if let Some(range) = char_style.border {
			let rect = &draw_char.rect;
			let min = &rect.min;
			let top = min.y;
			let offset = draw_char.offset;
			let (color, padding) = match &draw_char.cell {
				RenderCell::Image(_) => (context.colors.color.clone(), 0.0),
				RenderCell::Char(CharCell { color, cell_size, .. }) => (color.clone(), cell_size.y / 4.0),
			};
			let margin = padding / 2.0;
			let mut left = min.x;
			let mut right = rect.max.x;
			let (start, border_top) = if offset == range.start {
				(true, top + margin)
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
					let new_right = e.1.0.rect.right();
					if right < new_right {
						right = new_right;
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
			let border_bottom = max.y - margin;
			let border_left = left - margin;
			let border_right = right + margin;
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
			render_line.add_decoration(decoration);
		} else if let Some((_, range)) = char_style.link {
			let decoration = setup_underline(draw_char, &range, render_line, index, len, &mut iter, context);
			render_line.add_decoration(decoration);
		} else {
			render_line.chars.push(draw_char);
		}
	}
}
