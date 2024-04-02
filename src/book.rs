use std::borrow::Cow;
use std::cmp;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::fs::OpenOptions;
use std::io::Read;
use std::ops::Range;
use std::slice::Iter;
use anyhow::{anyhow, Result};
use fancy_regex::Regex;
use indexmap::IndexSet;

use crate::book::epub::EpubLoader;
use crate::book::haodoo::HaodooLoader;
use crate::book::html::HtmlLoader;
use crate::book::txt::TxtLoader;
#[cfg(feature = "gui")]
use crate::color::Color32;
use crate::common::{char_index_for_byte, Position};
use crate::common::TraceInfo;
use crate::config::{BookLoadingInfo, ReadingInfo};
use crate::container::BookContent;
use crate::container::BookContent::{Buf, File};
use crate::controller::{HighlightInfo, HighlightMode};
#[cfg(feature = "gui")]
use crate::gui::HtmlFonts;
#[cfg(feature = "gui")]
use crate::html_convertor::{FontScale, FontWeight};
use crate::html_convertor::{BlockStyle, TextStyle};
use crate::terminal::Listable;

mod epub;
mod txt;
mod html;
mod haodoo;

pub const EMPTY_CHAPTER_CONTENT: &str = "No content.";
pub const IMAGE_CHAR: char = '🖼';

/// this array is sorted, modify carefully
pub const TEXT_SELECTION_SPLITTER: [char; 92] = [
	' ',
	'#',
	'%',
	'&',
	'(',
	')',
	'+',
	',',
	'-',
	'.',
	'/',
	';',
	'<',
	'=',
	'>',
	'?',
	'@',
	'[',
	'\\',
	'\t',
	']',
	'_',
	'{',
	'}',
	'~',
	'—',
	'‘',
	'’',
	'“',
	'”',
	'…',
	'─',
	'ⸯ',
	'　',
	'、',
	'。',
	'〈',
	'〉',
	'《',
	'》',
	'「',
	'」',
	'『',
	'』',
	'【',
	'】',
	'〔',
	'〕',
	'〖',
	'〗',
	'︗',
	'︘',
	'︙',
	'︱',
	'︵',
	'︶',
	'︷',
	'︸',
	'︹',
	'︺',
	'︻',
	'︼',
	'︽',
	'︾',
	'︿',
	'﹀',
	'﹁',
	'﹂',
	'﹃',
	'﹄',
	'！',
	'＃',
	'％',
	'＆',
	'（',
	'）',
	'＊',
	'＋',
	'，',
	'－',
	'／',
	'：',
	'；',
	'＝',
	'？',
	'［',
	'］',
	'｀',
	'｛',
	'｜',
	'｝',
	'～',
];

pub enum ImageData<'a> {
	Borrowed((Cow<'a, str>, &'a [u8])),
	Owned((String, Vec<u8>)),
}

#[cfg(feature = "gui")]
impl<'a> ImageData<'a> {
	#[inline]
	pub fn path_dup(&self) -> String
	{
		match self {
			ImageData::Borrowed((path, _)) => path.to_string(),
			ImageData::Owned((path, _)) => path.clone(),
		}
	}
	#[inline]
	pub fn path(self) -> String
	{
		match self {
			ImageData::Borrowed((path, _)) => path.to_string(),
			ImageData::Owned((path, _)) => path,
		}
	}
	#[inline]
	pub fn bytes(&self) -> &[u8]
	{
		match self {
			ImageData::Borrowed((_, bytes)) => bytes,
			ImageData::Owned((_, vec)) => vec,
		}
	}
}

#[cfg(feature = "gui")]
type TextDecorationLine = lightningcss::properties::text::TextDecorationLine;

#[cfg(feature = "gui")]
#[derive(Debug)]
pub struct CharStyle {
	pub font_scale: FontScale,
	pub font_weight: FontWeight,
	pub font_family: Option<u16>,
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
	pub fn new(str: &str) -> Self
	{
		let mut chars = vec![];
		for ch in str.chars() {
			chars.push(ch);
		}
		Line { chars, styles: vec![] }
	}

	pub fn concat(&mut self, str: &str)
	{
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

	#[inline]
	pub fn push_style(&mut self, style: TextStyle, range: Range<usize>)
	{
		self.styles.push((style, range));
	}

	#[inline]
	pub fn push(&mut self, ch: char)
	{
		if ch == '\0' {
			return;
		}
		self.chars.push(ch);
	}

	pub fn to_string(&self) -> String
	{
		let mut string = String::new();
		for char in &self.chars {
			string.push(*char)
		}
		string
	}

	#[inline]
	pub fn len(&self) -> usize
	{
		self.chars.len()
	}

	#[inline]
	pub fn is_empty(&self) -> bool
	{
		self.chars.is_empty()
	}

	#[inline]
	#[allow(unused)]
	pub fn is_blank(&self) -> bool
	{
		for char in &self.chars {
			if !char.is_whitespace() {
				return false;
			}
		}
		true
	}

	#[inline]
	pub fn char_at(&self, index: usize) -> Option<char>
	{
		match self.chars.get(index) {
			Some(ch) => Some(*ch),
			None => None,
		}
	}

	#[inline]
	pub fn iter(&self) -> Iter<char>
	{
		self.chars.iter()
	}

	pub fn search_pattern(&self, regex: &Regex, start: Option<usize>, stop: Option<usize>, rev: bool) -> Option<Range<usize>>
	{
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

	pub fn link_at(&self, link_index: usize) -> Option<Link>
	{
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

	#[allow(unused)]
	pub fn image_at(&self, char_offset: usize) -> Option<&str>
	{
		for style in self.styles.iter().rev() {
			if let (TextStyle::Image(url), range) = style {
				if range.contains(&char_offset) {
					return Some(url);
				}
			}
		}
		None
	}

	#[cfg(feature = "gui")]
	pub fn char_style_at(&self, char_index: usize, custom_color: bool,
		colors: &Colors) -> CharStyle
	{
		let mut char_style = CharStyle {
			font_scale: Default::default(),
			font_weight: Default::default(),
			font_family: None,
			color: colors.color.clone(),
			background: None,
			line: None,
			border: None,
			link: None,
			image: None,
		};
		let mut new_color = None;
		for (index, (style, range)) in self.styles.iter().enumerate().rev() {
			if range.contains(&char_index) {
				match style {
					TextStyle::FontSize { scale, relative } =>
						char_style.font_scale.update(scale, *relative),
					TextStyle::FontWeight(weight) =>
						char_style.font_weight.update(weight),
					TextStyle::FontFamily(families) => char_style.font_family = Some(families.clone()),
					TextStyle::Image(href) => char_style.image = Some(href.clone()),
					TextStyle::Link(_) => {
						char_style.link = Some((index, range.clone()));
						if new_color.is_none() {
							new_color = Some(colors.link.clone());
						}
					}
					TextStyle::Border => char_style.border = Some(range.clone()),
					TextStyle::Line(line) => char_style.line = Some((*line, range.clone())),
					TextStyle::Color(color) => if custom_color { new_color = Some(color.clone()) },
					TextStyle::BackgroundColor(color) => if custom_color { char_style.background = Some(color.clone()) },
				}
			}
		}
		if let Some(color) = new_color {
			char_style.color = color;
		}
		char_style
	}

	#[allow(unused)]
	pub fn word_at_offset(&self, offset: usize) -> Option<(usize, usize)>
	{
		let pointer_char = self.chars.get(offset)?;
		if TEXT_SELECTION_SPLITTER.binary_search(pointer_char).is_ok() {
			return Some((offset, offset));
		}

		let mut from = offset;
		for idx in (0..offset).rev() {
			if TEXT_SELECTION_SPLITTER.binary_search(&self.chars[idx]).is_ok() {
				break;
			}
			from = idx;
		}

		let mut to = offset;
		while let Some(ch) = self.chars.get(to + 1) {
			if TEXT_SELECTION_SPLITTER.binary_search(ch).is_ok() {
				break;
			}
			to += 1;
		}
		Some((from, to))
	}

	#[allow(unused)]
	pub fn sub_str(&self, target: &mut String, range: Range<usize>) {
		target.clear();
		for idx in range {
			target.push(self.chars[idx]);
		}
	}
}

impl Default for Line {
	fn default() -> Self {
		Line { chars: vec![], styles: vec![] }
	}
}

impl PartialEq for Line {
	fn eq(&self, other: &Self) -> bool
	{
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

pub struct TocInfo<'a> {
	pub title: &'a str,
	pub index: usize,
	pub level: usize,
}

impl<'a> Listable for TocInfo<'a> {
	#[inline]
	fn title(&self) -> &str
	{
		self.title
	}

	#[inline]
	fn id(&self) -> usize
	{
		self.index
	}
}

pub trait Book {
	#[inline]
	fn name(&self) -> Option<&str> { None }
	#[inline]
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
	#[inline]
	fn current_chapter(&self) -> usize { 0 }
	#[inline]
	fn title(&self, _line: usize, _offset: usize) -> Option<&str> { None }
	#[inline]
	fn toc_index(&self, _line: usize, _offset: usize) -> usize { 0 }
	#[inline]
	fn toc_iterator(&self) -> Option<Box<dyn Iterator<Item=TocInfo> + '_>> { None }
	#[inline]
	fn toc_position(&mut self, _toc_index: usize) -> Option<TraceInfo> { None }
	fn lines(&self) -> &Vec<Line>;
	#[inline]
	fn leading_space(&self) -> usize { 2 }
	#[inline]
	fn link_position(&mut self, _line: usize, _link_index: usize) -> Option<TraceInfo> { None }
	// (absolute path, content)
	#[inline]
	fn image<'a>(&'a self, _href: &'a str) -> Option<ImageData<'a>> { None }
	#[inline]
	fn font_family_names(&self) -> Option<&IndexSet<String>> { None }
	#[inline]
	#[cfg(feature = "gui")]
	fn color_customizable(&self) -> bool { false }
	#[inline]
	#[cfg(feature = "gui")]
	fn fonts_customizable(&self) -> bool { false }
	#[inline]
	#[cfg(feature = "gui")]
	fn custom_fonts(&self) -> Option<&HtmlFonts> { None }
	#[inline]
	#[cfg(feature = "gui")]
	fn style_customizable(&self) -> bool { false }
	#[inline]
	#[cfg(feature = "gui")]
	fn block_styles(&self) -> Option<&Vec<BlockStyle>> { None }

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
	fn support(&self, filename: &str) -> bool
	{
		let filename = filename.to_lowercase();
		for extension in self.extensions() {
			if filename.ends_with(extension) {
				return true;
			}
		}
		false
	}
	fn load_file(&self, filename: &str, mut file: std::fs::File,
		loading_chapter: LoadingChapter, loading: BookLoadingInfo)
		-> Result<(Box<dyn Book>, ReadingInfo)>
	{
		let mut content: Vec<u8> = Vec::new();
		file.read_to_end(&mut content)?;
		self.load_buf(filename, content, loading_chapter, loading)
	}

	fn load_buf(&self, filename: &str, content: Vec<u8>,
		loading_chapter: LoadingChapter, loading: BookLoadingInfo)
		-> Result<(Box<dyn Book>, ReadingInfo)>;
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

	pub fn support(&self, filename: &str) -> bool
	{
		for loader in self.loaders.iter() {
			if loader.support(filename) {
				return true;
			}
		}
		false
	}

	pub fn load(&self, filename: &str, content: BookContent,
		loading_chapter: LoadingChapter, loading: BookLoadingInfo)
		-> Result<(Box<dyn Book>, ReadingInfo)>
	{
		for loader in self.loaders.iter() {
			if loader.support(filename) {
				let (book, mut reading) = match content {
					File(filepath) => {
						let file = OpenOptions::new().read(true).open(filepath)?;
						loader.load_file(filename, file, loading_chapter, loading)?
					}
					Buf(buf) => loader.load_buf(filename, buf, loading_chapter, loading)?,
				};
				reading.chapter = book.current_chapter();
				let lines = book.lines();
				if reading.line >= lines.len() {
					reading.line = lines.len() - 1;
				}
				let chars = lines[reading.line].len();
				if reading.position >= chars {
					reading.position = 0;
				}
				return Ok((book, reading));
			}
		}
		Err(anyhow!("Not support open book: {}", filename))
	}
}

impl Default for BookLoader {
	fn default() -> Self
	{
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
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result
	{
		f.write_str(&format!("Chapter error: {}", self.msg))
	}
}

impl Display for ChapterError {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result
	{
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

#[cfg(test)]
mod test {
	use std::collections::HashSet;
	use crate::book::{FontWeightValue, TextDecorationLine, TextStyle};
	use crate::color::Color32;

	#[test]
	fn test() {
		let mut set = HashSet::new();
		set.insert(TextStyle::Link("keep me".to_string()));
		set.insert(TextStyle::Border);

		assert!(set.insert(TextStyle::Line(TextDecorationLine::all())));
		assert_eq!(set.insert(TextStyle::Border), false);
		assert!(set.insert(TextStyle::FontSize { scale: 1., relative: true }));
		assert!(set.insert(TextStyle::FontWeight(FontWeightValue::Lighter)));
		assert!(set.insert(TextStyle::FontFamily(1)));
		assert!(set.insert(TextStyle::Image("image".to_string())));
		assert_eq!(set.insert(TextStyle::Link("link".to_string())), false);
		assert!(set.insert(TextStyle::Color(Color32::from_rgb(0, 0, 0))));
		assert!(set.insert(TextStyle::BackgroundColor(Color32::from_rgb(0, 0, 0))));
		assert_eq!(set.len(), 9);
		if let TextStyle::Link(link) = set.get(&TextStyle::Link("not me".to_string())).unwrap() {
			assert_eq!("keep me", link);
		} else {
			panic!("failed");
		}
		let replaced = set.replace(TextStyle::Link("no way".to_string()));
		if let TextStyle::Link(link) = replaced.unwrap() {
			assert_eq!("keep me", link);
		} else {
			panic!("failed");
		}
		if let TextStyle::Link(link) = set.get(&TextStyle::Link("not me".to_string())).unwrap() {
			assert_eq!("no way", link);
		} else {
			panic!("failed");
		}
	}
}