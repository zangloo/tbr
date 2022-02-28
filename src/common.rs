use std::borrow::Borrow;
use anyhow::{Result};
use chardetng::EncodingDetector;
use encoding_rs::UTF_8;
use unicode_width::UnicodeWidthChar;
use crate::book::Line;

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
	let mut detector = EncodingDetector::new();
	let text = if detector.feed(content.borrow(), full_scan) {
		let encoding = detector.guess(None, true);
		if encoding.eq(UTF_8) {
			String::from_utf8(content)?
		} else {
			let (cow, ..) = encoding.decode(content.borrow());
			String::from(cow)
		}
	} else {
		String::from_utf8(content)?
	};
	Ok(text)
}

pub(crate) fn plain_text_lines(content: Vec<u8>) -> Result<Vec<Line>> {
	let text = plain_text(content, false)?;
	Ok(txt_lines(&text))
}

pub(crate) fn txt_lines(txt: &String) -> Vec<Line> {
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
