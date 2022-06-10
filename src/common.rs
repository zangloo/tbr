use std::borrow::Borrow;

use anyhow::{anyhow, Result};
use chardetng::EncodingDetector;
use cursive::theme::Theme;
use encoding_rs::{Encoding, UTF_8};
use unicode_width::UnicodeWidthChar;

use crate::book::Line;
use crate::{ReadingInfo, ThemeEntry};

pub const HAN_RENDER_CHARS_PAIRS: [(char, char); 34] = [
	(' ', '　'),
	('「', '﹁'),
	('」', '﹂'),
	('〈', '︿'),
	('〉', '﹀'),
	('『', '﹃'),
	('』', '﹄'),
	('（', '︵'),
	('）', '︶'),
	('《', '︽'),
	('》', '︾'),
	('〔', '︹'),
	('〕', '︺'),
	('［', '︹'),
	('］', '︺'),
	('【', '︻'),
	('】', '︼'),
	('｛', '︷'),
	('｝', '︸'),
	('─', '︱'),
	('…', '︙'),
	('\t', '　'),
	('(', '︵'),
	(')', '︶'),
	('[', '︹'),
	(']', '︺'),
	('<', '︻'),
	('>', '︼'),
	('{', '︷'),
	('}', '︸'),
	('-', '︱'),
	('—', '︱'),
	('〖', '︘'),
	('〗', '︗'),
];

#[derive(Clone)]
pub struct Position {
	pub line: usize,
	pub offset: usize,
}

impl Position {
	pub fn new(line: usize, offset: usize) -> Self {
		Position { line, offset }
	}
}

pub struct TraceInfo {
	pub chapter: usize,
	pub line: usize,
	pub offset: usize,
}

pub fn reading_info(history: &mut Vec<ReadingInfo>, current: &String) -> ReadingInfo {
	let mut i = 0;
	while i < history.len() {
		if history[i].filename.eq(current) {
			return history.remove(i);
		}
		i += 1;
	}
	ReadingInfo::new(&current)
}

pub fn get_theme<'a>(theme_name: &String, theme_entries: &'a Vec<ThemeEntry>) -> Result<&'a Theme> {
	for entry in theme_entries {
		if entry.0.eq(theme_name) {
			return Ok(&entry.1);
		}
	}
	Err(anyhow!("No theme defined: {}",theme_name))
}

pub fn with_leading(text: &Line) -> bool {
	if let Some(leader) = text.char_at(0) {
		!leader.is_whitespace()
	} else {
		false
	}
}

pub fn length_with_leading(text: &Line, leading_space: usize) -> usize {
	let length = text.len();
	return if with_leading(text) {
		length + leading_space
	} else {
		length
	};
}

pub(crate) fn plain_text(content: Vec<u8>, full_scan: bool) -> Result<String> {
	let encoding = detect_charset(&content, full_scan);
	decode_text(content, encoding)
}

#[inline]
pub(crate) fn detect_charset(content: &Vec<u8>, full_scan: bool) -> &'static Encoding {
	let mut detector = EncodingDetector::new();
	if detector.feed(content, full_scan) {
		detector.guess(None, true)
	} else {
		UTF_8
	}
}

#[inline]
pub(crate) fn decode_text(content: Vec<u8>, encoding: &'static Encoding) -> Result<String> {
	let text = if encoding.eq(UTF_8) {
		String::from_utf8(content)?
	} else {
		let (cow, ..) = encoding.decode(content.borrow());
		String::from(cow)
	};
	Ok(text)
}

pub(crate) fn plain_text_lines(content: Vec<u8>) -> Result<Vec<Line>> {
	let text = plain_text(content, false)?;
	Ok(txt_lines(&text))
}

pub(crate) fn txt_lines(txt: &str) -> Vec<Line> {
	let mut lines: Vec<Line> = vec![];
	let mut line = Line::default();
	for c in txt.chars() {
		if c == '\r' {
			continue;
		}
		if c == '\n' {
			lines.push(line);
			line = Line::default();
		} else {
			line.push(c);
		}
	}
	lines.push(line);
	lines
}

#[allow(dead_code)]
pub(crate) fn byte_index_for_char(text: &str, char_index: usize) -> Option<usize> {
	if char_index == 0 {
		return Some(0);
	}
	if char_index == text.chars().count() {
		return Some(text.len());
	}
	let mut indices = text.char_indices();
	for _index in 0..char_index {
		indices.next();
	}
	match indices.next() {
		Some(index) => Some(index.0),
		None => None,
	}
}

pub(crate) fn char_index_for_byte(text: &str, byte_index: usize) -> Option<usize> {
	if byte_index == 0 {
		return Some(0);
	}
	if byte_index == text.len() {
		return Some(text.chars().count());
	}
	let indices = text.char_indices();
	let mut char_index = 0;
	for index in indices {
		if index.0 == byte_index {
			return Some(char_index);
		} else if index.0 > byte_index {
			return None;
		} else {
			char_index += 1;
		}
	}
	None
}

#[inline]
pub fn char_width(ch: char) -> usize {
	match ch.width() {
		Some(w) => w,
		None => 0,
	}
}
