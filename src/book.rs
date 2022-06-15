use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::fs::OpenOptions;
use std::io::Read;
use std::ops::Range;
use std::slice::Iter;
use anyhow::{anyhow, Result};
use regex::Regex;
#[cfg(feature = "gui")]
use eframe::egui::Color32;

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
pub const IMAGE_CHAR: char = '🖼';

type TextDecorationLine = parcel_css::properties::text::TextDecorationLine;

#[derive(Clone)]
pub enum TextStyle {
	Line(TextDecorationLine),
	Border,
	FontSize { scale: f32, relative: bool },
	Image(String),
	Link(String),
}

#[cfg(feature = "gui")]
pub struct CharStyle {
	pub font_scale: f32,
	pub color: Color32,
	pub background: Option<Color32>,
	pub line: Option<(TextDecorationLine, Range<usize>)>,
	pub border: Option<Range<usize>>,
	pub link: Option<(String, Range<usize>)>,
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
			if ignore_whitespace && ch.is_whitespace() {
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
			Some(ch) => Some(ch.clone()),
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
			regex.find_iter(&line).last()
		} else {
			regex.find_at(&line, 0)
		}?;
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
						target: target.as_str(),
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
				target: target.as_str(),
				range,
			})
		} else {
			None
		}
	}

	#[cfg(feature = "gui")]
	pub fn char_style_at(&self, index: usize, colors: &Colors) -> CharStyle
	{
		let mut char_style = CharStyle {
			font_scale: 1.0,
			color: colors.color,
			background: None,
			line: None,
			border: None,
			link: None,
			image: None,
		};
		for (style, range) in &self.styles {
			if range.contains(&index) {
				match style {
					TextStyle::FontSize { scale, relative } => {
						if *relative {
							char_style.font_scale *= scale;
						} else {
							char_style.font_scale = *scale;
						}
					}
					TextStyle::Image(href) => char_style.image = Some(href.clone()),
					TextStyle::Link(target) => {
						char_style.link = Some((target.clone(), range.clone()));
						char_style.color = colors.link;
					}
					TextStyle::Border => char_style.border = Some(range.clone()),
					TextStyle::Line(line) => char_style.line = Some((*line, range.clone())),
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
	// (absolute path, content)
	fn image(&self, _href: &str) -> Option<(String, &Vec<u8>)> { None }
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
