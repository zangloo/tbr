use std::cell::RefCell;
use std::collections::HashMap;
use std::mem;

use anyhow::Result;
use html5ever::{parse_document, ParseOpts};
use html5ever::tendril::TendrilSink;
use html5ever::tree_builder::TreeBuilderOpts;
use markup5ever::Attribute;
use markup5ever::LocalName;
use markup5ever_rcdom::{Handle, RcDom};
use markup5ever_rcdom::NodeData::{Document, Element, Text};

use crate::book::Line;
use crate::common::plain_text;
use crate::view::Position;

pub struct HtmlContent {
	pub lines: Vec<Line>,
	pub id_map: HashMap<String, Position>,
}

impl Default for HtmlContent {
	fn default() -> Self {
		HtmlContent { lines: vec![], id_map: HashMap::new() }
	}
}

struct ParseContext {
	buf: Line,
	content: HtmlContent,
}

pub(crate) fn html_content(text: Vec<u8>) -> Result<HtmlContent> {
	let text = plain_text(text, false)?;
	html_str_content(text.as_str())
}

pub(crate) fn html_str_content(str: &str) -> Result<HtmlContent> {
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
		buf: Default::default(),
		content: Default::default(),
	};
	convert_dom_to_lines(&dom.document, &mut context);
	if !context.buf.is_empty() {
		context.content.lines.push(context.buf);
	}
	if context.content.lines.is_empty() {
		context.content.lines.push(Line::from("No content."));
	}
	Ok(context.content)
}

fn push_for_class(context: &mut ParseContext, attrs: &RefCell<Vec<Attribute>>) {
	if !context.buf.is_empty() {
		if let Some(class) = attr_value("class", attrs) {
			for class_name in DIV_PUSH_CLASSES {
				if class.contains(class_name) {
					push_buf(context);
					return;
				}
			}
		}
	}
}

fn push_buf(context: &mut ParseContext) {
	// ignore empty line if prev line is empty too.
	context.buf.trim();
	if context.buf.is_empty() {
		let line_count = context.content.lines.len();
		if line_count == 0 || context.content.lines[line_count - 1].is_empty() {
			return;
		}
	}
	let buf = mem::take(&mut context.buf);
	context.content.lines.push(buf);
}

const DIV_PUSH_CLASSES: [&str; 3] = ["contents", "toc", "mulu"];

fn convert_dom_to_lines(handle: &Handle, context: &mut ParseContext) {
	match &handle.data {
		Text { contents } => {
			let mut space_prev = true;
			for c in contents.borrow().chars() {
				if c.is_whitespace() {
					if !space_prev && !context.buf.is_empty() {
						context.buf.push(' ');
						space_prev = true;
					}
				} else {
					space_prev = false;
					context.buf.push(c);
				}
			}
		}
		Element { name, attrs, .. } => {
			if let Some(id) = attr_value("id", &attrs) {
				let position = Position::new(context.content.lines.len(), context.buf.len());
				context.content.id_map.insert(id, position);
			}
			match name.local {
				local_name!("head") | local_name!("style") | local_name!("script") => {}
				local_name!("div") | local_name!("dt") => {
					push_for_class(context, attrs);
					process_children(handle, context);
					push_for_class(context, attrs);
				}
				local_name!("p") | local_name!("h4") | local_name!("h3") | local_name!("h2") | local_name!("li") => {
					if !context.buf.is_empty() {
						push_buf(context);
					}
					process_children(handle, context);
					push_buf(context);
				}
				local_name!("br") => {
					if !context.buf.is_empty() {
						push_buf(context);
					}
					process_children(handle, context)
				}
				local_name!("a") => {
					let start_line = context.content.lines.len();
					let mut start_position = context.buf.len();
					let end = process_children(handle, context);
					if let Some(href) = attr_value("href", &attrs) {
						let end_line = context.content.lines.len();
						let end_position = context.buf.len();
						if start_line != end_line {
							for line_index in start_line..end_line {
								let line = &mut context.content.lines[line_index];
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
					end
				}
				_ => process_children(handle, context),
			}
		}
		Document {} => process_children(handle, context),
		_ => {}
	}
}

fn attr_value(attr_name: &str, attrs: &RefCell<Vec<Attribute>>) -> Option<String> {
	let attrs = attrs.borrow();
	let attr = attrs.iter().find(move |attr| {
		attr.name.local == LocalName::from(attr_name)
	})?;
	Some(attr.value.to_string())
}

fn process_children(handle: &Handle, context: &mut ParseContext) {
	for child in handle.children.borrow().iter() {
		convert_dom_to_lines(&child, context)
	}
}
