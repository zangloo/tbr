use std::collections::HashMap;

use cursive::Vec2;
use unicode_width::UnicodeWidthChar;

use crate::book::Line;
use crate::common::{char_width, length_with_leading, with_leading};
use crate::ReadingInfo;
use crate::view::{DrawChar, DrawCharMode, NextPageInfo, Render, RenderContext};

const CHARS_PAIRS: [(char, char); 31] = [
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

pub struct Han {
	chars_map: HashMap<char, char>,
	line_count: usize,
}

impl Default for Han {
	fn default() -> Self {
		Han {
			chars_map: CHARS_PAIRS.into_iter().collect(),
			line_count: 1,
		}
	}
}

impl Han {
	fn setup_print_lines(&mut self, draw_lines: &Vec<String>, context: &mut RenderContext) {
		let print_lines = &mut context.print_lines;
		print_lines.clear();
		let line_count = self.line_count;
		let blank_lines = line_count - draw_lines.len();
		let (blank_prefix, mut need_split_space) = if blank_lines > 0 {
			let mut blank_prefix_str = "   ".repeat(blank_lines - 1);
			blank_prefix_str.push_str("  ");
			(Some(blank_prefix_str), true)
		} else {
			(None, false)
		};
		let print_suffix_length = context.width - (line_count * 3 - 1);
		let print_suffix = if print_suffix_length > 0 {
			Some(" ".repeat(print_suffix_length))
		} else {
			None
		};
		for _x in 0..context.height {
			print_lines.push(match &blank_prefix {
				Some(prefix) => prefix.clone(),
				None => String::new(),
			});
		}
		// let mut need_split_space = false;
		for line in draw_lines.iter().rev() {
			let mut chars = line.chars();
			for idx in 0..context.height {
				let draw_line = &mut print_lines[idx];
				let ch = chars.next();
				if need_split_space {
					draw_line.push(' ');
				}
				self.append_char(draw_line, ch);
			}
			need_split_space = true;
		}
		match print_suffix {
			Some(suffix) => {
				for line in print_lines {
					line.push_str(suffix.as_str());
				}
			}
			None => (),
		}
	}
	fn append_char(&self, line: &mut String, ch: Option<char>) {
		match ch {
			Some(oc) => {
				let c = self.map_char(oc);
				match c.width() {
					Some(s) => {
						line.push(c);
						if s == 1 {
							line.push(' ');
						}
					}
					None => {
						line.push(' ');
						line.push(' ');
					}
				}
			}
			None => {
				line.push(' ');
				line.push(' ');
			}
		};
	}
}

impl Render for Han {
	fn resized(&mut self, context: &mut RenderContext) {
		let width = context.width;
		self.line_count = if width % 3 == 2 { width / 3 + 1 } else { width / 3 };
	}

	fn redraw(&mut self, lines: &Vec<Line>, reading: &ReadingInfo, context: &mut RenderContext) {
		let height = context.height;
		let mut line = reading.line;
		let mut position = reading.position;
		let leading_space = context.leading_space;
		let mut text = &lines[line];
		let mut line_length = text.len();
		let mut draw_lines: Vec<String> = vec![];
		context.special_char_map.clear();
		for x in 0..self.line_count {
			let left = line_length - position;
			let mut charts_to_draw = if left >= context.height { context.height } else { left };
			let mut draw_line = String::new();
			let mut y_offset = 0;
			if position == 0 && left > 0 {
				if with_leading(text) {
					for _i in 0..leading_space {
						draw_line.push(' ');
					}
					y_offset = leading_space;
					if charts_to_draw > height - leading_space {
						charts_to_draw = height - leading_space;
					}
				}
			}
			for y in 0..charts_to_draw {
				let char = text.char_at(position).unwrap();
				draw_line.push(char);
				if let Some(dc) = self.setup_special_char(line, position, char, lines, reading) {
					let y_pos = y + y_offset;
					let x_pos = (self.line_count - x - 1) * 3;
					match &dc.mode {
						DrawCharMode::Search => {}
						DrawCharMode::Link { .. }
						| DrawCharMode::HighlightLink { .. }
						| DrawCharMode::SearchOnLink { .. } => {
							let draw_char = &dc.char.unwrap();
							let mode = dc.mode.clone();
							// for char which width is 1 and display with 2 byte width
							// this is the 2nd cell, for mouse click and open link
							let extra_draw_char = if char_width(*draw_char) == 1 {
								DrawChar { char: Some(' '), mode }
							} else {
								DrawChar { char: None, mode }
							};
							context.special_char_map.insert(
								Vec2 { x: x_pos + 1, y: y_pos },
								extra_draw_char);
						}
					}
					context.special_char_map.insert(Vec2 { x: x_pos, y: y_pos }, dc);
				}
				position += 1;
			}
			draw_lines.push(draw_line);
			if position == line_length {
				line += 1;
				if line == lines.len() {
					self.setup_print_lines(&draw_lines, context);
					context.next = None;
					return;
				}
				text = &lines[line];
				line_length = text.len();
				position = 0;
			}
		}
		self.setup_print_lines(&draw_lines, context);
		context.next = Some(NextPageInfo { line, position });
	}

	fn prev(&mut self, lines: &Vec<Line>, reading: &mut ReadingInfo, context: &mut RenderContext) {
		let height = context.height;
		let mut line = reading.line;
		let mut position = reading.position;
		let leading_space = context.leading_space;
		if position == 0 {
			line -= 1;
			position = length_with_leading(&lines[line], leading_space);
		}
		for _x in 0..self.line_count {
			if position <= height {
				if line == 0 {
					reading.line = 0;
					reading.position = 0;
					return self.redraw(lines, reading, context);
				}
				line -= 1;
				position = length_with_leading(&lines[line], leading_space);
			} else {
				position -= height;
			}
		}
		let text = &lines[line];
		if position == length_with_leading(text, leading_space) {
			position = 0;
			line += 1;
		} else {
			let mut p = height;
			while p < position {
				p += height;
			}
			position = p;
			if with_leading(text) {
				position -= leading_space;
			}
		}
		reading.line = line;
		reading.position = position;
		self.redraw(lines, reading, context)
	}

	fn next_line(&mut self, lines: &Vec<Line>, reading: &mut ReadingInfo, context: &mut RenderContext) {
		let height = context.height;
		let mut line = reading.line;
		let text = &lines[line];
		let text_length = length_with_leading(text, context.leading_space);
		let mut position = reading.position;
		if position == 0 {
			if text_length <= height {
				line += 1;
			} else if with_leading(text) {
				position = height - context.leading_space;
			} else {
				position = height;
			}
		} else {
			position += height;
			if position + context.leading_space >= text_length {
				position = 0;
				line += 1;
			}
		}
		reading.line = line;
		reading.position = position;
		self.redraw(lines, reading, context)
	}

	fn prev_line(&mut self, lines: &Vec<Line>, reading: &mut ReadingInfo, context: &mut RenderContext) {
		let height = context.height;
		let mut line = reading.line;
		let mut position = reading.position;
		if position == 0 {
			if line == 0 {
				context.next = None;
				return;
			} else {
				line -= 1;
				let text = &lines[line];
				let text_length = length_with_leading(text, context.leading_space);
				if text_length <= height {
					position = 0;
				} else {
					position = height;
					while position + height < text_length - 1 {
						position += height;
					}
					if with_leading(text) {
						position -= context.leading_space;
					}
				}
			}
		} else if position < height {
			position = 0
		} else {
			position -= height;
		}
		reading.line = line;
		reading.position = position;
		self.redraw(lines, reading, context)
	}

	fn setup_highlight(&mut self, lines: &Vec<Line>, reading: &mut ReadingInfo, context: &mut RenderContext) {
		let highlight = &reading.highlight.as_ref().unwrap();
		let highlight_line = highlight.line;
		let highlight_start = highlight.start;
		let height = context.height;
		let mut position = 0;
		loop {
			if position == 0 {
				let leading = if with_leading(&lines[highlight_line]) {
					context.leading_space
				} else {
					0
				};
				if height - leading > highlight_start {
					break;
				} else {
					position = height - leading;
				}
			} else if position + height > highlight_start {
				break;
			} else {
				position += height;
			}
		}
		reading.line = highlight_line;
		reading.position = position;
	}

	fn map_char(&self, ch: char) -> char {
		*self.chars_map.get(&ch).unwrap_or(&ch)
	}
}