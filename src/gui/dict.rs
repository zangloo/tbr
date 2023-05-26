use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use egui::{Rect, RichText, TextStyle, Ui};
use elsa::FrozenMap;
use regex::{Regex, Captures};
use stardict::{StarDict, WordDefinition};
use crate::book::{Book, Colors, Line};
use crate::Color32;
use crate::controller::Render;
use crate::gui::view::GuiView;
use crate::html_convertor::{html_str_content, HtmlContent};
use crate::i18n::I18n;

const SYS_DICT_PATH: &str = "/usr/share/stardict/dic";
const USER_DICT_PATH_SUFFIXES: [&str; 2] = [
	".stardict",
	"dic",
];
const HTML_DEFINITION_HEAD: &str = "
<style type=\"text/css\">
	.dict-name {
	  color: blue;
	}
	.dict-word {
	}
</style>
<body>
";
const HTML_DEFINITION_TAIL: &str = "</body>";

pub(super) struct DictionaryManager {
	book: DictionaryBook,
	view: GuiView,
	replacer: Regex,
}

pub(super) struct LookupResult {
	dict_name: String,
	definitions: Vec<WordDefinition>,
}

struct DictionaryBook {
	dictionaries: Vec<StarDict>,
	cache: HashMap<String, Vec<LookupResult>>,
	resources: FrozenMap<String, Vec<u8>>,

	content: HtmlContent,
}

impl Book for DictionaryBook
{
	#[inline]
	fn lines(&self) -> &Vec<Line>
	{
		&self.content.lines
	}

	#[inline]
	fn leading_space(&self) -> usize
	{
		0
	}

	fn image(&self, href: &str) -> Option<(String, &[u8])>
	{
		if let Some(bytes) = self.resources.get(href) {
			return if bytes.is_empty() {
				None
			} else {
				Some((href.to_owned(), bytes))
			};
		}

		let (dict_name, href) = href.split_once(":")?;
		for dict in &self.dictionaries {
			if dict.dict_name() == dict_name {
				let bytes = dict.get_resource(href)?;
				let bytes = self.resources.insert(href.to_owned(), bytes);
				return Some((href.to_owned(), bytes));
			}
		}
		self.resources.insert(href.to_owned(), vec![]);
		None
	}
}

impl DictionaryBook {
	pub fn lookup(&mut self, word: &str) -> Option<&Vec<LookupResult>>
	{
		let result = self.cache
			.entry(word.to_owned())
			.or_insert_with(|| {
				let mut result = vec![];
				for dict in &mut self.dictionaries {
					let dict_name = dict.dict_name().to_owned();
					if let Some(definitions) = dict.lookup(word) {
						result.push(LookupResult {
							dict_name,
							definitions,
						});
					}
				}
				result
			});
		if result.len() == 0 {
			None
		} else {
			Some(result)
		}
	}
}

impl DictionaryManager {
	pub fn from(data_path: &Option<PathBuf>, render_type: &str) -> Self
	{
		let mut dictionaries = vec![];
		load_dictionaries(data_path, &mut dictionaries);
		let cache = HashMap::new();
		let book = DictionaryBook {
			dictionaries,
			cache,
			resources: FrozenMap::new(),
			content: HtmlContent {
				title: None,
				lines: vec![],
				id_map: Default::default(),
			},
		};
		let mut view = GuiView::new(render_type, create_colors());
		view.set_custom_color(true);
		DictionaryManager {
			book,
			view,
			replacer: Regex::new("(<[\\\\b]*img[^>]+src[\\\\b]*=[\\\\b]*\")([^\"]+)(\"[^>]*>)").unwrap(),
		}
	}

	#[inline]
	pub fn reload(&mut self, data_path: &Option<PathBuf>)
	{
		self.book.dictionaries.clear();
		self.book.cache.clear();
		load_dictionaries(data_path, &mut self.book.dictionaries);
	}

	#[inline]
	#[allow(unused)]
	pub fn reload_render(&mut self, render_type: &str)
	{
		self.view.reload_render(render_type);
	}

	pub fn lookup_and_render(&mut self, ui: &mut Ui, i18n: &I18n, word: &str,
		font_size: u8, view_port: Rect)
	{
		if let Some(orig_word) = &self.book.content.title {
			if orig_word == word {
				self.render_view(font_size, view_port, ui);
				return;
			}
		}
		if let Some(results) = self.book.lookup(word) {
			let mut text = String::from(HTML_DEFINITION_HEAD);
			for single in results {
				render_definition(single, &mut text, &self.replacer);
			}
			text.push_str(HTML_DEFINITION_TAIL);
			if let Ok(mut content) = html_str_content(&text, None::<fn(String) -> Option<&'static String>>) {
				content.title = Some(String::from(word));
				self.book.content = content;
				self.render_view(font_size, view_port, ui);
			} else {
				for single in results {
					render_definition_text(ui, single, font_size as f32);
				}
			}
		} else {
			let msg = i18n.msg("dictionary-no-definition");
			ui.label(RichText::from(msg.as_ref())
				.text_style(TextStyle::Heading)
				.color(Color32::RED)
				.strong());
		}
	}

	fn render_view(&mut self, font_size: u8, view_port: Rect, ui: &mut Ui)
	{
		let (_, redraw, _) = self.view.show(ui, font_size, &self.book, false, Some(view_port));
		if redraw {
			self.view.redraw(&self.book, &self.book.lines(), 0, 0, &None, ui);
		}
		self.view.draw(ui);
	}
}

fn load_dictionaries(
	data_path: &Option<PathBuf>,
	dictionaries: &mut Vec<StarDict>)
{
	#[cfg(not(windows))]
	if let Ok(sys_data_path) = PathBuf::from_str(SYS_DICT_PATH) {
		if sys_data_path.is_dir() {
			load_dictionaries_dir(&sys_data_path, dictionaries);
		}
	}

	let user_home = dirs::home_dir();
	if let Some(user_home) = user_home {
		let mut user_data_path = user_home;
		for suffix in USER_DICT_PATH_SUFFIXES {
			user_data_path = user_data_path.join(suffix);
		}
		if user_data_path.is_dir() {
			load_dictionaries_dir(&user_data_path, dictionaries);
		}
	}

	if let Some(custom_data_path) = data_path {
		if custom_data_path.is_dir() {
			load_dictionaries_dir(&custom_data_path, dictionaries);
		}
	}
}

fn load_dictionaries_dir(path: &PathBuf, dictionaries: &mut Vec<StarDict>)
{
	if let Ok(read) = path.read_dir() {
		for entry in read {
			if let Ok(entry) = entry {
				if let Ok(dict) = StarDict::new(&entry.path()) {
					dictionaries.push(dict);
				}
			}
		}
	}
}

#[inline]
fn render_definition(result: &LookupResult, text: &mut String, replacer: &Regex)
{
	text.push_str(&format!("<h3 class=\"dict-name\">{}</h3>", result.dict_name));
	for definition in &result.definitions {
		text.push_str(&format!("<h3 class=\"dict-word\">{}</h3>", definition.word));
		for segment in &definition.segments {
			let html = str::replace(&segment.text, "\n", "<br/>");
			if segment.types.contains('h') || segment.types.contains('g') {
				let inject_html = inject_image(&html, &result.dict_name, replacer);
				text.push_str(&inject_html);
			} else {
				let escaped = html_escape::encode_text(&html);
				text.push_str(&escaped);
			}
		}
	}
}

#[inline]
fn inject_image<'a>(html: &'a str, dict_name: &str, replacer: &Regex) -> Cow<'a, str>
{
	replacer.replace_all(html, |caps: &Captures| {
		format!("{}{}:{}{}", &caps[1], dict_name, &caps[2], &caps[3])
	})
}

#[inline]
fn render_definition_text(ui: &mut Ui, result: &LookupResult, font_size: f32)
{
	ui.label(RichText::from(&result.dict_name)
		.color(Color32::BLUE)
		.text_style(TextStyle::Heading)
		.strong()
		.size(font_size)
	);
	ui.separator();
	for definition in &result.definitions {
		ui.label(RichText::from(&definition.word)
			.text_style(TextStyle::Heading)
			.strong()
			.size(font_size)
		);
		for segment in &definition.segments {
			ui.label(RichText::from(&segment.text)
				.size(font_size));
		}
	}
	ui.separator();
}

#[inline]
fn create_colors() -> Colors
{
	Colors {
		color: Color32::BLACK,
		background: Color32::LIGHT_GRAY,
		highlight: Color32::BLUE,
		highlight_background: Color32::LIGHT_GRAY,
		link: Color32::BLUE,
	}
}
