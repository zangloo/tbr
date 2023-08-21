use std::any::Any;
use std::cmp;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::fs::OpenOptions;
use std::io::Read;
use std::ops::Range;
use std::slice::Iter;
use anyhow::{anyhow, Result};
use fancy_regex::Regex;

use crate::book::epub::EpubLoader;
use crate::book::haodoo::HaodooLoader;
use crate::book::html::HtmlLoader;
use crate::book::txt::TxtLoader;
use crate::color::Color32;
use crate::common::{char_index_for_byte, Position};
use crate::common::TraceInfo;
use crate::container::BookContent;
use crate::container::BookContent::{Buf, File};
use crate::controller::{HighlightInfo, HighlightMode};

mod epub;
mod txt;
mod html;
mod haodoo;

pub const EMPTY_CHAPTER_CONTENT: &str = "No content.";
pub const IMAGE_CHAR: char = '🖼';
#[allow(unused)]
pub const HAN_CHAR: char = '漢';

type TextDecorationLine = lightningcss::properties::text::TextDecorationLine;

#[derive(Clone, Debug)]
pub enum TextStyle {
	Line(TextDecorationLine),
	Border,
	FontSize { scale: f32, relative: bool },
	Image(String),
	Link(String),
	Color(Color32),
	BackgroundColor(Color32),
}

#[cfg(feature = "gui")]
#[derive(Debug)]
pub struct CharStyle {
	pub font_scale: f32,
	pub color: Color32,
	pub background: Option<Color32>,
	pub line: Option<(TextDecorationLine, Range<usize>)>,
	pub border: Option<Range<usize>>,
	pub link: Option<(usize, Range<usize>)>,
	pub image: Option<String>,
}

#[derive(Clone)]
#[cfg(feature = "gui")]
pub struct Colors
{
	pub color: Color32,
	pub background: Color32,
	pub highlight: Color32,
	pub highlight_background: Color32,
	pub link: Color32,
}

#[cfg(feature = "gui")]
impl Default for Colors {
	fn default() -> Self {
		Colors {
			color: Color32::BLACK,
			background: Color32::WHITE,
			highlight: Color32::LIGHT_RED,
			highlight_background: Color32::LIGHT_GREEN,
			link: Color32::BLUE,
		}
	}
}

pub struct Line {
	chars: Vec<char>,
	styles: Vec<(TextStyle, Range<usize>)>,
}

pub struct Link<'a> {
	pub index: usize,
	pub target: &'a str,
	pub range: &'a Range<usize>,
}

impl Line {
	pub fn new(str: &str) -> Self {
		let mut chars = vec![];
		for ch in str.chars() {
			chars.push(ch);
		}
		Line { chars, styles: vec![] }
	}

	pub fn concat(&mut self, str: &str) {
		if str.len() == 0 {
			return;
		}
		let mut ignore_whitespace = true;
		for ch in str.chars() {
			if ch == '\r' {
				continue;
			}
			if ch == '\n' {
				ignore_whitespace = true;
				continue;
			}
			if ignore_whitespace && ch.is_ascii_whitespace() {
				continue;
			} else {
				ignore_whitespace = false;
			}
			self.chars.push(ch);
		}
	}

	pub fn push_style(&mut self, style: TextStyle, range: Range<usize>)
	{
		self.styles.push((style, range));
	}

	pub fn push(&mut self, ch: char) {
		if ch == '\0' {
			return;
		}
		self.chars.push(ch);
	}

	pub fn to_string(&self) -> String {
		let mut string = String::new();
		for char in &self.chars {
			string.push(*char)
		}
		string
	}

	pub fn len(&self) -> usize {
		self.chars.len()
	}

	pub fn is_empty(&self) -> bool {
		self.chars.is_empty()
	}

	pub fn char_at(&self, index: usize) -> Option<char> {
		match self.chars.get(index) {
			Some(ch) => Some(*ch),
			None => None,
		}
	}

	pub fn iter(&self) -> Iter<char> {
		self.chars.iter()
	}

	pub fn search_pattern(&self, regex: &Regex, start: Option<usize>, stop: Option<usize>, rev: bool) -> Option<Range<usize>> {
		let mut line = String::new();
		let start = start.unwrap_or(0);
		let stop = stop.unwrap_or(self.len());
		for index in start..stop {
			line.push(self.chars[index])
		}
		let m = if rev {
			regex.find_iter(&line).last()?.ok()?
		} else {
			regex.find_from_pos(&line, 0).ok()??
		};
		let match_start = char_index_for_byte(&line, m.start()).unwrap();
		let match_end = char_index_for_byte(&line, m.end()).unwrap();
		Some(Range { start: match_start + start, end: match_end + start })
	}

	pub fn link_iter<F, T>(&self, forward: bool, f: F) -> Option<T>
		where F: Fn(Link) -> (bool, Option<T>),
	{
		let range = 0..self.styles.len();
		let indeies: Vec<usize> = if forward {
			range.collect()
		} else {
			range.rev().collect()
		};
		for index in indeies {
			let style = &self.styles[index];
			match style {
				(TextStyle::Link(target), range) => {
					let (stop, found) = f(Link {
						index,
						target,
						range,
					});
					if stop {
						return found;
					}
				}
				_ => continue,
			}
		}
		None
	}

	pub fn link_at(&self, link_index: usize) -> Option<Link> {
		if let Some((TextStyle::Link(target), range)) = self.styles.get(link_index) {
			Some(Link {
				index: link_index,
				target,
				range,
			})
		} else {
			None
		}
	}

	#[cfg(feature = "gui")]
	pub fn char_style_at(&self, char_index: usize, custom_color: bool,
		colors: &Colors) -> CharStyle
	{
		let mut char_style = CharStyle {
			font_scale: 1.0,
			color: colors.color.clone(),
			background: None,
			line: None,
			border: None,
			link: None,
			image: None,
		};
		for (index, (style, range)) in self.styles.iter().enumerate().rev() {
			if range.contains(&char_index) {
				match style {
					TextStyle::FontSize { scale, relative } => {
						if *relative {
							char_style.font_scale *= scale;
						} else {
							char_style.font_scale = *scale;
						}
					}
					TextStyle::Image(href) => char_style.image = Some(href.clone()),
					TextStyle::Link(_) => {
						char_style.link = Some((index, range.clone()));
						char_style.color = colors.link.clone();
					}
					TextStyle::Border => char_style.border = Some(range.clone()),
					TextStyle::Line(line) => char_style.line = Some((*line, range.clone())),
					TextStyle::Color(color) => if custom_color { char_style.color = color.clone() },
					TextStyle::BackgroundColor(color) => if custom_color { char_style.background = Some(color.clone()) },
				}
			}
		}
		char_style
	}
}

impl Default for Line {
	fn default() -> Self {
		Line { chars: vec![], styles: vec![] }
	}
}

impl PartialEq for Line {
	fn eq(&self, other: &Self) -> bool {
		let len = self.len();
		if len != other.len() {
			return false;
		}
		let mut iter1 = self.chars.iter();
		let mut iter2 = self.chars.iter();
		loop {
			if let Some(ch1) = iter1.next() {
				let ch2 = iter2.next().unwrap();
				if ch1 != ch2 {
					return false;
				}
			} else {
				break;
			}
		}
		return true;
	}
}

pub enum LoadingChapter {
	Index(usize),
	Last,
}

pub trait AsAny: 'static {
	fn as_any(&mut self) -> &mut dyn Any;
}

impl<T: 'static> AsAny for T {
	fn as_any(&mut self) -> &mut dyn Any
	{
		self
	}
}

pub trait Book: AsAny {
	fn chapter_count(&self) -> usize { 1 }
	fn prev_chapter(&mut self) -> Result<Option<usize>>
	{
		let current = self.current_chapter();
		if current == 0 {
			Ok(None)
		} else {
			self.goto_chapter(current - 1)
		}
	}

	fn next_chapter(&mut self) -> Result<Option<usize>>
	{
		self.goto_chapter(self.current_chapter() + 1)
	}

	fn goto_chapter(&mut self, chapter_index: usize) -> Result<Option<usize>>
	{
		if chapter_index >= self.chapter_count() {
			return Ok(None);
		} else {
			Ok(Some(chapter_index))
		}
	}
	fn current_chapter(&self) -> usize { 0 }
	fn title(&self, _line: usize, _offset: usize) -> Option<&str> { None }
	fn toc_index(&self, _line: usize, _offset: usize) -> usize { 0 }
	fn toc_iterator(&self) -> Option<Box<dyn Iterator<Item=(&str, usize)> + '_>> { None }
	fn toc_position(&mut self, _toc_index: usize) -> Option<TraceInfo> { None }
	fn lines(&self) -> &Vec<Line>;
	fn leading_space(&self) -> usize { 2 }
	fn link_position(&mut self, _line: usize, _link_index: usize) -> Option<TraceInfo> { None }
	// (absolute path, content)
	fn image(&self, _href: &str) -> Option<(String, &[u8])> { None }
	fn range_highlight(&self, from: Position, to: Position)
		-> Option<HighlightInfo>
	{
		#[inline]
		fn push_chars(line: &Line, range: Range<usize>, text: &mut String)
		{
			if !text.is_empty() {
				text.push('\n');
			}
			for offset in range {
				text.push(line.char_at(offset).unwrap())
			}
		}

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
		let lines = self.lines();
		let lines_count = lines.len();
		if lines_count == 0 {
			return None;
		}
		let mut selected_text = String::new();
		let (line_to, offset_to) = if line2 >= lines_count {
			(lines_count - 1, usize::MAX)
		} else {
			(line2, offset2)
		};
		let mut offset_from = offset1;
		for line in line1..line_to {
			let text = &lines[line];
			push_chars(text, offset_from..text.len(), &mut selected_text);
			offset_from = 0;
		}
		let last_text = &lines[line_to];
		let offset_to = cmp::min(last_text.len(), offset_to);
		push_chars(last_text, offset_from..offset_to, &mut selected_text);

		if selected_text.len() == 0 {
			None
		} else {
			let highlight = HighlightInfo {
				line: line1,
				start: offset1,
				end: offset_to,
				mode: HighlightMode::Selection(selected_text, line_to),
			};
			Some(highlight)
		}
	}
}

pub struct BookLoader {
	loaders: Vec<Box<dyn Loader>>,
}

pub(crate) trait Loader {
	fn extensions(&self) -> &Vec<&'static str>;
	fn support(&self, filename: &str) -> bool {
		let filename = filename.to_lowercase();
		for extension in self.extensions() {
			if filename.ends_with(extension) {
				return true;
			}
		}
		false
	}
	fn load_file(&self, filename: &str, mut file: std::fs::File, loading_chapter: LoadingChapter) -> Result<Box<dyn Book>> {
		let mut content: Vec<u8> = Vec::new();
		file.read_to_end(&mut content)?;
		self.load_buf(filename, content, loading_chapter)
	}
	fn load_buf(&self, filename: &str, buf: Vec<u8>, loading_chapter: LoadingChapter) -> Result<Box<dyn Book>>;
}

impl BookLoader {
	#[allow(unused)]
	pub fn extension(&self) -> Vec<&'static str>
	{
		let mut vec = vec![];
		for loader in self.loaders.iter() {
			for ext in loader.extensions() {
				vec.push(*ext);
			}
		}
		vec
	}

	pub fn support(&self, filename: &str) -> bool {
		for loader in self.loaders.iter() {
			if loader.support(filename) {
				return true;
			}
		}
		false
	}
	pub fn load(&self, filename: &str, content: BookContent, loading_chapter: LoadingChapter) -> Result<Box<dyn Book>> {
		for loader in self.loaders.iter() {
			if loader.support(filename) {
				let book = match content {
					File(..) => {
						let file = OpenOptions::new().read(true).open(filename)?;
						loader.load_file(filename, file, loading_chapter)?
					}
					Buf(buf) => loader.load_buf(filename, buf, loading_chapter)?,
				};
				return Ok(book);
			}
		}
		Err(anyhow!("Not support open book: {}", filename))
	}
}

impl Default for BookLoader {
	fn default() -> Self {
		let mut loaders: Vec<Box<dyn Loader>> = Vec::new();
		loaders.push(Box::new(TxtLoader::new()));
		loaders.push(Box::new(EpubLoader::new()));
		loaders.push(Box::new(HtmlLoader::new()));
		loaders.push(Box::new(HaodooLoader::new()));
		BookLoader { loaders }
	}
}

pub struct ChapterError {
	msg: String,
}

impl Debug for ChapterError {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		f.write_str(&format!("Chapter error: {}", self.msg))
	}
}

impl Display for ChapterError {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		f.write_str(&format!("Chapter error: {}", self.msg))
	}
}

impl Error for ChapterError {}

impl ChapterError
{
	#[inline]
	pub fn new(msg: String) -> Self
	{
		ChapterError { msg }
	}

	#[inline]
	pub fn anyhow(msg: String) -> anyhow::Error
	{
		anyhow::Error::new(ChapterError::new(msg))
	}
}