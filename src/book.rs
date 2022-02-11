use anyhow::anyhow;
use anyhow::Result;

use crate::book::epub::EpubLoader;
use crate::book::txt::TxtLoader;

mod epub;
mod txt;

pub trait Book {
	fn chapter_count(&self) -> usize;
	fn set_chapter(&mut self, chapter: usize) -> Result<()>;
	fn current_chapter(&self) -> usize;
	fn title(&self) -> &String;
	fn chapter_title(&self, chapter: usize) -> Option<&String>;
	fn lines(&self) -> &Vec<String>;
	fn leading_space(&self) -> usize { 2 }
}

pub(crate) struct BookLoader {
	loaders: Vec<Box<dyn Loader>>,
}

pub(crate) trait Loader {
	fn support(&self, filename: &String) -> bool;
	fn load(&self, filename: &String) -> Result<Box<dyn Book>>;
}

impl BookLoader {
	pub fn load(&self, filename: &String, chapter: usize) -> Result<Box<dyn Book>> {
		for loader in self.loaders.iter() {
			if loader.support(filename) {
				let mut book = loader.load(filename)?;
				book.set_chapter(chapter)?;
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
		BookLoader { loaders }
	}
}
