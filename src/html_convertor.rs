use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::ops::Deref;
use anyhow::{anyhow, Result};
use cssparser::RGBA;
use ego_tree::iter::Children;
use ego_tree::{NodeId, NodeRef};
use markup5ever::{LocalName, Namespace, Prefix, QualName};
use parcel_css::properties::{border, font, Property};
use parcel_css::properties::border::{Border, BorderSideWidth};
use parcel_css::properties::font::FontSize;
use parcel_css::rules::CssRule;
use parcel_css::stylesheet::{ParserOptions, StyleSheet};
use parcel_css::traits::Parse;
use parcel_css::values::color::CssColor;
use parcel_css::values::length::{Length, LengthPercentage, LengthValue};
use parcel_css::values::percentage;
use scraper::{Html, Node, Selector};
use scraper::node::Element;

use crate::book::{EMPTY_CHAPTER_CONTENT, IMAGE_CHAR, Line, TextStyle};
use crate::common::plain_text;
use crate::common::Position;
use crate::gui::Color32;

pub struct HtmlContent {
	pub title: Option<String>,
	pub lines: Vec<Line>,
	pub id_map: HashMap<String, Position>,
}

impl Default for HtmlContent
{
	fn default() -> Self {
		HtmlContent { title: None, lines: vec![Line::default()], id_map: HashMap::new() }
	}
}

struct ParseContext {
	title: Option<String>,
	content: HtmlContent,
	element_styles: HashMap<NodeId, Vec<TextStyle>>,
}

pub(crate) fn html_content(text: Vec<u8>) -> Result<HtmlContent>
{
	let text = plain_text(text, false)?;
	html_str_content(&text, None::<fn(String) -> Option<&'static String>>)
}

pub(crate) fn html_str_content<'a, F>(str: &str, file_resolver: Option<F>) -> Result<HtmlContent>
	where F: Fn(String) -> Option<&'a String>
{
	let document = Html::parse_document(str);
	let element_styles = load_styles(&document, file_resolver);
	let mut context = ParseContext {
		title: None,
		content: Default::default(),
		element_styles,
	};

	let body_selector = match Selector::parse("body") {
		Ok(s) => s,
		Err(_) => return Err(anyhow!("Failed parse html"))
	};
	let body = document.select(&body_selector).next().unwrap();
	if let Some(id) = &body.value().id {
		context.content.id_map.insert(id.to_string(), Position::new(0, 0));
	}
	convert_dom_to_lines(body.children(), &mut context);
	while let Some(last_line) = context.content.lines.last() {
		if last_line.len() == 0 {
			context.content.lines.pop();
		} else {
			break;
		}
	}
	if context.content.lines.len() == 0 {
		context.content.lines.push(Line::new(EMPTY_CHAPTER_CONTENT));
	}
	Ok(context.content)
}

fn newline_for_class(context: &mut ParseContext, element: &Element)
{
	if !context.content.lines.last().unwrap().is_empty() {
		if let Some(class) = element.attr("class") {
			for class_name in DIV_PUSH_CLASSES {
				if class.contains(class_name) {
					new_line(context, true);
					return;
				}
			}
		}
	}
}

fn new_line(context: &mut ParseContext, ignore_empty_buf: bool)
{
	let mut empty_count = 0;
	// no more then 2 empty lines
	for line in context.content.lines.iter().rev() {
		if line.is_empty() {
			empty_count += 1;
			if empty_count == 2 {
				return;
			}
		} else {
			break;
		}
	}
	if empty_count == 0 || !ignore_empty_buf {
		context.content.lines.push(Line::default())
	}
}

#[inline]
fn new_paragraph(child: NodeRef<Node>, context: &mut ParseContext)
{
	new_line(context, true);
	convert_dom_to_lines(child.children(), context);
	new_line(context, false);
}

#[inline]
fn push_font_size(styles: &mut Vec<TextStyle>, font_level: u8, relative: bool)
{
	for style in styles.iter() {
		if let TextStyle::FontSize { .. } = style {
			return;
		}
	}
	styles.push(font_size_level(font_level, relative));
}

const DIV_PUSH_CLASSES: [&str; 3] = ["contents", "toc", "mulu"];

fn convert_dom_to_lines(children: Children<Node>, context: &mut ParseContext)
{
	for child in children {
		match child.value() {
			Node::Text(contents) => {
				let string = contents.text.to_string();
				let text = string.trim();
				let line = context.content.lines.last_mut().unwrap();
				if text.len() > 0 {
					if line.len() > 0
						&& line.char_at(line.len() - 1).unwrap().is_ascii_alphanumeric()
						&& text.chars().next().unwrap().is_ascii_alphanumeric() {
						line.push(' ');
					}
					line.concat(text);
				}
			}
			Node::Element(element) => {
				let position = Position::new(
					context.content.lines.len() - 1,
					context.content.lines.last().unwrap().len());
				if let Some(id) = &element.id {
					context.content.id_map.insert(id.to_string(), position.clone());
				}
				let mut element_styles = if let Some(styles) = context.element_styles.remove(&child.id()) {
					styles
				} else {
					vec![]
				};
				match element.name.local {
					local_name!("title") => {
						// title is in head, no other text should parsed
						reset_lines(context);
						convert_dom_to_lines(child.children(), context);
						let mut title = String::new();
						for line in &mut context.content.lines {
							if !line.is_empty() {
								title.push_str(&line.to_string());
							}
						}
						if !title.is_empty() {
							context.title = Some(title);
						}
						// ensure no lines parsed
						reset_lines(context);
						context.content.id_map.clear()
					}
					local_name!("script") => {}
					local_name!("div") => {
						newline_for_class(context, element);
						convert_dom_to_lines(child.children(), context);
						newline_for_class(context, element);
					}
					local_name!("h1") => {
						push_font_size(&mut element_styles, 6, true);
						new_paragraph(child, context);
					}
					| local_name!("h2") => {
						push_font_size(&mut element_styles, 5, true);
						new_paragraph(child, context);
					}
					| local_name!("h3") => {
						push_font_size(&mut element_styles, 4, true);
						new_paragraph(child, context);
					}
					| local_name!("h4") => {
						push_font_size(&mut element_styles, 3, true);
						new_paragraph(child, context);
					}
					| local_name!("h5") => {
						push_font_size(&mut element_styles, 2, true);
						new_paragraph(child, context);
					}
					| local_name!("h6") => {
						push_font_size(&mut element_styles, 1, true);
						new_paragraph(child, context);
					}
					local_name!("p")
					| local_name!("blockquote")
					| local_name!("tr")
					| local_name!("dt")
					| local_name!("li") => new_paragraph(child, context),
					local_name!("br") => {
						new_line(context, true);
						convert_dom_to_lines(child.children(), context);
					}
					local_name!("font") => {
						if let Some(level_text) = element.attr("size") {
							if let Ok(level) = level_text.parse::<u8>() {
								push_font_size(&mut element_styles, level, true);
							}
						}
						if let Some(color_text) = element.attr("color") {
							if let Ok(color) = CssColor::parse_string(color_text) {
								if let Some(color) = css_color(&color) {
									element_styles.push(TextStyle::Color(color));
								}
							}
						}
						convert_dom_to_lines(child.children(), context);
					}
					local_name!("a") => {
						if let Some(href) = element.attr("href") {
							element_styles.push(TextStyle::Link(href.to_string()));
						}
						convert_dom_to_lines(child.children(), context);
					}
					local_name!("img") => {
						if let Some(href) = element.attr("src") {
							add_image(href, context);
						}
					}
					local_name!("image") => {
						let name = QualName::new(
							Some(Prefix::from("xlink")),
							Namespace::from("http://www.w3.org/1999/xlink"),
							LocalName::from("href"));
						let href = element.attrs.get(&name).map(Deref::deref);
						if let Some(href) = href {
							add_image(href, context);
						}
					}
					_ => convert_dom_to_lines(child.children(), context),
				}
				if element_styles.len() > 0 {
					let mut offset = position.offset;
					for i in position.line..context.content.lines.len() {
						let line = &mut context.content.lines[i];
						let len = line.len();
						if len > offset {
							let range = offset..len;
							for style in &element_styles {
								line.push_style(style.clone(), range.clone());
							}
						}
						offset = 0;
					}
				}
			}
			Node::Document {} => convert_dom_to_lines(child.children(), context),
			_ => {}
		}
	}
}

fn add_image(href: &str, context: &mut ParseContext)
{
	let line = context.content.lines.last_mut().unwrap();
	let start = line.len();
	line.push(IMAGE_CHAR);
	line.push_style(TextStyle::Image(href.to_string()), start..start + 1);
}

#[inline]
fn reset_lines(context: &mut ParseContext)
{
	context.content.lines.clear();
	context.content.lines.push(Line::default());
}

fn load_styles<'a, F>(document: &Html, file_resolver: Option<F>) -> HashMap<NodeId, Vec<TextStyle>>
	where F: Fn(String) -> Option<&'a String>
{
	let mut element_styles = HashMap::new();
	let stylesheets = if let Some(file_resolver) = file_resolver {
		let mut stylesheets: Vec<StyleSheet> = vec![];
		if let Ok(link_selector) = Selector::parse("link") {
			let selection = document.select(&link_selector);
			for element in selection.into_iter() {
				if let Some(href) = element.value().attr("href") {
					if href.to_lowercase().ends_with(".css") {
						if let Some(content) = file_resolver(href.to_string()) {
							if let Ok(style_sheet) = StyleSheet::parse(&content, ParserOptions::default()) {
								stylesheets.push(style_sheet);
							}
						}
					}
				}
			}
		}
		if stylesheets.len() == 0 {
			return element_styles;
		}
		stylesheets
	} else {
		return element_styles;
	};

	for style_sheet in stylesheets {
		for rule in &style_sheet.rules.0 {
			if let CssRule::Style(style_rule) = rule {
				let mut styles = vec![];
				for property in &style_rule.declarations.declarations {
					if let Some(style) = convert_style(property) {
						styles.push(style);
					}
				}
				if styles.len() == 0 {
					continue;
				}
				let selector_str = style_rule.selectors.to_string();
				if let Ok(selector) = Selector::parse(&selector_str) {
					for element in document.select(&selector) {
						let mut styles = styles.clone();
						match element_styles.entry(element.id()) {
							Entry::Occupied(o) => o.into_mut().append(&mut styles),
							Entry::Vacant(v) => { v.insert(styles); }
						};
					}
				};
			}
		}
	}
	element_styles
}

const HTML_DEFAULT_FONT_SIZE: f32 = 16.0;

#[inline]
fn convert_style(property: &Property) -> Option<TextStyle>
{
	match property {
		Property::Border(border) => border_style(border),
		Property::BorderWidth(width)
		if border_width(&width.top)
			| border_width(&width.left)
			| border_width(&width.right)
			| border_width(&width.bottom) => {
			Some(TextStyle::Border)
		}
		Property::FontSize(size) => Some(font_size(size)),
		Property::TextDecorationLine(line, _) => Some(TextStyle::Line(*line)),
		Property::Color(color) => Some(TextStyle::Color(css_color(color)?)),
		Property::BackgroundColor(color) => Some(TextStyle::BackgroundColor(css_color(color)?)),
		Property::Background(bg) => Some(TextStyle::BackgroundColor(css_color(&bg[0].color)?)),
		_ => None,
	}
}

#[inline]
fn font_size_level(level: u8, relative: bool) -> TextStyle
{
	let scale: f32 = match level {
		1 => 3.0 / 5.0,
		2 => 8.0 / 9.0,
		3 => 1.0,
		4 => 6.0 / 5.0,
		5 => 3.0 / 2.0,
		6 => 2.0 / 1.0,
		7 => 3.0 / 1.0,
		_ => 1.0 // no other level
	};
	TextStyle::FontSize { scale, relative }
}

fn font_size(size: &FontSize) -> TextStyle
{
	match size {
		FontSize::Length(lp) => match lp {
			LengthPercentage::Dimension(lv) => {
				let (scale, relative) = length_value(lv, HTML_DEFAULT_FONT_SIZE);
				TextStyle::FontSize { scale, relative }
			}
			LengthPercentage::Percentage(percentage::Percentage(p)) =>
				TextStyle::FontSize { scale: *p, relative: true },
			LengthPercentage::Calc(_) => // 视而不见
				TextStyle::FontSize { scale: 1.0, relative: false }
		}
		FontSize::Absolute(size) => match size {
			font::AbsoluteFontSize::XXSmall => font_size_level(1, false),
			font::AbsoluteFontSize::XSmall => TextStyle::FontSize { scale: 3.0 / 4.0, relative: false },
			font::AbsoluteFontSize::Small => font_size_level(2, false),
			font::AbsoluteFontSize::Medium => font_size_level(3, false),
			font::AbsoluteFontSize::Large => font_size_level(4, false),
			font::AbsoluteFontSize::XLarge => font_size_level(5, false),
			font::AbsoluteFontSize::XXLarge => font_size_level(6, false),
		}
		FontSize::Relative(size) => match size {
			font::RelativeFontSize::Smaller => font_size_level(2, true),
			font::RelativeFontSize::Larger => font_size_level(4, true),
		}
	}
}

#[inline]
fn border_style(border: &Border) -> Option<TextStyle>
{
	match border.style {
		border::LineStyle::Inset
		| border::LineStyle::Groove
		| border::LineStyle::Outset
		| border::LineStyle::Ridge
		| border::LineStyle::Dotted
		| border::LineStyle::Dashed
		| border::LineStyle::Solid
		| border::LineStyle::Double
		if border_width(&border.width) => Some(TextStyle::Border),
		_ => None
	}
}

#[inline]
fn border_width(width: &BorderSideWidth) -> bool
{
	match width {
		BorderSideWidth::Thin => true,
		BorderSideWidth::Medium => true,
		BorderSideWidth::Thick => true,
		BorderSideWidth::Length(l) => length(l) > 0.0,
	}
}

#[inline]
fn length(length: &Length) -> f32
{
	match length {
		Length::Value(value) => length_value(value, 1.0).0,
		Length::Calc(_) => 1.0,
	}
}

#[inline]
fn length_value(value: &LengthValue, default_size: f32) -> (f32, bool)
{
	match value {
		LengthValue::Px(v) => (v / default_size as f32, false),
		LengthValue::Em(v) => (*v, true),
		LengthValue::Rem(v) => (*v, false),
		// 没见过，无视之
		LengthValue::In(_)
		| LengthValue::Cm(_)
		| LengthValue::Mm(_)
		| LengthValue::Q(_)
		| LengthValue::Pt(_)
		| LengthValue::Pc(_)
		| LengthValue::Ex(_)
		| LengthValue::Rex(_)
		| LengthValue::Ch(_)
		| LengthValue::Rch(_)
		| LengthValue::Cap(_)
		| LengthValue::Rcap(_)
		| LengthValue::Ic(_)
		| LengthValue::Ric(_)
		| LengthValue::Lh(_)
		| LengthValue::Rlh(_)
		| LengthValue::Vw(_)
		| LengthValue::Lvw(_)
		| LengthValue::Svw(_)
		| LengthValue::Dvw(_)
		| LengthValue::Vh(_)
		| LengthValue::Lvh(_)
		| LengthValue::Svh(_)
		| LengthValue::Dvh(_)
		| LengthValue::Vi(_)
		| LengthValue::Svi(_)
		| LengthValue::Lvi(_)
		| LengthValue::Dvi(_)
		| LengthValue::Vb(_)
		| LengthValue::Svb(_)
		| LengthValue::Lvb(_)
		| LengthValue::Dvb(_)
		| LengthValue::Vmin(_)
		| LengthValue::Svmin(_)
		| LengthValue::Lvmin(_)
		| LengthValue::Dvmin(_)
		| LengthValue::Vmax(_)
		| LengthValue::Svmax(_)
		| LengthValue::Lvmax(_)
		| LengthValue::Dvmax(_)
		| LengthValue::Cqw(_)
		| LengthValue::Cqh(_)
		| LengthValue::Cqi(_)
		| LengthValue::Cqb(_)
		| LengthValue::Cqmin(_)
		| LengthValue::Cqmax(_)
		=> (1.0, false),
	}
}

fn css_color(color: &CssColor) -> Option<Color32>
{
	#[inline]
	fn convert(rgba: &RGBA) -> Color32 {
		Color32::from_rgba_unmultiplied(rgba.red, rgba.green, rgba.blue, rgba.alpha)
	}
	match color {
		CssColor::CurrentColor => None,
		CssColor::RGBA(rgba) => Some(convert(rgba)),
		CssColor::LAB(_)
		| CssColor::Predefined(_)
		| CssColor::Float(_) => Some(convert(&color.into())),
	}
}
