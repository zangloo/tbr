use cursive::Cursive;
use cursive::event::Key::Esc;
use cursive::traits::Scrollable;
use cursive::views::{Dialog, OnEventView, SelectView};

pub(crate) fn list_dialog<'a, F, I>(title: &str, iterator: I, current_value: usize, callback: F) -> OnEventView<Dialog>
	where
		F: Fn(&mut Cursive, usize) + 'static,
		I: Iterator<Item=(&'a str, usize)>,
{
	let mut select_view = SelectView::new()
		.on_submit(move |s, v| {
			s.pop_layer();
			callback(s, *v);
		});
	let mut selected = 0;
	for (idx, (title, value)) in iterator.enumerate() {
		select_view.add_item(title, value);
		if current_value == value {
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

pub struct ListIterator<'a, F>
	where F: Fn(usize) -> Option<(&'a str, usize)>
{
	index: usize,
	mapper: F,
}

impl<'a, F> ListIterator<'a, F>
	where F: Fn(usize) -> Option<(&'a str, usize)>
{
	pub fn new(mapper: F) -> Self
	{
		ListIterator { index: 0, mapper }
	}
}

impl<'a, F> Iterator for ListIterator<'a, F>
	where F: Fn(usize) -> Option<(&'a str, usize)>
{
	type Item = (&'a str, usize);

	fn next(&mut self) -> Option<Self::Item> {
		let ret = (self.mapper)(self.index)?;
		self.index += 1;
		Some(ret)
	}
}
