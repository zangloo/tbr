use anyhow::{anyhow, Result};
use cursive::{Printer, Vec2, View, XY};
use cursive::event::{Event, EventResult, Key};
use cursive::theme::{ColorStyle, PaletteColor};
use regex::{Match, Regex};

use crate::{ContainerManager, ReadingInfo};
use crate::book::Book;
use crate::common::{byte_index_for_char, char_index_for_byte};
use crate::container::Container;
use crate::controller::{ReverseInfo, update_status_callback};
use crate::view::han::Han;
use crate::view::xi::Xi;

mod han;
mod xi;

const TRACE_SIZE: usize = 100;

pub struct NextPageInfo {
	line: usize,
	position: usize,
}

struct TraceInfo {
	chapter: usize,
	line: usize,
	position: usize,
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

pub struct ReverseChar(char, Vec2);

pub struct RenderContext {
	width: usize,
	height: usize,
	reverse_chars: Vec<ReverseChar>,
	print_lines: Vec<String>,
	reverse_color: ColorStyle,
	color: ColorStyle,
	leading_space: usize,
	next: Option<NextPageInfo>,
}

impl RenderContext {
	fn build(book: &Box<dyn Book>) -> Self {
		RenderContext {
			width: 0,
			height: 0,
			print_lines: vec!["".to_string()],
			reverse_chars: vec![],
			reverse_color: ColorStyle::highlight(),
			color: ColorStyle::new(PaletteColor::Primary, PaletteColor::Background),
			leading_space: book.leading_space(),
			next: None,
		}
	}
}

pub(crate) trait Render {
	fn resized(&mut self, _context: &mut RenderContext) {}
	fn redraw(&mut self, lines: &Vec<String>, reading: &ReadingInfo, context: &mut RenderContext);
	fn prev(&mut self, lines: &Vec<String>, reading: &mut ReadingInfo, context: &mut RenderContext);
	fn next_line(&mut self, lines: &Vec<String>, reading: &mut ReadingInfo, context: &mut RenderContext);
	fn prev_line(&mut self, lines: &Vec<String>, reading: &mut ReadingInfo, context: &mut RenderContext);
	// move to reverse line if not displayed in current view
	fn setup_reverse(&mut self, lines: &Vec<String>, reading: &mut ReadingInfo, context: &mut RenderContext);
}

fn load_render(render_type: &String) -> Box<dyn Render> {
	match render_type.as_str() {
		"han" => Box::new(Han::default()),
		// for now, only "xi"
		_ => Box::new(Xi::default()),
	}
}

fn is_reverse_displayed(reading: &ReadingInfo, context: &RenderContext) -> bool {
	let reverse = match &reading.reverse {
		Some(r) => r,
		None => return true,
	};
	let revers_line = reverse.line;
	let revers_start = reverse.start;
	let reading_line = reading.line;
	let reading_position = reading.position;
	if (revers_line == reading_line && revers_start >= reading_position) || (revers_line > reading_line) {
		match &context.next {
			Some(next) => if (revers_line == next.line && revers_start < next.position) || (revers_line < next.line) {
				return true;
			}
			None => return true,
		}
	}
	return false;
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
		let reverse_chars = &context.reverse_chars;
		if reverse_chars.len() > 0 {
			let mut tmp = [0u8; 4];
			printer.with_color(context.reverse_color, |printer| {
				for ReverseChar(ch, xy) in reverse_chars {
					printer.print(xy, ch.encode_utf8(&mut tmp));
				}
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
				let (line, position) = match &self.reading.reverse {
					Some(reverse) => (reverse.line, reverse.end),
					None => (self.reading.line, self.reading.position),
				};
				self.search_next(line, position)?;
			}
			Event::Char('N') => {
				let (line, position) = match &self.reading.reverse {
					Some(reverse) => (reverse.line, reverse.start),
					None => (self.reading.line, self.reading.position),
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
						let reading = ReadingInfo {
							filename: self.reading.filename.clone(),
							inner_book: book_index,
							chapter: 0,
							line: 0,
							position: 0,
							ts: 0,
							reverse: None,
						};
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
					let mut new_reading = ReadingInfo {
						filename: reading.filename.clone(),
						inner_book: reading.inner_book - 1,
						chapter: usize::MAX,
						line: 0,
						position: 0,
						ts: 0,
						reverse: None,
					};
					self.book = load_book(&self.container_manager, &mut self.container, &mut new_reading)?;
					new_reading.chapter = self.book.current_chapter();
					new_reading.line = self.book.lines().len();
					new_reading.position = self.book.lines()[new_reading.line - 1].chars().count();
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

	fn build_reverse(text: &str, line: usize, m: Match) -> Option<ReverseInfo> {
		Some(ReverseInfo {
			line,
			start: char_index_for_byte(text, m.start()).unwrap(),
			end: char_index_for_byte(text, m.end()).unwrap(),
		})
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
		let mut position = byte_index_for_char(&lines[start_line], start_position).unwrap();
		for idx in start_line..lines.len() {
			let line = &lines[idx];
			match regex.find_at(line, position) {
				Some(m) => {
					self.reading.reverse = Self::build_reverse(line, idx, m);
					if !is_reverse_displayed(&self.reading, &self.render_context) {
						self.render.setup_reverse(lines, &mut self.reading, &mut self.render_context);
					}
					self.render.redraw(self.book.lines(), &self.reading, &mut self.render_context);
					self.push_trace(false);
					return Ok(());
				}
				None => (),
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
			let text = if idx == start_line {
				if start_position == 0 {
					continue;
				} else {
					let text = &lines[idx];
					let byte_index = byte_index_for_char(text, start_position).unwrap();
					&text[0..byte_index]
				}
			} else {
				&lines[idx]
			};
			match regex.find_iter(text).last() {
				Some(m) => {
					self.reading.reverse = Self::build_reverse(text, idx, m);
					self.render.setup_reverse(lines, &mut self.reading, &mut self.render_context);
					self.render.redraw(self.book.lines(), &self.reading, &mut self.render_context);
					self.push_trace(false);
					return Ok(());
				}
				None => (),
			}
		}
		Ok(())
	}

	fn push_trace(&mut self, clear_reverse: bool) {
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
		if clear_reverse {
			self.reading.reverse = None;
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
		self.reading.reverse = None;
		Ok(())
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
	let chars = lines[reading.line].chars().count();
	if reading.position >= chars {
		reading.position = 0;
	}
	Ok(book)
}