use std::sync::{Arc, RwLock};
use eframe::egui::{Align2, FontFamily, FontId, Rect, Rounding, Stroke, Ui};
use eframe::emath::{Pos2, Vec2};
use eframe::epaint::Color32;

use crate::book::{Colors, Line, StylePosition, TextStyle};
use crate::common::Position;
use crate::controller::{HighlightInfo, Render};
use crate::gui::render::han::GuiHanRender;
use crate::gui::render::xi::GuiXiRender;
use crate::gui::render_context_id;

mod han;
mod xi;

pub(crate) struct RenderChar
{
	pub char: char,
	pub font_size: f32,
	pub color: Color32,
	pub background: Option<Color32>,
	pub style: Option<(TextStyle, StylePosition)>,

	pub line: usize,
	pub offset: usize,
	pub rect: Rect,
	pub draw_offset: Pos2,
}

pub(crate) struct RenderLine
{
	chars: Vec<RenderChar>,
	draw_size: f32,
	line_space: f32,
}

impl RenderLine
{
	fn new(draw_size: f32, line_space: f32) -> Self
	{
		RenderLine { chars: vec![], draw_size, line_space }
	}
}

pub(super) struct RenderContext
{
	// draw rect
	pub rect: Rect,
	pub colors: Colors,
	// font size in configuration
	pub font_size: u8,
	// default single char size
	pub default_font_measure: Vec2,

	pub leading_space: f32,
	// for calculate chars in single line
	pub max_page_size: f32,
	// current line base
	pub line_base: f32,
	pub render_lines: Vec<RenderLine>,
}

pub(super) trait GuiRender: Render<Ui> {
	// return (max_page_size, baseline, leading_space)
	fn reset_render_context(&self, render_context: &mut RenderContext);
	fn create_render_line(&self, default_font_measure: &Vec2) -> RenderLine;
	fn wrap_line(&self, text: &Line, line: usize, start_offset: usize, end_offset: usize, ui: &mut Ui, context: &mut RenderContext) -> Vec<RenderLine>;
	fn draw_style(&self, text: &Line, draw_text: &RenderLine, ui: &mut Ui);

	fn gui_redraw(&self, lines: &Vec<Line>, reading_line: usize, reading_offset: usize,
		highlight: &Option<HighlightInfo>, ui: &mut Ui) -> Option<Position>
	{
		let render_context: Arc<RwLock<RenderContext>> = ui.data().get_temp(render_context_id()).unwrap();
		let mut context = match render_context.write() {
			Ok(c) => c,
			Err(e) => panic!("{}", e.to_string()),
		};
		context.render_lines.clear();
		let mut drawn_size = 0.0;
		let mut offset = reading_offset;
		for (index, line) in lines[reading_line..].iter().enumerate() {
			if let Some((target, offset)) = line.with_image() {
				return if reading_line == index {
					let mut draw_line = self.create_render_line(&context.default_font_measure);
					draw_line.chars.push(RenderChar {
						char: 'I',
						font_size: 1.0,
						color: context.colors.color,
						background: None,
						style: Some((TextStyle::Image(target.to_string()), StylePosition::Single)),

						line: index,
						offset,
						rect: context.rect.clone(),
						draw_offset: Pos2::ZERO,
					});
					context.render_lines.push(draw_line);
					let next_line = index + 1;
					if next_line < lines.len() {
						Some(Position::new(next_line, 0))
					} else {
						None
					}
				} else {
					Some(Position::new(index, 0))
				};
			}
			let wrapped_lines = self.wrap_line(&line, index, offset, line.len(), ui, &mut context);
			offset = 0;
			for wrapped_line in wrapped_lines {
				drawn_size += wrapped_line.draw_size + wrapped_line.line_space;
				if drawn_size > context.max_page_size {
					let next = if let Some(char) = wrapped_line.chars.first() {
						Some(Position::new(index, char.offset))
					} else {
						Some(Position::new(index, 0))
					};
					context.render_lines.push(wrapped_line);
					return next;
				}
				context.render_lines.push(wrapped_line);
			}
		}
		self.draw(&context.render_lines, ui);
		None
	}

	fn draw(&self, render_lines: &Vec<RenderLine>, ui: &mut Ui)
	{
		for render_line in render_lines {
			for dc in &render_line.chars {
				if let Some(bg) = dc.background {
					ui.painter().rect(dc.rect.clone(), Rounding::none(), bg, Stroke::default());
				}
				let draw_position = Pos2::new(dc.rect.min.x + dc.draw_offset.x, dc.rect.min.y + dc.draw_offset.y);
				paint_char(ui, dc.char, dc.font_size, &draw_position, Align2::LEFT_TOP, dc.color);
			}
		}
	}
}

pub(crate) fn measure_char_size(ui: &mut Ui, char: char, font_size: f32) -> Vec2 {
	let old_clip_rect = ui.clip_rect();
	ui.set_clip_rect(Rect::NOTHING);
	let rect = paint_char(ui, char, font_size, &Pos2::ZERO, Align2::LEFT_TOP, Color32::BLACK);
	ui.set_clip_rect(old_clip_rect);
	rect.size()
}

#[inline]
pub(super) fn paint_char(ui: &Ui, char: char, font_size: f32, position: &Pos2, align: Align2, color: Color32) -> Rect {
	let rect = ui.painter().text(
		*position,
		align,
		char,
		FontId::new(font_size, FontFamily::Proportional),
		color);
	rect
}

#[inline]
pub(super) fn scale_font_size(font_size: u8, scale: f32) -> f32
{
	return font_size as f32 * scale;
}

pub(super) fn create_render(render_type: &str) -> Box<dyn GuiRender>
{
	if render_type == "han" {
		Box::new(GuiHanRender::new())
	} else {
		Box::new(GuiXiRender::new())
	}
}
