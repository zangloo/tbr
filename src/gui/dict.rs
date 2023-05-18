use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use egui::{RichText, TextStyle, Ui};
use stardict::StarDict;
use crate::Color32;
use crate::i18n::I18n;

const SYS_DICT_PATH: &str = "/usr/share/stardict/dic";
const USER_DICT_PATH_SUFFIES: [&str; 2] = [
	".stardict",
	"dic",
];

pub(super) struct DictionaryManager {
	dictionaries: Vec<StarDict>,
	cache: HashMap<String, Vec<DictDefinition>>,
}

pub(super) struct DictDefinition {
	dict_name: String,
	word: String,
	definition: String,
}

impl DictionaryManager {
	pub fn from(data_path: &Option<PathBuf>) -> Self
	{
		let mut dictionaries = vec![];
		load_dictionaries(data_path, &mut dictionaries);
		let cache = HashMap::new();
		DictionaryManager { dictionaries, cache }
	}

	#[inline]
	pub fn reload(&mut self, data_path: &Option<PathBuf>)
	{
		self.dictionaries.clear();
		self.cache.clear();
		load_dictionaries(data_path, &mut self.dictionaries);
	}

	pub fn lookup(&mut self, word: &str) -> Option<&Vec<DictDefinition>>
	{
		let result = self.cache
			.entry(word.to_owned())
			.or_insert_with(|| {
				let mut result = vec![];
				for dict in &mut self.dictionaries {
					let dict_name = dict.dict_name().to_owned();
					if let Some(def) = dict.lookup(word) {
						result.push(DictDefinition {
							dict_name,
							word: def.word.to_owned(),
							definition: def.definition.to_string(),
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

	pub fn lookup_and_render(&mut self, ui: &mut Ui, i18n: &I18n, font_size: f32,
		word: &str)
	{
		if let Some(definitions) = self.lookup(word) {
			for definition in definitions {
				render_definition(ui, definition, font_size);
			}
		} else {
			let msg = i18n.msg("dictionary-no-definition");
			ui.label(RichText::from(msg.as_ref())
				.color(Color32::RED)
				.strong());
		}
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
		for suffix in USER_DICT_PATH_SUFFIES {
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
fn render_definition(ui: &mut Ui, definition: &DictDefinition, font_size: f32)
{
	ui.label(RichText::from(&definition.dict_name)
		.color(Color32::BLUE)
		.text_style(TextStyle::Heading)
		.strong()
		.size(font_size)
	);
	ui.separator();
	ui.label(RichText::from(&definition.word)
		.text_style(TextStyle::Heading)
		.strong()
		.size(font_size)
	);
	ui.label(RichText::from(&definition.definition)
		.size(font_size));
	ui.separator();
}