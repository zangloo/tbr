use cursive::Cursive;
use cursive::event::Key::Esc;
use cursive::traits::Scrollable;
use cursive::views::{Dialog, OnEventView, SelectView};
use crate::terminal::Listable;

pub(crate) fn list_dialog<'a, F, I, T: Listable>(title: &str, iterator: I, current_value: usize, callback: F) -> OnEventView<Dialog>
	where
		F: Fn(&mut Cursive, usize) + 'static,
		I: Iterator<Item=T>,
{
	let mut select_view = SelectView::new()
		.on_submit(move |s, v| {
			s.pop_layer();
			callback(s, *v);
		});
	let mut selected = 0;
	for (idx, info) in iterator.enumerate() {
		let index = info.index();
		select_view.add_item(info.title(), index);
		if current_value == index {
			selected = idx;
		}
	}
	let mut scroll_view = select_view
		.selected(selected)
		.scrollable()
		.show_scrollbars(false);
	scroll_view.scroll_to_important_area();
	let dialog = OnEventView::new(Dialog::around(scroll_view).title(title))
		.on_event('q', |s| { s.pop_layer(); })
		.on_event(Esc, |s| { s.pop_layer(); });
	dialog
}

pub struct ListIterator<F, T>
	where F: Fn(usize) -> Option<T>
{
	index: usize,
	mapper: F,
}

impl<F, T> ListIterator<F, T>
	where F: Fn(usize) -> Option<T>
{
	pub fn new(mapper: F) -> Self
	{
		ListIterator { index: 0, mapper }
	}
}

impl<F, T> Iterator for ListIterator<F, T>
	where F: Fn(usize) -> Option<T>
{
	type Item = T;

	fn next(&mut self) -> Option<Self::Item> {
		let ret = (self.mapper)(self.index)?;
		self.index += 1;
		Some(ret)
	}
}
