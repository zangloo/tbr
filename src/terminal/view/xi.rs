use crate::book::{Book, Line};
use crate::common::{char_width, with_leading};
use crate::config::ReadingInfo;
use crate::controller::HighlightInfo;
use crate::terminal::view::{DrawChar, DrawCharMode, Position, Render, RenderContext, TerminalRender};

const TAB_SIZE: usize = 4;

pub struct Xi {}

impl TerminalRender for Xi {}

impl Render<RenderContext> for Xi {
	fn book_loaded(&mut self, book: &dyn Book, _reading: &ReadingInfo, context: &mut RenderContext)
	{
		context.leading_space = book.leading_space();
	}

	fn redraw(&mut self, _book: &dyn Book, lines: &Vec<Line>, line: usize,
		mut offset: usize, highlight: &Option<HighlightInfo>,
		context: &mut RenderContext) -> Option<Position>
	{
		let height = context.height;
		let width = context.width;
		context.print_lines.clear();
		for line in line..lines.len() {
			let text = &lines[line];
			let wrapped_breaks = self.wrap_line(text, offset, usize::MAX, width,
				Some(WrapLineDrawingContext {
					line,
					highlight,
					lines,
				}), context);
			let current_lines = context.print_lines.len();
			if current_lines == height {
				return if line >= lines.len() - 1 {
					None
				} else {
					Some(Position { line: line + 1, offset: 0 })
				};
			} else if current_lines > height {
				let gap = current_lines - height;
				return Some(Position { line, offset: wrapped_breaks[wrapped_breaks.len() - gap] });
			}
			offset = 0;
		}
		let blank_lines = height - context.print_lines.len();
		for _x in 0..blank_lines {
			let mut print_line = vec![];
			for _y in 0..width {
				print_line.push(DrawChar::space());
			}
			context.print_lines.push(print_line);
		}
		None
	}

	fn prev_page(&mut self, _book: &dyn Book, lines: &Vec<Line>, line: usize,
		offset: usize, context: &mut RenderContext) -> Position
	{
		let height = context.height;
		let width = context.width;
		let (mut line, mut end_position) = if offset == 0 {
			(line - 1, usize::MAX)
		} else {
			(line, offset)
		};
		let mut rows = 0;
		let position;
		context.print_lines.clear();
		loop {
			let text = &lines[line];
			let wrapped_breaks = self.wrap_line(text, 0, end_position, width, None, context);
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
		Position::new(line, position)
	}

	fn next_line(&mut self, _book: &dyn Book, lines: &Vec<Line>, line: usize,
		offset: usize, context: &mut RenderContext) -> Position
	{
		let width = context.width;
		let text = &lines[line];
		let wrapped_breaks = self.wrap_line(text, offset, usize::MAX, width, None, context);
		let (new_line, new_offset) = if wrapped_breaks.len() == 1 {
			(line + 1, 0)
		} else {
			(line, wrapped_breaks[1])
		};
		Position::new(new_line, new_offset)
	}

	fn prev_line(&mut self, _book: &dyn Book, lines: &Vec<Line>, line: usize,
		offset: usize, context: &mut RenderContext) -> Position
	{
		let width = context.width;
		let (text, new_line, new_offset) = if offset == 0 {
			let new_line = if line == 0 {
				return Position::new(0, 0);
			} else {
				line - 1
			};
			let text = &lines[new_line];
			(text, new_line, usize::MAX)
		} else {
			(&lines[line], line, offset)
		};
		let wrapped_breaks = self.wrap_line(text, 0, new_offset, width, None, context);
		let breaks_count = wrapped_breaks.len();
		Position::new(new_line, wrapped_breaks[breaks_count - 1])
	}

	fn setup_highlight(&mut self, _book: &dyn Book, lines: &Vec<Line>,
		highlight_line: usize, highlight_start: usize,
		context: &mut RenderContext) -> Position
	{
		let width = context.width;
		let text = &lines[highlight_line];
		let wrapped_breaks = self.wrap_line(text, 0, highlight_start + 1, width, None, context);
		Position::new(highlight_line, wrapped_breaks[wrapped_breaks.len() - 1])
	}
}

#[inline]
fn fill_print_line(print_line: &mut Vec<DrawChar>, chars: usize) {
	for _x in 0..chars {
		print_line.push(DrawChar::space());
	}
}

struct WrapLineDrawingContext<'a> {
	line: usize,
	highlight: &'a Option<HighlightInfo>,
	lines: &'a Vec<Line>,
}

impl Xi
{
	pub fn new() -> Self
	{
		Xi {}
	}

	fn wrap_line(&mut self, text: &Line, start_position: usize, end_position: usize, width: usize, draw_context: Option<WrapLineDrawingContext>, context: &mut RenderContext) -> Vec<usize> {
		let with_leading_space = if context.leading_space > 0 {
			start_position == 0 && with_leading(text)
		} else {
			false
		};
		let (mut x, mut print_line) = if with_leading_space {
			let mut chars = vec![];
			for _x in 0..context.leading_space {
				chars.push(DrawChar::space());
			}
			(context.leading_space, chars)
		} else {
			(0, vec![])
		};
		let mut wrapped_breaks = vec![start_position];
		let mut break_position = None;
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
				// for unicode, can_break, or prev break not exists, or breaking content too long
				if cw > 1 || can_break || break_position.is_none() || position - break_position.unwrap() > 20 {
					fill_print_line(&mut print_line, gap);
					context.print_lines.push(print_line);
					print_line = vec![];
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
					let the_break_position = break_position.unwrap_or(0);
					let chars_count = if prev_position == 0 && with_leading_space {
						the_break_position + context.leading_space
					} else {
						the_break_position - prev_position
					};
					let mut print_chars = print_line.iter();
					let mut line = vec![];
					let mut w = 0;
					for _x in 0..chars_count {
						let dc = print_chars.next().unwrap();
						line.push(dc.clone());
						w += char_width(dc.char);
					}
					fill_print_line(&mut line, width - w);
					context.print_lines.push(line);
					line = vec![];
					for ch in print_chars {
						line.push(ch.clone());
					}
					print_line = line;
					wrapped_breaks.push(the_break_position);
					break_position = None;
					for ch in &print_line {
						x += char_width(ch.char);
					}
				}
			}
			x += cw;
			if can_break {
				break_position = Some(position + 1);
				print_line.push(DrawChar::space());
				if *char == '\t' {
					let tab_chars_left = TAB_SIZE - (x % TAB_SIZE);
					for _c in 0..tab_chars_left {
						if x == width {
							break;
						}
						x += 1;
						print_line.push(DrawChar::space());
					}
				}
			} else {
				let dc = match &draw_context {
					Some(context) => self.setup_draw_char(*char, context.line, position, context.lines, context.highlight),
					None => DrawChar::new(*char, DrawCharMode::Plain),
				};
				print_line.push(dc);
			}
			position += 1;
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

#[cfg(test)]
mod tests {
	use crate::book::{Book, Line};
	use crate::terminal::view::{DrawChar, DrawCharMode, Render, RenderContext};
	use crate::terminal::view::xi::{fill_print_line, Xi};

	const TEST_WIDTH: usize = 80;

	struct DummyBook {
		lines: Vec<Line>,
	}

	impl Book for DummyBook {
		fn lines(&self) -> &Vec<Line> {
			&self.lines
		}
	}

	fn to_draw_line(str: &str) -> Vec<DrawChar> {
		let mut line = vec![];
		for char in str.chars() {
			line.push(DrawChar::new(char, DrawCharMode::Plain));
		}
		let len = line.len();
		fill_print_line(&mut line, TEST_WIDTH - len);
		line
	}

	#[test]
	fn test_wrap() {
		let mut lines = vec![];
		lines.push(Line::new("SIGNET"));
		lines.push(Line::new("Published by New American Library, a division of Penguin Group (USA) Inc., 375 Hudson Street, New York, New York 10014, USA Penguin Group (Canada), 90 Eglinton Avenue East, Suite 700, Toronto, Ontario M4P 2Y3, Canada (a division of Pearson Penguin Canada Inc.) Penguin Books Ltd., 80 Strand, London WC2R 0RL, England Penguin Ireland, 25 St. Stephen’s Green, Dublin 2, Ireland (a division of Penguin Books Ltd.) Penguin Group (Australia), 250 Camberwell Road, Camberwell, Victoria 3124, Australia (a division of Pearson Australia Group Pty. Ltd.) Penguin Books India Pvt. Ltd., 11 Community Centre, Panchsheel Park, New Delhi - 110 017, India Penguin Group (NZ), 67 Apollo Drive, Mairangi Bay, Albany, Auckland 1311, New Zealand (a division of Pearson New Zealand Ltd.) Penguin Books (South Africa) (Pty.) Ltd., 24 Sturdee Avenue, Rosebank, Johannesburg 2196, South Africa"));
		lines.push(Line::new("Penguin Books Ltd., Registered Offices: 80 Strand, London WC2R 0RL, England"));
		lines.push(Line::new("Published by Signet, an imprint of New American Library, a division of Penguin Group (USA) Inc. Previously published in a Viking edition. First Signet Printing, August 1983 70"));
		lines.push(Line::new("Copyright © Stephen King, 1982"));
		lines.push(Line::new("All rights reserved"));
		lines.push(Line::new("eISBN : 978-1-101-13808-3"));
		lines.push(Line::new("Grateful acknowledgment is made to the following for permission to reprint copyrighted material."));
		lines.push(Line::new("Beechwood Music Corporation and Castle Music Pty. Limited:Portions of lyrics from “Tie Me Kangaroo Down, Sport,” by Rolf Harris. Copyright © Castle Music Pty. Limited, 1960. Assigned to and copyrighted © Beechwood Music Corp., 1961 for the United States and Canada. Copyright © Castle Music Pty. Limited for other territories. Used by permission. All rights reserved."));
		lines.push(Line::new("Big Seven Music Corporation:Portions of lyrics from “Party Doll,” by Buddy Knox and Jimmy Bowen. Copyright © Big Seven Music Corp., 1956. Portions of lyrics from “Sorry (I Ran All the Way Home)” by Zwirn/Giosasi. Copyright © Big Seven Music Corp., 1959. All rights reserved."));
		lines.push(Line::new("Holt, Rinehart and Winston, Publishers; Jonathan Cape Ltd.; and the Estate of Robert Frost:Two lines from “Mending Wall” from The Poetry of Robert Frost,edited by Edward Connery Lathem. Copyright ©Holt, Rinehart and Winston, 1930, 1939, 1969. Copyright © Robert Frost, 1958. Copyright © Lesley Frost Ballantine, 1967."));
		lines.push(Line::new("REGISTERED TRADEMARK—MARCA REGISTRADA"));
		lines.push(Line::new("Without limiting the rights under copyright reserved above, no part of this publication may be reproduced, stored in or introduced into a retrieval system, or transmitted, in any form, or by any means (electronic, mechanical, photocopying, recording, or otherwise), without the prior written permission of both the copyright owner and the above publisher of this book."));
		lines.push(Line::new("PUBLISHER’S NOTE"));
		lines.push(Line::new("These are works of fiction. Names, characters, places, and incidents either are the product of the author’s imagination or are used fictitiously, and any resemblance to actual persons, living or dead, business establishments, events, or locales is entirely coincidental."));
		lines.push(Line::new("The publisher does not have any control over and does not assume any responsibility for author or third-party Web sites or their content."));
		lines.push(Line::new("The scanning, uploading, and distribution of this book via the Internet or via any other means without the permission of the publisher is illegal and punishable by law. Please purchase only authorized electronic editions, and do not participate in or encourage electronic piracy of copyrighted materials. Your support of the author’s rights is appreciated."));
		lines.push(Line::new("http://us.penguingroup.com"));

		let mut context = RenderContext {
			width: TEST_WIDTH,
			height: 23,
			print_lines: vec![],
			leading_space: 2,
		};
		let book: Box<dyn Book> = Box::new(DummyBook { lines });
		let mut xi = Xi {};
		// first page draw result verify
		let next = xi.redraw(book.as_ref(), &book.lines(), 0, 0, &None, &mut context);

		assert!(next.is_some());
		let next = next.unwrap();
		assert_eq!(next.line, 8);
		assert_eq!(next.offset, 77);

		let mut result_lines = vec![];
		result_lines.push(to_draw_line("  SIGNET"));
		result_lines.push(to_draw_line("  Published by New American Library, a division of Penguin Group (USA) Inc., 375"));
		result_lines.push(to_draw_line("Hudson Street, New York, New York 10014, USA Penguin Group (Canada), 90 Eglinton"));
		result_lines.push(to_draw_line("Avenue East, Suite 700, Toronto, Ontario M4P 2Y3, Canada (a division of Pearson"));
		result_lines.push(to_draw_line("Penguin Canada Inc.) Penguin Books Ltd., 80 Strand, London WC2R 0RL, England"));
		result_lines.push(to_draw_line("Penguin Ireland, 25 St. Stephen’s Green, Dublin 2, Ireland (a division of"));
		result_lines.push(to_draw_line("Penguin Books Ltd.) Penguin Group (Australia), 250 Camberwell Road, Camberwell,"));
		result_lines.push(to_draw_line("Victoria 3124, Australia (a division of Pearson Australia Group Pty. Ltd.)"));
		result_lines.push(to_draw_line("Penguin Books India Pvt. Ltd., 11 Community Centre, Panchsheel Park, New Delhi -"));
		result_lines.push(to_draw_line("110 017, India Penguin Group (NZ), 67 Apollo Drive, Mairangi Bay, Albany,"));
		result_lines.push(to_draw_line("Auckland 1311, New Zealand (a division of Pearson New Zealand Ltd.) Penguin"));
		result_lines.push(to_draw_line("Books (South Africa) (Pty.) Ltd., 24 Sturdee Avenue, Rosebank, Johannesburg"));
		result_lines.push(to_draw_line("2196, South Africa"));
		result_lines.push(to_draw_line("  Penguin Books Ltd., Registered Offices: 80 Strand, London WC2R 0RL, England"));
		result_lines.push(to_draw_line("  Published by Signet, an imprint of New American Library, a division of Penguin"));
		result_lines.push(to_draw_line("Group (USA) Inc. Previously published in a Viking edition. First Signet"));
		result_lines.push(to_draw_line("Printing, August 1983 70"));
		result_lines.push(to_draw_line("  Copyright © Stephen King, 1982"));
		result_lines.push(to_draw_line("  All rights reserved"));
		result_lines.push(to_draw_line("  eISBN : 978-1-101-13808-3"));
		result_lines.push(to_draw_line("  Grateful acknowledgment is made to the following for permission to reprint"));
		result_lines.push(to_draw_line("copyrighted material."));
		result_lines.push(to_draw_line("  Beechwood Music Corporation and Castle Music Pty. Limited:Portions of lyrics"));

		for index in 0..result_lines.len() {
			let line = &context.print_lines[index];
			let result_line = &result_lines[index];
			assert_eq!(line.len(), result_line.len());
		}

		// 2nd page draw result verify
		let next = xi.redraw(book.as_ref(), &book.lines(), next.line, next.offset, &None, &mut context);

		assert!(next.is_some());
		let next = next.unwrap();
		assert_eq!(next.line, 14);
		assert_eq!(next.offset, 234);

		let mut result_lines = vec![];
		result_lines.push(to_draw_line("from “Tie Me Kangaroo Down, Sport,” by Rolf Harris. Copyright © Castle Music"));
		result_lines.push(to_draw_line("Pty. Limited, 1960. Assigned to and copyrighted © Beechwood Music Corp., 1961"));
		result_lines.push(to_draw_line("for the United States and Canada. Copyright © Castle Music Pty. Limited for"));
		result_lines.push(to_draw_line("other territories. Used by permission. All rights reserved."));
		result_lines.push(to_draw_line("  Big Seven Music Corporation:Portions of lyrics from “Party Doll,” by Buddy"));
		result_lines.push(to_draw_line("Knox and Jimmy Bowen. Copyright © Big Seven Music Corp., 1956. Portions of"));
		result_lines.push(to_draw_line("lyrics from “Sorry (I Ran All the Way Home)” by Zwirn/Giosasi. Copyright © Big"));
		result_lines.push(to_draw_line("Seven Music Corp., 1959. All rights reserved."));
		result_lines.push(to_draw_line("  Holt, Rinehart and Winston, Publishers; Jonathan Cape Ltd.; and the Estate of"));
		result_lines.push(to_draw_line("Robert Frost:Two lines from “Mending Wall” from The Poetry of Robert"));
		result_lines.push(to_draw_line("Frost,edited by Edward Connery Lathem. Copyright ©Holt, Rinehart and Winston,"));
		result_lines.push(to_draw_line("1930, 1939, 1969. Copyright © Robert Frost, 1958. Copyright © Lesley Frost"));
		result_lines.push(to_draw_line("Ballantine, 1967."));
		result_lines.push(to_draw_line("  REGISTERED TRADEMARK—MARCA REGISTRADA"));
		result_lines.push(to_draw_line("  Without limiting the rights under copyright reserved above, no part of this"));
		result_lines.push(to_draw_line("publication may be reproduced, stored in or introduced into a retrieval system,"));
		result_lines.push(to_draw_line("or transmitted, in any form, or by any means (electronic, mechanical,"));
		result_lines.push(to_draw_line("photocopying, recording, or otherwise), without the prior written permission of"));
		result_lines.push(to_draw_line("both the copyright owner and the above publisher of this book."));
		result_lines.push(to_draw_line("  PUBLISHER’S NOTE"));
		result_lines.push(to_draw_line("  These are works of fiction. Names, characters, places, and incidents either"));
		result_lines.push(to_draw_line("are the product of the author’s imagination or are used fictitiously, and any"));
		result_lines.push(to_draw_line("resemblance to actual persons, living or dead, business establishments, events,"));

		for index in 0..result_lines.len() {
			let line = &context.print_lines[index];
			let result_line = &result_lines[index];
			assert_eq!(line.len(), result_line.len());
		}

		// 3rd page draw result verify
		let next = xi.redraw(book.as_ref(), &book.lines(), next.line, next.offset, &None, &mut context);

		assert!(next.is_none());
		let mut result_lines = vec![];
		result_lines.push(to_draw_line("or locales is entirely coincidental."));
		result_lines.push(to_draw_line("  The publisher does not have any control over and does not assume any"));
		result_lines.push(to_draw_line("responsibility for author or third-party Web sites or their content."));
		result_lines.push(to_draw_line("  The scanning, uploading, and distribution of this book via the Internet or via"));
		result_lines.push(to_draw_line("any other means without the permission of the publisher is illegal and"));
		result_lines.push(to_draw_line("punishable by law. Please purchase only authorized electronic editions, and do"));
		result_lines.push(to_draw_line("not participate in or encourage electronic piracy of copyrighted materials. Your"));
		result_lines.push(to_draw_line("support of the author’s rights is appreciated."));
		result_lines.push(to_draw_line("  http://us.penguingroup.com"));
		result_lines.push(to_draw_line(""));
		result_lines.push(to_draw_line(""));
		result_lines.push(to_draw_line(""));
		result_lines.push(to_draw_line(""));
		result_lines.push(to_draw_line(""));
		result_lines.push(to_draw_line(""));
		result_lines.push(to_draw_line(""));
		result_lines.push(to_draw_line(""));
		result_lines.push(to_draw_line(""));
		result_lines.push(to_draw_line(""));
		result_lines.push(to_draw_line(""));
		result_lines.push(to_draw_line(""));
		result_lines.push(to_draw_line(""));
		result_lines.push(to_draw_line(""));

		assert_eq!(context.print_lines.len(), result_lines.len());
		for index in 0..result_lines.len() {
			let line = &context.print_lines[index];
			let result_line = &result_lines[index];
			assert_eq!(line.len(), result_line.len());
		}
	}
}
