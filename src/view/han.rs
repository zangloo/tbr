use std::collections::HashMap;
use unicode_width::UnicodeWidthChar;

use crate::book::Line;
use crate::common::{length_with_leading, with_leading};
use crate::ReadingInfo;
use crate::view::{DrawChar, Position, Render, RenderContext};

const CHARS_PAIRS: [(char, char); 33] = [
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
	fn setup_print_lines(&mut self, draw_lines: &Vec<Vec<DrawChar>>, context: &mut RenderContext) {
		let print_lines = &mut context.print_lines;
		print_lines.clear();
		let line_count = self.line_count;
		let blank_lines = line_count - draw_lines.len();
		let (blank_prefix_length, mut need_split_space) = if blank_lines > 0 {
			((blank_lines - 1) * 3 + 2, true)
		} else {
			(0, false)
		};
		let print_suffix_length = context.width - (line_count * 3 - 1);
		for _x in 0..context.height {
			let mut line = vec![];
			if blank_prefix_length > 0 {
				for _y in 0..blank_prefix_length {
					line.push(DrawChar::space())
				}
			}
			print_lines.push(line);
		}
		for line in draw_lines.iter().rev() {
			let mut chars = line.iter();
			for idx in 0..context.height {
				let print_line = &mut print_lines[idx];
				if need_split_space {
					print_line.push(DrawChar::space());
				}
				let dc = chars.next();
				self.append_char(print_line, dc);
			}
			need_split_space = true;
		}
		if print_suffix_length > 0 {
			for line in print_lines {
				for _x in 0..print_suffix_length {
					line.push(DrawChar::space());
				}
			}
		}
	}
	fn append_char(&self, line: &mut Vec<DrawChar>, dc_option: Option<&DrawChar>) {
		match dc_option {
			Some(dc) => {
				match dc.char.width() {
					Some(s) => {
						line.push(DrawChar::new(dc.char, dc.mode.clone()));
						if s == 1 {
							line.push(DrawChar::new(' ', dc.mode.clone()));
						}
					}
					None => {
						line.push(DrawChar::new(' ', dc.mode.clone()));
						line.push(DrawChar::new(' ', dc.mode.clone()));
					}
				}
			}
			None => {
				line.push(DrawChar::space());
				line.push(DrawChar::space());
			}
		};
	}

	fn map_char(&self, ch: char) -> char {
		*self.chars_map.get(&ch).unwrap_or(&ch)
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
		let mut draw_lines: Vec<Vec<DrawChar>> = vec![];
		for _x in 0..self.line_count {
			let left = line_length - position;
			let mut charts_to_draw = if left >= context.height { context.height } else { left };
			let mut draw_line = vec![];
			if position == 0 && left > 0 {
				if with_leading(text) {
					for _i in 0..leading_space {
						draw_line.push(DrawChar::space());
					}
					if charts_to_draw > height - leading_space {
						charts_to_draw = height - leading_space;
					}
				}
			}
			for _y in 0..charts_to_draw {
				let char = text.char_at(position).unwrap();
				let draw_char = self.map_char(char);
				let dc = self.setup_draw_char(draw_char, line, position, lines, reading);
				draw_line.push(dc);
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
		context.next = Some(Position {
			line,
			position,
		});
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
}