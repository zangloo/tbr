use anyhow::Result;

use crate::book::{Book, Line, Loader};
use crate::html_convertor::html_lines;

pub(crate) struct HtmlLoader {}

pub(crate) struct HtmlBook {
	lines: Vec<Line>,
}

impl HtmlLoader {
	pub(crate) fn support(filename: &str) -> bool {
		let filename = filename.to_lowercase();
		filename.ends_with(".html") || filename.ends_with(".htm")
	}
}

impl Loader for HtmlLoader {
	fn support(&self, filename: &str) -> bool {
		Self::support(filename)
	}

	fn load_buf(&self, _filename: &str, content: Vec<u8>, _chapter: usize) -> Result<Box<dyn Book>> {
		let lines = html_lines(content)?;
		Ok(Box::new(HtmlBook { lines }))
	}
}

impl Book for HtmlBook {
	fn lines(&self) -> &Vec<Line> {
		&self.lines
	}
}