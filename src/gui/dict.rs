use std::path::PathBuf;
use std::str::FromStr;
use egui::{Align, Key, Layout, Modifiers, RichText, ScrollArea, Ui, Vec2};
use egui_modal::Modal;
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
		DictionaryManager { dictionaries }
	}

	#[inline]
	pub fn reload(&mut self, data_path: &Option<PathBuf>) {
		self.dictionaries.clear();
		load_dictionaries(data_path, &mut self.dictionaries);
	}

	pub fn lookup<'a>(&'a mut self, word: &'a str)
		-> Option<Vec<DictDefinition>>
	{
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
		if result.len() == 0 {
			None
		} else {
			Some(result)
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

pub(super) fn show(ui: &mut Ui, window_size: &Vec2, i18n: &I18n, word: &str,
	definitions: &mut Vec<DictDefinition>) -> bool
{
	if definitions.len() == 0 {
		return true;
	}
	let mut close = false;
	let modal = Modal::new(ui.ctx(), "dict_modal");
	modal.show(|ui| {
		modal.title(ui, word);
		modal.frame(ui, |ui| {
			ScrollArea::vertical()
				.max_height(window_size.y * 3.0 / 4.0)
				.show(ui, |ui| {
					ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
						for definition in definitions {
							render_definition(ui, definition);
						}
					});
				});
		});
		modal.buttons(ui, |ui| {
			if modal.suggested_button(ui, i18n.msg("ok-title")).clicked() {
				close = true;
			};
		});
		if modal.was_outside_clicked() {
			close = true;
		}
		ui.input_mut(|input| {
			if input.consume_key(Modifiers::NONE, Key::Escape) {
				close = true;
			}
		})
	});

	if close {
		true
	} else {
		modal.open();
		false
	}
}

pub(super) fn render_definition(ui: &mut Ui, definition: &DictDefinition) {
	ui.label(RichText::from(&definition.dict_name)
		.color(Color32::BLUE)
		.strong());
	ui.separator();
	ui.label(RichText::from(&definition.word).strong());
	ui.label(&definition.definition);
	ui.separator();
}