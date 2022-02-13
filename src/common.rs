use anyhow::{anyhow, Result};
use chardet::{charset2encoding, detect};
use encoding::DecoderTrap;
use encoding::label::encoding_from_whatwg_label;
use html2text::from_read;
use unicode_width::UnicodeWidthChar;

pub fn with_leading(text: &String) -> bool {
	let leader = text.chars().nth(0);
	{
		match leader {
			Some(leader) => leader != ' ' && leader != '\t' && leader != '　',
			None => false,
		}
	}
}

pub fn length_with_leading(text: &String, leading_space: usize) -> usize {
	let length = text.chars().count();
	return if with_leading(text) {
		length + leading_space
	} else {
		length
	};
}

pub(crate) fn html_lines(content: &Vec<u8>) -> Result<Vec<String>> {
	let text = from_read(&content[..], usize::MAX);
	Ok(txt_lines(&text))
}

pub(crate) fn plain_text_lines(content: Vec<u8>) -> Result<Vec<String>> {
	let result = detect(&content);
	let coder = encoding_from_whatwg_label(charset2encoding(&result.0));
	let text = match coder {
		Some(coder) => {
			match coder.decode(&content, DecoderTrap::Ignore) {
				Ok(text) => text,
				Err(e) => return Err(anyhow!(e.to_string())),
			}
		}
		None => String::from_utf8(content)?
	};
	Ok(txt_lines(&text))
}

pub(crate) fn txt_lines(txt: &String) -> Vec<String> {
	let mut lines: Vec<String> = vec![];
	let mut line = "".to_string();
	for c in txt.chars() {
		if c == '\r' {
			continue;
		}
		if c == '\n' {
			lines.push(line);
			line = "".to_string();
		} else {
			line.push(c);
		}
	}
	lines.push(line);
	lines
}

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
