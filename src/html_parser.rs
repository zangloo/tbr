use std::cmp::Ordering;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::ops::{Deref, Range};
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use bitflags::bitflags;
use ego_tree::{NodeId, NodeRef};
use ego_tree::iter::Children;
use indexmap::IndexSet;
use lightningcss::declaration::DeclarationBlock;
use lightningcss::properties::{border, font, Property};
use lightningcss::properties::border::{Border, BorderSideWidth};
use lightningcss::properties::display::{Display, DisplayKeyword, DisplayOutside, DisplayPair};
use lightningcss::properties::font::{AbsoluteFontWeight, FontFamily, FontSize, FontWeight as CssFontWeight};
use lightningcss::properties::size::Size;
use lightningcss::properties::text::{TextDecoration as CssTextDecoration, TextDecorationLine as CssTextDecorationLine, TextDecorationStyle as CssTextDecorationStyle};
use lightningcss::rules::{CssRule, font_face};
use lightningcss::rules::font_face::FontFaceProperty;
use lightningcss::stylesheet::{ParserOptions, StyleSheet};
use lightningcss::traits::Parse;
use lightningcss::values;
use lightningcss::values::color::CssColor;
use lightningcss::values::length::{Length, LengthPercentage, LengthValue};
use lightningcss::values::percentage;
use markup5ever::{LocalName, Namespace, Prefix, QualName};
use roxmltree::{Document, ParsingOptions};
use scraper::{Html, Node, Selector};
use scraper::node::Element;

use crate::book::{EMPTY_CHAPTER_CONTENT, IMAGE_CHAR, Line};
use crate::color::Color32;
use crate::common::Position;

const DEFAULT_FONT_WEIGHT: u16 = 400;
const DEFAULT_FONT_SIZE: f32 = 16.0;

pub struct HtmlParseOptions<'a> {
	html: &'a str,
	font_family: Option<&'a mut IndexSet<String>>,
	resolver: Option<&'a dyn HtmlResolver>,
	custom_title: Option<String>,
	dark_mode: bool,
}

impl<'a> HtmlParseOptions<'a> {
	#[inline]
	pub fn new(html: &'a str) -> HtmlParseOptions<'a>
	{
		HtmlParseOptions {
			html,
			font_family: None,
			resolver: None,
			custom_title: None,
			dark_mode: false,
		}
	}
	pub fn with_font_family(mut self, font_family: &'a mut IndexSet<String>) -> Self
	{
		self.font_family = Some(font_family);
		self
	}
	pub fn with_resolver(mut self, resolver: &'a dyn HtmlResolver) -> Self
	{
		self.resolver = Some(resolver);
		self
	}
	#[allow(unused)]
	pub fn with_custom_title(mut self, custom_title: String) -> Self
	{
		self.custom_title = Some(custom_title);
		self
	}
}

pub struct HtmlFontFaceDesc {
	pub sources: Vec<PathBuf>,
	pub family: String,
}

#[derive(Clone, Debug)]
pub enum FontWeightValue {
	Absolute(FontWeight),
	Bolder,
	Lighter,
}

/// https://developer.mozilla.org/en-US/docs/Web/CSS/font-weight
impl From<&CssFontWeight> for FontWeightValue {
	fn from(value: &CssFontWeight) -> Self
	{
		match value {
			CssFontWeight::Absolute(weight) => match weight {
				AbsoluteFontWeight::Weight(number) => {
					let value: f32 = *number;
					FontWeightValue::Absolute(FontWeight(value as u16))
				}
				AbsoluteFontWeight::Normal =>
					FontWeightValue::Absolute(FontWeight::NORMAL),
				AbsoluteFontWeight::Bold =>
					FontWeightValue::Absolute(FontWeight::BOLD),
			}
			CssFontWeight::Bolder => FontWeightValue::Bolder,
			CssFontWeight::Lighter => FontWeightValue::Lighter,
		}
	}
}

#[derive(Clone, Debug, Copy)]
pub enum TextDecorationStyle {
	Solid,
	Double,
	Dotted,
	Dashed,
	Wavy,
}

impl From<CssTextDecorationStyle> for TextDecorationStyle {
	fn from(value: CssTextDecorationStyle) -> Self
	{
		match value {
			CssTextDecorationStyle::Solid => TextDecorationStyle::Solid,
			CssTextDecorationStyle::Double => TextDecorationStyle::Double,
			CssTextDecorationStyle::Dotted => TextDecorationStyle::Dotted,
			CssTextDecorationStyle::Dashed => TextDecorationStyle::Dashed,
			CssTextDecorationStyle::Wavy => TextDecorationStyle::Wavy,
		}
	}
}

bitflags! {
	#[derive(Clone, Debug, Copy)]
	pub struct TextDecorationLine: u8 {
		const Underline = 0b00000001;
		const Overline = 0b00000010;
		const LineThrough = 0b00000100;
	}
}

impl From<CssTextDecorationLine> for TextDecorationLine {
	fn from(value: CssTextDecorationLine) -> Self
	{
		TextDecorationLine::from_bits_truncate(value.bits())
	}
}

bitflags! {
	#[derive(Clone, Debug, Copy)]
	pub struct BorderLines: u8 {
			const Top = 0x01;
			const Right = 0x02;
			const Bottom = 0x04;
			const Left = 0x08;
	}
}

pub enum BlockStyle {
	Border { range: Range<usize>, lines: BorderLines, color: Option<Color32> },
	Background { range: Range<usize>, color: Color32 },
}

#[derive(Clone, Debug)]
pub struct TextDecoration {
	pub line: TextDecorationLine,
	pub style: TextDecorationStyle,
	pub color: Option<Color32>,
}

impl TextDecoration {
	pub(crate) fn line(line: TextDecorationLine) -> Self
	{
		Self {
			line,
			style: TextDecorationStyle::Solid,
			color: None,
		}
	}
}

#[derive(Clone, Debug)]
pub struct ElementSize {
	/// root font size based
	scale: FontScale,
	relative: bool,
}

impl ElementSize {
	#[inline]
	fn new(scale: FontScale, relative: bool) -> Self
	{
		Self { scale, relative }
	}
	#[inline]
	pub fn scale(&self) -> &FontScale
	{
		&self.scale
	}
	#[inline]
	pub fn relative(&self) -> bool
	{
		self.relative
	}
	#[inline]
	pub fn to_px(&self, font_scale: &FontScale, font_size: f32) -> f32
	{
		let scale = if self.relative {
			font_scale.0 * self.scale.0
		} else {
			self.scale.0
		};
		scale * font_size
	}
}

#[derive(Clone, Debug)]
pub enum ImageLength {
	ElementSize(ElementSize),
	Percent(f32),
}

#[derive(Clone, Debug)]
pub struct ImageStyle {
	pub href: String,
	pub width: Option<ImageLength>,
	pub height: Option<ImageLength>,
}
impl ImageStyle {
	#[inline]
	fn new(href: &str, width: Option<ImageLength>, height: Option<ImageLength>) -> Self
	{
		Self {
			href: href.to_owned(),
			width,
			height,
		}
	}
	#[inline]
	pub fn href(&self) -> &str
	{
		&self.href
	}
}

#[derive(Clone, Debug)]
pub enum TextStyle {
	Decoration(TextDecoration),
	Border(BorderLines, Option<Color32>),
	FontSize(ElementSize),
	FontWeight(FontWeightValue),
	FontFamily(u16),
	Image(ImageStyle),
	Link(String),
	Color(Color32),
	BackgroundColor(Color32),
	Title(String),
}

impl TextStyle {
	#[inline]
	fn id(&self) -> usize
	{
		match self {
			TextStyle::Decoration { .. } => 1,
			TextStyle::Border { .. } => 2,
			TextStyle::FontSize { .. } => 3,
			TextStyle::FontWeight(_) => 4,
			TextStyle::FontFamily(_) => 5,
			TextStyle::Image { .. } => 6,
			TextStyle::Link(_) => 7,
			TextStyle::Color(_) => 8,
			TextStyle::BackgroundColor(_) => 9,
			TextStyle::Title(_) => 10,
		}
	}
}

#[derive(Clone, Debug)]
enum ParseTag {
	Style(TextStyle),
	Width(ImageLength),
	Height(ImageLength),
	Paragraph,
	Hidden,
}

impl ParseTag {
	#[inline]
	fn id(&self) -> usize
	{
		match self {
			ParseTag::Style(style) => style.id(),
			ParseTag::Paragraph => 1000,
			ParseTag::Width(_) => 1001,
			ParseTag::Height(_) => 1002,
			ParseTag::Hidden => 9999,
		}
	}

	#[inline]
	fn cmp(&self, another: &Self) -> Ordering
	{
		let a = self.id();
		let b = another.id();
		if a > b {
			Ordering::Less
		} else if a < b {
			Ordering::Greater
		} else {
			Ordering::Equal
		}
	}
}

// style with !important or not
#[derive(Clone, Debug)]
struct LeveledParseTag(ParseTag, bool);

type LeveledParseTagSet = Vec<LeveledParseTag>;

#[derive(Clone, Debug)]
pub struct FontScale(f32);

impl Default for FontScale {
	#[inline]
	fn default() -> Self
	{
		FontScale(1.)
	}
}

#[cfg(feature = "gui")]
impl FontScale {
	pub const DEFAULT: FontScale = FontScale(1.);
}

#[cfg(feature = "gui")]
impl FontScale {
	#[inline]
	pub fn update(&mut self, scale: &FontScale, relative: bool)
	{
		if relative {
			self.0 *= scale.0;
		} else {
			self.0 = scale.0;
		}
	}

	#[inline]
	pub fn scale(&self, font_size: f32) -> f32
	{
		font_size * self.0
	}
}

#[derive(Clone, Debug)]
pub struct FontWeight(u16);

impl Default for FontWeight {
	#[inline]
	fn default() -> Self
	{
		FontWeight(DEFAULT_FONT_WEIGHT)
	}
}

impl FontWeight {
	pub const NORMAL: FontWeight = FontWeight(DEFAULT_FONT_WEIGHT);
	pub const BOLD: FontWeight = FontWeight(700);
}

#[cfg(feature = "gui")]
impl FontWeight {
	#[inline]
	pub fn key(&self) -> u8
	{
		(self.0 / 10) as u8
	}

	#[inline]
	pub fn value(&self) -> u16
	{
		self.0
	}

	#[inline]
	pub fn is_default(&self) -> bool
	{
		self.0 == DEFAULT_FONT_WEIGHT
	}

	#[inline]
	pub fn update(&mut self, value: &FontWeightValue)
	{
		self.0 = match value {
			FontWeightValue::Absolute(weight) => weight.0,
			FontWeightValue::Bolder => if self.0 <= 300 {
				400
			} else if self.0 <= 500 {
				700
			} else {
				900
			}
			FontWeightValue::Lighter => if self.0 <= 500 {
				100
			} else if self.0 <= 700 {
				400
			} else {
				700
			}
		};
	}
}

pub struct HtmlContent {
	title: Option<String>,
	lines: Vec<Line>,
	#[allow(unused)]
	block_styles: Option<Vec<BlockStyle>>,
	id_map: HashMap<String, Position>,
}

impl HtmlContent
{
	#[inline]
	#[allow(unused)]
	pub fn empty() -> Self
	{
		HtmlContent {
			title: None,
			lines: vec![],
			block_styles: None,
			id_map: HashMap::new(),
		}
	}
	#[inline]
	pub fn title(&self) -> Option<&str>
	{
		self.title.as_ref().map(|s| s.as_str())
	}
	#[inline]
	pub fn lines(&self) -> &Vec<Line>
	{
		&self.lines
	}
	#[inline]
	#[allow(unused)]
	pub fn block_styles(&self) -> Option<&Vec<BlockStyle>>
	{
		self.block_styles.as_ref()
	}
	#[inline]
	pub fn id_position(&self, id: &str) -> Option<&Position>
	{
		self.id_map.get(id)
	}
}

struct StyleDescription {
	start: Position,
	end: Position,
	style: TextStyle,
}

pub trait HtmlResolver {
	fn cwd(&self) -> PathBuf;
	fn resolve(&self, path: &PathBuf, sub: &str) -> PathBuf;
	fn css(&self, sub: &str) -> Option<(PathBuf, &str)>;
	fn custom_style(&self) -> Option<&str>;
}

pub struct HtmlParser<'a> {
	resolver: Option<&'a dyn HtmlResolver>,
	element_tags: HashMap<NodeId, LeveledParseTagSet>,
	font_families: Option<&'a mut IndexSet<String>>,
	font_faces: Vec<HtmlFontFaceDesc>,
	font_face_map: HashMap<&'a str, Option<String>>,
	styles: Vec<StyleDescription>,
	dark_mode: bool,

	title: Option<String>,
	lines: Vec<Line>,
	block_styles: Vec<BlockStyle>,
	id_map: HashMap<String, Position>,
}

impl<'a> HtmlParser<'a> {
	fn load_styles(&mut self, document: &'a Html,
		stylesheets: &'a Vec<(Option<PathBuf>, StyleSheet)>)
	{
		// generate font faces first,
		// will hack families depend on it
		for (path, style_sheet) in stylesheets {
			for rule in &style_sheet.rules.0 {
				match rule {
					CssRule::FontFace(face) => if let (Some(path), Some(resolver)) = (&path, self.resolver) {
						let mut src = None;
						let mut family = None;
						for prop in &face.properties {
							match prop {
								FontFaceProperty::Source(source) => src = Some(source),
								FontFaceProperty::FontFamily(ff) => match ff {
									FontFamily::Generic(gff) => family = Some(gff.as_str()),
									FontFamily::FamilyName(name) => family = Some(name.as_ref()),
								}
								FontFaceProperty::FontStyle(_) => {}
								FontFaceProperty::FontWeight(_) => {}
								FontFaceProperty::FontStretch(_) => {}
								FontFaceProperty::UnicodeRange(_) => {}
								FontFaceProperty::Custom(_) => {}
							}
						}

						fn append_local(locals: &mut Option<String>, local: &str) {
							let locals = if let Some(locals) = locals {
								locals.push(',');
								locals
							} else {
								locals.insert(String::new())
							};
							locals.push_str(local);
						}
						if let (Some(source), Some(family)) = (src, family) {
							let mut sources = vec![];
							let mut locals = None;
							for src in source {
								match src {
									font_face::Source::Url(font_face::UrlSource { url: values::url::Url { url, .. }, .. }) =>
										sources.push(resolver.resolve(path, url)),
									font_face::Source::Local(FontFamily::FamilyName(name)) =>
										append_local(&mut locals, &name),
									font_face::Source::Local(FontFamily::Generic(name)) =>
										append_local(&mut locals, name.as_str()),
								}
							}
							self.font_faces.push(HtmlFontFaceDesc {
								sources,
								family: family.to_owned(),
							});
							self.font_face_map.insert(family, locals);
						}
					}
					_ => {}
				}
			}
		}

		for (_, style_sheet) in stylesheets {
			for rule in &style_sheet.rules.0 {
				match rule {
					CssRule::Style(style_rule) => {
						let mut styles = vec![];
						for property in &style_rule.declarations.important_declarations {
							if let Some(style) = self.convert_style(property) {
								insert_or_replace_tag(&mut styles, style, true)
							}
						}
						for property in &style_rule.declarations.declarations {
							if let Some(style) = self.convert_style(property) {
								insert_or_replace_tag(&mut styles, style, false)
							}
						}
						if styles.len() == 0 {
							continue;
						}
						let selector_str = style_rule.selectors.to_string();
						if let Ok(selector) = Selector::parse(&selector_str) {
							for element in document.select(&selector) {
								let styles = styles.clone();
								match self.element_tags.entry(element.id()) {
									Entry::Occupied(o) => {
										let orig = o.into_mut();
										for new_style in styles {
											insert_or_replace_tag(orig, new_style.0, new_style.1);
										}
									}
									Entry::Vacant(v) => { v.insert(styles); }
								};
							}
						};
					}
					_ => {}
				}
			}
		}
	}

	fn finalize(mut self) -> (
		Option<String>,
		Vec<Line>,
		Option<Vec<BlockStyle>>,
		HashMap<String, Position>,
		Vec<HtmlFontFaceDesc>)
	{
		let lines = &mut self.lines;
		while let Some(last_line) = lines.last() {
			if last_line.len() == 0 {
				lines.pop();
			} else {
				break;
			}
		}
		if lines.is_empty() {
			lines.push(Line::new(EMPTY_CHAPTER_CONTENT));
		} else {
			// apply styles
			let styles = self.styles;
			for StyleDescription { start, end, style } in styles {
				if let Some((end_line, end_offset)) = setup_block_style(&start, &end, &style, lines, &mut self.block_styles) {
					if start.line == end_line {
						if let Some(line) = lines.get_mut(start.line) {
							line.push_style(style, start.offset..end_offset)
						}
					} else {
						if let Some(line) = lines.get_mut(start.line) {
							line.push_style(style.clone(), start.offset..line.len());
						}
						for line_idx in start.line + 1..end.line {
							if let Some(line) = lines.get_mut(line_idx) {
								line.push_style(style.clone(), 0..line.len());
							}
						}
						if let Some(line) = lines.get_mut(end.line) {
							line.push_style(style, 0..end_offset);
						}
					}
				}
			}
		}

		let block_styles = if self.block_styles.is_empty() {
			None
		} else {
			Some(self.block_styles)
		};
		(
			self.title,
			self.lines,
			block_styles,
			self.id_map,
			self.font_faces)
	}

	fn load_title(&mut self, title_node: NodeRef<Node>)
	{
		if let Some(child) = title_node.children().next() {
			if let Node::Text(text) = child.value() {
				self.title = Some(text.to_string());
			}
		}
	}

	#[inline]
	fn convert_node_children(&mut self, children: Children<Node>)
	{
		for child in children {
			self.convert_node_to_lines(child);
		}
	}

	fn convert_node_to_lines(&mut self, node: NodeRef<Node>)
	{
		match node.value() {
			Node::Text(contents) => {
				let string = contents.text.to_string();
				let text = string.trim_matches(|c: char| c.is_ascii_whitespace());
				let line = self.lines.last_mut().unwrap();
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
					self.lines.len() - 1,
					self.lines.last().unwrap().len());
				if let Some(id) = element.id() {
					self.id_map.insert(id.to_string(), position.clone());
				}
				let mut element_tags = self.load_element_tags(
					element,
					node.id());
				if find_tag(&element_tags, ParseTag::Hidden) {
					return;
				}
				let force_paragraph = remove_tag(&mut element_tags, ParseTag::Paragraph)
					.is_some();
				if force_paragraph {
					self.new_line();
				}
				match element.name.local {
					local_name!("title") => self.load_title(node),
					local_name!("script") => {}
					local_name!("b") => {
						insert_or_replace_tag(
							&mut element_tags,
							ParseTag::Style(TextStyle::FontWeight(FontWeightValue::Bolder)),
							false);
						self.convert_node_children(node.children());
					}
					local_name!("u") => {
						insert_or_replace_tag(
							&mut element_tags,
							ParseTag::Style(TextStyle::Decoration(TextDecoration::line(TextDecorationLine::Underline))),
							false);
						self.convert_node_children(node.children());
					}
					local_name!("div") => {
						self.newline_for_class(element);
						self.convert_node_children(node.children());
						self.newline_for_class(element);
					}
					local_name!("h1") => {
						unique_and_insert_font_size(&mut element_tags, 6, false);
						self.new_paragraph(node);
					}
					| local_name!("h2") => {
						unique_and_insert_font_size(&mut element_tags, 5, false);
						self.new_paragraph(node);
					}
					| local_name!("h3") => {
						unique_and_insert_font_size(&mut element_tags, 4, false);
						self.new_paragraph(node);
					}
					| local_name!("h4") => {
						unique_and_insert_font_size(&mut element_tags, 3, false);
						self.new_paragraph(node);
					}
					| local_name!("h5") => {
						unique_and_insert_font_size(&mut element_tags, 2, false);
						self.new_paragraph(node);
					}
					| local_name!("h6") => {
						unique_and_insert_font_size(&mut element_tags, 1, false);
						self.new_paragraph(node);
					}
					local_name!("small") => {
						unique_and_insert_font_size(&mut element_tags, 2, true);
						self.convert_node_children(node.children());
					}
					local_name!("big") => {
						unique_and_insert_font_size(&mut element_tags, 4, true);
						self.convert_node_children(node.children());
					}
					local_name!("p")
					| local_name!("blockquote")
					| local_name!("table")
					| local_name!("tr")
					| local_name!("dt")
					| local_name!("li") => self.new_paragraph(node),
					local_name!("br") => {
						self.new_line();
						self.convert_node_children(node.children());
					}
					local_name!("font") => {
						if let Some(level_text) = element.attr("size") {
							if let Ok(level) = level_text.parse::<u8>() {
								replace_font_size(&mut element_tags, level, true);
							}
						}
						if let Some(color_text) = element.attr("color") {
							if let Ok(color) = CssColor::parse_string(color_text) {
								if let Some(color) = self.css_color(&color) {
									insert_or_replace_tag(&mut element_tags, ParseTag::Style(TextStyle::Color(color)), false);
								}
							}
						}
						self.convert_node_children(node.children());
					}
					local_name!("a") => {
						if let Some(href) = element.attr("href") {
							let a = TextDecoration {
								line: TextDecorationLine::Underline,
								style: TextDecorationStyle::Solid,
								color: None,
							};
							unique_and_insert_tag(&mut element_tags, ParseTag::Style(TextStyle::Decoration(a)));
							insert_or_replace_tag(&mut element_tags, ParseTag::Style(TextStyle::Link(href.to_string())), false);
						}
						self.convert_node_children(node.children());
					}
					local_name!("img") => {
						if let Some(href) = element.attr("src") {
							self.add_image(href, &element_tags);
						}
					}
					local_name!("image") => {
						let name = QualName::new(
							Some(Prefix::from("xlink")),
							Namespace::from("http://www.w3.org/1999/xlink"),
							LocalName::from("href"));
						let href = element.attrs.get(&name).map(Deref::deref);
						if let Some(href) = href {
							self.add_image(href, &element_tags);
						}
					}
					local_name!("noscript") |
					local_name!("script") => {}
					_ => self.convert_node_children(node.children()),
				}
				if force_paragraph {
					self.new_line();
				}
				if !element_tags.is_empty() {
					let lines = &self.lines;
					// only for new lines
					for last_line in (position.line..lines.len()).rev() {
						let line = &lines[last_line];
						// ignore empty lines
						if line.is_empty() {
							continue;
						}
						let end = Position::new(last_line, line.len());
						for tag in &element_tags {
							if let LeveledParseTag(ParseTag::Style(style), _) = tag {
								self.styles.push(StyleDescription {
									start: position.clone(),
									end: end.clone(),
									style: style.clone(),
								});
							}
						}
						break;
					}
				}
			}
			Node::Document {} => self.convert_node_children(node.children()),
			_ => {}
		}
	}

	#[inline]
	fn load_element_tags(&mut self, element: &Element, node_id: NodeId) -> LeveledParseTagSet
	{
		let mut element_tags = vec![];
		if let Some(style) = element.attr("style") {
			if let Ok(declaration) = DeclarationBlock::parse_string(style, style_parse_options()) {
				for property in &declaration.declarations {
					if let Some(tag) = self.convert_style(property) {
						insert_or_replace_tag(&mut element_tags, tag, false);
					}
				}
				for property in &declaration.important_declarations {
					if let Some(tag) = self.convert_style(property) {
						insert_or_replace_tag(&mut element_tags, tag, false);
					}
				}
			}
		}
		if let Some(tags) = self.element_tags.remove(&node_id) {
			for tag in tags {
				insert_or_replace_tag(&mut element_tags, tag.0, tag.1);
			}
		};
		if let Some(title) = element.attr("title") {
			let tag = ParseTag::Style(TextStyle::Title(title.to_string()));
			insert_or_replace_tag(&mut element_tags, tag, false);
		}
		element_tags
	}

	fn add_image(&mut self, href: &str, element_tags: &LeveledParseTagSet)
	{
		let mut width = None;
		let mut height = None;
		for tag in element_tags {
			match &tag.0 {
				ParseTag::Width(size) if width.is_none() =>
					width = Some(size.clone()),
				ParseTag::Height(size) if height.is_none() =>
					height = Some(size.clone()),
				_ => {}
			}
		}
		let line = self.lines.last_mut().unwrap();
		let start = line.len();
		line.push(IMAGE_CHAR);
		line.push_style(TextStyle::Image(ImageStyle::new(href, width, height)), start..start + 1);
	}

	fn newline_for_class(&mut self, element: &Element)
	{
		if !self.lines.last().unwrap().is_empty() {
			if let Some(class) = element.attr("class") {
				for class_name in DIV_PUSH_CLASSES {
					if class.contains(class_name) {
						self.new_line();
						return;
					}
				}
			}
		}
	}

	fn new_line(&mut self)
	{
		let mut empty_count = 0;
		// no more then 2 empty lines
		for line in self.lines.iter().rev() {
			if line.is_empty() {
				empty_count += 1;
				if empty_count == 2 {
					return;
				}
			} else {
				break;
			}
		}
		if empty_count == 0 {
			self.lines.push(Line::default())
		}
	}

	#[inline]
	fn new_paragraph(&mut self, child: NodeRef<Node>)
	{
		self.new_line();
		self.convert_node_children(child.children());
		self.new_line();
	}

	#[inline]
	fn convert_style(&mut self, property: &Property) -> Option<ParseTag>
	{
		match property {
			Property::Border(border) => self.border_style(border),
			Property::BorderTop(line)
			if border_width(&line.width) => Some(ParseTag::Style(TextStyle::Border(BorderLines::Top, self.css_color(&line.color)))),
			Property::BorderRight(line)
			if border_width(&line.width) => Some(ParseTag::Style(TextStyle::Border(BorderLines::Right, self.css_color(&line.color)))),
			Property::BorderBottom(line)
			if border_width(&line.width) => Some(ParseTag::Style(TextStyle::Border(BorderLines::Bottom, self.css_color(&line.color)))),
			Property::BorderLeft(line)
			if border_width(&line.width) => Some(ParseTag::Style(TextStyle::Border(BorderLines::Left, self.css_color(&line.color)))),
			Property::BorderWidth(width) => {
				let mut borders = BorderLines::empty();
				if border_width(&width.top) {
					borders |= BorderLines::Top;
				}
				if border_width(&width.right) {
					borders |= BorderLines::Right;
				}
				if border_width(&width.bottom) {
					borders |= BorderLines::Bottom;
				}
				if border_width(&width.left) {
					borders |= BorderLines::Left;
				}
				if borders.is_empty() {
					None
				} else {
					Some(ParseTag::Style(TextStyle::Border(borders, None)))
				}
			}
			Property::FontSize(size) => Some(font_size(size)),
			Property::FontWeight(weight) => Some(ParseTag::Style(TextStyle::FontWeight(FontWeightValue::from(weight)))),
			Property::FontFamily(families) => self.font_family(families),
			Property::TextDecorationLine(line, _) => Some(ParseTag::Style(TextStyle::Decoration(TextDecoration::line((*line).into())))),
			Property::TextDecoration(decoration, _) => Some(self.text_decoration(decoration)),
			Property::Color(color) => Some(ParseTag::Style(TextStyle::Color(self.css_color(color)?))),
			Property::BackgroundColor(color) => Some(ParseTag::Style(TextStyle::BackgroundColor(self.css_color(color)?))),
			Property::Background(bg) => Some(ParseTag::Style(TextStyle::BackgroundColor(self.css_color(&bg[0].color)?))),
			Property::Display(Display::Pair(DisplayPair { outside: DisplayOutside::Block, .. })) => Some(ParseTag::Paragraph),
			Property::Display(Display::Keyword(DisplayKeyword::None)) => Some(ParseTag::Hidden),
			Property::Width(size) => Some(ParseTag::Width(image_size(size)?)),
			Property::Height(size) => Some(ParseTag::Height(image_size(size)?)),
			_ => None,
		}
	}

	#[inline]
	fn font_family(&mut self, families: &Vec<FontFamily>) -> Option<ParseTag>
	{
		if let Some(font_families) = &mut self.font_families {
			let mut string = String::new();
			for family in families {
				let name = match family {
					FontFamily::Generic(name) => name.as_str(),
					FontFamily::FamilyName(name) => &name,
				};
				if !string.is_empty() {
					string.push(',');
				}
				string.push_str(name);
				if let Some(Some(locals)) = self.font_face_map.get(name) {
					string.push(',');
					string.push_str(locals);
					break;
				}
			}
			let is_empty = string.is_empty();
			let (idx, _) = font_families.insert_full(string);
			if is_empty {
				None
			} else {
				Some(ParseTag::Style(TextStyle::FontFamily(idx as u16)))
			}
		} else {
			None
		}
	}

	fn css_color(&self, color: &CssColor) -> Option<Color32>
	{
		match color {
			CssColor::CurrentColor => None,
			CssColor::RGBA(rgba) => Some(Color32::from_rgba_unmultiplied(
				rgba.red, rgba.green, rgba.blue, rgba.alpha)),
			CssColor::LAB(_)
			| CssColor::Predefined(_)
			| CssColor::Float(_) => match &color.to_rgb() {
				Ok(CssColor::RGBA(rgba)) => Some(Color32::from_rgba_unmultiplied(
					rgba.red, rgba.green, rgba.blue, rgba.alpha)),
				_ => panic!("should not happen")
			},
			// todo should return a dynamic color, and get real color by dark_mode when render
			CssColor::LightDark(light_color, dark_color) => if self.dark_mode {
				self.css_color(dark_color)
			} else {
				self.css_color(light_color)
			},
			CssColor::System(_) => None,
		}
	}

	fn text_decoration(&self, decoration: &CssTextDecoration) -> ParseTag
	{
		let style = decoration.style.into();
		let line = decoration.line.into();
		let color = self.css_color(&decoration.color);
		let decoration = TextDecoration { line, style, color };
		ParseTag::Style(TextStyle::Decoration(decoration))
	}

	#[inline]
	fn border_style(&self, border: &Border) -> Option<ParseTag>
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
			if border_width(&border.width) => Some(ParseTag::Style(
				TextStyle::Border(
					BorderLines::all(),
					self.css_color(&border.color)))),
			_ => None
		}
	}
}

/// return end line and offset for non-block-styles
/// if the style is a block style then create block style
fn setup_block_style(start: &Position, end: &Position, style: &TextStyle,
	lines: &mut Vec<Line>, block_styles: &mut Vec<BlockStyle>) -> Option<(usize, usize)>
{
	let last_line_idx = lines.len() - 1;
	let (end_line, end_offset) = if end.line > last_line_idx {
		// line not exists
		if start.line > last_line_idx {
			return None;
		}
		(last_line_idx, lines[last_line_idx].len())
	} else {
		let end_line = end.line;
		if end.offset != lines[end_line].len() {
			return Some((end_line, end.offset));
		}
		(end_line, end.offset)
	};
	if start.offset != 0 {
		return Some((end_line, end_offset));
	}
	match style {
		TextStyle::Border(border_lines, color) => {
			block_styles.push(BlockStyle::Border {
				range: start.line..end_line + 1,
				lines: border_lines.clone(),
				color: color.clone(),
			});
			None
		}
		TextStyle::BackgroundColor(color) => {
			block_styles.push(BlockStyle::Background {
				range: start.line..end_line + 1,
				color: color.clone(),
			});
			None
		}
		_ => Some((end_line, end_offset))
	}
}

#[inline]
fn unique_and_insert_font_size(tags: &mut LeveledParseTagSet, font_level: u8, relative: bool)
{
	let style = font_size_level(font_level, relative);
	let tag = ParseTag::Style(style);
	unique_and_insert_tag(tags, tag);
}

#[inline]
fn replace_font_size(tags: &mut LeveledParseTagSet, font_level: u8, relative: bool)
{
	let style = font_size_level(font_level, relative);
	let tag = ParseTag::Style(style);
	insert_or_replace_tag(tags, tag, false);
}

const DIV_PUSH_CLASSES: [&str; 3] = ["contents", "toc", "mulu"];

#[inline]
fn style_parse_options<'a>() -> ParserOptions<'a, 'a>
{
	let mut options = ParserOptions::default();
	options.error_recovery = true;
	options
}

fn insert_or_replace_tag(styles: &mut LeveledParseTagSet, tag: ParseTag, important: bool)
{
	match styles.binary_search_by(|s| s.0.cmp(&tag)) {
		Ok(idx) => if important || !styles[idx].1 {
			styles[idx] = LeveledParseTag(tag, important);
		}
		Err(idx) => styles.insert(idx, LeveledParseTag(tag, important)),
	}
}

#[inline]
fn remove_tag(tags: &mut LeveledParseTagSet, tag: ParseTag) -> Option<LeveledParseTag>
{
	match tags.binary_search_by(|s| s.0.cmp(&tag)) {
		Ok(idx) => Some(tags.remove(idx)),
		Err(_) => None,
	}
}

#[inline]
fn find_tag(tags: &LeveledParseTagSet, tag: ParseTag) -> bool
{
	tags.binary_search_by(|s| s.0.cmp(&tag)).is_ok()
}

/// insert if unique, will not insert if exists
#[inline]
fn unique_and_insert_tag(tags: &mut LeveledParseTagSet, tag: ParseTag)
{
	if let Err(idx) = tags.binary_search_by(|s| s.0.cmp(&tag)) {
		tags.insert(idx, LeveledParseTag(tag, false));
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
	let scale = FontScale(scale);
	TextStyle::FontSize(ElementSize::new(scale, relative))
}

#[inline]
fn length_percentage(percentage: &LengthPercentage) -> ElementSize
{
	match percentage {
		LengthPercentage::Dimension(lv) => {
			let (scale, relative) = length_value(lv, DEFAULT_FONT_SIZE);
			ElementSize::new(FontScale(scale), relative)
		}
		LengthPercentage::Percentage(percentage::Percentage(p)) =>
			ElementSize::new(FontScale(*p), true),
		LengthPercentage::Calc(_) => // 视而不见
			ElementSize::new(Default::default(), false),
	}
}

fn font_size(size: &FontSize) -> ParseTag
{
	let style = match size {
		FontSize::Length(lp) => TextStyle::FontSize(length_percentage(lp)),
		FontSize::Absolute(size) => match size {
			font::AbsoluteFontSize::XXSmall => font_size_level(1, false),
			font::AbsoluteFontSize::XSmall => TextStyle::FontSize(ElementSize::new(FontScale(3.0 / 4.0), false)),
			font::AbsoluteFontSize::Small => font_size_level(2, false),
			font::AbsoluteFontSize::Medium => font_size_level(3, false),
			font::AbsoluteFontSize::Large => font_size_level(4, false),
			font::AbsoluteFontSize::XLarge => font_size_level(5, false),
			font::AbsoluteFontSize::XXLarge => font_size_level(6, false),
			font::AbsoluteFontSize::XXXLarge => font_size_level(7, false),
		}
		FontSize::Relative(size) => match size {
			font::RelativeFontSize::Smaller => font_size_level(2, true),
			font::RelativeFontSize::Larger => font_size_level(4, true),
		}
	};
	ParseTag::Style(style)
}

fn image_size(size: &Size) -> Option<ImageLength>
{
	let size = match size {
		Size::LengthPercentage(percentage) |
		Size::FitContentFunction(percentage) => match percentage {
			LengthPercentage::Dimension(lv) => {
				let (scale, relative) = length_value(lv, DEFAULT_FONT_SIZE);
				ImageLength::ElementSize(ElementSize::new(FontScale(scale), relative))
			}
			LengthPercentage::Percentage(percentage::Percentage(p)) =>
				ImageLength::Percent(*p),
			LengthPercentage::Calc(_) => return None,
		},
		Size::Auto |
		Size::MinContent(_) |
		Size::MaxContent(_) |
		Size::FitContent(_) |
		Size::Stretch(_) |
		Size::Contain => return None,
	};
	Some(size)
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
		LengthValue::Px(v) => (v / default_size, false),
		LengthValue::In(v) => (v / default_size * 96., false),
		LengthValue::Cm(v) => (v / default_size * 96. / 2.54, false),
		LengthValue::Mm(v) => (v / default_size * 96. / 2.54 / 10., false),
		LengthValue::Q(v) => (v / default_size * 96. / 2.54 / 40., false),
		LengthValue::Pt(v) => (v / default_size * 96. / 72., false),
		LengthValue::Pc(v) => (v / default_size * 96. / 6., false),
		LengthValue::Em(v) => (*v, true),
		LengthValue::Rem(v) => (*v, false),

		LengthValue::Ex(_)
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

fn load_stylesheets<'d>(document: &'d Html, resolver: Option<&'d dyn HtmlResolver>)
	-> Vec<(Option<PathBuf>, StyleSheet<'d, 'd>)>
{
	let mut stylesheets = vec![];

	if let Some(resolver) = resolver {
		if let Ok(link_selector) = Selector::parse("link") {
			let selection = document.select(&link_selector);
			for element in selection.into_iter() {
				if let Some(href) = element.value().attr("href") {
					if href.to_lowercase().ends_with(".css") {
						if let Some((path, content)) = resolver.css(href) {
							if let Ok(style_sheet) = StyleSheet::parse(&content, style_parse_options()) {
								stylesheets.push((Some(path), style_sheet));
							}
						}
					}
				}
			}
		}
	}

	// load embedded styles, , will overwrite previous
	if let Ok(style_selector) = Selector::parse("style") {
		let mut style_iterator = document.select(&style_selector);
		while let Some(style) = style_iterator.next() {
			let mut text_iterator = style.text();
			while let Some(text) = text_iterator.next() {
				if let Ok(style_sheet) = StyleSheet::parse(&text, style_parse_options()) {
					let path = resolver.map(|r| r.cwd());
					stylesheets.push((path, style_sheet));
				}
			}
		}
	}

	// load custom styles, will overwrite previous
	if let Some(resolver) = resolver {
		if let Some(custom_style) = resolver.custom_style() {
			if let Ok(style_sheet) = StyleSheet::parse(custom_style, style_parse_options()) {
				let path = resolver.cwd();
				stylesheets.push((Some(path), style_sheet));
			}
		}
	}

	stylesheets
}

#[inline]
pub fn parse_xml(xml: &str) -> Result<Document>
{
	let options = ParsingOptions {
		allow_dtd: true,
		nodes_limit: u32::MAX,
	};
	Ok(Document::parse_with_options(xml, options)?)
}

#[inline]
#[allow(unused)]
pub fn parse_stylesheet(css: &str, strict: bool) -> Result<StyleSheet>
{
	let mut options = ParserOptions::default();
	if !strict {
		options.error_recovery = true;
	}
	StyleSheet::parse(css, options)
		.map_err(|err| anyhow!("{}",err.to_string()))
}

pub fn parse(options: HtmlParseOptions) -> Result<(HtmlContent, Vec<HtmlFontFaceDesc>)>
{
	let html = Html::parse_document(&options.html);
	let stylesheets = load_stylesheets(&html, options.resolver);

	let mut parser = HtmlParser {
		resolver: options.resolver,
		element_tags: Default::default(),
		font_families: options.font_family,
		font_faces: vec![],
		font_face_map: Default::default(),
		styles: vec![],
		dark_mode: options.dark_mode,

		title: None,
		lines: vec![Line::default()],
		block_styles: vec![],
		id_map: Default::default(),
	};

	parser.load_styles(&html, &stylesheets);

	// load head infos
	let head_selector = match Selector::parse("head") {
		Ok(s) => s,
		Err(_) => return Err(anyhow!("Failed parse html"))
	};
	if let Some(head) = html
		.select(&head_selector)
		.next() {
		for child in head.children() {
			match child.value() {
				Node::Element(element) => {
					match element.name.local {
						local_name!("title") => {
							parser.load_title(child);
							break;
						}
						_ => {}
					}
				}
				_ => {}
			}
		}
	}
	let body_selector = match Selector::parse("body") {
		Ok(s) => s,
		Err(_) => return Err(anyhow!("Failed parse html"))
	};
	let body = html
		.select(&body_selector)
		.next()
		.ok_or(anyhow!("No body in the document"))?;

	parser.convert_node_to_lines(*body.deref());

	let (title, lines, block_styles, id_map, font_faces) = parser.finalize();
	let title = if options.custom_title.is_some() {
		options.custom_title
	} else {
		title
	};
	Ok((HtmlContent {
		title,
		lines,
		block_styles,
		id_map,
	}, font_faces))
}
