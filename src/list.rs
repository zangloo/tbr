use cursive::Cursive;
use cursive::event::Key::Esc;
use cursive::traits::Scrollable;
use cursive::views::{Dialog, OnEventView, SelectView};

pub struct ListEntry<'a> {
	pub title: &'a str,
	pub value: usize,
}

impl<'a> ListEntry<'a> {
	pub(crate) fn new(title: &'a str, value: usize) -> Self {
		ListEntry { title, value }
	}
}

pub(crate) fn list_dialog<'a, F, I>(title: &str, iterator: I, current_value: usize, callback: F) -> OnEventView<Dialog>
	where
		F: Fn(&mut Cursive, usize) + 'static,
		I: Iterator<Item=ListEntry<'a>>
{
	let mut select_view = SelectView::new()
		.on_submit(move |s, v| {
			s.pop_layer();
			callback(s, *v);
		});
	let mut selected = 0;
	for (idx, entry) in iterator.enumerate() {
		select_view.add_item(entry.title, entry.value);
		if current_value == entry.value {
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

pub(crate) struct ListIterator<'a, T, F>
	where F: Fn(&T, usize) -> Option<ListEntry>
{
	position: usize,
	data: &'a T,
	mapper: F,
}

impl<'a, T, F> ListIterator<'a, T, F>
	where F: Fn(&T, usize) -> Option<ListEntry>
{
	pub(crate) fn new(data: &'a T, mapper: F) -> Self
	{
		ListIterator { position: 0, data, mapper }
	}
}

impl<'a, T, F> Iterator for ListIterator<'a, T, F>
	where F: Fn(&T, usize) -> Option<ListEntry>
{
	type Item = ListEntry<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		let result = match (self.mapper)(self.data, self.position) {
			Some(entry) => entry,
			None => return None,
		};
		self.position += 1;
		Some(result)
	}
}
