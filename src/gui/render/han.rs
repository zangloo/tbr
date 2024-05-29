use std::collections::{HashMap, HashSet};
use std::ops::Range;

use gtk4::cairo::Context as CairoContext;
use gtk4::pango::Layout as PangoContext;

use crate::book::{Book, Line};
use crate::color::Color32;
use crate::common::{HAN_COMPACT_CHARS, HAN_RENDER_CHARS_PAIRS, with_leading};
use crate::controller::HighlightInfo;
use crate::gui::math::{Pos2, pos2, Rect, vec2};
use crate::gui::render::{CharCell, CharDrawData, GuiRender, ImageDrawingData, PointerPosition, RenderCell, RenderChar, RenderContext, RenderLine, ScrolledDrawData, ScrollSizing, TextDecoration, update_for_highlight, vline};
use crate::gui::render::imp::draw_border;
use crate::html_parser;
use crate::html_parser::{BorderLines, TextDecorationLine, TextStyle};

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
				let measures = self.get_char_measures(
					pango,
					char,
					&char_style.font_scale,
					&char_style.font_weight,
					&char_style.font_family,
					book.font_family_names(),
					book.custom_fonts(),
					context);
				let (char_height, y_offset) = if self.compact_chars.contains(&char) {
					(measures.draw_size.y * 2., -measures.draw_offset.y + (measures.draw_size.y / 2.))
				} else {
					(measures.size.y, 0.)
				};
				let mut cell_offset = vec2(-measures.draw_offset.x, y_offset);
				let cell_size = vec2(measures.draw_size.x, char_height);
				let color = char_style.color.clone();
				let mut rect = Rect::new(self.baseline - cell_size.x, top, cell_size.x, cell_size.y);
				if let Some((range, TextStyle::Border(lines, ..))) = &char_style.border {
					if lines.contains(BorderLines::Left) {
						if lines.contains(BorderLines::Right) {
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
						} else {
							let padding = cell_size.y / 4.0;
							let max = &mut rect.max;
							if i == range.start {
								max.y += padding;
								cell_offset.y += padding;
							}
						}
					} else if lines.contains(BorderLines::Right) {
						let padding = cell_size.y / 4.0;
						let max = &mut rect.max;
						if i == range.end - 1 {
							max.y += padding;
						}
					}
				}

				let background = update_for_highlight(line, i, char_style.background.clone(), &context.colors, highlight);
				let cell = CharCell {
					char,
					font_size: measures.font_size,
					font_weight: measures.font_weight,
					font_family: measures.font_family_idx,
					color,
					background,
					cell_offset,
					cell_size,
				};
				if let Some((link_index, _)) = char_style.link {
					(RenderCell::Link(cell, link_index), rect)
				} else {
					(RenderCell::Char(cell), rect)
				}
			};
			if top + rect.height() > max_top && !draw_chars.is_empty() {
				let mut render_line = RenderLine::new(line, line_size, line_space);
				line_size = 0.0;
				line_space = 0.0;
				align_line(&mut render_line, draw_chars);
				self.setup_decorations(text, &mut render_line, context);
				self.baseline -= render_line.line_size() + render_line.line_space();
				let line_delta = render_line.line_size() + render_line.line_space();
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
				has_title: char_style.title.is_some(),
			};
			draw_chars.push(dc);
		}
		if draw_chars.len() > 0 {
			let mut render_line = RenderLine::new(line, line_size, line_space);
			align_line(&mut render_line, draw_chars);
			self.setup_decorations(text, &mut render_line, context);
			self.baseline -= render_line.line_size() + render_line.line_space();
			draw_lines.push(render_line);
		}
		return draw_lines;
	}

	fn draw_decoration(&self, decoration: &TextDecoration, cairo: &CairoContext) {
		match decoration {
			TextDecoration::Border { rect, stroke_width, start, end, color, lines: bl } => {
				draw_border(cairo, *stroke_width, color,
					rect.min.x, rect.max.x, rect.min.y, rect.max.y,
					bl.contains(BorderLines::Bottom),
					bl.contains(BorderLines::Top),
					bl.contains(BorderLines::Left) && *start,
					bl.contains(BorderLines::Right) && *end);
			}
			TextDecoration::Line { start_points, style, length, stroke_width, color } =>
				for pos2 in start_points {
					vline(cairo, pos2.x, pos2.y, pos2.y + length, *style, *stroke_width, color);
				}
			TextDecoration::BlockBorder { rect, stroke_width, start, end, color, lines: bl } => {
				draw_border(cairo, *stroke_width, color,
					rect.min.x, rect.max.x, rect.min.y, rect.max.y,
					bl.contains(BorderLines::Bottom) && *end,
					bl.contains(BorderLines::Top) && *start,
					bl.contains(BorderLines::Left),
					bl.contains(BorderLines::Right));
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
			let left = line_base - render_line.line_size() - render_line.line_space();
			if x <= line_base && x > left {
				let y = pointer_pos.y;
				if y <= rect.top() {
					return (PointerPosition::Exact(i), PointerPosition::Head);
				}
				return render_line.find(|j, dc| {
					if y > dc.rect.top() && y <= dc.rect.bottom() {
						return Some((PointerPosition::Exact(i), PointerPosition::Exact(j)));
					} else {
						None
					}
				}).unwrap_or((PointerPosition::Exact(i), PointerPosition::Tail));
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
	fn default_line_size(&self, render_context: &RenderContext) -> f32
	{
		render_context.default_font_measure.x
	}

	fn calc_block_rect(&self, render_lines: &Vec<RenderLine>,
		range: Range<usize>, render_in_single_line: bool,
		context: &RenderContext) -> Rect
	{
		#[inline]
		fn calc_top_and_height(render_in_single_line: bool, range: &Range<usize>,
			render_lines: &Vec<RenderLine>, context: &RenderContext, bottom: f32) -> (f32, f32)
		{
			if render_in_single_line {
				if let Some(render_line) = render_lines.get(range.start) {
					if let (Some(first), Some(last)) = (render_line.first_render_char(), render_line.last_render_char()) {
						let top = first.rect.min.y;
						let first_margin = (first.rect.max.y - top) / 8.;
						let top = top - first_margin;

						let bottom = last.rect.max.y;
						let last_margin = (bottom - last.rect.min.y) / 8.;
						let bottom = bottom + last_margin;
						return (top, bottom - top);
					}
				}
			}
			let top = context.render_rect.min.y;
			let y_padding = context.y_padding();
			(top - y_padding / 4., bottom - top + y_padding / 2.)
		}
		let Pos2 { x: mut right, y: bottom } = context.render_rect.max;
		// for single line, render border around text only
		let (top, height) = calc_top_and_height(
			render_in_single_line, &range, render_lines, context, bottom);

		let mut right_padding = self.default_line_size(context) / 8.;
		let mut left_padding = right_padding;
		for idx in 0..range.start {
			let line = &render_lines[idx];
			let line_space = line.line_space();
			right -= line.line_size() + line_space;
			right_padding = line_space / 2.;
		}
		let mut left = right;
		for idx in range {
			let line = &render_lines[idx];
			let line_space = line.line_space();
			left -= line.line_size() + line_space;
			left_padding = line_space / 2.;
		}
		right += right_padding;
		left += left_padding;

		Rect::new(
			left,
			top,
			right - left,
			height)
	}

	#[inline]
	fn scroll_size(&self, context: &mut RenderContext) -> ScrollSizing
	{
		let width = context.render_rect.max.x - self.baseline
			+ context.default_font_measure.x / 2.;
		ScrollSizing {
			init_scroll_value: width,
			full_size: width,
			step_size: context.default_font_measure.x,
			page_size: context.render_rect.width(),
		}
	}

	fn visible_scrolling(&self, scroll_value: f32, scroll_size: f32,
		render_rect: &Rect, render_lines: &[RenderLine], )
		-> Option<ScrolledDrawData>
	{
		let mut start = 0;
		let mut end = None;
		let right = render_rect.max.x;
		let left = render_rect.min.x;
		let width = right - left;
		let offset = scroll_size - width - scroll_value;
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
		let draw_data = Some(ScrolledDrawData {
			offset: pos2(offset, 0.),
			range: start..end,
		});
		draw_data
	}

	#[inline]
	fn translate_mouse_pos(&self, mouse_pos: &mut Pos2, render_rect: &Rect,
		scroll_value: f32, scroll_size: f32)
	{
		let width = render_rect.width();
		mouse_pos.x -= scroll_size - width - scroll_value;
	}

	fn setup_decoration(&self, decoration: &html_parser::TextDecoration,
		decoration_chars_range: Range<usize>, start: bool, end: bool,
		render_line: &mut RenderLine, context: &RenderContext)
	{
		let mut draw_char = render_line.char_at_index(decoration_chars_range.start);
		let rect = &draw_char.rect;
		let min = &rect.min;
		let mut left = min.x;
		let mut right = rect.max.x;
		let top = min.y;
		let (color, padding) = match &draw_char.cell {
			RenderCell::Image(_) =>
				if let Some(color) = &decoration.color {
					(color.clone(), 0.0)
				} else {
					(context.colors.color.clone(), 0.0)
				},
			RenderCell::Char(CharCell { cell_size, .. }) =>
				if let Some(color) = &decoration.color {
					(color.clone(), cell_size.y / 4.0)
				} else {
					(context.colors.color.clone(), cell_size.y / 4.0)
				}
			RenderCell::Link(CharCell { cell_size, .. }, _) =>
				if let Some(color) = &decoration.color {
					(color.clone(), cell_size.y / 4.0)
				} else {
					(context.colors.link.clone(), cell_size.y / 4.0)
				}
		};
		let margin = padding / 2.0;
		let draw_top = if start {
			top + margin
		} else {
			top
		};
		if decoration_chars_range.len() > 1 {
			let last_char_idx = decoration_chars_range.end - 1;
			for i in decoration_chars_range.start + 1..last_char_idx {
				draw_char = render_line.char_at_index(i);
				let char_rect = &draw_char.rect;
				if left > char_rect.left() {
					left = char_rect.left();
				}
				if right < char_rect.right() {
					right = char_rect.right();
				}
			}
			draw_char = render_line.char_at_index(last_char_idx);
			let char_rect = &draw_char.rect;
			if left > char_rect.left() {
				left = char_rect.left()
			}
			if right < char_rect.right() {
				right = char_rect.right();
			}
		}
		let draw_bottom = if end {
			draw_char.rect.bottom() - margin
		} else {
			draw_char.rect.bottom()
		};
		let draw_left = left - margin;
		let draw_right = right + margin;
		let mut start_points = vec![];
		if decoration.line.contains(TextDecorationLine::Underline) {
			start_points.push(Pos2 { x: draw_left, y: draw_top });
		}
		if decoration.line.contains(TextDecorationLine::Overline) {
			start_points.push(Pos2 { x: draw_right, y: draw_top });
		}
		if decoration.line.contains(TextDecorationLine::LineThrough) {
			start_points.push(Pos2 { x: (draw_right + draw_left) / 2., y: draw_top });
		}
		render_line.add_decoration(TextDecoration::Line {
			style: decoration.style,
			start_points,
			length: draw_bottom - draw_top,
			stroke_width: margin / 2.0,
			color,
		})
	}

	fn setup_border(&self, render_line: &mut RenderLine, lines: BorderLines,
		decoration_chars_range: Range<usize>, start: bool, end: bool,
		color: Color32)
	{
		let mut draw_char = render_line.char_at_index(decoration_chars_range.start);
		let rect = &draw_char.rect;
		let min = &rect.min;
		let top = min.y;
		let padding = match &draw_char.cell {
			RenderCell::Image(_) => 0.0,
			RenderCell::Char(CharCell { cell_size, .. })
			| RenderCell::Link(CharCell { cell_size, .. }, _)
			=> cell_size.y / 4.0,
		};
		let margin = padding / 2.0;
		let mut left = min.x;
		let mut right = rect.max.x;
		let border_top = if start {
			top + margin
		} else {
			top
		};
		if decoration_chars_range.len() > 1 {
			let last_char_idx = decoration_chars_range.end - 1;
			for i in decoration_chars_range.start + 1..last_char_idx {
				draw_char = render_line.char_at_index(i);
				let new_rect = &draw_char.rect;
				let new_left = new_rect.left();
				if left > new_left {
					left = new_left;
				}
				let new_right = new_rect.right();
				if right < new_right {
					right = new_right;
				}
			}
			draw_char = render_line.char_at_index(last_char_idx);
			let new_rect = &draw_char.rect;
			let new_left = new_rect.left();
			if left > new_left {
				left = new_left;
			}
			let new_right = new_rect.right();
			if right < new_right {
				right = new_right;
			}
		}
		let max = &draw_char.rect.max;
		let border_bottom = max.y - margin;
		let border_left = left - margin;
		let border_right = right + margin;
		render_line.add_decoration(TextDecoration::Border {
			rect: Rect {
				min: Pos2 { x: border_left, y: border_top },
				max: Pos2 { x: border_right, y: border_bottom },
			},
			stroke_width: margin / 2.0,
			start,
			end,
			color,
			lines,
		});
	}
}

fn align_line(render_line: &mut RenderLine, draw_chars: Vec<RenderChar>)
{
	let line_size = render_line.line_size();
	for mut char in draw_chars.into_iter() {
		let rect = &mut char.rect;
		let width = rect.width();
		if width < line_size {
			let delta = (line_size - width) / 2.;
			rect.min.x -= delta;
			rect.max.x -= delta;
		}
		render_line.push(char);
	}
}
