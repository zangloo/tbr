use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use ab_glyph::{Font, FontVec};
use gtk4::gdk_pixbuf::{Colorspace, InterpType, Pixbuf};
use gtk4::{cairo, pango};
use gtk4::prelude::GdkCairoContextExt;
use gtk4::cairo::{Context as CairoContext};
use gtk4::pango::{Layout as PangoContext, FontDescription};
use gtk4::pango::ffi::PANGO_SCALE;
use crate::book;

use crate::book::{Book, CharStyle, Colors, HAN_CHAR, Line};
use crate::color::Color32;
use crate::common::Position;
use crate::controller::{HighlightInfo, HighlightMode};
use crate::gui::load_image;
use crate::gui::math::{Pos2, pos2, Rect, Vec2, vec2};

#[derive(Clone)]
pub enum TextDecoration {
	// rect, stroke width, is first, is last, color
	Border {
		rect: Rect,
		stroke_width: f32,
		start: bool,
		end: bool,
		color: Color32,
	},
	// start(x,y), length,stroke width, is first, color
	UnderLine {
		pos2: Pos2,
		length: f32,
		stroke_width: f32,
		color: Color32,
	},
}

#[derive(Clone, Debug)]
pub struct CharCell {
	pub char: char,
	pub font_size: f32,
	pub font_weight: u16,
	pub color: Color32,
	pub background: Option<Color32>,
	pub cell_offset: Vec2,
	pub cell_size: Vec2,
}

#[derive(Clone, Debug)]
pub enum RenderCell {
	Char(CharCell),
	Image(String),
	/// usize for link_index
	Link(CharCell, usize),
}

#[derive(Clone, Debug)]
pub struct RenderChar {
	pub cell: RenderCell,
	pub offset: usize,
	pub rect: Rect,
}

#[derive(Clone)]
pub struct RenderLine {
	chars: Vec<RenderChar>,
	line: usize,
	line_size: f32,
	line_space: f32,
	decorations: Vec<TextDecoration>,
}

impl RenderLine
{
	#[inline]
	pub fn new(line: usize, line_size: f32, line_space: f32) -> Self
	{
		RenderLine {
			chars: vec![],
			line,
			line_size,
			line_space,
			decorations: vec![],
		}
	}

	pub fn char_at_pos(&self, pos: &Pos2) -> Option<&RenderChar>
	{
		for dc in &self.chars {
			if dc.rect.contains(pos) {
				return Some(dc);
			}
		}
		None
	}

	#[inline]
	pub fn add_decoration(&mut self, decoration: TextDecoration)
	{
		self.decorations.push(decoration)
	}

	#[inline]
	pub fn line_size(&self) -> f32
	{
		self.line_size
	}

	#[inline]
	pub fn line_space(&self) -> f32
	{
		self.line_space
	}

	#[inline]
	pub fn size(&self) -> f32
	{
		self.line_size + self.line_space
	}

	#[inline]
	pub fn line(&self) -> usize
	{
		self.line
	}

	#[inline]
	pub fn first_offset(&self) -> usize
	{
		self.chars.first().map_or(0, |dc| dc.offset)
	}

	#[inline]
	pub fn char_offset(&self, index: usize) -> usize
	{
		self.chars[index].offset
	}

	#[inline]
	pub fn last_offset(&self) -> usize
	{
		self.chars.last().map_or(0, |dc| dc.offset)
	}

	#[inline]
	pub fn push(&mut self, render_char: RenderChar)
	{
		self.chars.push(render_char);
	}

	#[inline]
	pub fn find<F, T>(&self, f: F) -> Option<T>
		where F: Fn(usize, &RenderChar) -> Option<T>
	{
		for (index, char) in self.chars.iter().enumerate() {
			let found = f(index, char);
			if found.is_some() {
				return found;
			}
		}
		None
	}
}

pub enum CharDrawData {
	Outline(OutlineDrawData),
	Pango(PangoDrawData),
	Space(Vec2),
}

impl CharDrawData {
	#[inline(always)]
	fn size(&self) -> Vec2
	{
		match self {
			CharDrawData::Outline(data) => data.size,
			CharDrawData::Pango(data) => data.size,
			CharDrawData::Space(data) => *data,
		}
	}

	#[inline(always)]
	fn offset(&self) -> Pos2
	{
		match self {
			CharDrawData::Outline(data) => data.draw_offset,
			CharDrawData::Pango(data) => data.draw_offset,
			CharDrawData::Space(_) => Pos2::ZERO,
		}
	}

	#[inline(always)]
	fn draw_size(&self) -> Vec2
	{
		match self {
			CharDrawData::Outline(data) => data.draw_size,
			CharDrawData::Pango(data) => data.draw_size,
			CharDrawData::Space(data) => *data,
		}
	}
}

pub struct PangoDrawData {
	char: String,
	font_size: i32,
	font_weight: u16,
	size: Vec2,
	draw_offset: Pos2,
	draw_size: Vec2,
}

impl PangoDrawData {
	fn measure(char: char, font_size: f32, font_weight: u16, layout: &PangoContext) -> Self
	{
		let text = char.to_string();
		set_pango_font_size(font_size as i32, font_weight, layout);
		layout.set_text(&text);
		let (ink_rect, logical_rect) = layout.pixel_extents();
		let logical_x = logical_rect.x() as f32;
		let logical_y = logical_rect.y() as f32;
		let logical_w = logical_rect.width() as f32;
		let logical_h = logical_rect.height() as f32;
		let size = vec2(logical_w, logical_h);
		let draw_size = vec2(ink_rect.width() as f32, ink_rect.height() as f32);
		let draw_offset = pos2(
			ink_rect.x() as f32 - logical_x,
			ink_rect.y() as f32 - logical_y,
		);

		PangoDrawData {
			char: text,
			font_size: font_size as i32,
			font_weight,
			size,
			draw_offset,
			draw_size,
		}
	}

	fn draw(&self, cairo: &CairoContext, offset_x: f32, offset_y: f32, color: &Color32,
		layout: &PangoContext)
	{
		set_pango_font_size(self.font_size, self.font_weight, layout);
		layout.set_text(&self.char);

		let x_offset = offset_x as f64;
		let y_offset = offset_y as f64;
		color.apply(cairo);
		cairo.move_to(x_offset, y_offset);
		pangocairo::show_layout(cairo, &layout);
	}
}

pub struct OutlineDrawData {
	points: Vec<(u32, u32, u8)>,
	size: Vec2,
	draw_offset: Pos2,
	draw_size: Vec2,
}

impl OutlineDrawData {
	fn measure(char: char, font_size: f32, fonts: &Option<Vec<FontVec>>) -> Option<Self>
	{
		if let Some(fonts) = fonts {
			for font in fonts {
				if let Some(scale) = font.pt_to_px_scale(font_size) {
					let glyph = font.glyph_id(char)
						.with_scale(scale);
					if let Some(outline) = font.outline_glyph(glyph) {
						let mut points = vec![];
						outline.draw(|x, y, a| {
							points.push((x, y, (a * 255.) as u8));
						});
						let bounds = outline.px_bounds();
						let draw_size = vec2(bounds.width(), bounds.height());
						let rect = font.glyph_bounds(outline.glyph());
						let size = vec2(rect.width(), rect.height());
						let offset_x = bounds.min.x - rect.min.x;
						let offset_y = bounds.min.y - rect.min.y;
						let draw_offset = pos2(offset_x, offset_y);
						return Some(OutlineDrawData {
							points,
							size,
							draw_offset,
							draw_size,
						});
					}
				}
			}
		}
		None
	}

	fn draw(&self, cairo: &CairoContext, offset_x: f32, offset_y: f32, color: &Color32)
	{
		if let Some(pixbuf) = Pixbuf::new(Colorspace::Rgb, true, 8,
			self.draw_size.x as i32, self.draw_size.y as i32) {
			let r = color.r();
			let g = color.g();
			let b = color.b();
			for point in &self.points {
				pixbuf.put_pixel(point.0, point.1, r, g, b, point.2);
			}
			let draw_x = (offset_x + self.draw_offset.x) as f64;
			let draw_y = (offset_y + self.draw_offset.y) as f64;
			cairo.set_source_pixbuf(&pixbuf, draw_x, draw_y);
			handle_cairo(cairo.paint());
		}
	}
}

pub struct RenderContext
{
	pub colors: Colors,
	pub fonts: Rc<Option<Vec<FontVec>>>,

	// font size in configuration
	pub font_size: u8,
	// default single char size
	pub default_font_measure: Vec2,

	// use book custom color
	pub custom_color: bool,
	// strip empty lines
	pub strip_empty_lines: bool,

	pub render_rect: Rect,
	pub leading_chars: usize,
	pub leading_space: f32,
	// for calculate chars in single line
	pub max_page_size: f32,

	// method for redraw with scrolling
	pub scroll_redraw_method: ScrollRedrawMethod,
}

impl RenderContext {
	pub fn new(colors: Colors, font_size: u8, custom_color: bool,
		leading_chars: usize, strip_empty_lines: bool) -> Self
	{
		RenderContext {
			colors,
			fonts: Rc::new(None),
			font_size,
			default_font_measure: Pos2::ZERO,
			custom_color,
			strip_empty_lines,
			render_rect: Rect::NOTHING,
			leading_chars,
			leading_space: 0.0,
			max_page_size: 0.0,
			scroll_redraw_method: ScrollRedrawMethod::NoResetScroll,
		}
	}
}

pub struct ImageDrawingData {
	view_rect: Rect,
	texture: Pixbuf,
}

impl ImageDrawingData {
	#[inline]
	pub fn size(&self) -> Vec2
	{
		Vec2::new(self.texture.width() as f32, self.texture.height() as f32)
	}
}

pub enum PointerPosition {
	Head,
	Exact(usize),
	Tail,
}

pub struct ScrolledDrawData {
	pub offset: Pos2,
	pub range: Range<usize>,
}

pub enum ScrollRedrawMethod {
	NoResetScroll,
	ResetScroll,
	ScrollTo(f64),
}

pub struct ScrollSizing {
	pub init_scroll_value: f32,
	pub full_size: f32,
	pub step_size: f32,
	pub page_size: f32,
}

#[inline(always)]
fn cache_key(char: char, font_size: u16, font_weight: u16) -> u64
{
	(char as u64) << 32 | (font_size as u64) << 16 | font_weight as u64
}

pub trait GuiRender {
	fn render_han(&self) -> bool;
	fn reset_baseline(&mut self, render_context: &RenderContext);
	fn reset_render_context(&mut self, render_context: &mut RenderContext);
	fn create_render_line(&self, line: usize, render_context: &RenderContext)
		-> RenderLine;
	fn update_baseline_for_delta(&mut self, delta: f32);
	fn wrap_line(&mut self, book: &dyn Book, text: &Line, line: usize,
		start_offset: usize, end_offset: usize, highlight: &Option<HighlightInfo>,
		pango: &PangoContext, context: &mut RenderContext) -> Vec<RenderLine>;
	fn draw_decoration(&self, decoration: &TextDecoration, cairo: &CairoContext);
	fn image_cache(&self) -> &HashMap<String, ImageDrawingData>;
	fn image_cache_mut(&mut self) -> &mut HashMap<String, ImageDrawingData>;
	// return (line, offset) position
	fn pointer_pos(&self, pointer_pos: &Pos2, render_lines: &Vec<RenderLine>,
		rect: &Rect) -> (PointerPosition, PointerPosition);
	fn cache(&self) -> &HashMap<u64, CharDrawData>;
	fn cache_mut(&mut self) -> &mut HashMap<u64, CharDrawData>;

	/// for scrolling view
	/// get redraw lines size for scrollable size measure
	fn scroll_size(&self, context: &mut RenderContext) -> ScrollSizing;
	/// for scrolling view
	/// update scroll view draw data
	fn visible_scrolling(&self, scroll_value: f32, scroll_size: f32,
		render_rect: &Rect, render_lines: &[RenderLine], )
		-> Option<ScrolledDrawData>;
	/// for scrolling view
	/// translate mouse position in viewport
	fn translate_mouse_pos(&self, mouse_pos: &mut Pos2, render_rect: &Rect,
		scroll_value: f32, scroll_size: f32);

	#[inline]
	fn cache_get(&self, char: char, font_size: f32, font_weight: u16) -> Option<&CharDrawData>
	{
		let key = cache_key(char, font_size as u16, font_weight);
		self.cache().get(&key)
	}
	#[inline]
	fn cache_insert(&mut self, char: char, font_size: f32, font_weight: u16, data: CharDrawData)
	{
		let key = cache_key(char, font_size as u16, font_weight);
		self.cache_mut().insert(key, data);
	}
	fn cache_clear(&mut self)
	{
		self.cache_mut().clear()
	}

	#[inline]
	fn try_wrap_line(&mut self, book: &dyn Book, text: &Line, line: usize,
		start_offset: usize, end_offset: usize, highlight: &Option<HighlightInfo>,
		pango: &PangoContext, context: &mut RenderContext) -> Vec<RenderLine>
	{
		if context.strip_empty_lines && text.is_blank() {
			vec![]
		} else {
			self.wrap_line(book, text, line, start_offset, end_offset, highlight, pango, context)
		}
	}

	#[inline]
	fn prepare_wrap(&mut self, text: &Line, line: usize, start_offset: usize,
		end_offset: usize, context: &RenderContext)
		-> (usize, Option<Vec<RenderLine>>)
	{
		let end_offset = if end_offset > text.len() {
			text.len()
		} else {
			end_offset
		};
		if start_offset == end_offset {
			let draw_line = self.create_render_line(line, context);
			let line_delta = draw_line.line_size + draw_line.line_space;
			self.update_baseline_for_delta(line_delta);
			(end_offset, Some(vec![draw_line]))
		} else {
			(end_offset, None)
		}
	}

	fn gui_redraw(&mut self, book: &dyn Book, lines: &[Line],
		reading_line: usize, reading_offset: usize,
		highlight: &Option<HighlightInfo>, pango: &PangoContext,
		context: &mut RenderContext) -> (Vec<RenderLine>, Option<Position>)
	{
		let mut render_lines = vec![];
		self.reset_baseline(context);

		let mut drawn_size = 0.0;
		let mut offset = reading_offset;
		for index in reading_line..lines.len() {
			let line = &lines[index];
			let wrapped_lines = self.try_wrap_line(book, &line, index, offset, line.len(), highlight, pango, context);
			offset = 0;
			for wrapped_line in wrapped_lines {
				drawn_size += wrapped_line.line_size;
				if drawn_size > context.max_page_size {
					let next = if let Some(char) = wrapped_line.chars.first() {
						Some(Position::new(index, char.offset))
					} else {
						Some(Position::new(index, 0))
					};
					return (render_lines, next);
				}
				drawn_size += wrapped_line.line_space;
				render_lines.push(wrapped_line);
			}
		}
		(render_lines, None)
	}

	fn draw(&self, render_lines: &[RenderLine], cairo: &CairoContext, layout: &PangoContext)
	{
		cairo.set_line_width(1.0);
		for render_line in render_lines {
			for dc in &render_line.chars {
				match &dc.cell {
					RenderCell::Image(name) => {
						self.draw_image(name, &dc.rect, cairo);
					}
					RenderCell::Char(cell)
					| RenderCell::Link(cell, _) => {
						if let Some(bg) = &cell.background {
							draw_rect(cairo, &dc.rect, 1.0, bg);
						}
						let draw_position = Pos2::new(dc.rect.min.x + cell.cell_offset.x, dc.rect.min.y + cell.cell_offset.y);
						// should always exists
						if let Some(draw_data) = self.cache_get(cell.char, cell.font_size, cell.font_weight) {
							draw_char(
								cairo,
								draw_data,
								&draw_position,
								&cell.color,
								layout,
							);
						}
					}
				}
			}
			for decoration in &render_line.decorations {
				self.draw_decoration(decoration, cairo);
			}
		}
	}

	fn gui_prev_page(&mut self, book: &dyn Book, lines: &Vec<Line>,
		reading_line: usize, offset: usize, pango: &PangoContext, context: &mut RenderContext) -> Position
	{
		let (reading_line, mut offset) = if offset == 0 {
			(reading_line - 1, usize::MAX)
		} else {
			(reading_line, offset)
		};

		let mut drawn_size = 0.0;
		for index in (0..=reading_line).rev() {
			let line = &lines[index];
			let wrapped_lines = self.try_wrap_line(book, &line, index, 0, offset, &None, pango, context);
			offset = usize::MAX;
			for wrapped_line in wrapped_lines.iter().rev() {
				drawn_size += wrapped_line.line_size;
				if drawn_size > context.max_page_size {
					return if let Some(char) = wrapped_line.chars.last() {
						let offset = char.offset + 1;
						if offset >= line.len() {
							Position::new(index + 1, 0)
						} else {
							Position::new(index, offset)
						}
					} else {
						Position::new(index + 1, 0)
					};
				}
				drawn_size += wrapped_line.line_space;
			}
		}
		Position::new(0, 0)
	}

	fn gui_next_line(&mut self, book: &dyn Book, lines: &Vec<Line>,
		line: usize, offset: usize, pango: &PangoContext, context: &mut RenderContext)
		-> Position
	{
		let wrapped_lines = self.try_wrap_line(book, &lines[line], line, offset, usize::MAX, &None, pango, context);
		if wrapped_lines.len() > 1 {
			if let Some(next_line_char) = wrapped_lines[1].chars.first() {
				Position::new(line, next_line_char.offset)
			} else {
				Position::new(line + 1, 0)
			}
		} else {
			Position::new(line + 1, 0)
		}
	}

	fn gui_prev_line(&mut self, book: &dyn Book, lines: &Vec<Line>,
		line: usize, offset: usize, pango: &PangoContext, context: &mut RenderContext) -> Position
	{
		let (line, offset) = if offset == 0 {
			if line == 0 {
				return Position::new(0, 0);
			}
			(line - 1, usize::MAX)
		} else {
			(line, offset)
		};
		let text = &lines[line];
		let wrapped_lines = self.try_wrap_line(book, text, line, 0, offset, &None, pango, context);
		if let Some(last_line) = wrapped_lines.last() {
			if let Some(first_char) = last_line.chars.first() {
				Position::new(line, first_char.offset)
			} else {
				Position::new(line, 0)
			}
		} else {
			Position::new(line, 0)
		}
	}

	fn gui_setup_highlight(&mut self, book: &dyn Book, lines: &Vec<Line>,
		line: usize, start: usize, pango: &PangoContext, context: &mut RenderContext)
		-> Position
	{
		let text = &lines[line];
		let wrapped_lines = self.try_wrap_line(book, text, line, 0, start + 1, &None, pango, context);
		if let Some(last_line) = wrapped_lines.last() {
			if let Some(first_char) = last_line.chars.first() {
				Position::new(line, first_char.offset)
			} else {
				Position::new(line, 0)
			}
		} else {
			Position::new(line, 0)
		}
	}

	fn with_image(&mut self, char_style: &CharStyle, book: &dyn Book,
		view_rect: &Rect) -> Option<(String, Pos2)>
	{
		if let Some(href) = &char_style.image {
			if let Some((path, bytes)) = book.image(href) {
				let cache = self.image_cache_mut();
				let (image_data, mut size) = match cache.entry(path.clone().into_owned()) {
					Entry::Occupied(o) => {
						let data = o.into_mut();
						let size = data.size();
						(data, size)
					}
					Entry::Vacant(v) => if let Some((data, size)) = load_image_and_resize(view_rect, bytes) {
						(v.insert(data), size)
					} else {
						return None;
					}
				};

				if *view_rect != image_data.view_rect {
					if let Some((new_image_data, new_size)) = load_image_and_resize(view_rect, bytes) {
						cache.insert(path.clone().into_owned(), new_image_data);
						size = new_size
					} else {
						return None;
					}
				};

				Some((path.into_owned(), size))
			} else {
				None
			}
		} else {
			None
		}
	}

	fn draw_image(&self, name: &str, rect: &Rect, cairo: &CairoContext)
	{
		if let Some(image_data) = self.image_cache().get(name) {
			cairo.set_source_pixbuf(&image_data.texture, rect.min.x as f64, rect.min.y as f64);
			handle_cairo(cairo.paint());
		}
	}

	fn apply_font_modified(&mut self, pango: &PangoContext, render_context: &mut RenderContext)
	{
		self.cache_mut().clear();
		let (size, _draw_size, _draw_offset) = self.measure_char(
			pango,
			HAN_CHAR,
			render_context.font_size as f32,
			book::DEFAULT_FONT_WIDTH,
			render_context);
		render_context.default_font_measure = size;
	}

	fn measure_char(&mut self, layout: &PangoContext, char: char, font_size: f32,
		font_weight: u16, render_context: &mut RenderContext) -> (Vec2, Vec2, Pos2)
	{
		const SPACE: char = ' ';
		const FULL_SPACE: char = '　';
		if let Some(data) = self.cache_get(char, font_size, font_weight) {
			return (data.size(), data.draw_size(), data.offset());
		}
		match char {
			SPACE => {
				let (size, draw_size, draw_offset) = self.measure_char(
					layout, 'S', font_size, font_weight, render_context);
				self.cache_insert(SPACE, font_size, font_weight, CharDrawData::Space(size));
				return (size, draw_size, draw_offset);
			}
			FULL_SPACE => {
				let (size, draw_size, draw_offset) = self.measure_char(
					layout, HAN_CHAR, font_size, font_weight, render_context);
				self.cache_insert(FULL_SPACE, font_size, font_weight, CharDrawData::Space(size));
				return (size, draw_size, draw_offset);
			}
			_ => {}
		}

		if let Some(draw_data) = OutlineDrawData::measure(char, font_size, &render_context.fonts) {
			let data = (draw_data.size, draw_data.draw_size, draw_data.draw_offset);
			self.cache_insert(char, font_size, font_weight, CharDrawData::Outline(draw_data));
			data
		} else {
			let draw_data = PangoDrawData::measure(char, font_size, font_weight, layout);
			let data = (draw_data.size, draw_data.draw_size, draw_data.draw_offset);
			self.cache_insert(char, font_size, font_weight, CharDrawData::Pango(draw_data));
			data
		}
	}
}

fn load_image_and_resize(view_rect: &Rect, bytes: &[u8]) -> Option<(ImageDrawingData, Vec2)>
{
	let image = load_image(bytes)?;
	let width = view_rect.width();
	let height = view_rect.height();
	let image_width = image.width() as f32;
	let image_height = image.height() as f32;
	let image = if image_width > width || image_height > height {
		let image_ratio = image_width / image_height;
		let view_ratio = width / height;
		let (draw_width, draw_height) = if image_ratio > view_ratio {
			let draw_width = width;
			let draw_height = width / image_ratio;
			(draw_width, draw_height)
		} else if image_ratio < view_ratio {
			let draw_width = height * image_ratio;
			let draw_height = height;
			(draw_width, draw_height)
		} else {
			(width, height)
		};
		image.scale_simple(draw_width as i32, draw_height as i32, InterpType::Nearest)?
	} else {
		image
	};
	let draw_width = image.width() as f32;
	let draw_height = image.height() as f32;
	Some((
		ImageDrawingData {
			view_rect: view_rect.clone(),
			texture: image,
		},
		Pos2::new(draw_width, draw_height)
	))
}

#[inline]
pub fn update_for_highlight(render_line: usize, offset: usize, background: Option<Color32>, colors: &Colors, highlight: &Option<HighlightInfo>) -> Option<Color32>
{
	match highlight {
		Some(HighlightInfo { mode: HighlightMode::Search, line, start, end })
		| Some(HighlightInfo { mode: HighlightMode::Link(_), line, start, end })
		if *line == render_line && *start <= offset && *end > offset
		=> Some(colors.highlight_background.clone()),

		Some(HighlightInfo { mode: HighlightMode::Selection(_, line2), line, start, end })
		if (*line == render_line && *line2 == render_line && *start <= offset && *end > offset)
			|| (*line == render_line && *line2 > render_line && *start <= offset)
			|| (*line < render_line && *line2 == render_line && *end > offset)
			|| (*line < render_line && *line2 > render_line)
		=> {
			Some(colors.highlight_background.clone())
		}

		_ => background,
	}
}

#[inline]
pub fn scale_font_size(font_size: u8, scale: f32) -> f32
{
	let scaled_size = font_size as f32 * scale;
	if scaled_size < 9.0 {
		9.0
	} else {
		scaled_size
	}
}

#[inline]
pub fn vline(cairo: &CairoContext, x: f32, top: f32, bottom: f32, stroke_width: f32, color: &Color32)
{
	let x = x as f64;
	color.apply(cairo);
	cairo.move_to(x, top as f64);
	cairo.line_to(x, bottom as f64);
	cairo.set_line_width(stroke_width as f64);
	handle_cairo(cairo.stroke());
}

#[inline]
pub fn hline(cairo: &CairoContext, left: f32, right: f32, y: f32, stroke_width: f32, color: &Color32)
{
	let y = y as f64;
	color.apply(cairo);
	cairo.move_to(left as f64, y);
	cairo.line_to(right as f64, y);
	cairo.set_line_width(stroke_width as f64);
	handle_cairo(cairo.stroke());
}

#[inline]
pub fn draw_rect(cairo: &CairoContext, rect: &Rect, stroke_width: f32, color: &Color32)
{
	color.apply(cairo);
	cairo.set_line_width(stroke_width as f64);
	let size = rect.size();
	cairo.rectangle(rect.min.x as f64, rect.min.y as f64, size.x as f64, size.y as f64);
	handle_cairo(cairo.fill());
}

#[inline]
pub fn handle_cairo<T>(result: Result<T, cairo::Error>)
{
	if let Err(err) = result {
		eprintln!("Failed cairo call: {}", err.to_string());
	}
}

#[inline(always)]
fn set_pango_font_size(font_size: i32, font_weight: u16, layout: &PangoContext)
{
	let mut description = FontDescription::new();
	description.set_size(font_size * PANGO_SCALE);
	description.set_weight(match font_weight {
		100 => pango::Weight::Thin,
		200 => pango::Weight::Light,
		300 => pango::Weight::Book,
		400 => pango::Weight::Normal,
		500 => pango::Weight::Medium,
		600 => pango::Weight::Semibold,
		700 => pango::Weight::Bold,
		800 => pango::Weight::Ultrabold,
		900 => pango::Weight::Heavy,
		_ => pango::Weight::Normal,
	});
	layout.set_font_description(Some(&description));
}

#[inline]
fn draw_char(cairo: &CairoContext, draw_data: &CharDrawData, position: &Pos2,
	color: &Color32, layout: &PangoContext)
{
	match draw_data {
		CharDrawData::Outline(data) => {
			data.draw(cairo, position.x, position.y, &color);
		}
		CharDrawData::Pango(data) => {
			data.draw(cairo, position.x, position.y, &color, layout);
		}
		CharDrawData::Space(_) => {}
	}
}
