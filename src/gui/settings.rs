use egui::{Align, ComboBox, Key, Layout, Modifiers, RichText, Ui};
use egui_modal::Modal;
use crate::{I18n, i18n, ThemeEntry};
use crate::gui::ReaderApp;
use crate::i18n::LocaleEntry;

pub(super) struct SettingsData {
	pub render_han: bool,
	pub custom_color: bool,
	pub theme_name: String,
	pub locale: LocaleEntry,

	themes: Vec<String>,
}

impl SettingsData {
	pub fn new(render_han: bool, custom_color: bool, themes: &Vec<ThemeEntry>,
		theme_name: &str, i18n: &I18n, lang: &str) -> Self
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

		SettingsData {
			render_han,
			custom_color,
			themes: theme_names,
			theme_name: theme_name.to_owned(),
			locale,
		}
	}
}

pub(super) fn try_show(ui: &mut Ui, app: &mut ReaderApp)
{
	if let Some(ref mut settings_data) = &mut app.setting {
		let i18n = &app.i18n;
		let mut close = false;
		let mut ok = false;

		let modal = Modal::new(ui.ctx(), "setting_modal");

		modal.show(|ui| {
			modal.title(ui, i18n.msg("settings-dialog-title"));
			modal.frame(ui, |ui| {
				ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
					ui.label(RichText::from(i18n.msg("render-type")).strong());
					ui.horizontal(|ui| {
						if ui.radio(settings_data.render_han, i18n.msg("render-han")).clicked() {
							settings_data.render_han = true;
						}
						if ui.radio(!settings_data.render_han, i18n.msg("render-xi")).clicked() {
							settings_data.render_han = false;
						}
					});
					ui.checkbox(&mut settings_data.custom_color,
						RichText::from(i18n.msg("custom-color")).strong());

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
			if ok {
				app.approve_settings(ui);
			}
			app.setting = None;
		} else {
			modal.open();
		}
	}
}
