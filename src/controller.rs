use std::fmt::{Display, Formatter};
use std::marker::PhantomData;
use std::ops::Range;
use anyhow::{anyhow, bail, Result};
use fancy_regex::Regex;

use crate::{ContainerManager, Position};
use crate::book::{Book, Line};
use crate::common::TraceInfo;
use crate::config::{BookLoadingInfo, ReadingInfo};
use crate::container::{Container, load_book, load_container};

const TRACE_SIZE: usize = 100;

pub trait Render<C> {
	// init for book loaded
	fn book_loaded(&mut self, book: &dyn Book, reading: &ReadingInfo, context: &mut C);
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
	// selected text, line index for HighlightInfo.end
	Selection(String, usize),
}

pub struct HighlightInfo {
	pub line: usize,
	pub start: usize,
	pub end: usize,
	pub mode: HighlightMode,
}

pub struct ReadingStatus<'a> {
	pub title: Option<&'a str>,
	pub total_line: usize,
	pub current_line: usize,
}

impl<'a> ReadingStatus<'a> {
	#[inline]
	#[cfg(feature = "gui")]
	pub fn position(&self) -> String
	{
		format!("{}:{}", self.total_line, self.current_line)
	}
}

impl<'a> Display for ReadingStatus<'a> {
	#[inline]
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result
	{
		if let Some(title) = &self.title {
			write!(f, "{}({}:{})", title, self.total_line, self.current_line)
		} else {
			write!(f, "({}:{})", self.total_line, self.current_line)
		}
	}
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

	highlight: Option<HighlightInfo>,
	trace: Vec<TraceInfo>,
	current_trace: usize,
	next: Option<Position>,
}

impl<C, R: Render<C> + ?Sized> Controller<C, R>
{
	pub fn new(loading: BookLoadingInfo, render: Box<R>, render_context: &mut C) -> Result<Self>
	{
		let container_manager = Default::default();
		let mut container = load_container(&container_manager, loading.filename())?;
		let (book, reading) = load_book(&container_manager, &mut container, loading)?;
		Ok(Controller::from_data(
			reading,
			container_manager,
			container,
			book,
			render,
			render_context))
	}

	#[inline]
	pub fn from_data(reading: ReadingInfo, container_manager: ContainerManager,
		container: Box<dyn Container>, book: Box<dyn Book>, mut render: Box<R>,
		render_context: &mut C) -> Self
	{
		let trace = vec![TraceInfo { chapter: reading.chapter, line: reading.line, offset: reading.position }];
		render.book_loaded(book.as_ref(), &reading, render_context);
		Controller {
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
		}
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

	pub fn status(&self) -> ReadingStatus
	{
		let title = self.book
			.title(self.reading.line, self.reading.position);
		ReadingStatus {
			title,
			total_line: self.book.lines().len(),
			current_line: self.reading.line + 1,
		}
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
		self.render.book_loaded(self.book.as_ref(), &self.reading, context);
	}

	pub fn switch_container(&mut self, loading: BookLoadingInfo,
		context: &mut C) -> Result<String>
	{
		let mut container = load_container(&self.container_manager, loading.filename())?;
		let (book, reading) = load_book(
			&self.container_manager,
			&mut container, loading)?;
		self.container = container;
		self.book = book;
		self.reading = reading;
		self.trace.clear();
		self.trace.push(TraceInfo { chapter: self.reading.chapter, line: self.reading.line, offset: self.reading.position });
		self.current_trace = 0;
		self.book_loaded(context);
		self.redraw(context);
		Ok(self.status().to_string())
	}

	pub fn switch_book(&mut self, inner_book: usize, context: &mut C)
		-> Result<String>
	{
		self.do_switch_book(inner_book, context)
			.map(|_| self.status().to_string())
	}

	fn do_switch_book(&mut self, inner_book: usize, context: &mut C) -> Result<()>
	{
		let loading = if self.reading.inner_book == inner_book {
			// for reload content
			BookLoadingInfo::History(self.reading.clone())
		} else {
			self.reading.load_inner_book(inner_book)
		};
		let (book, reading) = load_book(&self.container_manager, &mut self.container, loading)?;
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
			if let Some(names) = self.container.inner_book_names() {
				let book_count = names.len();
				if book_index < book_count {
					self.do_switch_book(book_index, context)?;
				}
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
					let loading = BookLoadingInfo::NewReading(
						&reading.filename,
						reading.inner_book - 1,
						usize::MAX,
						reading.font_size);
					let (book, mut new_reading) = load_book(&self.container_manager, &mut self.container, loading)?;
					self.book = book;
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

	pub fn step_prev(&mut self, context: &mut C) -> Result<()>
	{
		let lines = self.book.lines();
		let reading = &self.reading;
		let line = reading.line;
		let offset = reading.position;
		if offset == 0 {
			if line == 0 {
				return self.prev_page(context);
			} else {
				let new_line = line - 1;
				let text = &lines[new_line];
				if text.len() == 0 {
					self.redraw_at(new_line, 0, context);
					return Ok(());
				}
			}
		}
		let position = self.render.prev_line(self.book.as_ref(), lines, line, offset, context);
		self.redraw_at(position.line, position.offset, context);
		Ok(())
	}

	pub fn step_next(&mut self, context: &mut C) -> Result<()>
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
					return Ok(());
				}
			}

			let position = self.render.next_line(self.book.as_ref(), lines, line, reading.position, context);
			self.redraw_at(position.line, position.offset, context);
		} else {
			self.switch_chapter(true, context)?;
		}
		Ok(())
	}

	#[inline]
	pub fn goto_toc(&mut self, toc_index: usize, context: &mut C) -> Option<String>
	{
		if let Some(trace_info) = self.book.toc_position(toc_index) {
			self.do_goto_toc(trace_info, context)
		} else {
			None
		}
	}

	fn do_goto_toc(&mut self, trace_info: TraceInfo, context: &mut C) -> Option<String>
	{
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
		Some(self.status().to_string())
	}

	pub fn switch_toc(&mut self, forward: bool, context: &mut C) -> Result<bool>
	{
		let toc_index = self.toc_index();
		let target_toc = if forward {
			self.book.toc_position(toc_index + 1)
		} else {
			// if not at the head of the toc, just goto the head of it
			let current_toc = self.book.toc_position(toc_index);
			let goto_prev = if let Some(toc) = &current_toc {
				toc.chapter == self.book.current_chapter()
					&& toc.line == self.reading.line
					&& toc.offset == self.reading.position
			} else {
				false
			};
			if goto_prev {
				if toc_index == 0 {
					return self.switch_chapter(false, context);
				} else {
					self.book.toc_position(toc_index - 1)
				}
			} else {
				current_toc
			}
		};
		if let Some(toc) = target_toc {
			self.do_goto_toc(toc, context);
			Ok(true)
		} else {
			Ok(false)
		}
	}

	pub fn switch_chapter(&mut self, forward: bool, context: &mut C) -> Result<bool>
	{
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
			| Some(HighlightInfo { mode: HighlightMode::Selection(..), .. })
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
			if let Some(range) = line.search_pattern_once(&regex, Some(position), None, false) {
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
					lines[idx].search_pattern_once(&regex, None, Some(start_position), true)
				}
			} else {
				lines[idx].search_pattern_once(&regex, None, None, true)
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

	pub fn goto(&mut self, inner_book: usize, chapter: usize, line: usize,
		offset: usize, highlight: Option<Range<usize>>, context: &mut C)
		-> Result<String>
	{
		let mut chapter_change = false;
		if inner_book != self.reading.inner_book {
			self.switch_book(inner_book, context)?;
			chapter_change = true;
		}
		if chapter_change || chapter != self.reading.chapter {
			if let Some(chapter_index) = self.book.goto_chapter(chapter)? {
				if chapter_index != chapter {
					bail!("Chapter {} not exists", chapter);
				}
			} else {
				bail!("Chapter {} not exists", chapter);
			}
			chapter_change = true;
		}
		self.reading.chapter = chapter;
		if let Some(highlight) = highlight {
			self.highlight = Some(HighlightInfo {
				line,
				start: highlight.start,
				end: highlight.end,
				mode: HighlightMode::Search,
			});
		} else {
			self.highlight = None;
		}
		if chapter_change {
			if offset == 0 {
				self.redraw_at(line, 0, context);
			} else {
				self.reading.line = line;
				self.reading.position = offset;
				self.step_prev(context)?;
			}
		} else {
			self.highlight_setup(context);
		};
		Ok(self.status().to_string())
	}

	pub fn switch_link_prev(&mut self, context: &mut C)
	{
		let (mut line, mut position) = match &self.highlight {
			Some(HighlightInfo { mode: HighlightMode::Link(..), line, start, .. }) => (*line, *start),
			None
			| Some(HighlightInfo { mode: HighlightMode::Selection(..), .. })
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
			| Some(HighlightInfo { mode: HighlightMode::Selection(..), .. })
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
			None | Some(HighlightInfo { mode: HighlightMode::Selection(..), .. }) => {}
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
				self.step_prev(context)?;
			}
		}
		Ok(())
	}

	#[allow(unused)]
	pub fn select_text(&mut self, from: Position, to: Position, context: &mut C)
	{
		self.highlight = self.book.range_highlight(from, to);
		self.redraw(context);
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

	#[allow(unused)]
	#[inline]
	pub fn clear_highlight(&mut self, context: &mut C)
	{
		if self.highlight.is_some() {
			self.highlight = None;
			self.redraw(context);
		}
	}

	#[inline]
	pub fn toc_index(&self) -> usize
	{
		self.book.toc_index(self.reading.line, self.reading.position)
	}

	#[inline]
	#[allow(unused)]
	pub fn selected(&self) -> Option<&str>
	{
		highlight_selection(&self.highlight)
	}

	#[inline]
	#[allow(unused)]
	pub fn has_selection(&self) -> bool
	{
		self.highlight.is_some()
	}

	#[inline]
	#[allow(unused)]
	pub fn reading_book_name(&self) -> &str
	{
		self.book
			.name()
			.unwrap_or_else(|| {
				self.container.book_name(self.reading.inner_book)
			})
	}
}

#[inline]
#[allow(unused)]
pub fn highlight_selection(highlight: &Option<HighlightInfo>) -> Option<&str>
{
	if let Some(HighlightInfo { mode: HighlightMode::Selection(selected_text, ..), .. }) = highlight {
		Some(selected_text)
	} else {
		None
	}
}