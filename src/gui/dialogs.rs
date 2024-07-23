use std::borrow::Cow;

use anyhow::Result;
use gtk4::{Align, Button, Entry, EventControllerKey, glib, Orientation, ScrolledWindow, Separator, TextBuffer, TextView, Widget, Window};
use gtk4::gdk::Key;
use gtk4::prelude::{BoxExt, ButtonExt, EditableExt, EntryExt, GtkWindowExt, IsA, TextBufferExt, WidgetExt};

use crate::gui::{alert, GuiContext, MODIFIER_NONE};
use crate::html_parser;

pub(crate) fn custom_styles<F>(style: &Option<String>, gc: &GuiContext,
	main_win: &impl IsA<Window>, callback: F)
	where F: Fn(String) + 'static
{
	let buf = TextBuffer::builder()
		.enable_undo(true)
		.build();
	if let Some(style) = style {
		buf.set_text(style);
	}
	let text = TextView::builder()
		.buffer(&buf)
		.editable(true)
		.height_request(450)
		.width_request(450)
		.build();
	let scroll_view = ScrolledWindow::builder()
		.child(&text)
		.width_request(500)
		.height_request(500)
		.hexpand(true)
		.build();
	let gc2 = gc.clone();
	input_dialog(&scroll_view, "custom-style-dialog-title", gc, main_win, move |_, _| {
		let (start, end) = buf.bounds();
		let text = buf.text(&start, &end, true);
		html_parser::parse_stylesheet(&text, true)
			.map_err(|err|
				Cow::Owned(gc2.i18n.args_msg("invalid-style", vec![
					("error", err.to_string()),
				])))?;
		callback(text.to_string());
		Ok(())
	});
}

#[inline]
pub(crate) fn goto<F>(gc: &GuiContext, main_win: &impl IsA<Window>, callback: F)
	where F: Fn(usize) -> Result<()> + 'static
{
	let entry = Entry::builder()
		.placeholder_text(gc.i18n.msg("goto-placeholder"))
		.build();
	let ok_btn = input_dialog(&entry, "goto-dialog-title", gc, main_win, move |gc, entry| {
		let line_no = entry
			.text()
			.to_string()
			.trim()
			.parse()
			.map_err(|_| gc.i18n.msg("invalid-format"))?;
		callback(line_no)
			.map_err(|e| Cow::Owned(e.to_string()))?;
		Ok(())
	});
	entry.connect_activate(move |_| ok_btn.emit_clicked());
}

fn input_dialog<F, W>(widget: &W, title: &str,
	gc: &GuiContext, main_win: &impl IsA<Window>, callback: F) -> Button
	where
		F: for<'a> Fn(&'a GuiContext, &W) -> Result<(), Cow<'a, str>> + 'static,
		W: IsA<Widget>
{
	let i18n = &gc.i18n;
	let main = gtk4::Box::new(Orientation::Vertical, 10);
	main.set_margin_top(10);
	main.set_margin_bottom(10);
	main.set_margin_start(10);
	main.set_margin_end(10);
	let dialog = Window::builder()
		.title(i18n.msg(title))
		.transient_for(main_win)
		.resizable(false)
		.modal(true)
		.child(&main)
		.default_widget(widget)
		.build();

	main.append(widget);

	main.append(&Separator::new(Orientation::Horizontal));

	let button_box = gtk4::Box::new(Orientation::Horizontal, 10);
	button_box.set_halign(Align::End);
	let ok_btn = {
		let dialog = dialog.clone();
		let ok_btn = Button::builder()
			.label(i18n.msg("ok-title"))
			.build();
		let gc = gc.clone();
		let widget = widget.clone();
		ok_btn.connect_clicked(move |_| if let Err(msg) = callback(&gc, &widget) {
			alert(&gc.i18n.msg("invalid-input-title"), &msg, &dialog);
		} else {
			dialog.close();
		});
		button_box.append(&ok_btn);
		ok_btn
	};
	{
		let dialog = dialog.clone();
		let cancel_btn = Button::builder()
			.label(i18n.msg("cancel-title"))
			.build();
		cancel_btn.connect_clicked(move |_| {
			dialog.close();
		});
		button_box.append(&cancel_btn);
	}
	main.append(&button_box);

	let key_event = EventControllerKey::new();
	{
		let dialog = dialog.clone();
		key_event.connect_key_pressed(move |_, key, _, modifier| {
			if key == Key::Escape && modifier == MODIFIER_NONE {
				dialog.close();
				glib::Propagation::Stop
			} else {
				glib::Propagation::Proceed
			}
		});
	}
	dialog.add_controller(key_event);
	dialog.present();

	ok_btn
}