use egui::{Pos2, Rect, Ui};
use crate::book::{Book, Line};
use crate::common::Position;
use crate::controller::{HighlightInfo, Render};
use crate::gui::render::{create_render, GuiRender, PointerPosition, RenderLine};

const MIN_TEXT_SELECT_DISTANCE: f32 = 4.0;

pub(super) struct GuiView {
	pub render: Box<dyn GuiRender>,
	pub render_lines: Vec<RenderLine>,
}

impl Render<Ui> for GuiView {
	#[inline]
	fn redraw(&mut self, book: &Box<dyn Book>, lines: &Vec<Line>, line: usize,
		offset: usize, highlight: &Option<HighlightInfo>, ui: &mut Ui)
		-> Option<Position>
	{
		self.render_lines.clear();
		self.render.gui_redraw(book, lines, line, offset, highlight, ui, &mut self.render_lines)
	}

	#[inline]
	fn prev_page(&mut self, book: &Box<dyn Book>, lines: &Vec<Line>,
		line: usize, offset: usize, ui: &mut Ui) -> Position
	{
		self.render.gui_prev_page(book, lines, line, offset, ui)
	}

	#[inline]
	fn next_line(&mut self, book: &Box<dyn Book>, lines: &Vec<Line>,
		line: usize, offset: usize, ui: &mut Ui) -> Position
	{
		self.render.gui_next_line(book, lines, line, offset, ui)
	}

	#[inline]
	fn prev_line(&mut self, book: &Box<dyn Book>, lines: &Vec<Line>,
		line: usize, offset: usize, ui: &mut Ui) -> Position
	{
		self.render.gui_prev_line(book, lines, line, offset, ui)
	}

	#[inline]
	fn setup_highlight(&mut self, book: &Box<dyn Book>, lines: &Vec<Line>,
		line: usize, start: usize, ui: &mut Ui) -> Position
	{
		self.render.gui_setup_highlight(book, lines, line, start, ui)
	}
}

impl GuiView {
	#[inline]
	pub fn new(render_type: &str) -> Self
	{
		let render = create_render(render_type);
		GuiView {
			render,
			render_lines: vec![],
		}
	}

	#[inline]
	pub fn reload_render(&mut self, render_type: &str)
	{
		self.render = create_render(render_type);
	}

	#[inline]
	pub fn draw(&mut self, ui: &mut Ui)
	{
		self.render.draw(&self.render_lines, ui);
	}

	pub fn calc_selection(&self, original_pos: Pos2, current_pos: Pos2, rect: &Rect)
		-> Option<(Position, Position)>
	{
		#[inline]
		fn offset_index(line: &RenderLine, offset: &PointerPosition) -> usize {
			match offset {
				PointerPosition::Head => line.chars.first().map_or(0, |dc| dc.offset),
				PointerPosition::Exact(offset) => line.chars[*offset].offset,
				PointerPosition::Tail => line.chars.last().map_or(0, |dc| dc.offset),
			}
		}
		fn select_all(lines: &Vec<RenderLine>) -> (Position, Position)
		{
			let render_line = lines.first().unwrap();
			let from = Position::new(
				render_line.line,
				render_line.chars.first().map_or(0, |dc| dc.offset),
			);
			let render_line = lines.last().unwrap();
			let to = Position::new(
				render_line.line,
				render_line.chars.last().map_or(0, |dc| dc.offset),
			);
			(from, to)
		}

		fn head_to_exact(line: usize, offset: &PointerPosition, lines: &Vec<RenderLine>) -> (Position, Position) {
			let render_line = lines.first().unwrap();
			let from = Position::new(
				render_line.line,
				render_line.chars.first().map_or(0, |dc| dc.offset),
			);
			let render_line = &lines[line];
			let to = Position::new(
				render_line.line,
				offset_index(render_line, offset),
			);
			(from, to)
		}
		fn exact_to_tail(line: usize, offset: &PointerPosition, lines: &Vec<RenderLine>) -> (Position, Position) {
			let render_line = &lines[line];
			let from = Position::new(
				render_line.line,
				offset_index(render_line, offset),
			);
			let render_line = lines.last().unwrap();
			let to = Position::new(
				render_line.line,
				render_line.chars.last().map_or(0, |dc| dc.offset),
			);
			(from, to)
		}

		let lines = &self.render_lines;
		let line_count = lines.len();
		if line_count == 0 {
			return None;
		}
		if (original_pos.x - current_pos.x).abs() < MIN_TEXT_SELECT_DISTANCE
			&& (original_pos.y - current_pos.y).abs() < MIN_TEXT_SELECT_DISTANCE {
			return None;
		}
		let (line1, offset1) = self.render.pointer_pos(&original_pos, &self.render_lines, rect);
		let (line2, offset2) = self.render.pointer_pos(&current_pos, &self.render_lines, rect);

		let (from, to) = match line1 {
			PointerPosition::Head => match line2 {
				PointerPosition::Head => return None,
				PointerPosition::Exact(line2) => head_to_exact(line2, &offset2, lines),
				PointerPosition::Tail => select_all(lines),
			}
			PointerPosition::Exact(line1) => match line2 {
				PointerPosition::Head => head_to_exact(line1, &offset1, lines),
				PointerPosition::Exact(line2) => {
					let render_line = &lines[line1];
					let from = Position::new(
						render_line.line,
						offset_index(render_line, &offset1),
					);
					let render_line = &lines[line2];
					let to = Position::new(
						render_line.line,
						offset_index(render_line, &offset2),
					);
					(from, to)
				}
				PointerPosition::Tail => exact_to_tail(line1, &offset1, lines),
			}
			PointerPosition::Tail => match line2 {
				PointerPosition::Head => select_all(lines),
				PointerPosition::Exact(line2) => exact_to_tail(line2, &offset2, lines),
				PointerPosition::Tail => return None
			}
		};
		Some((from, to))
	}

	pub fn link_resolve(&self, mouse_position: Pos2, lines: &Vec<Line>) -> Option<(usize, usize)>
	{
		for line in &self.render_lines {
			if let Some(dc) = line.char_at_pos(mouse_position) {
				if let Some(link_index) = lines[line.line].link_iter(true, |link| {
					if link.range.contains(&dc.offset) {
						(true, Some(link.index))
					} else {
						(false, None)
					}
				}) {
					return Some((line.line, link_index));
				}
			}
		}
		None
	}
}