use egui::{CursorIcon, InputState, Pos2, Rect, Response, Sense, Ui, Vec2};
use crate::book::{Book, Colors, Line};
use crate::common::Position;
use crate::controller::{HighlightInfo, Render};
use crate::gui::render::{create_render, GuiRender, measure_char_size, PointerPosition, RenderContext, RenderLine};

const MIN_TEXT_SELECT_DISTANCE: f32 = 4.0;

pub enum ViewAction {
	Goto(usize, usize),
	SelectText(Pos2, Pos2),
	TextSelectedDone,
	StepBackward,
	StepForward,
	None,
}

enum InternalAction {
	Action(ViewAction),
	Cursor(bool),
}

pub(super) struct GuiView {
	pub render: Box<dyn GuiRender>,
	pub render_lines: Vec<RenderLine>,
	pub dragging: bool,
	pub render_context: RenderContext,
}

impl Render<Ui> for GuiView {
	fn book_loaded(&mut self, book: &dyn Book, _ui: &mut Ui)
	{
		self.render_context.leading_chars = book.leading_space();
		self.render.reset_render_context(&mut self.render_context);
	}

	#[inline]
	fn redraw(&mut self, book: &dyn Book, lines: &Vec<Line>, line: usize,
		offset: usize, highlight: &Option<HighlightInfo>, ui: &mut Ui)
		-> Option<Position>
	{
		self.render.gui_redraw(book, lines, line, offset, highlight, ui, &mut self.render_lines, &self.render_context)
	}

	#[inline]
	fn prev_page(&mut self, book: &dyn Book, lines: &Vec<Line>,
		line: usize, offset: usize, ui: &mut Ui) -> Position
	{
		self.render.gui_prev_page(book, lines, line, offset, ui, &self.render_context)
	}

	#[inline]
	fn next_line(&mut self, book: &dyn Book, lines: &Vec<Line>,
		line: usize, offset: usize, ui: &mut Ui) -> Position
	{
		self.render.gui_next_line(book, lines, line, offset, ui, &self.render_context)
	}

	#[inline]
	fn prev_line(&mut self, book: &dyn Book, lines: &Vec<Line>,
		line: usize, offset: usize, ui: &mut Ui) -> Position
	{
		self.render.gui_prev_line(book, lines, line, offset, ui, &self.render_context)
	}

	#[inline]
	fn setup_highlight(&mut self, book: &dyn Book, lines: &Vec<Line>,
		line: usize, start: usize, ui: &mut Ui) -> Position
	{
		self.render.gui_setup_highlight(book, lines, line, start, ui, &self.render_context)
	}
}

impl GuiView {
	#[inline]
	pub fn new(render_type: &str, colors: Colors) -> Self
	{
		let render = create_render(render_type);
		let render_context = RenderContext {
			colors,
			font_size: 0,
			default_font_measure: Default::default(),
			custom_color: false,
			view_port: Rect::NOTHING,
			render_rect: Rect::NOTHING,
			leading_chars: 0,
			leading_space: 0.0,
			max_page_size: 0.0,
		};
		GuiView {
			render,
			render_lines: vec![],
			dragging: false,
			render_context,
		}
	}

	#[inline]
	pub fn reload_render(&mut self, render_type: &str)
	{
		self.render = create_render(render_type);
	}

	#[inline]
	fn absolute_view_port(&self) -> Rect
	{
		let origin = Pos2::new(
			self.render_context.view_port.min.x + self.render_context.render_rect.min.x,
			self.render_context.view_port.min.y + self.render_context.render_rect.min.y,
		);
		Rect::from_min_size(origin, self.render_context.view_port.size())
	}

	#[inline]
	pub fn draw(&mut self, ui: &mut Ui)
	{
		ui.set_clip_rect(self.absolute_view_port());
		self.render.draw(&self.render_lines, ui);
	}

	pub fn show(&mut self, ui: &mut Ui, font_size: u8, book: &dyn Book,
		detect_actions: bool, view_port: Option<Rect>) -> (Response, bool, ViewAction)
	{
		let font_redraw = if self.render_context.font_size != font_size {
			self.render_context.font_size = font_size;
			self.render_context.default_font_measure = measure_char_size(ui, '漢', font_size as f32);
			true
		} else {
			false
		};

		let margin = create_margin(&self.render_context.default_font_measure);
		let mut render_rect = ui.available_rect_before_wrap().shrink2(margin);
		self.render_context.view_port = if let Some(view_port) = view_port {
			ui.set_clip_rect(Rect::NOTHING);
			let mut dummy_context = RenderContext {
				colors: self.render_context.colors.clone(),
				font_size: self.render_context.font_size,
				default_font_measure: self.render_context.default_font_measure,
				custom_color: false,
				view_port: Rect::NOTHING,
				render_rect,

				leading_chars: book.leading_space(),
				leading_space: 0.0,
				max_page_size: 0.0,
			};
			render_rect = self.render.measure_lines_size(
				book,
				ui,
				&mut dummy_context);
			let min = view_port.min;
			let view_port = view_port.shrink2(margin);
			Rect::from_min_size(min, view_port.size())
		} else {
			Rect::from_min_size(Pos2::ZERO, render_rect.size())
		};

		let max_rect = render_rect.expand2(margin);

		let size = max_rect.size();
		let response = ui.allocate_response(size, Sense::click_and_drag());
		let action = if detect_actions {
			let action = response.ctx.input(|input| {
				if let Some(pointer_pos) = input.pointer.interact_pos() {
					let view_port = self.absolute_view_port();
					if view_port.contains(pointer_pos) {
						return self.detect_action(
							&response,
							input,
							pointer_pos,
							book);
					}
				}
				InternalAction::Action(ViewAction::None)
			});
			match action {
				InternalAction::Action(action) => action,
				InternalAction::Cursor(hand) => {
					if hand {
						ui.output_mut(|output| output.cursor_icon = CursorIcon::PointingHand);
					} else {
						ui.output_mut(|output| output.cursor_icon = CursorIcon::Default);
					}
					ViewAction::None
				}
			}
		} else {
			ViewAction::None
		};

		let rect_redraw = if render_rect.min != self.render_context.render_rect.min
			|| render_rect.max != self.render_context.render_rect.max {
			self.render_context.render_rect = render_rect;
			self.render.reset_baseline(&mut self.render_context);
			self.render.reset_render_context(&mut self.render_context);
			true
		} else {
			false
		};

		let redraw = font_redraw | rect_redraw;
		if redraw {
			self.render.reset_render_context(&mut self.render_context)
		}

		(response, redraw, action)
	}

	fn detect_action(&mut self, response: &Response, input: &InputState,
		pointer_pos: Pos2, book: &dyn Book) -> InternalAction
	{
		if input.pointer.primary_clicked() {
			if let Some((line, link_index)) = self.link_resolve(pointer_pos, &book.lines()) {
				return InternalAction::Action(ViewAction::Goto(line, link_index));
			}
		} else if input.pointer.primary_down() {
			if let Some(from_pos) = input.pointer.press_origin() {
				if response.rect.contains(from_pos) {
					self.dragging = true;
					return InternalAction::Action(ViewAction::SelectText(from_pos, pointer_pos));
				}
			}
		} else if input.pointer.primary_released() {
			if self.dragging {
				self.dragging = false;
				return InternalAction::Action(ViewAction::TextSelectedDone);
			}
		} else if input.scroll_delta.y != 0.0 {
			let delta = input.scroll_delta.y;
			// delta > 0.0 for scroll up
			if delta > 0.0 {
				return InternalAction::Action(ViewAction::StepBackward);
			} else {
				return InternalAction::Action(ViewAction::StepForward);
			}
		} else {
			let link = self.link_resolve(pointer_pos, &book.lines());
			return InternalAction::Cursor(link.is_some());
		}
		InternalAction::Action(ViewAction::None)
	}

	pub fn set_colors(&mut self, colors: Colors)
	{
		self.render_context.colors = colors;
	}

	pub fn set_custom_color(&mut self, custom_color: bool)
	{
		self.render_context.custom_color = custom_color;
	}

	pub fn calc_selection(&self, original_pos: Pos2, current_pos: Pos2)
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
		let (line1, offset1) = self.render.pointer_pos(&original_pos, &self.render_lines, &self.render_context.render_rect);
		let (line2, offset2) = self.render.pointer_pos(&current_pos, &self.render_lines, &self.render_context.render_rect);

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

#[inline]
fn create_margin(font_measure: &Vec2) -> Vec2
{
	Vec2::new(font_measure.x / 2.0, font_measure.y / 2.0)
}