use eframe::egui::{Rect, Ui, Vec2};

use crate::book::{Colors, Line};
use crate::gui::render::{DrawContext, DrawLine, GuiRender};

pub(crate) struct GuiXiRender {}

impl GuiXiRender
{
	pub fn new() -> Self
	{
		GuiXiRender {}
	}
}

impl GuiRender for GuiXiRender
{
	#[inline]
	fn create_draw_context<'a>(&self, ui: &'a Ui, rect: &'a Rect, colors: &'a Colors, font_size: u8, default_char_size: &Vec2) -> DrawContext<'a> {
		todo!()
	}

	#[inline]
	fn create_draw_line(&self, default_char_size: &Vec2) -> DrawLine {
		todo!()
	}

	fn wrap_line(&self, text: &Line, line: usize, start_offset: usize, end_offset: usize, context: &mut DrawContext) -> Vec<DrawLine> {
		todo!()
	}

	fn draw_style(&self, text: &Line, draw_text: &DrawLine, ui: &mut Ui) {
		todo!()
	}
}