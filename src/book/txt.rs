use std::fs::OpenOptions;
use std::io::Read;

use crate::book::{Book, Loader};
use crate::common::plain_text_lines;

pub struct TxtBook {
	lines: Vec<String>,
	leading_space: usize,
}

impl Book for TxtBook {
	fn lines(&self) -> &Vec<String> {
		&self.lines
	}

	fn leading_space(&self) -> usize {
		self.leading_space
	}
}

pub struct TxtLoader {}

impl Loader for TxtLoader {
	fn support(&self, filename: &String) -> bool {
		let filename = filename.to_lowercase();
		filename.ends_with(".txt")
			|| filename.ends_with(".log")
			|| filename.ends_with(".json")
			|| filename.ends_with(".yaml")
			|| filename.ends_with(".yml")
			|| filename.ends_with(".js")
	}

	fn load(&self, filename: &String) -> anyhow::Result<Box<dyn Book>> {
		let mut file = OpenOptions::new().read(true).open(filename)?;
		let mut reader: Vec<u8> = Vec::new();
		file.read_to_end(&mut reader)?;
		let lines = plain_text_lines(reader)?;
		let leading_space = if filename.to_lowercase().ends_with(".log") {
			0
		} else {
			2
		};
		let book = TxtBook { lines, leading_space };
		Ok(Box::new(book))
	}
}