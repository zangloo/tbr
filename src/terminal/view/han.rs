use std::collections::HashMap;
use unicode_width::UnicodeWidthChar;

use crate::book::{Book, Line};
use crate::common::{HAN_RENDER_CHARS_PAIRS, length_with_leading, with_leading};
use crate::controller::HighlightInfo;
use crate::terminal::view::{DrawChar, Position, Render, RenderContext, TerminalRender};

pub struct Han {
	chars_map: HashMap<char, char>,
	line_count: usize,
}

impl Han {
	pub fn new() -> Self
	{
		Han {
			chars_map: HAN_RENDER_CHARS_PAIRS.into_iter().collect(),
			line_count: 1,
		}
	}

	fn setup_print_lines(&mut self, draw_lines: &Vec<Vec<DrawChar>>, context: &mut RenderContext)
	{
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
				append_char(print_line, dc);
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

	fn map_char(&self, ch: char) -> char {
		*self.chars_map.get(&ch).unwrap_or(&ch)
	}
}

fn append_char(line: &mut Vec<DrawChar>, dc_option: Option<&DrawChar>) {
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

impl TerminalRender for Han
{
	fn resized(&mut self, context: &RenderContext)
	{
		let width = context.width;
		self.line_count = if width % 3 == 2 { width / 3 + 1 } else { width / 3 };
	}
}

impl Render<RenderContext> for Han {
	fn redraw(&mut self, _book: &Box<dyn Book>, lines: &Vec<Line>, new_line: usize, new_offset: usize, highlight: &Option<HighlightInfo>, context: &mut RenderContext) -> Option<Position> {
		let mut line = new_line;
		let mut offset = new_offset;
		let height = context.height;
		let leading_space = context.leading_space;
		let mut text = &lines[line];
		let mut line_length = text.len();
		let mut draw_lines: Vec<Vec<DrawChar>> = vec![];
		for _x in 0..self.line_count {
			let left = line_length - offset;
			let mut charts_to_draw = if left >= context.height { context.height } else { left };
			let mut draw_line = vec![];
			if offset == 0 && left > 0 {
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
				let char = text.char_at(offset).unwrap();
				let draw_char = self.map_char(char);
				let dc = self.setup_draw_char(draw_char, line, offset, lines, highlight);
				draw_line.push(dc);
				offset += 1;
			}
			draw_lines.push(draw_line);
			if offset == line_length {
				line += 1;
				if line == lines.len() {
					self.setup_print_lines(&draw_lines, context);
					return None;
				}
				text = &lines[line];
				line_length = text.len();
				offset = 0;
			}
		}
		self.setup_print_lines(&draw_lines, context);
		Some(Position {
			line,
			offset,
		})
	}

	fn prev_page(&mut self, _book: &Box<dyn Book>, lines: &Vec<Line>, new_line: usize, new_offset: usize, context: &mut RenderContext) -> Position
	{
		let height = context.height;
		let mut line = new_line;
		let mut offset = new_offset;
		let leading_space = context.leading_space;
		if offset == 0 {
			line -= 1;
			offset = length_with_leading(&lines[line], leading_space);
		}
		for _x in 0..self.line_count {
			if offset <= height {
				if line == 0 {
					return Position::new(0, 0);
				}
				line -= 1;
				offset = length_with_leading(&lines[line], leading_space);
			} else {
				offset -= height;
			}
		}
		let text = &lines[line];
		if offset == length_with_leading(text, leading_space) {
			offset = 0;
			line += 1;
		} else {
			let mut p = height;
			while p < offset {
				p += height;
			}
			offset = p;
			if with_leading(text) {
				offset -= leading_space;
			}
		}
		Position::new(line, offset)
	}

	fn next_line(&mut self, _book: &Box<dyn Book>, lines: &Vec<Line>, new_line: usize, new_offset: usize, context: &mut RenderContext) -> Position {
		let height = context.height;
		let mut line = new_line;
		let mut offset = new_offset;
		let text = &lines[line];
		let text_length = length_with_leading(text, context.leading_space);
		if offset == 0 {
			if text_length <= height {
				line += 1;
			} else if with_leading(text) {
				offset = height - context.leading_space;
			} else {
				offset = height;
			}
		} else {
			offset += height;
			if offset + context.leading_space >= text_length {
				offset = 0;
				line += 1;
			}
		}
		Position::new(line, offset)
	}

	fn prev_line(&mut self, _book: &Box<dyn Book>, lines: &Vec<Line>, new_line: usize, new_offset: usize, context: &mut RenderContext) -> Position {
		let height = context.height;
		let mut line = new_line;
		let mut offset = new_offset;
		if offset == 0 {
			if line == 0 {
				return Position::new(line, offset);
			} else {
				line -= 1;
				let text = &lines[line];
				let text_length = length_with_leading(text, context.leading_space);
				if text_length <= height {
					offset = 0;
				} else {
					offset = height;
					while offset + height < text_length - 1 {
						offset += height;
					}
					if with_leading(text) {
						offset -= context.leading_space;
					}
				}
			}
		} else if offset < height {
			offset = 0
		} else {
			offset -= height;
		}
		Position::new(line, offset)
	}

	fn setup_highlight(&mut self, _book: &Box<dyn Book>, lines: &Vec<Line>, highlight_line: usize, highlight_start: usize, context: &mut RenderContext) -> Position {
		let height = context.height;
		let mut offset = 0;
		loop {
			if offset == 0 {
				let leading = if with_leading(&lines[highlight_line]) {
					context.leading_space
				} else {
					0
				};
				if height - leading > highlight_start {
					break;
				} else {
					offset = height - leading;
				}
			} else if offset + height > highlight_start {
				break;
			} else {
				offset += height;
			}
		}
		Position::new(highlight_line, offset)
	}
}