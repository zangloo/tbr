use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use anyhow::{anyhow, Result};

use crate::book::epub::EpubLoader;
use crate::book::html::HtmlLoader;
use crate::book::txt::TxtLoader;
use crate::book::zip::ZipLoader;

mod epub;
mod txt;
mod zip;
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

pub(crate) struct BookLoader {
	loaders: Vec<Box<dyn Loader>>,
}

pub(crate) trait Loader {
	fn support(&self, filename: &str) -> bool;
	fn load(&self, filename: &String, chapter: usize) -> Result<Box<dyn Book>>;
}

impl BookLoader {
	pub fn load(&self, filename: &String, chapter: usize) -> Result<Box<dyn Book>> {
		for loader in self.loaders.iter() {
			if loader.support(filename) {
				let book = loader.load(filename, chapter)?;
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
		loaders.push(Box::new(ZipLoader::default()));
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