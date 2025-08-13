use anyhow::Result;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

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
	let mut reader = quick_xml::Reader::from_str(&text);
	reader.config_mut().trim_text(true);
	let mut html = String::with_capacity(xhtml.len());
	let mut found = false;
	loop {
		match reader.read_event()? {
			Event::Start(e) => if found {
				write_start(&e, &mut html, &mut reader)?;
			} else if e.name().as_ref() == b"html" {
				found = true;
				write_start(&e, &mut html, &mut reader)?;
			}
			Event::End(e) => if found {
				write_end(e.name().as_ref(), &mut html);
			} else if e.name().as_ref() == b"html" {
				html.push_str("</html>");
				break;
			}
			Event::Empty(e) => if found {
				write_start(&e, &mut html, &mut reader)?;
				write_end(e.name().as_ref(), &mut html);
			}
			Event::Text(e) => if found {
				let cow = e.into_inner();
				let text = String::from_utf8_lossy(cow.as_ref());
				if text.len() > 0 {
					html.push_str(&text);
				}
			}
			Event::CData(_) => {}
			Event::Comment(_) => {}
			Event::Decl(_) => {}
			Event::PI(_) => {}
			Event::DocType(_) => {}
			Event::GeneralRef(_) => {}
			Event::Eof => break,
		}
	}
	Ok(html)
}

#[inline]
fn is_void_element(name: &[u8]) -> bool
{
	const VOID_ELEMENTS: [&[u8]; 16] = [b"area", b"base", b"br", b"col", b"command", b"embed", b"hr", b"img", b"input", b"keygen", b"link", b"meta", b"param", b"source", b"track", b"wbr"];
	VOID_ELEMENTS.binary_search(&name).is_ok()
}

fn write_start(start: &BytesStart, html: &mut String, reader: &mut Reader<&[u8]>) -> Result<()>
{
	let tag_name = String::from_utf8_lossy(start.as_ref());
	html.push('<');
	html.push_str(&tag_name);
	write_attrs(start, html, reader)?;
	html.push('>');
	Ok(())
}

fn write_end(name: &[u8], html: &mut String)
{
	if is_void_element(name) {
		return;
	}
	html.push_str("</");
	html.push_str(&String::from_utf8_lossy(name));
	html.push('>');
}

#[inline(always)]
fn write_attrs(start: &BytesStart, html: &mut String, reader: &mut Reader<&[u8]>) -> Result<()>
{
	for attr in start.attributes() {
		let attr = attr?;
		html.push(' ');

		// concat name
		let name = attr.key;
		if let Some(prefix) = name.prefix() {
			let prefix = String::from_utf8_lossy(prefix.as_ref());
			html.push_str(&prefix);
			html.push(':');
		}
		html.push_str(&String::from_utf8_lossy(name.local_name().as_ref()));

		// concat value
		html.push_str(r#"=""#);
		html.push_str(&String::from_utf8_lossy(&attr.value));
		html.push('"');
	}
	Ok(())
}
