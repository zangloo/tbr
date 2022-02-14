use anyhow::{Error, Result};
use std::fs::{File, OpenOptions};
use std::io::Read;
use lexical_sort::{natural_lexical_cmp, StringSort};
use zip::ZipArchive;
use crate::book::{Book, InvalidChapterError, Loader};
use crate::book::html::HtmlLoader;
use crate::book::txt::TxtLoader;
use crate::common::{plain_text, plain_text_lines};
use crate::html_convertor::html_lines;

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
		let mut buf = vec![];
		for i in 0..zip.len() {
			let zip_file = zip.by_index(i)?;
			if buf.len() > 0 {
				buf.push(b'\n');
			}
			buf.extend(zip_file.name_raw().to_vec());
		}
		let names = plain_text(buf, true)?;
		let names = names.split('\n');
		let mut toc = vec![];
		for (i, name) in names.enumerate() {
			if TxtLoader::support(&name) || HtmlLoader::support(&name) {
				toc.push(ZipTocEntry { name: String::from(name), index: i });
			}
		}
		toc.string_sort_unstable(natural_lexical_cmp);
		if chapter >= toc.len() {
			return Err(Error::new(InvalidChapterError {}));
		}
		let single = &toc[chapter];
		let title = single.name.clone();
		let lines = load_chapter(&mut zip, single)?;
		Ok(Box::new(ZipBook { zip, toc, chapter, title, lines }))
	}
}

fn load_chapter(zip: &mut ZipArchive<File>, single: &ZipTocEntry) -> Result<Vec<String>> {
	let mut zip_file = zip.by_index(single.index)?;
	let mut content = vec![];
	zip_file.read_to_end(&mut content)?;
	if TxtLoader::support(&single.name) {
		plain_text_lines(content)
	} else {
		html_lines(content)
	}
}

struct ZipTocEntry {
	name: String,
	index: usize,
}

impl AsRef<str> for ZipTocEntry {
	fn as_ref(&self) -> &str {
		self.name.as_str()
	}
}

struct ZipBook {
	zip: ZipArchive<File>,
	toc: Vec<ZipTocEntry>,
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
		Some(&self.toc[chapter].name)
	}

	fn lines(&self) -> &Vec<String> {
		&self.lines
	}
}