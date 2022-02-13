use std::fs::OpenOptions;
use std::io::Read;
use anyhow::Result;

use crate::book::{Book, Loader};
use crate::common::html_lines;

pub(crate) struct HtmlLoader {}

pub(crate) struct HtmlBook {
	lines: Vec<String>,
}

impl Loader for HtmlLoader {
	fn support(&self, filename: &String) -> bool {
		let filename = filename.to_lowercase();
		filename.ends_with(".html") || filename.ends_with(".htm")
	}

	fn load(&self, filename: &String) -> Result<Box<dyn Book>> {
		let mut file = OpenOptions::new().read(true).open(filename)?;
		let mut reader: Vec<u8> = Vec::new();
		file.read_to_end(&mut reader)?;
		let lines = html_lines(&reader)?;
		Ok(Box::new(HtmlBook { lines }))
	}
}

impl Book for HtmlBook {
	fn lines(&self) -> &Vec<String> {
		&self.lines
	}
}