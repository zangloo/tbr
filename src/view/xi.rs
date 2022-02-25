use cursive::Vec2;

use crate::book::Line;
use crate::common::{char_width, with_leading};
use crate::ReadingInfo;
use crate::view::{DrawChar, DrawCharMode, Position, Render, RenderContext};

const TAB_SIZE: usize = 4;

pub struct Xi {}

impl Default for Xi {
	fn default() -> Self {
		Xi {}
	}
}

impl Render for Xi {
	fn redraw(&mut self, lines: &Vec<Line>, reading: &ReadingInfo, context: &mut RenderContext) {
		let height = context.height;
		let width = context.width;
		let mut position = reading.position;
		context.print_lines.clear();
		context.special_char_map.clear();
		for line in reading.line..lines.len() {
			let text = &lines[line];
			let wrapped_breaks = self.wrap_line(text, position, usize::MAX, width, context, Some(WrapLineDrawingContext {
				line,
				reading,
				lines,
			}));
			let current_lines = context.print_lines.len();
			if current_lines == height {
				if line >= lines.len() - 1 {
					context.next = None;
				} else {
					context.next = Some(Position { line: line + 1, position: 0 });
				}
				return;
			} else if current_lines > height {
				let gap = current_lines - height;
				context.next = Some(Position { line, position: wrapped_breaks[wrapped_breaks.len() - gap] });
				return;
			}
			position = 0;
		}
		let blank_lines = height - context.print_lines.len();
		for _x in 0..blank_lines {
			context.print_lines.push(" ".repeat(width));
		}
		context.next = None;
	}

	fn prev(&mut self, lines: &Vec<Line>, reading: &mut ReadingInfo, context: &mut RenderContext) {
		let height = context.height;
		let width = context.width;
		let (mut line, mut end_position) = if reading.position == 0 {
			(reading.line - 1, usize::MAX)
		} else {
			(reading.line, reading.position)
		};
		let mut rows = 0;
		let position;
		context.print_lines.clear();
		loop {
			let text = &lines[line];
			let wrapped_breaks = self.wrap_line(text, 0, end_position, width, context, None);
			end_position = usize::MAX;
			let new_lines = wrapped_breaks.len();
			rows += new_lines;
			if rows >= height {
				position = wrapped_breaks[rows - height];
				break;
			}
			if line == 0 {
				position = 0;
				break;
			}
			line -= 1;
		}
		reading.line = line;
		reading.position = position;
		self.redraw(lines, reading, context);
	}

	fn next_line(&mut self, lines: &Vec<Line>, reading: &mut ReadingInfo, context: &mut RenderContext) {
		let line = reading.line;
		let width = context.width;
		let text = &lines[line];
		let position = reading.position;
		let wrapped_breaks = self.wrap_line(text, position, usize::MAX, width, context, None);
		if wrapped_breaks.len() == 1 {
			reading.line += 1;
			reading.position = 0;
		} else {
			reading.position = wrapped_breaks[1];
		}
		self.redraw(lines, reading, context);
	}

	fn prev_line(&mut self, lines: &Vec<Line>, reading: &mut ReadingInfo, context: &mut RenderContext) {
		let width = context.width;
		let (text, line, position) = if reading.position == 0 {
			let line = if reading.line == 0 {
				return;
			} else {
				reading.line - 1
			};
			let text = &lines[line];
			(text, line, usize::MAX)
		} else {
			let line = reading.line;
			(&lines[line], line, reading.position)
		};
		let wrapped_breaks = self.wrap_line(text, 0, position, width, context, None);
		let breaks_count = wrapped_breaks.len();
		reading.line = line;
		reading.position = wrapped_breaks[breaks_count - 1];
		self.redraw(lines, reading, context);
	}

	fn setup_highlight(&mut self, lines: &Vec<Line>, reading: &mut ReadingInfo, context: &mut RenderContext) {
		let highlight = &reading.highlight.as_ref().unwrap();
		let highlight_line = highlight.line;
		let highlight_start = highlight.start;
		let width = context.width;
		let text = &lines[highlight_line];
		let wrapped_breaks = self.wrap_line(text, 0, highlight_start + 1, width, context, None);
		reading.line = highlight_line;
		reading.position = wrapped_breaks[wrapped_breaks.len() - 1];
	}
}

#[inline]
fn fill_print_line(print_line: &mut String, chars: usize) {
	if chars > 0 {
		print_line.push_str(" ".repeat(chars).as_str());
	}
}

struct WrapLineDrawingContext<'a> {
	line: usize,
	reading: &'a ReadingInfo,
	lines: &'a Vec<Line>,
}

impl Xi {
	fn wrap_line(&mut self, text: &Line, start_position: usize, end_position: usize, width: usize,
	             context: &mut RenderContext, mut draw_context: Option<WrapLineDrawingContext>) -> Vec<usize> {
		let with_leading_space = if context.leading_space > 0 {
			start_position == 0 && with_leading(text)
		} else {
			false
		};
		let (mut x, mut print_line) = if with_leading_space {
			(context.leading_space, " ".repeat(context.leading_space))
		} else {
			(0, "".to_string())
		};
		let mut wrapped_breaks = vec![0];
		let mut break_position = 0;
		let mut chars = text.iter();
		for _x in 0..start_position {
			chars.next();
		}
		let mut position = start_position;
		for char in chars {
			if position == end_position {
				break;
			}
			let cw = char_width(*char);
			let can_break = *char == ' ' || *char == '\t';
			if x + cw > width {
				let gap = width - x;
				x = 0;
				// for unicode, can_break, or prev break not exists
				if cw > 1 || can_break || break_position == 0 {
					fill_print_line(&mut print_line, gap);
					context.print_lines.push(print_line);
					print_line = String::from("");
					// for break char, will not print it any more
					// skip it for line break
					if can_break {
						position += 1;
						wrapped_breaks.push(position);
						continue;
					}
					wrapped_breaks.push(position);
				} else {
					let prev_position = wrapped_breaks[wrapped_breaks.len() - 1];
					let chars_count = if prev_position == 0 {
						if with_leading_space {
							break_position + context.leading_space
						} else {
							break_position
						}
					} else {
						break_position
					};
					let mut print_chars = print_line.chars();
					let mut line = String::from("");
					let mut w = 0;
					for _x in 0..chars_count {
						let ch = print_chars.next().unwrap();
						line.push(ch);
						w += char_width(ch);
					}
					fill_print_line(&mut line, width - w);
					context.print_lines.push(line);
					line = String::from("");
					for ch in print_chars {
						line.push(ch);
					}
					print_line = line;
					wrapped_breaks.push(break_position);
					break_position = 0;
					for ch in print_line.chars() {
						x += char_width(ch);
					}
				}
			}
			if let Some(ref mut drawing_context) = draw_context {
				if let Some(dc) = self.setup_special_char(drawing_context.line, position, *char, drawing_context.lines, drawing_context.reading) {
					let y = context.print_lines.len();
					if let Some(draw_char) = &dc.char {
						match &dc.mode {
							DrawCharMode::Search => {}
							DrawCharMode::Link { .. }
							| DrawCharMode::HighlightLink { .. }
							| DrawCharMode::SearchOnLink { .. } => {
								// for char which width is 1 and display with 2 byte width
								// this is the 2nd cell, for mouse click and open link
								if char_width(*draw_char) == 2 {
									let mode = dc.mode.clone();
									context.special_char_map.insert(
										Vec2 { x: x + 1, y },
										DrawChar { char: None, mode });
								};
							}
						}
					}
					context.special_char_map.insert(Vec2 { x, y }, dc);
				}
			}
			position += 1;
			x += cw;
			if can_break {
				break_position += 1;
				print_line.push(' ');
				if *char == '\t' {
					let tab_chars_left = TAB_SIZE - (x % TAB_SIZE);
					for _c in 0..tab_chars_left {
						if x == width {
							break;
						}
						x += 1;
						print_line.push(' ');
					}
				}
			} else {
				print_line.push(*char);
			}
		}
		if start_position != position {
			if x > 0 {
				fill_print_line(&mut print_line, width - x);
				context.print_lines.push(print_line);
			} else {
				wrapped_breaks.pop();
			}
		} else {
			fill_print_line(&mut print_line, width - x);
			context.print_lines.push(print_line);
		}
		return wrapped_breaks;
	}
}