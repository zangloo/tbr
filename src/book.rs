use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::fs::OpenOptions;
use std::io::Read;
use std::ops::Range;
use std::slice::Iter;

use anyhow::{anyhow, Result};
use regex::Regex;

use crate::book::epub::EpubLoader;
use crate::book::haodoo::HaodooLoader;
use crate::book::html::HtmlLoader;
use crate::book::txt::TxtLoader;
use crate::common::char_index_for_byte;
use crate::container::BookContent;
use crate::container::BookContent::{Buf, File};
use crate::list::ListEntry;
use crate::common::TraceInfo;

mod epub;
mod txt;
mod html;
mod haodoo;

pub const EMPTY_CHAPTER_CONTENT: &str = "No content.";

#[derive(Clone)]
pub enum TextStyle {
	Underline,
	Border,
	Scale(u8),
}

pub enum StylePosition {
	Start,
	Middle,
	End,
	Single,
}

pub struct Line {
	chars: Vec<char>,
	links: Vec<Link>,
	image: bool,
	styles: Vec<(TextStyle, Range<usize>)>,
}

pub struct Link {
	pub target: String,
	pub range: Range<usize>,
}

impl Line {
	pub fn push(&mut self, ch: char) {
		if ch == '\0' {
			return;
		}
		self.chars.push(ch);
	}

	pub fn clear(&mut self) {
		self.chars.clear();
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
			Some(ch) => Some(ch.clone()),
			None => None,
		}
	}

	pub fn iter(&self) -> Iter<char> {
		self.chars.iter()
	}

	pub fn trim(&mut self) {
		for index in (0..self.chars.len()).rev() {
			if self.chars[index].is_whitespace() {
				self.chars.pop();
			} else {
				break;
			}
		}
		let mut trim_start = 0;
		for (index, ch) in self.chars.iter().enumerate() {
			if ch.is_whitespace() {
				trim_start = index + 1;
			} else {
				break;
			}
		}
		if trim_start == 0 {
			return;
		}
		if trim_start == self.chars.len() {
			self.chars.clear();
			return;
		}
		self.chars.drain(0..trim_start);
	}

	pub fn search_pattern(&self, regex: &Regex, start: Option<usize>, stop: Option<usize>, rev: bool) -> Option<Range<usize>> {
		let mut line = String::new();
		let start = start.unwrap_or(0);
		let stop = stop.unwrap_or(self.len());
		for index in start..stop {
			line.push(self.chars[index])
		}
		let m = if rev {
			regex.find_iter(&line).last()
		} else {
			regex.find_at(&line, 0)
		}?;
		let match_start = char_index_for_byte(&line, m.start()).unwrap();
		let match_end = char_index_for_byte(&line, m.end()).unwrap();
		Some(Range { start: match_start + start, end: match_end + start })
	}

	pub fn add_link(&mut self, target: &str, start: usize, end: usize) {
		let link = Link { target: String::from(target), range: Range { start, end } };
		self.links.push(link);
	}

	pub fn link_iter(&self) -> Iter<Link> {
		self.links.iter()
	}

	pub fn link_at(&self, link_index: usize) -> Option<&Link> {
		self.links.get(link_index)
	}

	pub fn is_image(&self) -> bool
	{
		self.image
	}

	// in percent
	pub fn char_scale_at(&self, index: usize) -> u8
	{
		for (style, range) in &self.styles {
			if range.contains(&index) {
				match style {
					TextStyle::Scale(scale) => return *scale,
					_ => continue,
				}
			}
		}
		100
	}

	pub fn char_style_at(&self, index: usize) -> Option<(TextStyle, StylePosition)>
	{
		for (style, range) in &self.styles {
			if range.contains(&index) {
				match style {
					TextStyle::Scale(_) => continue,
					_ => {
						let position = if range.len() == 1 {
							StylePosition::Single
						} else if index == range.start {
							StylePosition::Start
						} else if index == range.end - 1 {
							StylePosition::End
						} else {
							StylePosition::Middle
						};
						return Some((style.clone(), position));
					}
				}
			}
		}
		None
	}
}

impl Default for Line {
	fn default() -> Self {
		Line { chars: vec![], links: vec![], image: false, styles: vec![] }
	}
}

impl From<&str> for Line {
	fn from(str: &str) -> Self {
		let mut chars = vec![];
		for ch in str.chars() {
			chars.push(ch);
		}
		Line { chars, links: vec![], image: false, styles: vec![] }
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

pub trait Book {
	fn chapter_count(&self) -> usize { 1 }
	fn prev_chapter(&mut self) -> Result<Option<usize>> {
		let current = self.current_chapter();
		if current == 0 {
			Ok(None)
		} else {
			self.goto_chapter(current - 1)
		}
	}
	fn next_chapter(&mut self) -> Result<Option<usize>> {
		self.goto_chapter(self.current_chapter() + 1)
	}
	fn goto_chapter(&mut self, chapter_index: usize) -> Result<Option<usize>> {
		if chapter_index >= self.chapter_count() {
			return Ok(None);
		} else {
			Ok(Some(chapter_index))
		}
	}
	fn current_chapter(&self) -> usize { 0 }
	fn title(&self) -> Option<&String> { None }
	fn toc_index(&self) -> usize { 0 }
	fn toc_list(&self) -> Option<Vec<ListEntry>> { None }
	fn toc_position(&mut self, _toc_index: usize) -> Option<TraceInfo> { None }
	fn lines(&self) -> &Vec<Line>;
	fn leading_space(&self) -> usize { 2 }
	fn link_position(&mut self, _line: usize, _link_index: usize) -> Option<TraceInfo> { None }
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
	fn load_file(&self, filename: &str, mut file: std::fs::File, chapter_index: usize) -> Result<Box<dyn Book>> {
		let mut content: Vec<u8> = Vec::new();
		file.read_to_end(&mut content)?;
		self.load_buf(filename, content, chapter_index)
	}
	fn load_buf(&self, filename: &str, buf: Vec<u8>, chapter_index: usize) -> Result<Box<dyn Book>>;
}

impl BookLoader {
	pub fn support(&self, filename: &str) -> bool {
		for loader in self.loaders.iter() {
			if loader.support(filename) {
				return true;
			}
		}
		false
	}
	pub fn load(&self, filename: &str, content: BookContent, chapter: usize) -> Result<Box<dyn Book>> {
		for loader in self.loaders.iter() {
			if loader.support(filename) {
				let book = match content {
					File(..) => {
						let file = OpenOptions::new().read(true).open(filename)?;
						loader.load_file(filename, file, chapter)?
					}
					Buf(buf) => loader.load_buf(filename, buf, chapter)?,
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

pub(crate) struct InvalidChapterError {}

const INVALID_CHAPTER_ERROR_MESSAGE: &str = "invalid chapter";

impl Debug for InvalidChapterError {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		f.write_str(INVALID_CHAPTER_ERROR_MESSAGE)
	}
}

impl Display for InvalidChapterError {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		f.write_str(INVALID_CHAPTER_ERROR_MESSAGE)
	}
}

impl Error for InvalidChapterError {}

#[cfg(test)]
mod tests {
	use crate::book::Line;

	#[test]
	fn test_trim() {
		let result = Line::from("测 \t试");
		let mut s = Line::from(" \t 测 \t试  ");
		s.trim();
		assert_eq!(s == result, true);
		let mut s = Line::from("\t测 \t试  ");
		s.trim();
		assert_eq!(s == result, true);
		let mut s = Line::from("测 \t试  ");
		s.trim();
		assert_eq!(s == result, true);
		let mut s = Line::from(" \t 测 \t试");
		s.trim();
		assert_eq!(s == result, true);
		let mut s = Line::from("测 \t试");
		s.trim();
		assert_eq!(s == result, true);
		let mut s = Line::from("   \t    ");
		s.trim();
		assert_eq!(s == Line::from(""), true);
	}
}