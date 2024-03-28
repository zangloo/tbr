use anyhow::{anyhow, Result};
use roxmltree::{Children, Document, Node, NodeType};

/// referenced to xhtml_entities.md
/// token from https://www.webstandards.org/learn/reference/charts/entities/named_entities/index.html
const XHTML_ENTITIES: [(&str, &str); 95] = [
	("&Aacute;", "&#193;"),
	("&aacute;", "&#225;"),
	("&Acirc;", "&#194;"),
	("&acirc;", "&#226;"),
	("&acute;", "&#180;"),
	("&AElig;", "&#198;"),
	("&aelig;", "&#230;"),
	("&Agrave;", "&#192;"),
	("&agrave;", "&#224;"),
	("&Aring;", "&#197;"),
	("&aring;", "&#229;"),
	("&Atilde;", "&#195;"),
	("&atilde;", "&#227;"),
	("&Auml;", "&#196;"),
	("&auml;", "&#228;"),
	("&brvbar;", "&#166;"),
	("&Ccedil;", "&#199;"),
	("&ccedil;", "&#231;"),
	("&cedil;", "&#184;"),
	("&cent;", "&#162;"),
	("&copy;", "&#169;"),
	("&curren;", "&#164;"),
	("&deg;", "&#176;"),
	("&divide;", "&#247;"),
	("&Eacute;", "&#201;"),
	("&eacute;", "&#233;"),
	("&Ecirc;", "&#202;"),
	("&ecirc;", "&#234;"),
	("&Egrave;", "&#200;"),
	("&egrave;", "&#232;"),
	("&eth;", "&#208;"),
	("&eth;", "&#240;"),
	("&Euml;", "&#203;"),
	("&euml;", "&#235;"),
	("&frac12;", "&#189;"),
	("&frac14;", "&#188;"),
	("&frac34;", "&#190;"),
	("&Iacute;", "&#205;"),
	("&iacute;", "&#237;"),
	("&Icirc;", "&#206;"),
	("&icirc;", "&#238;"),
	("&Igrave;", "&#204;"),
	("&igrave;", "&#236;"),
	("&iquest;", "&#191;"),
	("&Iuml;", "&#207;"),
	("&iuml;", "&#239;"),
	("&laquo;", "&#171;"),
	("&macr;", "&#175;"),
	("&micro;", "&#181;"),
	("&middot;", "&#183;"),
	("&nbsp;", "&#160;"),
	("&not;", "&#172;"),
	("&Ntilde;", "&#209;"),
	("&ntilde;", "&#241;"),
	("&Oacute;", "&#211;"),
	("&oacute;", "&#243;"),
	("&Ocirc;", "&#212;"),
	("&ocirc;", "&#244;"),
	("&Ograve;", "&#210;"),
	("&ograve;", "&#242;"),
	("&ordf;", "&#170;"),
	("&ordm;", "&#186;"),
	("&Oslash;", "&#216;"),
	("&oslash;", "&#248;"),
	("&Otilde;", "&#213;"),
	("&otilde;", "&#245;"),
	("&Ouml;", "&#214;"),
	("&ouml;", "&#246;"),
	("&para;", "&#182;"),
	("&plusmn;", "&#177;"),
	("&pound;", "&#163;"),
	("&raquo;", "&#187;"),
	("&reg;", "&#174;"),
	("&sect;", "&#167;"),
	("&shy;", "&#173;"),
	("&sup1;", "&#185;"),
	("&sup2;", "&#178;"),
	("&sup3;", "&#179;"),
	("&szlig;", "&#223;"),
	("&thorn;", "&#222;"),
	("&thorn;", "&#254;"),
	("&times;", "&#215;"),
	("&Uacute;", "&#218;"),
	("&uacute;", "&#250;"),
	("&Ucirc;", "&#219;"),
	("&ucirc;", "&#251;"),
	("&Ugrave;", "&#217;"),
	("&ugrave;", "&#249;"),
	("&uml;", "&#168;"),
	("&Uuml;", "&#220;"),
	("&uuml;", "&#252;"),
	("&Yacute;", "&#221;"),
	("&yacute;", "&#253;"),
	("&yen;", "&#165;"),
	("&yuml;", "&#255;"),
];

/// Some xhtml contain html entity like &nbsp;
/// So replace these using codes in the xhtml
fn preprocess(xhtml: &str) -> String
{
	let mut text = String::with_capacity(xhtml.len());
	let mut entity = String::with_capacity(10);
	for ch in xhtml.chars() {
		if entity.is_empty() {
			if ch == '&' {
				entity.push('&');
			} else {
				text.push(ch);
			}
		} else {
			entity.push(ch);
			if ch == ';' {
				if let Ok(idx) = XHTML_ENTITIES
					.binary_search_by(|(name, _)| name.cmp(&entity.as_str())) {
					text.push_str(XHTML_ENTITIES[idx].1);
				} else {
					text.push_str(&entity);
				}
				entity.clear();
			}
		}
	}

	if !entity.is_empty() {
		text.push_str(&entity);
	}
	text
}

pub fn xhtml_to_html(xhtml: &str) -> Result<String>
{
	let text = preprocess(xhtml);
	let doc = Document::parse(&text)?;
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
			write_attrs(&node, html);
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
fn write_attrs(node: &Node, html: &mut String)
{
	const DOUBLE_QUOTA_ESCAPE: &str = "&quot;";

	for attr in node.attributes() {
		html.push(' ');
		if let Some(namespace) = attr.namespace() {
			let prefix = if let Some(prefix) = node.lookup_prefix(namespace) {
				prefix
			} else {
				namespace
			};
			html.push_str(prefix);
			html.push(':');
		}
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
	VOID_ELEMENTS.binary_search(&name).is_ok()
}
