use anyhow::Result;
use chardetng::EncodingDetector;
use encoding_rs::{Encoding, UTF_8};
use std::borrow::Borrow;
use unicode_width::UnicodeWidthChar;

use crate::book::Line;

pub const HAN_RENDER_CHARS_PAIRS: [(char, char); 36] = [
	(' ', '　'),
	('─', '︱'),
	('…', '︙'),
	('\t', '　'),
	('-', '︱'),
	('—', '︱'),
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
	('(', '︵'),
	(')', '︶'),
	('[', '︹'),
	(']', '︺'),
	('<', '︻'),
	('>', '︼'),
	('{', '︷'),
	('}', '︸'),
	('〖', '︘'),
	('〗', '︗'),
	('～', 'ⸯ'),
	('~', 'ⸯ'),
];

#[allow(unused)]
pub const HAN_COMPACT_CHARS: [char; 29] = [
	'﹁',
	'﹂',
	'︿',
	'﹀',
	'﹃',
	'﹄',
	'︵',
	'︶',
	'︽',
	'︾',
	'︹',
	'︺',
	'︹',
	'︺',
	'︻',
	'︼',
	'︷',
	'︸',
	'︵',
	'︶',
	'︹',
	'︺',
	'︻',
	'︼',
	'︷',
	'︸',
	'︘',
	'︗',
	'·',
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
		} else if line.len() > 0 || !c.is_whitespace() {
			line.push(c);
		}
	}
	lines.push(line);
	lines
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

#[macro_export]
macro_rules! frozen_map_get {
    ($map:expr, $key:ident, ||$resolver:block) => ({
	    let value = if let Some(value) = $map.get(&$key) {
		    value
	    } else {
		    let value = $resolver?;
		    $map.insert($key, value)
	    };
	    Some(value)
    });
    ($map:expr, $key:ident, true, ||$resolver:block) => ({
	    let value = if let Some(value) = $map.get(&$key) {
		    value
	    } else {
		    let value = $resolver?;
		    $map.insert($key.clone(), value)
	    };
	    Some(value)
    });
}
