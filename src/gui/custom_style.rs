use gtk4::{Align, Button, EventControllerKey, glib, Orientation, Separator, TextBuffer, TextView, Window};
use gtk4::gdk::Key;
use gtk4::glib::IsA;
use gtk4::prelude::{BoxExt, ButtonExt, GtkWindowExt, TextBufferExt, WidgetExt};
use crate::gui::MODIFIER_NONE;
use crate::i18n::I18n;

pub(crate) fn dialog<F>(style: &Option<String>, i18n: &I18n,
	main_win: &impl IsA<Window>, callback: F)
	where F: Fn(String) + 'static
{
	let buf = TextBuffer::builder()
		.enable_undo(true)
		.build();
	if let Some(style) = style {
		buf.set_text(style);
	}
	let main = gtk4::Box::new(Orientation::Vertical, 10);
	main.set_margin_top(10);
	main.set_margin_bottom(10);
	main.set_margin_start(10);
	main.set_margin_end(10);
	let dialog = Window::builder()
		.title(i18n.msg("custom-style-dialog-title"))
		.transient_for(main_win)
		.default_width(500)
		.default_height(500)
		.resizable(false)
		.modal(true)
		.child(&main)
		.build();

	main.append(&TextView::builder()
		.buffer(&buf)
		.editable(true)
		.height_request(450)
		.build());

	main.append(&Separator::new(Orientation::Horizontal));

	let button_box = gtk4::Box::new(Orientation::Horizontal, 10);
	button_box.set_halign(Align::End);
	{
		let dialog = dialog.clone();
		let ok_btn = Button::builder()
			.label(i18n.msg("ok-title"))
			.build();
		ok_btn.connect_clicked(move |_| {
			dialog.close();
			let (start, end) = buf.bounds();
			let text = buf.text(&start, &end, true);
			callback(text.to_string());
		});
		button_box.append(&ok_btn);
	}
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
}