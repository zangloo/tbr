use anyhow::Result;

use crate::book::{Book, LoadingChapter, Line, Loader};
use crate::common::plain_text_lines;
use crate::config::{BookLoadingInfo, ReadingInfo};

pub struct TxtBook {
	lines: Vec<Line>,
	leading_space: usize,
}

impl Book for TxtBook {
	fn lines(&self) -> &Vec<Line> {
		&self.lines
	}

	fn leading_space(&self) -> usize {
		self.leading_space
	}
}

pub struct TxtLoader {
	extensions: Vec<&'static str>,
}

impl TxtLoader {
	pub(crate) fn new() -> Self {
		let extensions = vec![".txt", ".log", ".json", ".yaml", ".yml", ".js"];
		TxtLoader { extensions }
	}
}

impl Loader for TxtLoader {
	#[inline]
	fn extensions(&self) -> &Vec<&'static str> {
		&self.extensions
	}

	fn load_buf(&self, filename: &str, content: Vec<u8>,
		_loading_chapter: LoadingChapter, loading: BookLoadingInfo)
		-> Result<(Box<dyn Book>, ReadingInfo)>
	{
		let lines = plain_text_lines(content)?;
		let leading_space = if filename.to_lowercase().ends_with(".log") {
			0
		} else {
			2
		};
		let book = TxtBook { lines, leading_space };
		Ok((Box::new(book), loading.get()))
	}
}