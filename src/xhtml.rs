use anyhow::{anyhow, Result};
use roxmltree::{Attributes, Children, Document, Node, NodeType};

pub fn xhtml_to_html(xhtml: &str) -> Result<String>
{
	let doc = Document::parse(xhtml)?;
	let root = doc.root_element();
	let html_node = if root.has_tag_name("html") {
		root
	} else {
		root.children()
			.find(|node| node.has_tag_name("html"))
			.ok_or(anyhow!("xhtml with no html node"))?
	};
	let mut html = String::with_capacity(xhtml.len());
	let mut stack = vec![];
	if let Some((mut iter, mut child, _)) = write_node(html_node, &mut html) {
		'main_loop:
		loop {
			if let Some((next_iter, next_child, tag_name)) = write_node(child, &mut html) {
				stack.push((iter, tag_name));
				iter = next_iter;
				child = next_child;
				continue;
			}
			loop {
				if let Some(next_child) = iter.next() {
					child = next_child;
					continue 'main_loop;
				}
				if let Some((prev_iter, tag_name)) = stack.pop() {
					write_tail(tag_name, &mut html);
					iter = prev_iter;
				} else {
					break 'main_loop;
				}
			}
		}
		html.push_str("</html>");
	}
	Ok(html)
}

#[inline(always)]
fn write_tail(tag_name: &str, html: &mut String)
{
	html.push_str("</");
	html.push_str(tag_name);
	html.push('>');
}

#[inline(always)]
fn write_node<'a, 'i>(node: Node<'a, 'i>, html: &mut String)
	-> Option<(Children<'a, 'i>, Node<'a, 'i>, &'a str)>
{
	match node.node_type() {
		NodeType::Element => {
			let tag_name = node.tag_name().name();
			html.push('<');
			html.push_str(tag_name);
			write_attrs(node.attributes(), html);
			html.push('>');
			if is_void_element(tag_name) {
				return None;
			}
			let mut children = node.children();
			if let Some(child) = children.next() {
				Some((children, child, tag_name))
			} else {
				write_tail(tag_name, html);
				None
			}
		}
		NodeType::Text => {
			if let Some(text) = node.text() {
				let text = text.trim();
				if !text.is_empty() {
					html.push_str(text)
				}
			}
			None
		}
		NodeType::Root |
		NodeType::Comment |
		NodeType::PI => None,
	}
}

#[inline(always)]
fn write_attrs(attrs: Attributes, html: &mut String)
{
	const DOUBLE_QUOTA_ESCAPE: &str = "&quot;";

	for attr in attrs {
		html.push(' ');
		html.push_str(attr.name());
		html.push_str(r#"=""#);
		for char in attr.value().chars() {
			if char == '"' {
				html.push_str(DOUBLE_QUOTA_ESCAPE);
			} else {
				html.push(char);
			}
		}
		html.push('"');
	}
}

#[inline(always)]
fn is_void_element(name: &str) -> bool
{
	const VOID_ELEMENTS: [&str; 16] = ["area", "base", "br", "col", "command", "embed", "hr", "img", "input", "keygen", "link", "meta", "param", "source", "track", "wbr"];

	let mut slice: &[&str] = &VOID_ELEMENTS;
	while !slice.is_empty() {
		let len = slice.len();
		let idx = len >> 1;
		let current = slice[idx];
		if current == name {
			return true;
		} else if current > name {
			slice = &slice[..idx];
		} else {
			let next = idx + 1;
			if next >= len {
				break;
			} else {
				slice = &slice[next..]
			}
		}
	}
	false
}
