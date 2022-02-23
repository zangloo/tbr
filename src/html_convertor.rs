use std::cell::RefCell;
use std::mem;

use anyhow::Result;
use html5ever::{parse_document, ParseOpts};
use html5ever::tendril::TendrilSink;
use html5ever::tree_builder::TreeBuilderOpts;
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
	let mut space_prev = false;
	match &handle.data {
		Text { contents } => {
			if context.started {
				for c in contents.borrow().chars() {
					match c {
						'\n' => {
							push_buf(context);
							space_prev = true;
						}
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
				local_name!("head") | local_name!("style") | local_name!("script") => {
					true
				}
				local_name!("p") | local_name!("br") => {
					push_buf(context);
					let end = process_children(handle, context);
					push_buf(context);
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
		attr.name.local == local_name!("id") && attr.value.to_string() == id
	}).is_some()
}

fn process_children(handle: &Handle, context: &mut ParseContext) -> bool {
	for child in handle.children.borrow().iter() {
		if !convert_dom_to_lines(&child, context) {
			return false;
		}
	}
	true
}
