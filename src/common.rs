use anyhow::Result;
use chardetng::EncodingDetector;
use encoding_rs::{Encoding, UTF_8};
use std::borrow::Borrow;
use std::ops::Range;
use unicode_width::UnicodeWidthChar;

use crate::book::Line;

// sorted, for binary search
const HAN_RENDER_CHARS_PAIRS: [(char, char); 36] = [
	('\t', '　'),
	(' ', '　'),
	('(', '︵'),
	(')', '︶'),
	('-', '︱'),
	('<', '︻'),
	('>', '︼'),
	('[', '︹'),
	(']', '︺'),
	('{', '︷'),
	('}', '︸'),
	('~', 'ⸯ'),
	('—', '︱'),
	('…', '︙'),
	('─', '︱'),
	('〈', '︿'),
	('〉', '﹀'),
	('《', '︽'),
	('》', '︾'),
	('「', '﹁'),
	('」', '﹂'),
	('『', '﹃'),
	('』', '﹄'),
	('【', '︻'),
	('】', '︼'),
	('〔', '︹'),
	('〕', '︺'),
	('〖', '︘'),
	('〗', '︗'),
	('（', '︵'),
	('）', '︶'),
	('［', '︹'),
	('］', '︺'),
	('｛', '︷'),
	('｝', '︸'),
	('～', 'ⸯ'),
];

/// sorted, for binary search
const HAN_COMPACT_CHARS: [char; 22] = [
	'·',
	'、',
	'︗',
	'︘',
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
	'，',
	'：',
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

#[cfg(feature = "gui")]
pub fn overlap_range(a: &Range<usize>, b: &Range<usize>)
	-> Option<Range<usize>>
{
	use std::cmp;

	let a_start = a.start;
	let a_stop = a.end;
	let b_start = b.start;
	let b_stop = b.end;
	if a_start >= b_start {
		if a_start < b_stop {
			let stop = cmp::min(a_stop, b_stop);
			Some(a_start..stop)
		} else {
			None
		}
	} else if a_stop > b_start {
		let stop = cmp::min(a_stop, b_stop);
		Some(b_start..stop)
	} else {
		None
	}
}

#[allow(unused)]
pub fn is_overlap(a: &Range<usize>, b: &Range<usize>) -> bool
{
	let a_start = a.start;
	let a_stop = a.end;
	let b_start = b.start;
	let b_stop = b.end;
	if a_start >= b_start {
		a_start < b_stop
	} else {
		a_stop > b_start
	}
}

#[allow(unused)]
#[inline]
pub fn is_compact_for_han(ch: char) -> bool
{
	HAN_COMPACT_CHARS.binary_search(&ch).is_ok()
}

#[inline]
pub fn han_render_char(ch: char) -> char
{
	match HAN_RENDER_CHARS_PAIRS.binary_search_by(|(key, _)| key.cmp(&ch)) {
		Ok(idx) => HAN_RENDER_CHARS_PAIRS[idx].1,
		Err(_) => ch,
	}
}

#[cfg(test)]
mod tests {
	use crate::common::{is_overlap, overlap_range};

	#[test]
	fn test_is_range_overlap()
	{
		assert!(is_overlap(&(10..15), &(9..14)));
		assert!(is_overlap(&(10..15), &(9..15)));
		assert!(is_overlap(&(10..15), &(9..16)));
		assert!(is_overlap(&(10..15), &(10..14)));
		assert!(is_overlap(&(10..15), &(10..15)));
		assert!(is_overlap(&(10..15), &(10..16)));
		assert!(is_overlap(&(10..15), &(11..14)));
		assert!(is_overlap(&(10..15), &(11..15)));
		assert!(is_overlap(&(10..15), &(11..16)));

		assert!(!is_overlap(&(10..15), &(8..9)));
		assert!(!is_overlap(&(10..15), &(15..16)));
	}

	#[test]
	fn test_overlap_range()
	{
		assert_eq!(overlap_range(&(10..15), &(9..14)), Some(10..14));
		assert_eq!(overlap_range(&(10..15), &(9..15)), Some(10..15));
		assert_eq!(overlap_range(&(10..15), &(9..16)), Some(10..15));
		assert_eq!(overlap_range(&(10..15), &(10..14)), Some(10..14));
		assert_eq!(overlap_range(&(10..15), &(10..15)), Some(10..15));
		assert_eq!(overlap_range(&(10..15), &(10..16)), Some(10..15));
		assert_eq!(overlap_range(&(10..15), &(11..14)), Some(11..14));
		assert_eq!(overlap_range(&(10..15), &(11..15)), Some(11..15));
		assert_eq!(overlap_range(&(10..15), &(11..16)), Some(11..15));

		assert!(overlap_range(&(10..15), &(8..9)).is_none());
		assert!(overlap_range(&(10..15), &(15..16)).is_none());
	}
}