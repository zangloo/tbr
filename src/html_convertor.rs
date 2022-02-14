use anyhow::Result;
use std::io;
use std::io::Write;
use html5ever::{parse_document, ParseOpts};
use html5ever::tendril::TendrilSink;
use html5ever::tree_builder::TreeBuilderOpts;
use markup5ever_rcdom::NodeData::{Document, Element, Text};
use markup5ever_rcdom::{Handle, RcDom};
use crate::common::plain_text;

struct Discard {}

// reference https://gitlab.com/spacecowboy/html2runes/-/blob/master/src/markdown.rs
impl Write for Discard {
	fn write(&mut self, bytes: &[u8]) -> std::result::Result<usize, io::Error> {
		Ok(bytes.len())
	}
	fn flush(&mut self) -> std::result::Result<(), io::Error> {
		Ok(())
	}
}

pub(crate) fn html_lines(text: Vec<u8>) -> Result<Vec<String>> {
	let opts = ParseOpts {
		tree_builder: TreeBuilderOpts {
			drop_doctype: true,
			..Default::default()
		},
		..Default::default()
	};
	let text = plain_text(text, false)?;
	let dom = parse_document(RcDom::default(), opts)
		.from_utf8()
		.read_from(&mut text.as_bytes())
		.unwrap();
	let mut lines = vec![];
	let mut buf = String::from("");
	convert_dom_to_lines(&dom.document, &mut buf, &mut lines);
	if buf.len() > 0 {
		lines.push(buf);
	}
	if lines.len() == 0 {
		lines.push("No content.".to_string());
	}
	Ok(lines)
}

fn push_buf(buf: &String, lines: &Vec<String>) -> bool {
	// ignore empty line if prev line is empty too.
	if buf.trim().len() == 0 {
		let line_count = lines.len();
		if line_count == 0 || lines[line_count - 1].len() == 0 {
			return false;
		}
	}
	return true;
}

fn convert_dom_to_lines(handle: &Handle, buf: &mut String, lines: &mut Vec<String>) {
	let mut space_prev = false;
	match &handle.data {
		Text { contents } => {
			for c in contents.borrow().chars() {
				match c {
					'\n' => {
						if push_buf(buf, lines) {
							lines.push(buf.clone());
							buf.clear();
						}
						space_prev = true;
					}
					' ' => {
						if !space_prev {
							buf.push(' ');
							space_prev = true;
						}
					}
					_ => {
						space_prev = false;
						buf.push(c);
					}
				}
			}
		}
		Element { name, .. } => {
			match name.local {
				local_name!("head") | local_name!("style") | local_name!("script") => {
					// ignore these
				}
				local_name!("p") | local_name!("br") => {
					if push_buf(buf, lines) {
						lines.push(buf.clone());
						buf.clear();
					}
					for child in handle.children.borrow().iter() {
						convert_dom_to_lines(&child, buf, lines);
					}
					if push_buf(buf, lines) {
						lines.push(buf.clone());
						buf.clear();
					}
				}
				_ => {
					for child in handle.children.borrow().iter() {
						convert_dom_to_lines(&child, buf, lines);
					}
				}
			}
		}
		Document {} => {
			for child in handle.children.borrow().iter() {
				convert_dom_to_lines(&child, buf, lines);
			}
		}
		_ => {}
	}
}
