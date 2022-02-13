use std::borrow::Borrow;
use anyhow::{anyhow, Result};
use std::fs::{File, OpenOptions};
use std::io::Read;
use lexical_sort::{natural_lexical_cmp, StringSort};
use zip::ZipArchive;
use crate::book::{Book, Loader};
use crate::book::html::HtmlLoader;
use crate::book::txt::TxtLoader;
use crate::common::{html_lines, plain_text_lines};

pub struct ZipLoader {}

impl Default for ZipLoader {
	fn default() -> Self { ZipLoader {} }
}

impl Loader for ZipLoader {
	fn support(&self, filename: &str) -> bool {
		let filename = filename.to_lowercase();
		filename.ends_with(".zip") || filename.ends_with(".bzip2")
	}

	fn load(&self, filename: &String, chapter: usize) -> Result<Box<dyn Book>> {
		let file = OpenOptions::new().read(true).open(filename)?;
		let mut zip = zip::ZipArchive::new(file)?;
		let mut toc = vec![];
		for name in zip.file_names() {
			if TxtLoader::support(name) || HtmlLoader::support(name) {
				toc.push(String::from(name));
			}
		}
		toc.string_sort_unstable(natural_lexical_cmp);
		if chapter >= toc.len() {
			return Err(anyhow!("invalid chapter: {}", chapter));
		}
		let title = toc[chapter].clone();
		let lines = load_chapter(&mut zip, title.borrow())?;
		Ok(Box::new(ZipBook { zip, toc, chapter, title, lines }))
	}
}

fn load_chapter(zip: &mut ZipArchive<File>, filename: &str) -> Result<Vec<String>> {
	let mut zip_file = zip.by_name(filename)?;
	let mut content = vec![];
	zip_file.read_to_end(&mut content)?;
	if TxtLoader::support(filename) {
		plain_text_lines(content)
	} else {
		html_lines(content)
	}
}

struct ZipBook {
	zip: ZipArchive<File>,
	toc: Vec<String>,
	chapter: usize,
	title: String,
	lines: Vec<String>,
}

impl Book for ZipBook {
	fn chapter_count(&self) -> usize {
		self.toc.len()
	}

	fn set_chapter(&mut self, chapter: usize) -> Result<()> {
		self.lines = load_chapter(&mut self.zip, &self.toc[chapter])?;
		Ok(())
	}

	fn current_chapter(&self) -> usize {
		self.chapter
	}

	fn title(&self) -> Option<&String> {
		Some(&self.title)
	}

	fn chapter_title(&self, chapter: usize) -> Option<&String> {
		Some(&self.toc[chapter])
	}

	fn lines(&self) -> &Vec<String> {
		&self.lines
	}
}