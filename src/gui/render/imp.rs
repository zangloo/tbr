use gtk4::cairo::{Context as CairoContext, LineJoin};
use gtk4::gdk_pixbuf::{Colorspace, InterpType, Pixbuf};
use gtk4::pango::ffi::PANGO_SCALE;
use gtk4::pango::{FontDescription, Layout as PangoContext};
use gtk4::prelude::GdkCairoContextExt;
use gtk4::{cairo, pango};
use indexmap::IndexSet;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use crate::book::{Book, CharStyle, Line};
use crate::color::{Color32, Colors};
use crate::common::{overlap_range, Position};
use crate::controller::{HighlightInfo, HighlightMode};
use crate::gui::font::{Fonts, HtmlFonts, UserFonts};
use crate::gui::load_image;
use crate::gui::math::{pos2, vec2, Pos2, Rect, Vec2};
use crate::html_parser;
use crate::html_parser::{BlockStyle, BorderLines, ElementSize, FontScale, FontWeight, ImageStyle, TextDecorationStyle};

pub const HAN_CHAR: char = '漢';

impl FontWeight {
	#[inline]
	pub fn gtk(&self) -> pango::Weight
	{
		match self.value() / 100 {
			1 => pango::Weight::Thin,
			2 => pango::Weight::Light,
			3 => pango::Weight::Book,
			4 => pango::Weight::Normal,
			5 => pango::Weight::Medium,
			6 => pango::Weight::Semibold,
			7 => pango::Weight::Bold,
			8 => pango::Weight::Ultrabold,
			9 => pango::Weight::Heavy,
			_ => pango::Weight::Normal,
		}
	}

	#[inline]
	pub fn outlined(&self) -> u16
	{
		self.value()
	}
}

#[derive(Clone, Debug)]
pub enum BlockStylePart {
	Begin,
	End,
	Middle,
	Single,
}

#[derive(Clone)]
pub enum TextDecoration {
	// rect, stroke width, is first, is last, color
	Border {
		rect: Rect,
		stroke_width: f32,
		start: bool,
		end: bool,
		color: Color32,
		lines: BorderLines,
	},
	// rect, stroke width, is first, is last, color
	BlockBorder {
		rect: Rect,
		stroke_width: f32,
		start: bool,
		end: bool,
		color: Color32,
		lines: BorderLines,
	},
	// start(x,y), length,stroke width, is first, color
	Line {
		start_points: Vec<Pos2>,
		style: TextDecorationStyle,
		length: f32,
		stroke_width: f32,
		color: Color32,
	},
}

#[derive(Clone, Debug)]
pub struct CharCell {
	pub char: char,
	pub font_size: f32,
	pub font_weight: FontWeight,
	pub font_family: Option<u16>,
	pub color: Color32,
	pub background: Option<Color32>,
	pub cell_offset: Vec2,
	pub cell_size: Vec2,
}

#[derive(Clone, Debug)]
pub enum RenderCell {
	Char(CharCell),
	Image(String, Option<usize>),
	/// usize for link_index
	Link(CharCell, usize),
}

#[derive(Clone, Debug)]
pub struct RenderChar {
	pub cell: RenderCell,
	pub offset: usize,
	pub rect: Rect,
	pub has_title: bool,
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
	pub fn char_at_index(&self, index: usize) -> &RenderChar
	{
		&self.chars[index]
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
	where
		F: Fn(usize, &RenderChar) -> Option<T>,
	{
		for (index, char) in self.chars.iter().enumerate() {
			let found = f(index, char);
			if found.is_some() {
				return found;
			}
		}
		None
	}

	#[inline]
	pub fn first_render_char(&self) -> Option<&RenderChar>
	{
		self.chars.first()
	}

	#[inline]
	pub fn last_render_char(&self) -> Option<&RenderChar>
	{
		self.chars.last()
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
	font_size: u8,
	font_weight: FontWeight,
	font_family: Option<u16>,
	size: Vec2,
	draw_offset: Pos2,
	draw_size: Vec2,
}

impl PangoDrawData {
	fn measure(char: char, font_size: f32, font_weight: &FontWeight,
		font_family_idx: &Option<u16>, font_family_names: Option<&IndexSet<String>>,
		layout: &PangoContext) -> Self
	{
		let text = char.to_string();
		let font_size = font_size as u8;
		let font_family_names = get_font_family_names(font_family_idx, font_family_names);
		set_pango_font_size(font_size, &font_weight, font_family_names, layout);
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
			font_size,
			font_weight: font_weight.clone(),
			font_family: font_family_idx.clone(),
			size,
			draw_offset,
			draw_size,
		}
	}

	fn draw(&self, cairo: &CairoContext, offset_x: f32, offset_y: f32, color: &Color32,
		font_family_names: Option<&IndexSet<String>>, layout: &PangoContext)
	{
		let font_family_names = get_font_family_names(&self.font_family, font_family_names);
		set_pango_font_size(self.font_size, &self.font_weight, font_family_names, layout);
		layout.set_text(&self.char);

		let x_offset = offset_x as f64;
		let y_offset = offset_y as f64;
		color.apply(cairo);
		cairo.move_to(x_offset, y_offset);
		pangocairo::functions::show_layout(cairo, &layout);
	}
}

pub struct OutlineDrawData {
	points: Vec<u8>,
	size: Vec2,
	draw_offset: Pos2,
	draw_size: Vec2,
}

impl OutlineDrawData {
	fn measure(char: char, font_size: f32, font_weight: &FontWeight,
		font_family_idx: &Option<u16>, font_family_names: Option<&IndexSet<String>>,
		fonts: Option<&impl Fonts>) -> Option<Self>
	{
		if let Some(fonts) = fonts {
			let font_family_names = get_font_family_names(font_family_idx, font_family_names);
			if let Some((outline, rect)) = fonts.query(char, font_size, font_weight, font_family_names) {
				let mut points = vec![];
				outline.draw(|_, _, a| {
					points.push((a * 255.) as u8);
				});
				let bounds = outline.px_bounds();
				let draw_size = vec2(bounds.width(), bounds.height());
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
		None
	}

	fn draw(&self, cairo: &CairoContext, offset_x: f32, offset_y: f32, color: &Color32)
	{
		let width = self.draw_size.x as usize;
		let height = self.draw_size.y as usize;
		if let Some(pixbuf) = Pixbuf::new(Colorspace::Rgb, true, 8,
			width as i32, height as i32) {
			let r = color.r();
			let g = color.g();
			let b = color.b();
			for y in 0..height {
				for x in 0..width {
					pixbuf.put_pixel(x as u32, y as u32, r, g, b, self.points[y * width + x]);
				}
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
	pub fonts: Rc<Option<UserFonts>>,

	// font size in configuration
	pub font_size: u8,
	// default single char size
	pub default_font_measure: Vec2,

	// use book custom color
	pub custom_color: bool,
	// use book custom font
	pub custom_font: bool,
	// strip empty lines
	pub strip_empty_lines: bool,

	pub render_rect: Rect,
	pub leading_chars: usize,
	pub leading_space: f32,
	// for calculate chars in single line
	pub max_page_size: f32,

	// method for redraw with scrolling
	pub scroll_redraw_method: ScrollRedrawMethod,

	// ignore font weight
	pub ignore_font_weight: bool,
}

impl RenderContext {
	pub fn new(colors: Colors, font_size: u8, custom_color: bool, custom_font: bool,
		leading_chars: usize, strip_empty_lines: bool, ignore_font_weight: bool)
		-> Self
	{
		RenderContext {
			colors,
			fonts: Rc::new(None),
			font_size,
			default_font_measure: Pos2::ZERO,
			custom_color,
			custom_font,
			strip_empty_lines,
			ignore_font_weight,
			render_rect: Rect::NOTHING,
			leading_chars,
			leading_space: 0.0,
			max_page_size: 0.0,
			scroll_redraw_method: ScrollRedrawMethod::NoResetScroll,
		}
	}

	#[inline]
	pub fn x_padding(&self) -> f32
	{
		self.default_font_measure.x / 2.
	}

	#[inline]
	pub fn y_padding(&self) -> f32
	{
		self.default_font_measure.y / 2.
	}

	#[inline]
	pub fn update_render_rect(&mut self, width: f32, height: f32)
	{
		self.render_rect = Rect::new(
			self.x_padding(),
			self.y_padding(),
			width - self.default_font_measure.x,
			height - self.default_font_measure.y);
	}
}

pub struct ImageDrawingData {
	orig_width: i32,
	orig_height: i32,
	texture: Pixbuf,
}

impl ImageDrawingData {
	/// image render size
	#[inline]
	pub fn match_size(&self, width: i32, height: i32) -> bool
	{
		self.texture.width() == width &&
			self.texture.height() == height
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

pub struct CharMeasures {
	pub size: Vec2,
	pub draw_size: Vec2,
	pub draw_offset: Pos2,
	pub font_size: f32,
	pub font_weight: FontWeight,
	pub font_family_idx: Option<u16>,
}

#[inline(always)]
fn cache_key(char: char, font_size: u8, font_weight: u8, font_family_idx: &Option<u16>) -> u64
{
	(char as u64) << 32
		| (font_size as u64) << 24
		| (font_weight as u64) << 16
		| font_family_idx.unwrap_or(0xffff) as u64
}

pub struct RedrawContext<'a> {
	offset: usize,
	block_styles: Option<&'a Vec<BlockStyle>>,
	render_lines: Vec<RenderLine>,
	block_backgrounds: Vec<BlockBackgroundEntry>,
	block_borders: Vec<TextDecoration>,
	current_block_background: Option<(usize, Color32)>,
	current_block_border: Option<(usize, BorderLines, &'a Option<Color32>, BlockStylePart)>,
	render_line_start: usize,
	complete_with_overflow: bool,
}

impl<'a> RedrawContext<'a> {
	fn from(offset: usize, block_styles: Option<&'a Vec<BlockStyle>>) -> Self
	{
		RedrawContext {
			offset,
			block_styles,
			render_lines: vec![],
			block_backgrounds: vec![],
			block_borders: vec![],
			current_block_background: None,
			current_block_border: None,
			render_line_start: 0,
			complete_with_overflow: false,
		}
	}
}

pub struct BlockBackgroundEntry {
	rect: Rect,
	color: Color32,
}

impl BlockBackgroundEntry {
	fn new(rect: Rect, color: Color32) -> Self
	{
		Self { rect, color }
	}
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
	fn default_line_size(&self, render_context: &RenderContext) -> f32;
	fn calc_block_rect(&self, render_lines: &Vec<RenderLine>,
		range: Range<usize>, render_in_single_line: bool,
		context: &RenderContext) -> Rect;

	fn setup_decoration(&self, decoration: &html_parser::TextDecoration,
		decoration_chars_range: Range<usize>, start: bool, end: bool,
		render_line: &mut RenderLine, context: &RenderContext);
	fn setup_border(&self, render_line: &mut RenderLine, lines: BorderLines,
		decoration_chars_range: Range<usize>, start: bool, end: bool,
		color: Color32);

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
	fn cache_get(&self, char: char, font_size: f32, font_weight: &FontWeight, font_family_idx: &Option<u16>) -> Option<&CharDrawData>
	{
		let key = cache_key(char, font_size as u8, font_weight.key(), font_family_idx);
		self.cache().get(&key)
	}
	#[inline]
	fn cache_insert(&mut self, char: char, font_size: f32, font_weight: &FontWeight,
		font_family_idx: &Option<u16>, data: CharDrawData)
	{
		let key = cache_key(char, font_size as u8, font_weight.key(), font_family_idx);
		self.cache_mut().insert(key, data);
	}
	fn clear_cache_with_family(&mut self)
	{
		self.cache_mut().retain(|k, _| k & 0xffff == 0xffff);
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

	#[inline]
	fn calc_block_border_decoration(&self, render_lines: &Vec<RenderLine>,
		range: Range<usize>, border_lines: &BorderLines, color: &Option<Color32>,
		position: BlockStylePart, context: &RenderContext) -> TextDecoration
	{
		let render_in_single_line = matches!(position, BlockStylePart::Single) && (range.end - range.start) == 1;
		let rect = self.calc_block_rect(render_lines, range, render_in_single_line, context);
		let color = if let Some(color) = color {
			color.clone()
		} else {
			context.colors.color.clone()
		};
		let stroke_width = self.default_line_size(context) / 16.;
		let (start, end) = match position {
			BlockStylePart::Begin => (true, false),
			BlockStylePart::End => (false, true),
			BlockStylePart::Middle => (false, false),
			BlockStylePart::Single => (true, true),
		};
		TextDecoration::BlockBorder {
			rect,
			stroke_width,
			start,
			end,
			color,
			lines: border_lines.clone(),
		}
	}

	fn setup_decorations(&self, line: &Line, render_line: &mut RenderLine,
		context: &RenderContext)
	{
		let render_start = render_line.first_offset();
		let render_end = render_line.last_offset() + 1;
		let render_range = render_start..render_end;
		line.decoration_iter(render_line,
			|range, decoration, render_line|
				if let Some((render_range, start, end)) = make_render_range(range, &render_range) {
					self.setup_decoration(
						decoration,
						render_range,
						start,
						end,
						render_line,
						context);
				},
			|range, lines, color, render_line|
				if let Some((render_range, start, end)) = make_render_range(range, &render_range) {
					self.setup_border(
						render_line,
						lines.clone(),
						render_range,
						start,
						end,
						color.unwrap_or_else(|| context.colors.color.clone()));
				});
	}

	fn setup_line_blocks(&self, rc: &mut RedrawContext, line_idx: usize,
		overflow: bool, render_context: &RenderContext)
	{
		let render_line_count = rc.render_lines.len();
		if rc.render_line_start == render_line_count {
			return;
		}
		let block_styles = match rc.block_styles {
			Some(bs) => bs,
			None => return,
		};
		let mut border_found = false;
		let mut background_found = false;
		for bs in block_styles {
			match bs {
				BlockStyle::Border { range, lines: border_lines, color } => if !border_found && range.contains(&line_idx) {
					border_found = true;
					let end_idx = range.end - 1;
					if line_idx == range.start {
						if line_idx == end_idx {
							// single line block
							let part = if rc.offset == 0 {
								if overflow {
									BlockStylePart::Begin
								} else {
									BlockStylePart::Single
								}
							} else {
								if overflow {
									BlockStylePart::Middle
								} else {
									BlockStylePart::End
								}
							};
							let border = self.calc_block_border_decoration(
								&rc.render_lines,
								rc.render_line_start..render_line_count,
								border_lines,
								color,
								part,
								render_context);
							rc.block_borders.push(border);
						} else {
							rc.current_block_border = Some((
								rc.render_line_start,
								border_lines.clone(),
								color,
								if rc.offset == 0 { BlockStylePart::Begin } else { BlockStylePart::Middle }));
						}
					} else if line_idx == end_idx {
						let (start, part) = if let Some((start, .., part)) = &rc.current_block_border {
							let target_part = match part {
								BlockStylePart::Begin => if overflow {
									BlockStylePart::Begin
								} else {
									BlockStylePart::Single
								},
								BlockStylePart::End => panic!("End part of block style should not exists in scan process."),
								BlockStylePart::Middle => if overflow {
									BlockStylePart::Middle
								} else {
									BlockStylePart::End
								}
								BlockStylePart::Single => panic!("Single part of block style should not exists in scan process."),
							};
							(*start, target_part)
						} else if overflow {
							(rc.render_line_start, BlockStylePart::Middle)
						} else {
							(rc.render_line_start, BlockStylePart::End)
						};
						let border = self.calc_block_border_decoration(
							&rc.render_lines,
							start..render_line_count,
							border_lines,
							color,
							part,
							render_context);
						rc.block_borders.push(border);
						rc.current_block_border = None;
					} else if rc.current_block_border.is_none() {
						rc.current_block_border = Some((
							rc.render_line_start,
							border_lines.clone(),
							color,
							BlockStylePart::Middle));
					}
				}
				BlockStyle::Background { range, color } => if !background_found && range.contains(&line_idx) {
					background_found = true;
					let end_idx = range.end - 1;
					if line_idx == range.start {
						if line_idx == end_idx {
							// single line block
							let render_in_single_line = !overflow && rc.offset == 0 && render_line_count - rc.render_line_start == 1;
							let rect = self.calc_block_rect(
								&rc.render_lines,
								rc.render_line_start..render_line_count,
								render_in_single_line,
								render_context);
							rc.block_backgrounds.push(BlockBackgroundEntry::new(rect, color.clone()));
						} else {
							rc.current_block_background = Some((
								rc.render_line_start,
								color.clone()));
						}
					} else if line_idx == end_idx {
						let start = if let Some((start, _)) = &rc.current_block_background {
							*start
						} else {
							rc.render_line_start
						};
						let rect = self.calc_block_rect(
							&rc.render_lines,
							start..render_line_count,
							false,
							render_context);
						rc.block_backgrounds.push(BlockBackgroundEntry::new(rect, color.clone()));
						rc.current_block_background = None;
					} else if rc.current_block_background.is_none() {
						rc.current_block_background = Some((
							rc.render_line_start,
							color.clone()));
					}
				}
			}
		}
		rc.render_line_start = render_line_count;
	}

	fn finalize_blocks(&self, rc: &mut RedrawContext, render_context: &RenderContext)
	{
		if let Some((start, border_lines, color, part)) = &rc.current_block_border {
			let border = self.calc_block_border_decoration(
				&rc.render_lines,
				*start..rc.render_lines.len(),
				border_lines,
				color,
				part.clone(),
				render_context);
			rc.block_borders.push(border);
			rc.current_block_border = None;
		}
		if let Some((start, color)) = &rc.current_block_background {
			let render_in_single_line = rc.offset == 0 && !rc.complete_with_overflow && rc.render_lines.len() - rc.render_line_start == 1;
			let rect = self.calc_block_rect(
				&rc.render_lines,
				*start..rc.render_lines.len(),
				render_in_single_line,
				render_context);
			rc.block_backgrounds.push(BlockBackgroundEntry::new(
				rect, color.clone()));
		}
		rc.current_block_background = None;
	}

	fn gui_redraw(&mut self, book: &dyn Book, lines: &[Line],
		reading_line: usize, reading_offset: usize,
		highlight: &Option<HighlightInfo>, pango: &PangoContext,
		context: &mut RenderContext)
		-> (Vec<RenderLine>, Vec<TextDecoration>, Vec<BlockBackgroundEntry>,
			Option<Position>)
	{
		let mut rc = RedrawContext::from(reading_offset, book.block_styles());
		self.reset_baseline(context);

		let mut drawn_size = 0.0;
		let mut next = None;
		'Done:
		for index in reading_line..lines.len() {
			let line = &lines[index];
			let wrapped_lines = self.try_wrap_line(book, &line, index, rc.offset, line.len(), highlight, pango, context);
			for wrapped_line in wrapped_lines {
				drawn_size += wrapped_line.line_size;
				if drawn_size > context.max_page_size {
					next = if let Some(char) = wrapped_line.chars.first() {
						Some(Position::new(index, char.offset))
					} else {
						Some(Position::new(index, 0))
					};
					self.setup_line_blocks(&mut rc, index, true, context);
					rc.complete_with_overflow = true;
					break 'Done;
				}
				drawn_size += wrapped_line.line_space;
				rc.render_lines.push(wrapped_line);
			}
			self.setup_line_blocks(&mut rc, index, false, context);
			rc.offset = 0;
		}
		self.finalize_blocks(&mut rc, context);
		(rc.render_lines, rc.block_borders, rc.block_backgrounds, next)
	}

	fn draw(&self, render_lines: &[RenderLine],
		block_borders: &[TextDecoration],
		block_backgrounds: &[BlockBackgroundEntry],
		font_family_names: &Option<IndexSet<String>>,
		cairo: &CairoContext, layout: &PangoContext)
	{
		cairo.set_line_width(1.0);
		for bg in block_backgrounds {
			draw_rect(cairo, &bg.rect, 1.0, &bg.color);
		}
		for border in block_borders {
			self.draw_decoration(border, cairo);
		}
		for render_line in render_lines {
			for dc in &render_line.chars {
				match &dc.cell {
					RenderCell::Image(name, _) => {
						self.draw_image(name, &dc.rect, cairo);
					}
					RenderCell::Char(cell)
					| RenderCell::Link(cell, _) => {
						if let Some(bg) = &cell.background {
							draw_rect(cairo, &dc.rect, 1.0, bg);
						}
						let draw_position = Pos2::new(dc.rect.min.x + cell.cell_offset.x, dc.rect.min.y + cell.cell_offset.y);
						// should always exists
						if let Some(draw_data) = self.cache_get(cell.char, cell.font_size, &cell.font_weight, &cell.font_family) {
							draw_char(
								cairo,
								draw_data,
								&draw_position,
								&cell.color,
								font_family_names,
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
		view_size: &Vec2, font_size: u8) -> Option<(String, Pos2)>
	{
		if let Some(image) = &char_style.image {
			if let Some(data) = book.image(image.href()) {
				let cache = self.image_cache_mut();
				let size = match cache.entry(data.path_dup()) {
					Entry::Occupied(o) => {
						let image_data = o.into_mut();
						let (render_size, _) = calc_image_size(
							image,
							&char_style.font_scale,
							font_size,
							image_data.orig_width,
							image_data.orig_height,
							view_size);
						if image_data.match_size(render_size.x as i32, render_size.y as i32) {
							Some(render_size)
						} else {
							// font size changed
							if let Some((data, render_size)) = load_image_and_resize(image, &char_style.font_scale, font_size, view_size, data.bytes()) {
								*image_data = data;
								Some(render_size)
							} else {
								None
							}
						}
					}
					Entry::Vacant(v) =>
						if let Some((data, render_size)) = load_image_and_resize(image, &char_style.font_scale, font_size, view_size, data.bytes()) {
							v.insert(data);
							Some(render_size)
						} else {
							return None;
						}
				}?;
				Some((data.path(), size))
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

	fn apply_font_modified(&mut self, book_fonts: Option<&HtmlFonts>,
		pango: &PangoContext, render_context: &mut RenderContext)
	{
		self.cache_mut().clear();
		let measures = self.get_char_measures(
			pango,
			HAN_CHAR,
			&FontScale::DEFAULT,
			&FontWeight::NORMAL,
			&None,
			None,
			book_fonts,
			render_context);
		render_context.default_font_measure = measures.size;
	}

	fn get_char_measures(&mut self, layout: &PangoContext, char: char,
		font_scale: &FontScale, font_weight: &FontWeight,
		mut font_family_idx: &Option<u16>, font_family_names: Option<&IndexSet<String>>,
		book_fonts: Option<&HtmlFonts>, render_context: &mut RenderContext) -> CharMeasures
	{
		const SPACE: char = ' ';
		const FULL_SPACE: char = '　';

		let font_size = scale_font_size(render_context.font_size, &font_scale);
		let font_weight = load_font_weight(&font_weight, render_context);
		let render_fonts = if render_context.custom_font {
			book_fonts
		} else {
			font_family_idx = &None;
			None
		};

		if let Some(data) = self.cache_get(char, font_size, &font_weight, font_family_idx) {
			return CharMeasures {
				size: data.size(),
				draw_size: data.draw_size(),
				draw_offset: data.offset(),
				font_size,
				font_weight: font_weight.clone(),
				font_family_idx: font_family_idx.clone(),
			};
		}
		match char {
			SPACE => {
				let measures = self.measure_char(
					layout, 'S', font_size, font_weight, font_family_idx,
					font_family_names, render_fonts, &render_context.fonts);
				self.cache_insert(SPACE, font_size, &font_weight, font_family_idx, CharDrawData::Space(measures.size));
				measures
			}
			FULL_SPACE => {
				let measures = self.measure_char(
					layout, HAN_CHAR, font_size, font_weight, font_family_idx,
					font_family_names, render_fonts, &render_context.fonts);
				self.cache_insert(FULL_SPACE, font_size, &font_weight, font_family_idx, CharDrawData::Space(measures.size));
				measures
			}
			_ => self.measure_char(
				layout,
				char,
				font_size,
				font_weight,
				font_family_idx,
				font_family_names,
				render_fonts,
				&render_context.fonts)
		}
	}

	fn measure_char(&mut self, layout: &PangoContext, char: char, font_size: f32,
		font_weight: &FontWeight, font_family_idx: &Option<u16>,
		font_family_names: Option<&IndexSet<String>>,
		book_fonts: Option<&HtmlFonts>, fonts: &Option<UserFonts>)
		-> CharMeasures
	{
		if let Some(draw_data) = OutlineDrawData::measure(
			char,
			font_size,
			font_weight,
			font_family_idx,
			font_family_names,
			book_fonts) {
			let measures = CharMeasures {
				size: draw_data.size,
				draw_size: draw_data.draw_size,
				draw_offset: draw_data.draw_offset,
				font_size,
				font_weight: font_weight.clone(),
				font_family_idx: font_family_idx.clone(),
			};
			self.cache_insert(char, font_size, &font_weight, font_family_idx, CharDrawData::Outline(draw_data));
			measures
		} else if let Some(draw_data) = OutlineDrawData::measure(
			char,
			font_size,
			font_weight,
			font_family_idx,
			font_family_names,
			fonts.as_ref()) {
			let measures = CharMeasures {
				size: draw_data.size,
				draw_size: draw_data.draw_size,
				draw_offset: draw_data.draw_offset,
				font_size,
				font_weight: font_weight.clone(),
				font_family_idx: font_family_idx.clone(),
			};
			self.cache_insert(char, font_size, &font_weight, font_family_idx, CharDrawData::Outline(draw_data));
			measures
		} else {
			let draw_data = PangoDrawData::measure(
				char,
				font_size,
				font_weight,
				font_family_idx,
				font_family_names,
				layout);
			let measures = CharMeasures {
				size: draw_data.size,
				draw_size: draw_data.draw_size,
				draw_offset: draw_data.draw_offset,
				font_size,
				font_weight: font_weight.clone(),
				font_family_idx: font_family_idx.clone(),
			};
			self.cache_insert(char, font_size, &font_weight, font_family_idx, CharDrawData::Pango(draw_data));
			measures
		}
	}
}

/// calc image render size
/// return
/// 1: render size
/// 2: raise needed
fn calc_image_size(image_style: &ImageStyle, font_scale: &FontScale,
	font_size: u8, orig_width: i32, orig_height: i32, view_size: &Vec2)
	-> (Vec2, bool)
{
	#[inline]
	fn calc_length(orig_length: i32, custom_length: &Option<ElementSize>,
		font_scale: &FontScale, font_size: f32) -> (f32, bool)
	{
		if let Some(length) = custom_length {
			let px = length.to_px(font_scale, font_size);
			(px, true)
		} else {
			(orig_length as f32, false)
		}
	}
	let width = view_size.x;
	let height = view_size.y;
	let font_size = font_size as f32;
	let (image_width, relative_with_font_size_w) = calc_length(
		orig_width,
		&image_style.width,
		font_scale,
		font_size);
	let (image_height, relative_with_font_size_h) = calc_length(
		orig_height,
		&image_style.height,
		font_scale,
		font_size);
	let relative_with_font_size = relative_with_font_size_w | relative_with_font_size_h;
	if image_width > width || image_height > height {
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
		(Vec2 { x: draw_width, y: draw_height }, true)
	} else if relative_with_font_size {
		(Vec2 { x: image_width, y: image_height }, true)
	} else {
		(Vec2 { x: image_width, y: image_height }, false)
	}
}

fn load_image_and_resize(image_style: &ImageStyle, font_scale: &FontScale,
	font_size: u8, view_size: &Vec2, bytes: &[u8])
	-> Option<(ImageDrawingData, Vec2)>
{
	let mut image = load_image(bytes)?;
	let orig_width = image.width();
	let orig_height = image.height();
	let (render_size, resize) = calc_image_size(
		image_style,
		font_scale,
		font_size,
		orig_width,
		orig_height,
		view_size);
	if resize {
		image = image.scale_simple(render_size.x as i32, render_size.y as i32, InterpType::Nearest)?
	}
	Some((
		ImageDrawingData {
			orig_width,
			orig_height,
			texture: image,
		},
		render_size,
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
fn scale_font_size(font_size: u8, scale: &FontScale) -> f32
{
	let scaled_size = scale.scale(font_size as f32);
	if scaled_size < 9.0 {
		9.0
	} else {
		scaled_size
	}
}

#[inline]
fn load_font_weight<'a>(font_weight: &'a FontWeight, render_context: &RenderContext) -> &'a FontWeight
{
	if render_context.ignore_font_weight {
		&FontWeight::NORMAL
	} else {
		font_weight
	}
}

#[inline]
fn draw_double_line<D>(cairo: &CairoContext, stroke_width: f32,
	middle: f64, draw: D)
where
	D: Fn(&CairoContext, f64),
{
	let width = stroke_width as f64 / 2.;
	let delta = width / 0.75;
	cairo.set_line_width(width);

	draw(cairo, middle - delta);
	draw(cairo, middle + delta);
}

#[inline]
fn draw_dotted_line<D>(cairo: &CairoContext, stroke_width: f32,
	mut start: f64, stop: f64, draw: D)
where
	D: Fn(&CairoContext, f64, f64),
{
	let size = stroke_width as f64;
	let step = size * 2.;
	cairo.set_line_width(size);
	while start < stop {
		draw(cairo, start, start + size);
		start += step;
	}
}

#[inline]
fn draw_dashed_line<F>(cairo: &CairoContext, stroke_width: f32,
	mut start: f64, stop: f64, draw: F)
where
	F: Fn(&CairoContext, f64, f64),
{
	let width = stroke_width as f64;
	let line_size = width * 4.;
	let step_size = width * 5.;
	cairo.set_line_width(width);
	loop {
		let mut end = start + line_size;
		if end > stop {
			end = stop;
		}
		draw(cairo, start, end);

		start += step_size;
		if start >= stop {
			break;
		}
	}
}

#[inline]
fn draw_wavy_line<I, D>(cairo: &CairoContext, stroke_width: f32,
	wave_start: f64, mut line_start: f64, line_stop: f64,
	init: I, draw: D)
where
	I: Fn(&CairoContext, f64, f64),
	D: Fn(&CairoContext, f64, f64),
{
	let width = stroke_width as f64;
	let size = width * 2.;
	cairo.set_line_width(width / 2.);
	cairo.set_line_join(LineJoin::Round);
	init(cairo, wave_start - width, line_start);
	let mut up = true;
	loop {
		let wave_pos = if up {
			up = false;
			size
		} else {
			up = true;
			-size
		};
		if line_start + size > line_stop {
			break;
		} else {
			line_start += size;
		}
		draw(cairo, wave_pos, size);
	}
}

#[inline]
pub fn vline(cairo: &CairoContext, x: f32, top: f32, bottom: f32,
	style: TextDecorationStyle, stroke_width: f32, color: &Color32)
{
	let x = x as f64;
	let top = top as f64;
	let bottom = bottom as f64;
	color.apply(cairo);
	match style {
		TextDecorationStyle::Solid => {
			cairo.move_to(x, top);
			cairo.set_line_width(stroke_width as f64);
			cairo.line_to(x, bottom);
		}
		TextDecorationStyle::Double =>
			draw_double_line(cairo, stroke_width, x, |cairo, x| {
				cairo.move_to(x, top);
				cairo.line_to(x, bottom);
			}),
		TextDecorationStyle::Dotted => {
			cairo.move_to(x, top);
			draw_dotted_line(cairo, stroke_width, top, bottom, |cairo, start, end| {
				cairo.move_to(x, start);
				cairo.line_to(x, end);
			});
		}
		TextDecorationStyle::Dashed =>
			draw_dashed_line(cairo, stroke_width, top, bottom, |cairo, start, end| {
				cairo.move_to(x, start);
				cairo.line_to(x, end);
			}),
		TextDecorationStyle::Wavy =>
			draw_wavy_line(cairo, stroke_width, x, top, bottom,
				|cairo, x, y| cairo.move_to(x, y),
				|cairo, x, y| cairo.rel_line_to(x, y)),
	}
	handle_cairo(cairo.stroke());
}

#[inline]
pub fn hline(cairo: &CairoContext, left: f32, right: f32, y: f32,
	style: TextDecorationStyle, stroke_width: f32, color: &Color32)
{
	let y = y as f64;
	let left = left as f64;
	let right = right as f64;
	color.apply(cairo);
	match style {
		TextDecorationStyle::Solid => {
			cairo.set_line_width(stroke_width as f64);
			cairo.move_to(left, y);
			cairo.line_to(right, y);
		}
		TextDecorationStyle::Double =>
			draw_double_line(cairo, stroke_width, y, |cairo, y| {
				cairo.move_to(left, y);
				cairo.line_to(right, y);
			}),
		TextDecorationStyle::Dotted => {
			cairo.move_to(left, y);
			draw_dotted_line(cairo, stroke_width, left, right, |cairo, start, end| {
				cairo.move_to(start, y);
				cairo.line_to(end, y);
			});
		}
		TextDecorationStyle::Dashed =>
			draw_dashed_line(cairo, stroke_width, left, right, |cairo, start, end| {
				cairo.move_to(start, y);
				cairo.line_to(end, y);
			}),
		TextDecorationStyle::Wavy =>
			draw_wavy_line(cairo, stroke_width, y, left, right,
				|cairo, y, x| cairo.move_to(x, y),
				|cairo, y, x| cairo.rel_line_to(x, y)),
	}
	handle_cairo(cairo.stroke());
}

#[inline]
pub fn draw_border(cairo: &CairoContext, stroke_width: f32, color: &Color32,
	left: f32, right: f32, top: f32, bottom: f32,
	draw_left: bool, draw_right: bool, draw_top: bool, draw_bottom: bool) {
	if draw_left {
		vline(cairo, left, top, bottom, TextDecorationStyle::Solid, stroke_width, &color);
	}
	if draw_right {
		vline(cairo, right, top, bottom, TextDecorationStyle::Solid, stroke_width, &color);
	}
	if draw_top {
		hline(cairo, left, right, top, TextDecorationStyle::Solid, stroke_width, &color);
	}
	if draw_bottom {
		hline(cairo, left, right, bottom, TextDecorationStyle::Solid, stroke_width, &color);
	}
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
fn set_pango_font_size(font_size: u8, font_weight: &FontWeight,
	font_families: Option<&str>, layout: &PangoContext)
{
	let mut description = FontDescription::new();
	description.set_size(font_size as i32 * PANGO_SCALE);
	description.set_weight(font_weight.gtk());
	if let Some(font_families) = font_families {
		description.set_family(font_families);
	}
	layout.set_font_description(Some(&description));
}

#[inline]
fn draw_char(cairo: &CairoContext, draw_data: &CharDrawData, position: &Pos2,
	color: &Color32, font_family_names: &Option<IndexSet<String>>,
	layout: &PangoContext)
{
	match draw_data {
		CharDrawData::Outline(data) => {
			data.draw(cairo, position.x, position.y, &color);
		}
		CharDrawData::Pango(data) => {
			data.draw(cairo, position.x, position.y, &color,
				font_family_names.as_ref(), layout);
		}
		CharDrawData::Space(_) => {}
	}
}

#[inline]
fn get_font_family_names<'a>(font_family_idx: &Option<u16>,
	font_family_names: Option<&'a IndexSet<String>>) -> Option<&'a str>
{
	if let Some(idx) = font_family_idx {
		if let Some(names) = font_family_names {
			names.get_index(*idx as usize)
				.map_or(None, |str| Some(str))
		} else { None }
	} else { None }
}

#[inline]
pub fn make_render_range(decoration_range: &Range<usize>, render_range: &Range<usize>)
	-> Option<(Range<usize>, bool, bool)>
{
	let range = overlap_range(decoration_range, render_range)?;
	let start = render_range.start;
	let render_chars_range = range.start - start..range.end - start;
	if render_chars_range.is_empty() {
		None
	} else {
		Some((
			render_chars_range,
			range.start == decoration_range.start,
			range.end == decoration_range.end,
		))
	}
}
