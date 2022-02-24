use std::cell::RefCell;
use std::mem;

use anyhow::Result;
use html5ever::{parse_document, ParseOpts};
use html5ever::tendril::TendrilSink;
use html5ever::tree_builder::TreeBuilderOpts;
use markup5ever::LocalName;
use markup5ever::Attribute;
use markup5ever_rcdom::{Handle, RcDom};
use markup5ever_rcdom::NodeData::{Document, Element, Text};
use crate::book::Line;

use crate::common::plain_text;

struct ParseContext<'a> {
	start_id: Option<&'a str>,
	started: bool,
	stop_id: Option<&'a str>,
	buf: Line,
	lines: Vec<Line>,
}

pub(crate) fn html_lines(text: Vec<u8>) -> Result<Vec<Line>> {
	let text = plain_text(text, false)?;
	html_str_lines(text.as_str(), None, None)
}

pub(crate) fn html_str_lines(str: &str, start_id: Option<&str>, stop_id: Option<&str>) -> Result<Vec<Line>> {
	let opts = ParseOpts {
		tree_builder: TreeBuilderOpts {
			drop_doctype: true,
			..Default::default()
		},
		..Default::default()
	};
	let dom = parse_document(RcDom::default(), opts)
		.from_utf8()
		.read_from(&mut str.as_bytes())
		.unwrap();
	let mut context = ParseContext {
		start_id,
		started: start_id.is_none(),
		stop_id,
		buf: Default::default(),
		lines: vec![],
	};
	convert_dom_to_lines(&dom.document, &mut context);
	if !context.buf.is_empty() {
		context.lines.push(context.buf);
	}
	if context.lines.is_empty() {
		context.lines.push(Line::from("No content."));
	}
	Ok(context.lines)
}

fn push_buf(context: &mut ParseContext) {
	// ignore empty line if prev line is empty too.
	context.buf.trim();
	if context.buf.is_empty() {
		let line_count = context.lines.len();
		if line_count == 0 || context.lines[line_count - 1].is_empty() {
			return;
		}
	}
	let buf = mem::take(&mut context.buf);
	context.lines.push(buf);
}

fn convert_dom_to_lines(handle: &Handle, context: &mut ParseContext) -> bool {
	match &handle.data {
		Text { contents } => {
			if context.started {
				let mut space_prev = true;
				for c in contents.borrow().chars() {
					match c {
						'\n' => {}
						' ' => {
							if !space_prev {
								context.buf.push(' ');
								space_prev = true;
							}
						}
						_ => {
							space_prev = false;
							context.buf.push(c);
						}
					}
				}
			}
			return true;
		}
		Element { name, attrs, .. } => {
			if !context.started {
				if match_id(context.start_id.unwrap(), &attrs) {
					context.started = true;
				}
			} else if let Some(stop_id) = context.stop_id {
				if match_id(stop_id, &attrs) {
					return false;
				}
			}
			match name.local {
				local_name!("img") => {
					if let Some(src) = attr_value("src", &attrs) {
						if !context.buf.is_empty() {
							push_buf(context);
						}
						context.buf.concat("[IMG:");
						context.buf.concat(&src);
						context.buf.push(']');
						context.buf.add_link(&src, 0, context.buf.len());
						push_buf(context);
					}
					true
				}
				local_name!("head") | local_name!("style") | local_name!("script") => {
					true
				}
				local_name!("p") | local_name!("h2") | local_name!("li") => {
					if context.started {
						push_buf(context);
					}
					process_children(handle, context)
				}
				local_name!("br") => {
					if context.started {
						push_buf(context);
					}
					process_children(handle, context)
				}
				local_name!("a") => {
					let start_line = context.lines.len();
					let mut start_position = context.buf.len();
					let end = process_children(handle, context);
					if context.started {
						if let Some(href) = attr_value("href", &attrs) {
							let end_line = context.lines.len();
							let end_position = context.buf.len();
							if start_line != end_line {
								for line_index in start_line..end_line {
									let line = &mut context.lines[line_index];
									let len = line.len();
									if start_position < len {
										line.add_link(&href, start_position, len);
									}
									start_position = 0;
								}
							}
							if start_position < end_position {
								context.buf.add_link(&href, start_position, end_position);
							}
						}
					}
					end
				}
				_ => process_children(handle, context),
			}
		}
		Document {} => process_children(handle, context),
		_ => true,
	}
}

fn match_id(id: &str, attrs: &RefCell<Vec<Attribute>>) -> bool {
	attrs.borrow().iter().find(|attr| {
		attr.name.local == LocalName::from("id") && attr.value.to_string() == id
	}).is_some()
}

fn attr_value(attr_name: &str, attrs: &RefCell<Vec<Attribute>>) -> Option<String> {
	let attrs = attrs.borrow();
	let attr = attrs.iter().find(move |attr| {
		attr.name.local == LocalName::from(attr_name)
	})?;
	Some(attr.value.to_string())
}

fn process_children(handle: &Handle, context: &mut ParseContext) -> bool {
	for child in handle.children.borrow().iter() {
		if !convert_dom_to_lines(&child, context) {
			return false;
		}
	}
	true
}
