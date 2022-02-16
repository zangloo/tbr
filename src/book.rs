use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::fs::OpenOptions;
use std::io::Read;

use anyhow::{anyhow, Result};

use crate::book::epub::EpubLoader;
use crate::book::html::HtmlLoader;
use crate::book::txt::TxtLoader;
use crate::container::BookContent;
use crate::container::BookContent::{Buf, File};

mod epub;
mod txt;
mod html;

pub trait Book {
	fn chapter_count(&self) -> usize { 1 }
	fn set_chapter(&mut self, chapter: usize) -> Result<()> {
		if chapter >= self.chapter_count() {
			return Err(anyhow!("Invalid chapter: {}", chapter));
		}
		Ok(())
	}
	fn current_chapter(&self) -> usize { 0 }
	fn title(&self) -> Option<&String> { None }
	fn chapter_title(&self, _chapter: usize) -> Option<&String> { None }
	fn lines(&self) -> &Vec<String>;
	fn leading_space(&self) -> usize { 2 }
}

pub struct BookLoader {
	loaders: Vec<Box<dyn Loader>>,
}

pub(crate) trait Loader {
	fn support(&self, filename: &str) -> bool;
	fn load_file(&self, filename: &str, chapter: usize) -> Result<Box<dyn Book>> {
		let mut file = OpenOptions::new().read(true).open(filename)?;
		let mut content: Vec<u8> = Vec::new();
		file.read_to_end(&mut content)?;
		self.load_buf(filename, content, chapter)
	}
	fn load_buf(&self, filename: &str, buf: Vec<u8>, chapter: usize) -> Result<Box<dyn Book>>;
}

impl BookLoader {
	pub fn support(&self, filename: &str) -> bool {
		for loader in self.loaders.iter() {
			if loader.support(filename) {
				return true;
			}
		}
		false
	}
	pub fn load(&self, filename: &str, content: BookContent, chapter: usize) -> Result<Box<dyn Book>> {
		for loader in self.loaders.iter() {
			if loader.support(filename) {
				let book = match content {
					File(..) => loader.load_file(filename, chapter)?,
					Buf(buf) => loader.load_buf(filename, buf, chapter)?,
				};
				return Ok(book);
			}
		}
		Err(anyhow!("Not support open book: {}", filename))
	}
}

impl Default for BookLoader {
	fn default() -> Self {
		let mut loaders: Vec<Box<dyn Loader>> = Vec::new();
		loaders.push(Box::new(TxtLoader {}));
		loaders.push(Box::new(EpubLoader {}));
		loaders.push(Box::new(HtmlLoader {}));
		BookLoader { loaders }
	}
}

pub(crate) struct InvalidChapterError {}

const INVALID_CHAPTER_ERROR_MESSAGE: &str = "invalid chapter";

impl Debug for InvalidChapterError {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		f.write_str(INVALID_CHAPTER_ERROR_MESSAGE)
	}
}

impl Display for InvalidChapterError {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		f.write_str(INVALID_CHAPTER_ERROR_MESSAGE)
	}
}

impl Error for InvalidChapterError {}