use std::collections::HashMap;
use std::ops::Range;

use gtk4::cairo::Context as CairoContext;
use gtk4::pango::Layout as PangoContext;

use crate::book::{Book, Line};
use crate::color::Color32;
use crate::common::with_leading;
use crate::controller::HighlightInfo;
use crate::gui::math::{Pos2, pos2, Rect, Vec2};
use crate::gui::render::{CharCell, CharDrawData, GuiRender, hline, ImageDrawingData, PointerPosition, RenderCell, RenderChar, RenderContext, RenderLine, ScrolledDrawData, ScrollSizing, TextDecoration, update_for_highlight};
use crate::gui::render::imp::draw_border;
use crate::html_parser;
use crate::html_parser::{BorderLines, TextDecorationLine, TextStyle};

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

	/// align chars and calculate line size and space,
	/// and reset context.line_base
	fn push_line(&self, draw_lines: &mut Vec<RenderLine>,
		draw_chars: Vec<RenderChar>, text: &Line,
		line: usize, context: &RenderContext, mut baseline: f32) -> f32
	{
		let mut line_size = 0.0;
		let mut line_space = 0.0;
		let default_size = context.default_font_measure.y;
		for dc in &draw_chars {
			let this_height = dc.rect.height();
			if this_height > line_size {
				line_size = this_height;
				if matches!(dc.cell, RenderCell::Image(_, _)) {
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
		let mut render_line = RenderLine::new(line, line_size, line_space);
		// align to bottom
		for mut dc in draw_chars {
			let rect = &mut dc.rect;
			let max = &mut rect.max;
			let delta = bottom - max.y;
			if delta != 0.0 {
				max.y += delta;
				rect.min.y += delta;
			}
			render_line.push(dc);
		}
		self.setup_decorations(text, &mut render_line, context);
		draw_lines.push(render_line);
		baseline
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
		let (end_offset, wrapped_empty_lines) = self.prepare_wrap(text, line, start_offset, end_offset, context);
		if let Some(wrapped_empty_lines) = wrapped_empty_lines {
			return wrapped_empty_lines;
		}
		let mut draw_lines = vec![];
		let mut draw_chars = vec![];
		let mut break_position = None;

		let mut left = context.render_rect.min.x;
		let max_left = context.render_rect.max.x;
		let view_rect = &context.render_rect;
		let view_size = view_rect.size();
		for i in start_offset..end_offset {
			let char_style = text.char_style_at(i, context.custom_color, &context.colors);
			let (cell, mut rect, is_blank_char, can_break) = if let Some((path, size)) = self.with_image(&char_style, book, &view_size, context.font_size) {
				let bottom = self.baseline + size.y;
				let right = left + size.x;
				let rect = Rect::from_min_max(
					Pos2::new(left, self.baseline),
					Pos2::new(right, bottom),
				);
				let link_index = char_style.link.map(|(i, _)| i);
				(RenderCell::Image(path, link_index), rect, false, true)
			} else {
				if i == 0 && with_leading(text) {
					left += context.leading_space;
				}
				let char = text.char_at(i).unwrap();
				let measures = self.get_char_measures(
					pango,
					char,
					&char_style.font_scale,
					&char_style.font_weight,
					&char_style.font_family,
					book.font_family_names(),
					book.custom_fonts(),
					context);

				let mut rect = Rect::new(left, self.baseline, measures.size.x, measures.size.y);
				let color = char_style.color.clone();
				let background = update_for_highlight(line, i, char_style.background.clone(), &context.colors, highlight);
				let cell_offset = if let Some((range, TextStyle::Border(lines, ..))) = &char_style.border {
					if lines.contains(BorderLines::Left) {
						if lines.contains(BorderLines::Right) {
							let draw_width = measures.size.x;
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
							let draw_width = measures.size.x;
							let padding = draw_width / 4.0;
							if i == range.start {
								rect.max.x += padding;
								Vec2::new(padding, 0.0)
							} else {
								Vec2::ZERO
							}
						}
					} else if lines.contains(BorderLines::Right) {
						let draw_width = measures.size.x;
						let padding = draw_width / 4.0;
						if i == range.end - 1 {
							rect.max.x += padding;
							Vec2::ZERO
						} else {
							Vec2::ZERO
						}
					} else {
						Vec2::ZERO
					}
				} else {
					Vec2::ZERO
				};
				let blank_char = char == ' ' || char == '\t';
				let cell = CharCell {
					char: if blank_char { ' ' } else { char },
					font_size: measures.font_size,
					font_weight: measures.font_weight,
					font_family: measures.font_family_idx,
					color,
					background,
					cell_offset,
					cell_size: measures.size,
				};
				let render_cell = if let Some((link_index, _)) = char_style.link {
					RenderCell::Link(cell, link_index)
				} else {
					RenderCell::Char(cell)
				};
				(render_cell, rect, blank_char, blank_char || !char.is_ascii_alphanumeric())
			};
			let draw_height = rect.height();
			let draw_width = rect.width();

			if left + draw_width > max_left && !draw_chars.is_empty() {
				left = context.render_rect.min.x;
				// for unicode, can_break, or prev break not exists, or breaking content too long
				if can_break || break_position.is_none()
					|| draw_chars.len() > break_position.unwrap() + 20
					|| break_position.unwrap() >= draw_chars.len() {
					self.baseline = self.push_line(
						&mut draw_lines,
						draw_chars,
						text,
						line,
						context,
						self.baseline);
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
					self.baseline = self.push_line(
						&mut draw_lines,
						draw_chars,
						text,
						line,
						context,
						self.baseline);
					draw_chars = break_draw_chars;
					for draw_char in &mut draw_chars {
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
			draw_chars.push(RenderChar {
				cell,
				offset: i,
				rect,
				has_title: char_style.title.is_some(),
			});
			if is_blank_char {
				break_position = Some(draw_chars.len());
			}
		}
		if draw_chars.len() > 0 {
			self.baseline = self.push_line(
				&mut draw_lines,
				draw_chars,
				text,
				line,
				context,
				self.baseline);
		}
		return draw_lines;
	}

	fn draw_decoration(&self, decoration: &TextDecoration, cairo: &CairoContext)
	{
		match decoration {
			TextDecoration::Border { rect, stroke_width, start, end, color, lines: bl } => {
				draw_border(cairo, *stroke_width, color,
					rect.min.x, rect.max.x, rect.min.y, rect.max.y,
					bl.contains(BorderLines::Left) && *start,
					bl.contains(BorderLines::Right) && *end,
					bl.contains(BorderLines::Top),
					bl.contains(BorderLines::Bottom));
			}
			TextDecoration::Line { start_points, style, length, stroke_width, color, .. } =>
				for pos2 in start_points {
					hline(cairo, pos2.x, pos2.x + length, pos2.y, *style, *stroke_width, color);
				}
			TextDecoration::BlockBorder { rect, stroke_width, start, end, color, lines: bl } => {
				draw_border(cairo, *stroke_width, color,
					rect.min.x, rect.max.x, rect.min.y, rect.max.y,
					bl.contains(BorderLines::Left),
					bl.contains(BorderLines::Right),
					bl.contains(BorderLines::Top) && *start,
					bl.contains(BorderLines::Bottom) && *end);
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
		let y = pointer_pos.y;
		let mut line_base = rect.top();
		if y < line_base {
			return (PointerPosition::Head, PointerPosition::Head);
		}
		for i in 0..render_lines.len() {
			let render_line = &render_lines[i];
			let bottom = line_base + render_line.line_size() + render_line.line_space();
			if y >= line_base && y < bottom {
				let x = pointer_pos.x;
				if x <= rect.left() {
					return (PointerPosition::Exact(i), PointerPosition::Head);
				}
				return render_line.find(|j, dc| {
					if x > dc.rect.left() && x <= dc.rect.right() {
						Some((PointerPosition::Exact(i), PointerPosition::Exact(j)))
					} else {
						None
					}
				}).unwrap_or((PointerPosition::Exact(i), PointerPosition::Tail));
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

	#[inline]
	fn default_line_size(&self, render_context: &RenderContext) -> f32
	{
		render_context.default_font_measure.y
	}

	fn calc_block_rect(&self, render_lines: &Vec<RenderLine>,
		range: Range<usize>, render_in_single_line: bool,
		context: &RenderContext) -> Rect
	{
		#[inline]
		fn calc_left_and_width(render_in_single_line: bool, range: &Range<usize>,
			render_lines: &Vec<RenderLine>, context: &RenderContext, left: f32) -> (f32, f32)
		{
			if render_in_single_line {
				if let Some(render_line) = render_lines.get(range.start) {
					if let (Some(first), Some(last)) = (render_line.first_render_char(), render_line.last_render_char()) {
						let left = first.rect.min.x;
						let first_margin = (first.rect.max.x - left) / 8.;
						let left = left - first_margin;

						let right = last.rect.max.x;
						let last_margin = (right - last.rect.min.x) / 8.;
						let right = right + last_margin;
						return (left, right - left);
					}
				}
			}
			let x_padding = context.x_padding();
			let right = context.render_rect.max.x;
			(left - x_padding / 2., right - left + x_padding)
		}

		let Pos2 { x: left, y: mut top } = context.render_rect.min;
		// for single line, render border around text only
		let (left, width) = calc_left_and_width(
			render_in_single_line, &range, render_lines, context, left);

		let mut top_padding = self.default_line_size(context) / 8.;
		let mut bottom_padding = top_padding;
		for idx in 0..range.start {
			let line = &render_lines[idx];
			let line_space = line.line_space();
			top += line.line_size() + line_space;
			top_padding = line_space / 2.;
		}
		let mut bottom = top;
		for idx in range {
			let line = &render_lines[idx];
			let line_space = line.line_space();
			bottom += line.line_size() + line_space;
			bottom_padding = line_space / 2.;
		}
		top -= top_padding;
		bottom -= bottom_padding;
		Rect::new(left,
			top,
			width,
			bottom - top)
	}

	fn setup_decoration(&self, decoration: &html_parser::TextDecoration,
		decoration_chars_range: Range<usize>, start: bool, end: bool,
		render_line: &mut RenderLine, context: &RenderContext)
	{
		let mut draw_char = render_line.char_at_index(decoration_chars_range.start);
		let rect = &draw_char.rect;
		let min = &rect.min;
		let left = min.x;
		let mut top = min.y;
		let (color, padding) = match &draw_char.cell {
			RenderCell::Image(_, _) =>
				if let Some(color) = &decoration.color {
					(color.clone(), context.default_font_measure.x / 4.0)
				} else {
					(context.colors.color.clone(), context.default_font_measure.x / 4.0)
				},
			RenderCell::Char(CharCell { cell_size, .. }) =>
				if let Some(color) = &decoration.color {
					(color.clone(), cell_size.x / 4.0)
				} else {
					(context.colors.color.clone(), cell_size.x / 4.0)
				}
			RenderCell::Link(CharCell { cell_size, .. }, _) =>
				if let Some(color) = &decoration.color {
					(color.clone(), cell_size.x / 4.0)
				} else {
					(context.colors.link.clone(), cell_size.x / 4.0)
				}
		};
		let margin = padding / 2.0;
		let draw_left = if start {
			left + margin
		} else {
			left
		};
		if decoration_chars_range.len() > 1 {
			let last_char_idx = decoration_chars_range.end - 1;
			for i in decoration_chars_range.start + 1..last_char_idx {
				draw_char = render_line.char_at_index(i);
				let char_top = draw_char.rect.top();
				if top > char_top {
					top = char_top;
				}
			}
			draw_char = render_line.char_at_index(last_char_idx);
			let char_top = draw_char.rect.top();
			if top > char_top {
				top = char_top;
			}
		}
		let max = draw_char.rect.max;
		let draw_right = if end {
			max.x - margin
		} else {
			max.x
		};
		let mut start_points = vec![];
		if decoration.line.contains(TextDecorationLine::Underline) {
			start_points.push(Pos2 { x: draw_left, y: max.y + margin });
		}
		if decoration.line.contains(TextDecorationLine::Overline) {
			start_points.push(Pos2 { x: draw_left, y: top - margin });
		}
		if decoration.line.contains(TextDecorationLine::LineThrough) {
			start_points.push(Pos2 { x: draw_left, y: (max.y + top) / 2. });
		}
		render_line.add_decoration(TextDecoration::Line {
			style: decoration.style,
			start_points,
			length: draw_right - draw_left,
			stroke_width: margin / 2.0,
			color,
		});
	}

	fn setup_border(&self, render_line: &mut RenderLine, lines: BorderLines,
		decoration_chars_range: Range<usize>, start: bool, end: bool,
		color: Color32)
	{
		let mut draw_char = render_line.char_at_index(decoration_chars_range.start);
		let rect = &draw_char.rect;
		let min = &rect.min;
		let left = min.x;
		let padding = match &draw_char.cell {
			RenderCell::Image(_, _) => 0.0,
			RenderCell::Char(CharCell { cell_size, .. })
			| RenderCell::Link(CharCell { cell_size, .. }, _)
			=> cell_size.x / 4.0,
		};
		let margin = padding / 2.0;
		let mut top = min.y;
		let border_left = if start {
			left + margin
		} else {
			left
		};
		if decoration_chars_range.len() > 1 {
			let last_char_idx = decoration_chars_range.end - 1;
			for i in decoration_chars_range.start + 1..last_char_idx {
				draw_char = render_line.char_at_index(i);
				let new_top = draw_char.rect.top();
				if top > new_top {
					top = new_top;
				}
			}
			draw_char = render_line.char_at_index(last_char_idx);
			let new_top = draw_char.rect.top();
			if top > new_top {
				top = new_top;
			}
		}
		let max = &draw_char.rect.max;
		let border_right = max.x - margin;
		let border_top = top - margin;
		let border_bottom = max.y + margin;
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

	fn scroll_size(&self, context: &mut RenderContext) -> ScrollSizing
	{
		let height = self.baseline - context.render_rect.min.y
			+ context.default_font_measure.y / 2.;
		ScrollSizing {
			init_scroll_value: 0.,
			full_size: height,
			step_size: context.default_font_measure.y,
			page_size: context.render_rect.height(),
		}
	}

	fn visible_scrolling(&self, scroll_value: f32, _scroll_size: f32,
		render_rect: &Rect, render_lines: &[RenderLine], )
		-> Option<ScrolledDrawData>
	{
		let mut start = 0;
		let mut end = None;
		let mut total = 0.;
		let max = render_rect.height() + scroll_value;
		for (index, line) in render_lines.iter().enumerate() {
			if total < scroll_value {
				start = index;
			}
			total += line.size();
			if total > max {
				end = Some(index + 1);
				break;
			}
		}
		let end = end.unwrap_or_else(|| render_lines.len());
		let draw_data = Some(ScrolledDrawData {
			offset: pos2(0., -scroll_value),
			range: start..end,
		});
		draw_data
	}

	#[inline]
	fn translate_mouse_pos(&self, mouse_pos: &mut Pos2, _render_rect: &Rect,
		scroll_value: f32, _scroll_size: f32)
	{
		mouse_pos.y += scroll_value;
	}
}
