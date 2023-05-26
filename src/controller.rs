#[cfg(feature = "gui")]
use std::cmp;
use std::marker::PhantomData;
use anyhow::{anyhow, Result};
use fancy_regex::Regex;

use crate::{ContainerManager, Position, ReadingInfo};
use crate::book::{Book, Line};
use crate::common::TraceInfo;
use crate::container::{Container, load_book, load_container};

const TRACE_SIZE: usize = 100;

pub trait Render<C> {
	// init for book loaded
	fn book_loaded(&mut self, book: &dyn Book, context: &mut C);
	// return next
	fn redraw(&mut self, book: &dyn Book, lines: &Vec<Line>, line: usize, offset: usize, highlight: &Option<HighlightInfo>, context: &mut C) -> Option<Position>;
	// return new position
	fn prev_page(&mut self, book: &dyn Book, lines: &Vec<Line>, line: usize, offset: usize, context: &mut C) -> Position;
	// return new position
	fn next_line(&mut self, book: &dyn Book, lines: &Vec<Line>, line: usize, offset: usize, context: &mut C) -> Position;
	// return new position
	fn prev_line(&mut self, book: &dyn Book, lines: &Vec<Line>, line: usize, offset: usize, context: &mut C) -> Position;
	// move to highlight line if not displayed in current view
	fn setup_highlight(&mut self, book: &dyn Book, lines: &Vec<Line>, line: usize, start: usize, context: &mut C) -> Position;
}

#[derive(Clone)]
pub enum HighlightMode {
	Search,
	// link index for current line
	Link(usize),
	// line index for HighlightInfo.end
	#[allow(dead_code)]
	Selection(usize),
}

pub struct HighlightInfo {
	pub line: usize,
	pub start: usize,
	pub end: usize,
	pub mode: HighlightMode,
}

pub struct Controller<C, R: Render<C> + ?Sized>
{
	_render_context: PhantomData<C>,
	pub container_manager: ContainerManager,
	pub container: Box<dyn Container>,
	pub book: Box<dyn Book>,
	pub reading: ReadingInfo,
	pub search_pattern: String,
	pub render: Box<R>,

	pub highlight: Option<HighlightInfo>,
	trace: Vec<TraceInfo>,
	current_trace: usize,
	next: Option<Position>,
}

impl<C, R: Render<C> + ?Sized> Controller<C, R>
{
	pub fn new(mut reading: ReadingInfo, render: Box<R>) -> Result<Self>
	{
		let container_manager = Default::default();
		let mut container = load_container(&container_manager, &reading)?;
		let book = load_book(&container_manager, &mut container, &mut reading)?;
		Controller::from_data(reading, container_manager, container, book, render)
	}

	#[inline]
	pub fn from_data(reading: ReadingInfo, container_manager: ContainerManager, container: Box<dyn Container>, book: Box<dyn Book>, render: Box<R>) -> Result<Self>
	{
		let trace = vec![TraceInfo { chapter: reading.chapter, line: reading.line, offset: reading.position }];
		Ok(Controller {
			_render_context: PhantomData,
			container_manager,
			container,
			book,
			reading,
			search_pattern: "".to_string(),
			trace,
			current_trace: 0,
			highlight: None,
			next: None,
			render,
		})
	}
	#[inline]
	pub fn reading_container(&self) -> &dyn Container
	{
		self.container.as_ref()
	}

	#[inline]
	pub fn reading_book(&self) -> &dyn Book
	{
		self.book.as_ref()
	}

	#[inline]
	pub fn reading_info(&self) -> &ReadingInfo
	{
		&self.reading
	}

	#[inline]
	pub fn redraw(&mut self, context: &mut C)
	{
		let next = self.render.redraw(
			self.book.as_ref(),
			self.book.lines(),
			self.reading.line,
			self.reading.position,
			&self.highlight,
			context);
		self.next = next;
	}

	#[inline]
	pub fn redraw_at(&mut self, line: usize, offset: usize, context: &mut C)
	{
		let next = self.render.redraw(
			self.book.as_ref(),
			self.book.lines(),
			line,
			offset,
			&self.highlight,
			context);
		self.reading.line = line;
		self.reading.position = offset;
		self.next = next;
		self.push_trace(true);
	}

	#[inline]
	pub fn search_pattern(&self) -> &str
	{
		&self.search_pattern
	}

	pub fn status_msg(&self) -> String
	{
		let title = match self.book.title(self.reading.line, self.reading.position) {
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

	pub fn search(&mut self, pattern: &str, context: &mut C) -> Result<()>
	{
		self.search_pattern = String::from(pattern);
		self.search_next(self.reading.line, self.reading.position, context)
	}

	#[inline]
	pub fn book_loaded(&mut self, context: &mut C)
	{
		self.highlight = None;
		self.render.book_loaded(self.book.as_ref(), context);
	}

	pub fn switch_container(&mut self, mut reading: ReadingInfo, context: &mut C) -> Result<String> {
		let mut container = load_container(&self.container_manager, &reading)?;
		let book = load_book(&self.container_manager, &mut container, &mut reading)?;
		self.container = container;
		self.book = book;
		self.reading = reading;
		self.trace.clear();
		self.trace.push(TraceInfo { chapter: self.reading.chapter, line: self.reading.line, offset: self.reading.position });
		self.current_trace = 0;
		self.book_loaded(context);
		self.redraw(context);
		Ok(self.status_msg())
	}

	pub fn switch_book(&mut self, reading: ReadingInfo, context: &mut C) -> String
	{
		match self.do_switch_book(reading, context) {
			Ok(..) => self.status_msg(),
			Err(e) => e.to_string(),
		}
	}

	fn do_switch_book(&mut self, mut reading: ReadingInfo, context: &mut C) -> Result<()>
	{
		let book = load_book(&self.container_manager, &mut self.container, &mut reading)?;
		self.book = book;
		self.reading = reading;
		self.trace.clear();
		self.trace.push(TraceInfo { chapter: self.reading.chapter, line: self.reading.line, offset: self.reading.position });
		self.current_trace = 0;
		self.book_loaded(context);
		self.redraw(context);
		Ok(())
	}

	pub fn goto_line(&mut self, line: usize, context: &mut C) -> Result<()>
	{
		let lines = &self.book.lines();
		if line > lines.len() || line == 0 {
			return Err(anyhow!("Invalid line number: {}", line));
		}
		self.redraw_at(line - 1, 0, context);
		Ok(())
	}

	pub fn next_page(&mut self, context: &mut C) -> Result<()> {
		if let Some(next) = &self.next {
			let line = next.line;
			let offset = next.offset;
			self.redraw_at(line, offset, context);
		} else if !self.switch_chapter(true, context)? {
			let book_index = self.reading.inner_book + 1;
			let book_count = self.container.inner_book_names().len();
			if book_index < book_count {
				let reading = ReadingInfo::new(&self.reading.filename)
					.with_inner_book(book_index);
				self.do_switch_book(reading, context)?;
			}
		}
		Ok(())
	}

	pub fn prev_page(&mut self, context: &mut C) -> Result<()>
	{
		if self.reading.line == 0 && self.reading.position == 0 {
			let reading = &mut self.reading;
			if let Some(current_chapter) = self.book.prev_chapter()? {
				reading.chapter = current_chapter;
				let lines = self.book.lines();
				// prev need decrease this invalid reading.line
				let position = self.render.prev_page(self.book.as_ref(), lines, lines.len(), 0, context);
				self.redraw_at(position.line, position.offset, context);
			} else {
				if reading.inner_book > 0 {
					let mut new_reading = ReadingInfo::new(&reading.filename)
						.with_inner_book(reading.inner_book - 1)
						.with_last_chapter();
					self.book = load_book(&self.container_manager, &mut self.container, &mut new_reading)?;
					let lines = self.book.lines();
					let line_index = lines.len() - 1;
					let position = self.render.prev_page(self.book.as_ref(), lines, line_index, lines[line_index].len(), context);
					new_reading.chapter = self.book.current_chapter();
					new_reading.line = position.line;
					new_reading.position = position.offset;
					self.reading = new_reading;
					self.trace.clear();
					self.trace.push(TraceInfo { chapter: self.reading.chapter, line: self.reading.line, offset: self.reading.position });
					self.current_trace = 0;
					self.book_loaded(context);
					self.redraw(context);
				}
			}
		} else {
			let position = self.render.prev_page(self.book.as_ref(), self.book.lines(), self.reading.line, self.reading.position, context);
			self.redraw_at(position.line, position.offset, context);
		}
		Ok(())
	}

	pub fn goto_end(&mut self, context: &mut C)
	{
		let lines = self.book.lines();
		let position = self.render.prev_page(self.book.as_ref(), lines, lines.len(), 0, context);
		self.redraw_at(position.line, position.offset, context);
	}

	pub fn step_prev(&mut self, context: &mut C)
	{
		let lines = self.book.lines();
		let reading = &self.reading;
		let line = reading.line;
		let offset = reading.position;
		if offset == 0 {
			if line == 0 {
				return;
			} else {
				let new_line = line - 1;
				let text = &lines[new_line];
				if text.len() == 0 {
					self.redraw_at(new_line, 0, context);
					return;
				}
			}
		}
		let position = self.render.prev_line(self.book.as_ref(), lines, line, offset, context);
		self.redraw_at(position.line, position.offset, context);
	}

	pub fn step_next(&mut self, context: &mut C)
	{
		if self.next.is_some() {
			let lines = self.book.lines();
			let reading = &self.reading;
			let line = reading.line;

			let text = &lines[line];
			if text.len() == 0 {
				let new_line = line + 1;
				if line < lines.len() {
					self.redraw_at(new_line, 0, context);
					return;
				}
			}

			let position = self.render.next_line(self.book.as_ref(), lines, line, reading.position, context);
			self.redraw_at(position.line, position.offset, context);
		}
	}

	pub fn goto_toc(&mut self, toc_index: usize, context: &mut C) -> Option<String> {
		if let Some(trace_info) = self.book.toc_position(toc_index) {
			if self.reading.chapter != trace_info.chapter {
				if let Ok(Some(new_chapter)) = self.book.goto_chapter(trace_info.chapter) {
					self.reading.chapter = new_chapter;
					if new_chapter == trace_info.chapter {
						self.reading.line = trace_info.line;
						self.reading.position = trace_info.offset;
					} else {
						// would happen???
						self.reading.line = 0;
						self.reading.position = 0;
					}
				} else {
					return None;
				}
			} else {
				self.reading.line = trace_info.line;
				self.reading.position = trace_info.offset;
			}
			self.push_trace(true);
			self.redraw(context);
			Some(self.status_msg())
		} else {
			None
		}
	}

	pub fn switch_chapter(&mut self, forward: bool, context: &mut C) -> Result<bool> {
		let option = if forward {
			self.book.next_chapter()?
		} else {
			self.book.prev_chapter()?
		};
		if let Some(new_chapter) = option {
			self.reading.chapter = new_chapter;
			self.reading.line = 0;
			self.reading.position = 0;
			self.push_trace(true);
			self.redraw(context);
			Ok(true)
		} else {
			Ok(false)
		}
	}

	pub fn search_again(&mut self, forward: bool, context: &mut C) -> Result<()>
	{
		let (line, position) = match &self.highlight {
			Some(HighlightInfo { mode: HighlightMode::Search, line, start, end }) => (*line, if forward { *end } else { *start }),
			None
			| Some(HighlightInfo { mode: HighlightMode::Selection(_), .. })
			| Some(HighlightInfo { mode: HighlightMode::Link(..), .. }) => (self.reading.line, self.reading.position),
		};
		if forward {
			self.search_next(line, position, context)?;
		} else {
			self.search_prev(line, position, context)?;
		}
		Ok(())
	}

	fn search_next(&mut self, start_line: usize, start_position: usize, context: &mut C) -> Result<()> {
		let book = self.book.as_ref();
		let lines = book.lines();
		let regex = Regex::new(&self.search_pattern)?;
		let mut position = start_position;
		for idx in start_line..lines.len() {
			let line = &lines[idx];
			if let Some(range) = line.search_pattern(&regex, Some(position), None, false) {
				self.highlight = Some(HighlightInfo {
					line: idx,
					start: range.start,
					end: range.end,
					mode: HighlightMode::Search,
				});
				self.highlight_setup(context);
				return Ok(());
			}
			position = 0;
		}
		Ok(())
	}

	fn search_prev(&mut self, start_line: usize, start_position: usize, context: &mut C) -> Result<()> {
		let lines = self.book.lines();
		let regex = Regex::new(&self.search_pattern)?;
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
				self.highlight = Some(HighlightInfo {
					line: idx,
					start: range.start,
					end: range.end,
					mode: HighlightMode::Search,
				});
				self.highlight_setup(context);
				return Ok(());
			}
		}
		Ok(())
	}

	fn push_trace(&mut self, clear_highlight: bool) {
		let reading = &self.reading;
		let trace = &mut self.trace;
		let last = &trace[self.current_trace];
		if last.chapter == reading.chapter && last.line == reading.line && last.offset == reading.position {
			return;
		}
		trace.drain(self.current_trace + 1..);
		trace.push(TraceInfo { chapter: reading.chapter, line: reading.line, offset: reading.position });
		if trace.len() > TRACE_SIZE {
			trace.drain(0..1);
		} else {
			self.current_trace += 1;
		}
		if clear_highlight {
			self.highlight = None;
		}
	}

	pub fn goto_trace(&mut self, backward: bool, context: &mut C) -> Result<()>
	{
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
			reading.position = current_trace.offset;
		} else if let Some(new_chapter) = self.book.goto_chapter(current_trace.chapter)? {
			assert_eq!(new_chapter, current_trace.chapter);
			reading.chapter = new_chapter;
			reading.line = current_trace.line;
			reading.position = current_trace.offset;
		} else {
			return Ok(());
		}
		self.highlight = None;
		self.redraw(context);
		Ok(())
	}

	pub fn switch_link_prev(&mut self, context: &mut C)
	{
		let (mut line, mut position) = match &self.highlight {
			Some(HighlightInfo { mode: HighlightMode::Link(..), line, start, .. }) => (*line, *start),
			None
			| Some(HighlightInfo { mode: HighlightMode::Selection(_), .. })
			| Some(HighlightInfo { mode: HighlightMode::Search, .. }) => (self.reading.line, self.reading.position)
		};
		let lines = self.book.lines();
		let mut text = &lines[line];
		loop {
			if let Some(highlight) = text.link_iter(false, |link| {
				if link.range.end <= position {
					return (true, Some(HighlightInfo {
						line,
						start: link.range.start,
						end: link.range.end,
						mode: HighlightMode::Link(link.index),
					}));
				}
				(false, None)
			}) {
				self.highlight = Some(highlight);
				break true;
			}
			if line == 0 {
				break false;
			}
			line -= 1;
			text = &lines[line];
			position = text.len();
		};
		self.highlight_setup(context);
	}

	pub fn switch_link_next(&mut self, context: &mut C)
	{
		let (line, mut position) = match &self.highlight {
			Some(HighlightInfo { mode: HighlightMode::Link(..), line, end, .. }) => (*line, *end),
			None
			| Some(HighlightInfo { mode: HighlightMode::Selection(_), .. })
			| Some(HighlightInfo { mode: HighlightMode::Search, .. }) => (self.reading.line, self.reading.position),
		};
		let lines = self.book.lines();
		for index in line..lines.len() {
			let text = &lines[index];
			if let Some(highlight) = text.link_iter(true, |link| {
				if link.range.start >= position {
					return (true, Some(HighlightInfo {
						line: index,
						start: link.range.start,
						end: link.range.end,
						mode: HighlightMode::Link(link.index),
					}));
				}
				(false, None)
			}) {
				self.highlight = Some(highlight);
				break;
			}
			position = 0;
		}
		self.highlight_setup(context);
	}

	pub fn try_goto_link(&mut self, context: &mut C) -> Result<()>
	{
		match self.highlight {
			Some(HighlightInfo { mode: HighlightMode::Search, line, start, end }) => {
				let text = &self.book.lines()[line];
				if let Some(link_index) = text.link_iter(true, |link| {
					let range = &link.range;
					if range.start <= start && range.end >= end {
						return (true, Some(link.index));
					}
					(false, None)
				}) {
					self.goto_link(line, link_index, context)?;
				}
			}
			Some(HighlightInfo { mode: HighlightMode::Link(link_index), line, .. }) => {
				self.goto_link(line, link_index, context)?;
			}
			None | Some(HighlightInfo { mode: HighlightMode::Selection(_), .. }) => {}
		}
		Ok(())
	}

	pub fn goto_link(&mut self, line: usize, link_index: usize, context: &mut C) -> Result<()>
	{
		if let Some(pos) = self.book.link_position(line, link_index) {
			if pos.chapter != self.book.current_chapter() {
				if let Some(new_chapter) = self.book.goto_chapter(pos.chapter)? {
					assert_eq!(new_chapter, pos.chapter);
					self.reading.chapter = new_chapter;
				}
			}
			self.reading.line = pos.line;
			self.reading.position = pos.offset;
			if pos.offset == 0 {
				self.push_trace(true);
				self.redraw(context);
			} else {
				self.step_prev(context);
			}
		}
		Ok(())
	}

	#[cfg(feature = "gui")]
	pub fn select_text(&mut self, from: Position, to: Position, context: &mut C) -> String
	{
		self.highlight = None;
		let mut selected_text = String::new();
		let (line1, offset1, line2, offset2) = if from.line > to.line {
			(to.line, to.offset, from.line, from.offset + 1)
		} else if from.line == to.line {
			if from.offset >= to.offset {
				(to.line, to.offset, from.line, from.offset + 1)
			} else {
				(from.line, from.offset, to.line, to.offset + 1)
			}
		} else {
			(from.line, from.offset, to.line, to.offset + 1)
		};
		let lines = self.book.lines();
		let lines_count = lines.len();
		if lines_count == 0 {
			self.redraw(context);
			return selected_text;
		}
		let (line_to, offset_to) = if line2 >= lines_count {
			(lines_count - 1, usize::MAX)
		} else {
			(line2, offset2)
		};
		let mut offset_from = offset1;
		for line in line1..line_to {
			let text = &lines[line];
			for offset in offset_from..text.len() {
				selected_text.push(text.char_at(offset).unwrap())
			}
			offset_from = 0;
		}
		let last_text = &lines[line_to];
		let offset_to = cmp::min(last_text.len(), offset_to);
		for offset in offset_from..offset_to {
			selected_text.push(last_text.char_at(offset).unwrap())
		}
		if selected_text.len() == 0 {
			self.redraw(context);
			return selected_text;
		}
		let highlight = HighlightInfo {
			line: line1,
			start: offset1,
			end: offset_to,
			mode: HighlightMode::Selection(line_to),
		};
		self.highlight = Some(highlight);
		self.redraw(context);
		selected_text
	}

	fn highlight_setup(&mut self, context: &mut C)
	{
		if let Some(highlight) = &self.highlight {
			let highlight_line = highlight.line;
			let highlight_start = highlight.start;
			let reading_line = self.reading.line;
			let reading_position = self.reading.position;
			let in_current_screen = if (highlight_line == reading_line && highlight_start >= reading_position) || (highlight_line > reading_line) {
				match &self.next {
					Some(next) => if (highlight_line == next.line && highlight_start < next.offset) || (highlight_line < next.line) {
						true
					} else {
						false
					}
					None => true,
				}
			} else {
				false
			};
			if !in_current_screen {
				let position = self.render.setup_highlight(self.book.as_ref(), self.book.lines(), highlight_line, highlight_start, context);
				self.reading.line = position.line;
				self.reading.position = position.offset;
				self.push_trace(false);
			}
		}
		self.redraw(context);
	}

	#[cfg(feature = "gui")]
	pub fn clear_highlight(&mut self, context: &mut C)
	{
		self.highlight = None;
		self.redraw(context);
	}

	#[inline]
	pub fn toc_index(&self) -> usize
	{
		self.book.toc_index(self.reading.line, self.reading.position)
	}
}
