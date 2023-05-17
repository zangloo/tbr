use std::path::PathBuf;
use egui::{Align, ComboBox, Key, Layout, Modifiers, RichText, Ui};
use egui_modal::Modal;
use crate::{I18n, i18n, ThemeEntry};
use crate::i18n::LocaleEntry;

pub(super) struct SettingsData {
	pub theme_name: String,
	pub locale: LocaleEntry,
	pub dictionary_data_path: String,
	themes: Vec<String>,
}

impl SettingsData {
	pub fn new(themes: &Vec<ThemeEntry>, theme_name: &str, i18n: &I18n,
		lang: &str, dictionary_data_path: &Option<PathBuf>) -> Self
	{
		let mut theme_names = vec![];
		for entry in themes {
			theme_names.push(entry.0.clone())
		}
		let mut locale = None;
		for entry in i18n.locales() {
			if entry.locale == lang {
				locale = Some(entry.clone());
				break;
			}
		};
		let locale = locale.unwrap_or(LocaleEntry::new(
			i18n::DEFAULT_LOCALE,
			i18n.locale_text(i18n::DEFAULT_LOCALE),
		));

		let dictionary_data_path = path_str(dictionary_data_path);
		SettingsData {
			themes: theme_names,
			theme_name: theme_name.to_owned(),
			locale,
			dictionary_data_path,
		}
	}
}

pub(super) fn show(ui: &mut Ui, settings_data: &mut SettingsData,
	i18n: &I18n, ) -> bool
{
	let mut close = false;
	let mut ok = false;

	let modal = Modal::new(ui.ctx(), "setting_modal");

	modal.show(|ui| {
		modal.title(ui, i18n.msg("settings-dialog-title"));
		modal.frame(ui, |ui| {
			ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
				ComboBox::from_label(i18n.msg("theme"))
					.selected_text(&settings_data.theme_name)
					.show_ui(ui, |ui| {
						for name in &settings_data.themes {
							ui.selectable_value(
								&mut settings_data.theme_name,
								name.to_owned(),
								name);
						}
					});
				let locales = i18n.locales();
				ComboBox::from_label(i18n.msg("lang"))
					.selected_text(settings_data.locale.name.clone())
					.show_ui(ui, |ui| {
						for locale in locales {
							ui.selectable_value(
								&mut settings_data.locale,
								locale.clone(),
								locale.name.clone());
						}
					});
				ui.separator();
				ui.label(RichText::from(i18n.msg("dictionary-data-path")).strong());
				ui.horizontal(|ui| {
					ui.text_edit_singleline(&mut settings_data.dictionary_data_path);
					if ui.button("...").clicked() {
						let path = rfd::FileDialog::new().pick_folder();
						if path.is_some() {
							settings_data.dictionary_data_path = path_str(&path);
						}
					}
				});
			});
		});
		modal.buttons(ui, |ui| {
			if modal.button(ui, i18n.msg("cancel-title")).clicked() {
				close = true;
			};
			if modal.suggested_button(ui, i18n.msg("ok-title")).clicked() {
				ok = true;
				close = true;
			};
		});
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

#[inline]
fn path_str(path: &Option<PathBuf>) -> String {
	match path {
		Some(path) => if let Some(path_str) = path.to_str() {
			path_str.to_owned()
		} else {
			String::new()
		}
		None => String::new()
	}
}