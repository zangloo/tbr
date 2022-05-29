use eframe::egui::{Align2, FontFamily, FontId, Rect, Rounding, Stroke, Ui};
use eframe::emath::{Pos2, Vec2};
use eframe::epaint::Color32;
use crate::book::{Book, Line, StylePosition, TextStyle};
use crate::common::Position;
use crate::gui::Colors;
use crate::gui::render::han::GuiHanRender;
use crate::gui::render::xi::GuiXiRender;
use crate::ReadingInfo;

mod han;
mod xi;

pub(crate) struct DrawChar
{
	pub char: char,
	pub font_size: u8,
	pub color: Color32,
	pub background: Option<Color32>,
	pub style: Option<(TextStyle, StylePosition)>,

	pub line: usize,
	pub offset: usize,
	pub rect: Rect,
	pub draw_offset: Pos2,
}

pub(crate) struct DrawLine
{
	chars: Vec<DrawChar>,
	draw_size: f32,
	line_space: f32,
}

impl DrawLine
{
	fn new(draw_size: f32, line_space: f32) -> Self
	{
		DrawLine { chars: vec![], draw_size, line_space }
	}
}

pub(crate) struct DrawContext<'a>
{
	ui: &'a Ui,
	// draw rect
	rect: &'a Rect,
	colors: &'a Colors,
	// for calculate chars in single line
	max_page_size: f32,
	// current line base
	line_base: f32,
	// font size in configuration
	font_size: u8,
	// default single char size
	default_char_size: Vec2,
	leading_space: f32,
}

impl<'a> DrawContext<'a>
{
	pub fn new(ui: &'a Ui, rect: &'a Rect, colors: &'a Colors, max_page_size: f32, line_base: f32, font_size: u8, default_char_size: &Vec2, leading_space: f32) -> Self
	{
		DrawContext { ui, rect, colors, max_page_size, line_base, font_size, default_char_size: default_char_size.clone(), leading_space }
	}
}

pub(crate) trait GuiRender {
	fn create_draw_context<'a>(&self, ui: &'a Ui, rect: &'a Rect, colors: &'a Colors, font_size: u8, default_char_size: &Vec2) -> DrawContext<'a>;
	fn create_draw_line(&self, default_char_size: &Vec2) -> DrawLine;
	fn wrap_line(&self, text: &Line, line: usize, start_offset: usize, end_offset: usize, context: &mut DrawContext) -> Vec<DrawLine>;
	fn draw_style(&self, text: &Line, draw_text: &DrawLine, ui: &mut Ui);

	fn redraw(&self, book: &Box<dyn Book>, reading: &ReadingInfo, colors: &Colors, font_size: u8, default_char_size: &Vec2,
		rect: &Rect, draw_lines: &mut Vec<DrawLine>, ui: &Ui) -> Option<Position>
	{
		draw_lines.clear();
		let mut drawn_size = 0.0;
		let mut offset = reading.position;
		let mut draw_context = self.create_draw_context(ui, rect, colors, font_size, default_char_size);
		for (index, line) in book.lines()[reading.line..].iter().enumerate() {
			if line.is_image() {
				return if reading.line == index {
					let mut draw_line = self.create_draw_line(default_char_size);
					draw_line.chars.push(DrawChar {
						char: 'I',
						font_size: 100,
						color: colors.color,
						background: None,
						style: None,

						line: index,
						offset: 0,
						rect: rect.clone(),
						draw_offset: Pos2::ZERO,
					});
					draw_lines.push(draw_line);
					let next_line = index + 1;
					if next_line < book.lines().len() {
						Some(Position::new(next_line, 0))
					} else {
						None
					}
				} else {
					Some(Position::new(index, 0))
				};
			}
			let wrapped_lines = self.wrap_line(&line, index, offset, line.len(), &mut draw_context);
			offset = 0;
			for wrapped_line in wrapped_lines {
				drawn_size += wrapped_line.draw_size + wrapped_line.line_space;
				if drawn_size > draw_context.max_page_size {
					let next = if let Some(char) = wrapped_line.chars.first() {
						Some(Position::new(index, char.offset))
					} else {
						Some(Position::new(index, 0))
					};
					draw_lines.push(wrapped_line);
					return next;
				}
				draw_lines.push(wrapped_line);
			}
		}
		None
	}

	fn draw(&self, draw_lines: &Vec<DrawLine>, ui: &mut Ui)
	{
		for draw_line in draw_lines {
			for dc in &draw_line.chars {
				if let Some(bg) = dc.background {
					ui.painter().rect(dc.rect.clone(), Rounding::none(), bg, Stroke::default());
				}
				let draw_position = Pos2::new(dc.rect.min.x + dc.draw_offset.x, dc.rect.min.y + dc.draw_offset.y);
				paint_char(ui, dc.char, dc.font_size, &draw_position, Align2::LEFT_TOP, dc.color);
			}
		}
	}
}

pub(crate) fn measure_char_size(ui: &mut Ui, char: char, font_size: u8) -> Vec2 {
	let old_clip_rect = ui.clip_rect();
	ui.set_clip_rect(Rect::NOTHING);
	let rect = paint_char(ui, char, font_size, &Pos2::ZERO, Align2::LEFT_TOP, Color32::BLACK);
	ui.set_clip_rect(old_clip_rect);
	rect.size()
}

#[inline]
pub(crate) fn paint_char(ui: &Ui, char: char, font_size: u8, position: &Pos2, align: Align2, color: Color32) -> Rect {
	let rect = ui.painter().text(
		*position,
		align,
		char,
		FontId::new(font_size as f32, FontFamily::Proportional),
		color);
	rect
}

#[inline]
pub(crate) fn scale_font_size(font_size: u8, percent: u8) -> u8
{
	if percent == 100 {
		return font_size;
	} else {
		return font_size * percent / 100;
	}
}

pub(crate) fn create_render(render_type: &str) -> Box<dyn GuiRender>
{
	if render_type == "han" {
		Box::new(GuiHanRender::new())
	} else {
		Box::new(GuiXiRender::new())
	}
}
