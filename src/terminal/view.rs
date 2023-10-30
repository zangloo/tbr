use anyhow::Result;
use cursive::{Printer, Vec2, View, XY};
use cursive::event::{Event, EventResult, Key, MouseButton, MouseEvent};
use cursive::theme::{ColorStyle, PaletteColor};


use crate::book::{Book, Line};
use crate::common::{char_width, Position};
use crate::config::{BookLoadingInfo, ReadingInfo};
use crate::container::Container;
use crate::controller::{Controller, HighlightInfo, HighlightMode, Render};
use crate::terminal::update_status_callback;
use crate::terminal::view::han::Han;
use crate::terminal::view::xi::Xi;

mod han;
mod xi;

pub struct ReadingView {
	controller: Controller<RenderContext, dyn TerminalRender>,
	render_context: RenderContext,

	search_color: ColorStyle,
	link_color: ColorStyle,
	highlight_link_color: ColorStyle,
	color: ColorStyle,
}

pub(crate) enum DrawCharMode {
	Plain,
	Search,
	SearchOnLink {
		line: usize,
		link_index: usize,
	},
	HighlightLink {
		line: usize,
		link_index: usize,
	},
	Link {
		line: usize,
		link_index: usize,
	},
}

impl Clone for DrawCharMode {
	fn clone(&self) -> Self {
		match self {
			DrawCharMode::Plain => DrawCharMode::Plain,
			DrawCharMode::Search => DrawCharMode::Search,
			DrawCharMode::Link { line, link_index } => DrawCharMode::Link { line: *line, link_index: *link_index },
			DrawCharMode::HighlightLink { line, link_index } => DrawCharMode::HighlightLink { line: *line, link_index: *link_index },
			DrawCharMode::SearchOnLink { line, link_index } => DrawCharMode::SearchOnLink { line: *line, link_index: *link_index },
		}
	}
}

pub(super) struct DrawChar {
	char: char,
	mode: DrawCharMode,
}

impl DrawChar {
	pub fn new(char: char, mode: DrawCharMode) -> Self {
		DrawChar { char, mode }
	}
	pub fn space() -> Self {
		DrawChar { char: ' ', mode: DrawCharMode::Plain }
	}
}

impl Clone for DrawChar {
	fn clone(&self) -> Self {
		DrawChar { char: self.char, mode: self.mode.clone() }
	}
}

pub struct RenderContext {
	width: usize,
	height: usize,
	print_lines: Vec<Vec<DrawChar>>,
	leading_space: usize,
}

impl RenderContext {
	fn new() -> Self {
		RenderContext {
			width: 0,
			height: 0,
			print_lines: vec![],
			leading_space: 0,
		}
	}
}

pub(super) trait TerminalRender: Render<RenderContext> {
	fn resized(&mut self, _context: &RenderContext) {}

	fn setup_draw_char(&mut self, char: char, line: usize, position: usize, lines: &Vec<Line>, highlight: &Option<HighlightInfo>) -> DrawChar
	{
		let mut mode = match highlight {
			Some(highlight) => if highlight.line == line && highlight.start <= position && highlight.end > position {
				match highlight.mode {
					HighlightMode::Search => DrawCharMode::Search,
					HighlightMode::Selection(..) => DrawCharMode::Plain,
					HighlightMode::Link(link_index) => DrawCharMode::HighlightLink { line, link_index },
				}
			} else {
				DrawCharMode::Plain
			},
			None => DrawCharMode::Plain,
		};
		let text = &lines[line];
		if let Some(m) = text.link_iter(true, |link| {
			if link.range.start <= position && link.range.end > position {
				let m = match mode {
					DrawCharMode::Plain => Some(DrawCharMode::Link { line, link_index: link.index }),
					DrawCharMode::Search => Some(DrawCharMode::SearchOnLink { line, link_index: link.index }),
					_ => None
				};
				return (true, m);
			}
			(false, None)
		}) {
			mode = m;
		}
		DrawChar { char, mode }
	}
}

#[inline]
fn load_render(render_han: bool) -> Box<dyn TerminalRender> {
	if render_han {
		Box::new(Han::new())
	} else {
		Box::new(Xi::new())
	}
}

impl View for ReadingView {
	fn draw(&self, printer: &Printer) {
		let context = &self.render_context;
		let mut xy = XY { x: 0, y: 0 };
		let mut tmp = [0u8; 4];
		for line in &context.print_lines {
			for dc in line {
				let color = match dc.mode {
					DrawCharMode::Plain => self.color,
					DrawCharMode::Search | DrawCharMode::SearchOnLink { .. } => self.search_color,
					DrawCharMode::Link { .. } => self.link_color,
					DrawCharMode::HighlightLink { .. } => self.highlight_link_color,
				};
				printer.with_color(color, |printer| {
					printer.print(xy, dc.char.encode_utf8(&mut tmp));
				});
				xy.x += char_width(dc.char);
			}
			xy.x = 0;
			xy.y += 1;
		}
	}

	fn layout(&mut self, xy: Vec2)
	{
		if self.render_context.width != xy.x || self.render_context.height != xy.y {
			self.render_context.width = xy.x;
			self.render_context.height = xy.y;
			self.controller.render.resized(&self.render_context);
			self.controller.redraw(&mut self.render_context);
		}
	}

	fn on_event(&mut self, e: Event) -> EventResult {
		let status = match self.process_event(e) {
			Ok(consumed) => if consumed {
				self.controller.status().to_string()
			} else {
				return EventResult::Ignored;
			},
			Err(e) => e.to_string(),
		};
		EventResult::Consumed(Some(update_status_callback(status)))
	}
}

impl ReadingView {
	pub(crate) fn new(render_han: bool, reading: BookLoadingInfo) -> Result<ReadingView> {
		let render: Box<dyn TerminalRender> = load_render(render_han);
		let mut render_context = RenderContext::new();
		let controller = Controller::new(
			reading,
			render,
			&mut render_context)?;
		let link_color = ColorStyle::new(ColorStyle::secondary().front, PaletteColor::Background);
		let highlight_link_color = ColorStyle::new(ColorStyle::secondary().front, ColorStyle::highlight().back);
		Ok(ReadingView {
			controller,
			render_context,

			search_color: ColorStyle::highlight(),
			link_color,
			highlight_link_color,
			color: ColorStyle::new(PaletteColor::Primary, PaletteColor::Background),
		})
	}

	#[inline]
	pub fn reading_info(&self) -> ReadingInfo
	{
		self.controller.reading_info().clone()
	}

	#[inline]
	pub fn status_msg(&self) -> String
	{
		self.controller.status().to_string()
	}

	#[inline]
	pub fn reading_container(&self) -> &dyn Container
	{
		self.controller.reading_container()
	}

	#[inline]
	pub fn reading_book(&self) -> &dyn Book
	{
		self.controller.reading_book()
	}

	#[inline]
	pub fn toc_index(&self) -> usize
	{
		self.controller.toc_index()
	}

	#[inline]
	pub fn switch_book(&mut self, inner_book: usize) -> String
	{
		self.controller.switch_book(inner_book, &mut self.render_context)
	}

	#[inline]
	pub fn switch_container(&mut self, loading: BookLoadingInfo) -> Result<String>
	{
		self.controller.switch_container(loading, &mut self.render_context)
	}

	#[inline]
	pub fn goto_line(&mut self, line: usize) -> Result<()>
	{
		self.controller.goto_line(line, &mut self.render_context)
	}

	#[inline]
	pub fn search(&mut self, pattern: &str) -> Result<()>
	{
		self.controller.search(pattern, &mut self.render_context)
	}

	#[inline]
	pub fn search_pattern(&self) -> &str
	{
		self.controller.search_pattern()
	}

	#[inline]
	pub fn goto_toc(&mut self, toc_index: usize) -> Option<String>
	{
		self.controller.goto_toc(toc_index, &mut self.render_context)
	}

	pub(crate) fn switch_render(&mut self, render_han: bool) {
		self.controller.render = load_render(render_han);
		self.controller.render.resized(&self.render_context);
		self.controller.redraw(&mut self.render_context);
	}

	fn process_event(&mut self, e: Event) -> Result<bool> {
		match e {
			Event::Char(' ') | Event::Key(Key::PageDown) => self.controller.next_page(&mut self.render_context)?,
			Event::Key(Key::PageUp) => self.controller.prev_page(&mut self.render_context)?,
			Event::Key(Key::Home) => self.controller.redraw_at(0, 0, &mut self.render_context),
			Event::Key(Key::End) => self.controller.goto_end(&mut self.render_context),
			Event::Key(Key::Down) => self.controller.step_next(&mut self.render_context)?,
			Event::Key(Key::Up) => self.controller.step_prev(&mut self.render_context)?,
			Event::Char('n') => self.controller.search_again(true, &mut self.render_context)?,
			Event::Char('N') => self.controller.search_again(false, &mut self.render_context)?,
			Event::CtrlChar('d') => { self.controller.switch_chapter(true, &mut self.render_context)?; }
			Event::CtrlChar('b') => { self.controller.switch_chapter(false, &mut self.render_context)?; }
			Event::Key(Key::Right) => self.controller.goto_trace(false, &mut self.render_context)?,
			Event::Key(Key::Left) => self.controller.goto_trace(true, &mut self.render_context)?,
			Event::Key(Key::Tab) => self.controller.switch_link_next(&mut self.render_context),
			Event::Shift(Key::Tab) => self.controller.switch_link_prev(&mut self.render_context),
			Event::Key(Key::Enter) => self.controller.try_goto_link(&mut self.render_context)?,
			Event::Mouse { event: MouseEvent::Press(MouseButton::Left), position, .. } =>
				self.left_click(position)?,
			_ => return Ok(false),
		};
		Ok(true)
	}

	fn left_click(&mut self, position: Vec2) -> Result<()>
	{
		let print_lines = &self.render_context.print_lines;
		if let Some(print_line) = print_lines.get(position.y) {
			let mut x = 0;
			for index in 0..print_line.len() {
				if x >= position.x {
					let dc = if x == position.x {
						&print_line[index]
					} else {
						// for cjk char draw at position x, but click at x+1
						&print_line[index - 1]
					};
					match dc.mode {
						DrawCharMode::Link { line, link_index, .. }
						| DrawCharMode::HighlightLink { line, link_index, .. }
						| DrawCharMode::SearchOnLink { line, link_index } => self.controller.goto_link(line, link_index, &mut self.render_context)?,
						DrawCharMode::Search | DrawCharMode::Plain => {}
					}
					break;
				}
				let wc = char_width(print_line[index].char);
				x += wc;
			}
		}
		Ok(())
	}
}
