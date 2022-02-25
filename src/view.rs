use std::collections::HashMap;

use anyhow::{anyhow, Result};
use cursive::{Printer, Vec2, View, XY};
use cursive::event::{Event, EventResult, Key, MouseButton, MouseEvent};
use cursive::theme::{ColorStyle, PaletteColor};
use regex::Regex;

use crate::{ContainerManager, ReadingInfo};
use crate::book::{Book, Line};
use crate::container::Container;
use crate::controller::update_status_callback;
use crate::view::han::Han;
use crate::view::xi::Xi;

mod han;
mod xi;

const TRACE_SIZE: usize = 100;

pub enum HighlightMode {
	Search,
	Link(usize),
}

impl Clone for HighlightMode {
	fn clone(&self) -> Self {
		match self {
			HighlightMode::Search => HighlightMode::Search,
			HighlightMode::Link(link_index) => HighlightMode::Link(*link_index),
		}
	}
}

pub struct HighlightInfo {
	pub line: usize,
	pub start: usize,
	pub end: usize,
	pub mode: HighlightMode,
}

pub struct Position {
	pub line: usize,
	pub position: usize,
}

impl Position {
	pub fn new(line: usize, position: usize) -> Self {
		Position { line, position }
	}
}

pub struct TraceInfo {
	pub chapter: usize,
	pub line: usize,
	pub position: usize,
}

pub struct ReadingView {
	render: Box<dyn Render>,
	container_manager: ContainerManager,
	container: Box<dyn Container>,
	book: Box<dyn Book>,
	reading: ReadingInfo,
	search_pattern: Option<String>,
	trace: Vec<TraceInfo>,
	current_trace: usize,
	render_context: RenderContext,
}

pub(crate) enum DrawCharMode {
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
			DrawCharMode::Search => DrawCharMode::Search,
			DrawCharMode::Link { line, link_index } => DrawCharMode::Link { line: *line, link_index: *link_index },
			DrawCharMode::HighlightLink { line, link_index } => DrawCharMode::HighlightLink { line: *line, link_index: *link_index },
			DrawCharMode::SearchOnLink { line, link_index } => DrawCharMode::SearchOnLink { line: *line, link_index: *link_index },
		}
	}
}

pub(crate) struct DrawChar {
	char: Option<char>,
	mode: DrawCharMode,
}

pub struct RenderContext {
	width: usize,
	height: usize,
	print_lines: Vec<String>,
	special_char_map: HashMap<Vec2, DrawChar>,
	search_color: ColorStyle,
	link_color: ColorStyle,
	highlight_link_color: ColorStyle,
	color: ColorStyle,
	leading_space: usize,
	next: Option<Position>,
}

impl RenderContext {
	fn build(book: &Box<dyn Book>) -> Self {
		let link_color = ColorStyle::new(ColorStyle::secondary().front, PaletteColor::Background);
		let highlight_link_color = ColorStyle::new(ColorStyle::secondary().front, ColorStyle::highlight().back);
		RenderContext {
			width: 0,
			height: 0,
			print_lines: vec!["".to_string()],
			special_char_map: HashMap::new(),
			search_color: ColorStyle::highlight(),
			link_color,
			highlight_link_color,
			color: ColorStyle::new(PaletteColor::Primary, PaletteColor::Background),
			leading_space: book.leading_space(),
			next: None,
		}
	}
}

pub(crate) trait Render {
	fn resized(&mut self, _context: &mut RenderContext) {}
	fn redraw(&mut self, lines: &Vec<Line>, reading: &ReadingInfo, context: &mut RenderContext);
	fn prev(&mut self, lines: &Vec<Line>, reading: &mut ReadingInfo, context: &mut RenderContext);
	fn next_line(&mut self, lines: &Vec<Line>, reading: &mut ReadingInfo, context: &mut RenderContext);
	fn prev_line(&mut self, lines: &Vec<Line>, reading: &mut ReadingInfo, context: &mut RenderContext);
	// move to highlight line if not displayed in current view
	fn setup_highlight(&mut self, lines: &Vec<Line>, reading: &mut ReadingInfo, context: &mut RenderContext);
	#[inline]
	fn map_char(&self, ch: char) -> char { ch }
	fn setup_special_char(&mut self, line: usize, position: usize, char: char, lines: &Vec<Line>, reading: &ReadingInfo) -> Option<DrawChar> {
		let mut special_char = match &reading.highlight {
			Some(highlight) => if highlight.line == line && highlight.start <= position && highlight.end > position {
				let draw_char = self.map_char(char);
				Some(DrawChar {
					char: Some(draw_char),
					mode: match highlight.mode {
						HighlightMode::Search => DrawCharMode::Search,
						HighlightMode::Link(link_index) => DrawCharMode::HighlightLink { line, link_index },
					},
				})
			} else {
				None
			},
			None => None,
		};
		let text = &lines[line];
		for (link_index, link) in text.link_iter().enumerate() {
			if link.range.start <= position && link.range.end > position {
				if let Some(DrawChar { ref mut mode, .. }) = special_char {
					if matches!(mode, DrawCharMode::Search) {
						*mode = DrawCharMode::SearchOnLink { line, link_index };
					}
				} else {
					let draw_char = self.map_char(char);
					special_char = Some(DrawChar { char: Some(draw_char), mode: DrawCharMode::Link { line, link_index } });
				}
				break;
			}
		}
		special_char
	}
}

fn load_render(render_type: &String) -> Box<dyn Render> {
	match render_type.as_str() {
		"han" => Box::new(Han::default()),
		// for now, only "xi"
		_ => Box::new(Xi::default()),
	}
}

impl View for ReadingView {
	fn draw(&self, printer: &Printer) {
		let context = &self.render_context;
		printer.with_color(context.color, |printer| {
			let mut xy = XY { x: 0, y: 0 };
			for line in &context.print_lines {
				printer.print(xy, line.as_str());
				xy.y += 1;
			}
		});
		for (xy, dc) in &context.special_char_map {
			let char = match dc.char {
				Some(ch) => ch,
				None => continue,
			};
			let mut tmp = [0u8; 4];
			let color = match dc.mode {
				DrawCharMode::Search | DrawCharMode::SearchOnLink { .. } => context.search_color,
				DrawCharMode::Link { .. } => context.link_color,
				DrawCharMode::HighlightLink { .. } => context.highlight_link_color,
			};
			printer.with_color(color, |printer| {
				printer.print(xy, char.encode_utf8(&mut tmp));
			});
		}
	}

	fn layout(&mut self, xy: Vec2) {
		let context = &mut self.render_context;
		context.width = xy.x;
		context.height = xy.y;
		self.render.resized(&mut self.render_context);
		self.render.redraw(self.book.lines(), &mut self.reading, &mut self.render_context);
	}

	fn on_event(&mut self, e: Event) -> EventResult {
		let status = match self.process_event(e) {
			Ok(consumed) => if consumed {
				self.status_msg()
			} else {
				return EventResult::Ignored;
			},
			Err(e) => e.to_string(),
		};
		EventResult::Consumed(Some(update_status_callback(status)))
	}
}

impl ReadingView {
	pub(crate) fn new(render_type: &String, mut reading: ReadingInfo, search_pattern: &Option<String>) -> Result<ReadingView> {
		let container_manager = Default::default();
		let mut container = load_container(&container_manager, &reading)?;
		let book = load_book(&container_manager, &mut container, &mut reading)?;
		let render_context = RenderContext::build(&book);
		let render: Box<dyn Render> = load_render(render_type);
		let trace = vec![TraceInfo { chapter: reading.chapter, line: reading.line, position: reading.position }];
		Ok(ReadingView {
			container_manager,
			container,
			book,
			reading,
			render,
			search_pattern: search_pattern.clone(),
			trace,
			current_trace: 0,
			render_context,
		})
	}

	pub(crate) fn status_msg(&self) -> String {
		let title = match self.book.title() {
			Some(t) => t,
			None => {
				let names = self.container.inner_book_names();
				if names.len() == 1 {
					&self.reading.filename
				} else {
					let name = &names[self.reading.inner_book];
					name.name()
				}
			}
		};
		format!("{}({}:{})", title, self.book.lines().len(), self.reading.line)
	}

	pub(crate) fn reading_info(&self) -> ReadingInfo {
		self.reading.clone()
	}

	pub(crate) fn search(&mut self, pattern: &str) -> Result<()> {
		self.search_pattern = Some(String::from(pattern));
		self.search_next(self.reading.line, self.reading.position)
	}

	pub(crate) fn search_pattern(&self) -> &Option<String> {
		&self.search_pattern
	}

	pub(crate) fn reading_book(&self) -> &Box<dyn Book> {
		&self.book
	}

	pub(crate) fn reading_container(&self) -> &Box<dyn Container> {
		&self.container
	}

	pub(crate) fn switch_container(&mut self, mut reading: ReadingInfo) -> Result<String> {
		let mut container = load_container(&self.container_manager, &reading)?;
		let book = load_book(&self.container_manager, &mut container, &mut reading)?;
		self.container = container;
		self.book = book;
		self.reading = reading;
		self.render.redraw(&self.book.lines(), &self.reading, &mut self.render_context);
		Ok(self.status_msg())
	}

	pub(crate) fn switch_book(&mut self, reading: ReadingInfo) -> String {
		match self.do_switch_book(reading) {
			Ok(..) => self.status_msg(),
			Err(e) => e.to_string(),
		}
	}
	fn do_switch_book(&mut self, mut reading: ReadingInfo) -> Result<()> {
		let book = load_book(&self.container_manager, &mut self.container, &mut reading)?;
		self.book = book;
		self.reading = reading;
		self.trace.clear();
		self.trace.push(TraceInfo { chapter: self.reading.chapter, line: self.reading.line, position: self.reading.position });
		self.current_trace = 0;
		self.render.redraw(self.book.lines(), &mut self.reading, &mut self.render_context);
		Ok(())
	}

	pub(crate) fn goto_line(&mut self, line: usize) -> Result<()> {
		let lines = &self.book.lines();
		if line > lines.len() || line == 0 {
			return Err(anyhow!("Invalid line number: {}", line));
		}
		self.reading.line = line - 1;
		self.reading.position = 0;
		self.render.redraw(lines, &self.reading, &mut self.render_context);
		Ok(())
	}

	pub(crate) fn switch_render(&mut self, render_type: &String) {
		self.render_context.next = None;
		self.render_context.print_lines.clear();
		self.render = load_render(render_type);
	}

	fn process_event(&mut self, e: Event) -> Result<bool> {
		match e {
			Event::Char(' ') | Event::Key(Key::PageDown) => { self.next_page()?; }
			Event::Key(Key::PageUp) => { self.prev_page()?; }
			Event::Key(Key::Home) => {
				self.reading.line = 0;
				self.reading.position = 0;
				self.render.redraw(self.book.lines(), &mut self.reading, &mut self.render_context);
				self.push_trace(true);
			}
			Event::Key(Key::End) => {
				self.reading.line = self.book.lines().len();
				self.reading.position = 0;
				self.render.prev(self.book.lines(), &mut self.reading, &mut self.render_context);
				self.push_trace(true);
			}
			Event::Key(Key::Down) => {
				if self.render_context.next.is_some() {
					self.render.next_line(self.book.lines(), &mut self.reading, &mut self.render_context);
				}
				self.push_trace(true);
			}
			Event::Key(Key::Up) => {
				self.render.prev_line(self.book.lines(), &mut self.reading, &mut self.render_context);
				self.push_trace(true);
			}
			Event::Char('n') => {
				let (line, position) = match &self.reading.highlight {
					Some(HighlightInfo { mode: HighlightMode::Search, line, end, .. }) => (*line, *end),
					None | Some(HighlightInfo { mode: HighlightMode::Link(..), .. }) => (self.reading.line, self.reading.position),
				};
				self.search_next(line, position)?;
			}
			Event::Char('N') => {
				let (line, position) = match &self.reading.highlight {
					Some(HighlightInfo { mode: HighlightMode::Search, line, start, .. }) => (*line, *start),
					None | Some(HighlightInfo { mode: HighlightMode::Link(..), .. }) => (self.reading.line, self.reading.position),
				};
				self.search_prev(line, position)?;
			}
			Event::CtrlChar('d') => {
				self.switch_chapter_internal(self.reading.chapter + 1)?;
			}
			Event::CtrlChar('b') => {
				if self.reading.chapter > 0 {
					self.switch_chapter_internal(self.reading.chapter - 1)?;
				}
			}
			Event::Key(Key::Right) => self.goto_trace(false)?,
			Event::Key(Key::Left) => self.goto_trace(true)?,

			Event::Key(Key::Tab) => self.switch_link_next()?,
			Event::Shift(Key::Tab) => self.switch_link_prev()?,
			Event::Key(Key::Enter) => {
				match self.reading.highlight {
					Some(HighlightInfo { mode: HighlightMode::Search, line, start, end }) => {
						let text = &self.book.lines()[line];
						for (link_index, link) in text.link_iter().enumerate() {
							let range = &link.range;
							if range.start <= start && range.end >= end {
								self.goto_link(line, link_index)?;
								break;
							}
						}
					}
					Some(HighlightInfo { mode: HighlightMode::Link(link_index), line, .. }) => {
						self.goto_link(line, link_index)?;
					}
					None => {}
				}
			}
			Event::Mouse { event: MouseEvent::Press(MouseButton::Left), position, .. } => {
				let option = self.render_context.special_char_map
					.get(&position)
					.and_then(|dc| {
						match dc.mode {
							DrawCharMode::Link { line, link_index, .. }
							| DrawCharMode::HighlightLink { line, link_index, .. }
							| DrawCharMode::SearchOnLink { line, link_index } => {
								Some((line, link_index))
							}
							DrawCharMode::Search => None
						}
					});
				if let Some((line, link_index)) = option {
					self.goto_link(line, link_index)?;
				}
			}
			_ => return Ok(false),
		};
		Ok(true)
	}

	fn next_page(&mut self) -> Result<bool> {
		match &self.render_context.next {
			Some(next) => {
				self.reading.line = next.line;
				self.reading.position = next.position;
				self.render.redraw(self.book.lines(), &mut self.reading, &mut self.render_context);
				self.push_trace(true);
				Ok(true)
			}
			None => {
				if self.switch_chapter_internal(self.reading.chapter + 1)? {
					Ok(true)
				} else {
					let book_index = self.reading.inner_book + 1;
					let book_count = self.container.inner_book_names().len();
					if book_index >= book_count {
						Ok(false)
					} else {
						let reading = ReadingInfo::new(&self.reading.filename)
							.with_inner_book(book_index);
						self.do_switch_book(reading)?;
						Ok(true)
					}
				}
			}
		}
	}

	fn prev_page(&mut self) -> Result<bool> {
		if self.reading.line == 0 && self.reading.position == 0 {
			let reading = &mut self.reading;
			if reading.chapter > 0 {
				reading.chapter -= 1;
				let book = &mut self.book;
				book.set_chapter(reading.chapter)?;
				let lines = book.lines();
				reading.line = lines.len();
				reading.position = 0;
				// prev need decrease this invalid reading.line
				self.render.prev(book.lines(), reading, &mut self.render_context);
				self.push_trace(true);
				Ok(true)
			} else {
				if reading.inner_book > 0 {
					let mut new_reading = ReadingInfo::new(&reading.filename)
						.with_inner_book(reading.inner_book - 1)
						.with_last_chapter();
					self.book = load_book(&self.container_manager, &mut self.container, &mut new_reading)?;
					new_reading.chapter = self.book.current_chapter();
					new_reading.line = self.book.lines().len();
					new_reading.position = self.book.lines()[new_reading.line - 1].len();
					self.reading = new_reading;
					self.prev_page()?;
					self.trace.clear();
					self.trace.push(TraceInfo { chapter: self.reading.chapter, line: self.reading.line, position: self.reading.position });
					self.current_trace = 0;
					Ok(true)
				} else {
					Ok(false)
				}
			}
		} else {
			self.render.prev(self.book.lines(), &mut self.reading, &mut self.render_context);
			self.push_trace(true);
			Ok(true)
		}
	}

	pub(crate) fn switch_chapter(&mut self, chapter: usize) -> String {
		match self.switch_chapter_internal(chapter) {
			Ok(_) => self.status_msg(),
			Err(e) => e.to_string(),
		}
	}

	fn switch_chapter_internal(&mut self, chapter: usize) -> Result<bool> {
		if chapter < self.book.chapter_count() {
			self.book.set_chapter(chapter)?;
			self.reading.chapter = chapter;
			self.reading.line = 0;
			self.reading.position = 0;
			self.render.redraw(self.book.lines(), &mut self.reading, &mut self.render_context);
			self.push_trace(true);
			Ok(true)
		} else {
			Ok(false)
		}
	}

	fn search_next(&mut self, start_line: usize, start_position: usize) -> Result<()> {
		let search_text = match &self.search_pattern {
			Some(text) => text,
			None => return Ok(()),
		};
		let book = &self.book;
		let lines = book.lines();
		let regex = Regex::new(search_text.as_str())?;
		let mut position = start_position;
		for idx in start_line..lines.len() {
			let line = &lines[idx];
			if let Some(range) = line.search_pattern(&regex, Some(position), None, false) {
				self.reading.highlight = Some(HighlightInfo {
					line: idx,
					start: range.start,
					end: range.end,
					mode: HighlightMode::Search,
				});
				self.highlight_setup();
				return Ok(());
			}
			position = 0;
		}
		Ok(())
	}

	fn search_prev(&mut self, start_line: usize, start_position: usize) -> Result<()> {
		let search_text = match &self.search_pattern {
			Some(text) => text,
			None => return Ok(()),
		};
		let lines = self.book.lines();
		let regex = Regex::new(search_text.as_str())?;
		for idx in (0..=start_line).rev() {
			let range = if idx == start_line {
				if start_position == 0 {
					continue;
				} else {
					lines[idx].search_pattern(&regex, None, Some(start_position), true)
				}
			} else {
				lines[idx].search_pattern(&regex, None, None, true)
			};
			if let Some(range) = range {
				self.reading.highlight = Some(HighlightInfo {
					line: idx,
					start: range.start,
					end: range.end,
					mode: HighlightMode::Search,
				});
				self.highlight_setup();
				return Ok(());
			}
		}
		Ok(())
	}

	fn push_trace(&mut self, clear_highlight: bool) {
		let reading = &self.reading;
		let trace = &mut self.trace;
		let last = &trace[self.current_trace];
		if last.chapter == reading.chapter && last.line == reading.line && last.position == reading.position {
			return;
		}
		trace.drain(self.current_trace + 1..);
		trace.push(TraceInfo { chapter: reading.chapter, line: reading.line, position: reading.position });
		if trace.len() > TRACE_SIZE {
			trace.drain(0..1);
		} else {
			self.current_trace += 1;
		}
		if clear_highlight {
			self.reading.highlight = None;
		}
	}
	fn goto_trace(&mut self, backward: bool) -> Result<()> {
		let reading = &mut self.reading;
		if backward {
			if self.current_trace == 0 {
				return Ok(());
			} else {
				self.current_trace -= 1;
			}
		} else if self.current_trace == self.trace.len() - 1 {
			return Ok(());
		} else {
			self.current_trace += 1;
		}
		let current_trace = &self.trace[self.current_trace];
		if reading.chapter == current_trace.chapter {
			reading.line = current_trace.line;
			reading.position = current_trace.position;
		} else {
			reading.chapter = current_trace.chapter;
			reading.line = current_trace.line;
			reading.position = current_trace.position;
			self.book.set_chapter(reading.chapter)?;
		}
		self.render.redraw(self.book.lines(), reading, &mut self.render_context);
		self.reading.highlight = None;
		Ok(())
	}

	fn switch_link_prev(&mut self) -> Result<()> {
		let reading = &mut self.reading;
		let (mut line, mut position) = match &reading.highlight {
			Some(HighlightInfo { mode: HighlightMode::Link(..), line, start, .. }) => (*line, *start),
			None | Some(HighlightInfo { mode: HighlightMode::Search, .. }) => (reading.line, reading.position)
		};
		let lines = self.book.lines();
		let mut text = &lines[line];
		'outer: loop {
			for (link_index, link) in text.link_iter().rev().enumerate() {
				if link.range.end <= position {
					reading.highlight = Some(HighlightInfo {
						line,
						start: link.range.start,
						end: link.range.end,
						mode: HighlightMode::Link(link_index),
					});
					break 'outer;
				}
			}
			if line == 0 {
				break;
			}
			line -= 1;
			text = &lines[line];
			position = text.len();
		}
		self.highlight_setup();
		Ok(())
	}

	fn switch_link_next(&mut self) -> Result<()> {
		let reading = &mut self.reading;
		let (line, mut position) = match &reading.highlight {
			Some(HighlightInfo { mode: HighlightMode::Link(..), line, end, .. }) => (*line, *end),
			None | Some(HighlightInfo { mode: HighlightMode::Search, .. }) => (reading.line, reading.position),
		};
		let lines = self.book.lines();
		'outer: for index in line..lines.len() {
			let text = &lines[index];
			for (link_index, link) in text.link_iter().enumerate() {
				if link.range.start >= position {
					reading.highlight = Some(HighlightInfo {
						line: index,
						start: link.range.start,
						end: link.range.end,
						mode: HighlightMode::Link(link_index),
					});
					break 'outer;
				}
			}
			position = 0;
		}
		self.highlight_setup();
		Ok(())
	}

	fn goto_link(&mut self, line: usize, link_index: usize) -> Result<()> {
		if let Some(pos) = self.book.link_position(line, link_index) {
			if pos.chapter != self.book.current_chapter() {
				self.book.set_chapter(pos.chapter)?;
			}
			self.reading.chapter = pos.chapter;
			self.reading.line = pos.line;
			self.reading.position = pos.position;
			if pos.position == 0 {
				self.render.redraw(self.book.lines(), &self.reading, &mut self.render_context);
			} else {
				self.render.prev_line(self.book.lines(), &mut self.reading, &mut self.render_context);
			}
			self.push_trace(true);
		}
		Ok(())
	}

	fn highlight_setup(&mut self) {
		let in_current_screen = match &self.reading.highlight {
			Some(highlight) => {
				let highlight_line = highlight.line;
				let highlight_start = highlight.start;
				let reading_line = self.reading.line;
				let reading_position = self.reading.position;
				if (highlight_line == reading_line && highlight_start >= reading_position) || (highlight_line > reading_line) {
					match &self.render_context.next {
						Some(next) => if (highlight_line == next.line && highlight_start < next.position) || (highlight_line < next.line) {
							true
						} else {
							false
						}
						None => true,
					}
				} else {
					false
				}
			}
			None => true,
		};
		if !in_current_screen {
			self.render.setup_highlight(self.book.lines(), &mut self.reading, &mut self.render_context);
			self.push_trace(false);
		}
		self.render.redraw(self.book.lines(), &self.reading, &mut self.render_context);
	}
}

fn load_container(container_manager: &ContainerManager, reading: &ReadingInfo) -> Result<Box<dyn Container>> {
	let container = container_manager.open(&reading.filename)?;
	let book_names = container.inner_book_names();
	if book_names.len() == 0 {
		return Err(anyhow!("No content supported."));
	}
	Ok(container)
}

fn load_book(container_manager: &ContainerManager, container: &mut Box<dyn Container>, reading: &mut ReadingInfo) -> Result<Box<dyn Book>> {
	let book = container_manager.load_book(container, reading.inner_book, reading.chapter)?;
	let lines = book.lines();
	if reading.line >= lines.len() {
		reading.line = lines.len() - 1;
	}
	let chars = lines[reading.line].len();
	if reading.position >= chars {
		reading.position = 0;
	}
	Ok(book)
}