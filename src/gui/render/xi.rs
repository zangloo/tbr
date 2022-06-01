use eframe::egui::{Ui, Vec2};

use crate::book::Line;
use crate::controller::{HighlightInfo, Render};
use crate::gui::render::{RenderContext, RenderLine, GuiRender};
use crate::Position;

pub(super) struct GuiXiRender {}

impl GuiXiRender
{
	pub fn new() -> Self
	{
		GuiXiRender {}
	}
}

impl Render<Ui> for GuiXiRender {
	fn redraw(&mut self, lines: &Vec<Line>, line: usize, offset: usize, highlight: &Option<HighlightInfo>, context: &mut Ui) -> Option<Position> {
		todo!()
	}

	fn prev(&mut self, lines: &Vec<Line>, line: usize, offset: usize, context: &mut Ui) -> Position {
		todo!()
	}

	fn next_line(&mut self, lines: &Vec<Line>, line: usize, offset: usize, context: &mut Ui) -> Position {
		todo!()
	}

	fn prev_line(&mut self, lines: &Vec<Line>, line: usize, offset: usize, context: &mut Ui) -> Position {
		todo!()
	}

	fn setup_highlight(&mut self, lines: &Vec<Line>, line: usize, start: usize, context: &mut Ui) -> Position {
		todo!()
	}
}

impl GuiRender for GuiXiRender
{
	#[inline]
	fn reset_render_context(&self, render_context: &mut RenderContext) {
		todo!()
	}

	#[inline]
	fn create_render_line(&self, default_char_size: &Vec2) -> RenderLine {
		todo!()
	}

	fn wrap_line(&self, text: &Line, line: usize, start_offset: usize, end_offset: usize, ui: &mut Ui, draw_context: &mut RenderContext) -> Vec<RenderLine>
	{
		todo!()
	}

	fn draw_style(&self, text: &Line, draw_text: &RenderLine, ui: &mut Ui) {
		todo!()
	}
}