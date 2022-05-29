use anyhow::{anyhow, Result};

use crate::book::{Book, EMPTY_CHAPTER_CONTENT};
use crate::{BookLoader, ReadingInfo};
use crate::container::zip::ZipLoader;

mod zip;

pub struct ContainerManager {
	book_loader: BookLoader,
	zip_loader: ZipLoader,
}

impl Default for ContainerManager {
	fn default() -> Self {
		ContainerManager { zip_loader: ZipLoader {}, book_loader: Default::default() }
	}
}

impl ContainerManager {
	pub fn open(&self, filename: &String) -> Result<Box<dyn Container>>
	{
		if self.zip_loader.accept(filename) {
			self.zip_loader.open(filename, &self.book_loader)
		} else {
			Ok(Box::new(DummyContainer { filenames: vec![BookName { name: filename.clone(), index: 0 }] }))
		}
	}

	pub fn load_book(&self, container: &mut Box<dyn Container>, book_index: usize, mut chapter: usize) -> Result<Box<dyn Book>> {
		let book_name = if chapter == usize::MAX {
			chapter = container.inner_book_names().len() - 1;
			&container.inner_book_names()[chapter]
		} else {
			match container.inner_book_names().get(book_index) {
				Some(name) => name,
				None => return Err(anyhow!("Invalid book index: {}", book_index)),
			}
		};
		let filename = book_name.name().clone();
		let content = container.book_content(book_index)?;
		let book = self.book_loader.load(&filename, content, chapter)?;
		let lines = &mut book.lines();
		let line_count = lines.len();
		if line_count == 0 {
			return Err(anyhow!(EMPTY_CHAPTER_CONTENT));
		}
		Ok(book)
	}
}

pub trait ContainerLoader {
	fn accept(&self, filename: &str) -> bool;
	fn open(&self, filename: &str, book_loader: &BookLoader) -> Result<Box<dyn Container>>;
}

pub trait Container {
	fn inner_book_names(&self) -> &Vec<BookName>;
	fn book_content(&mut self, inner_index: usize) -> Result<BookContent>;
}

pub struct BookName {
	name: String,
	index: usize,
}

impl AsRef<str> for BookName {
	fn as_ref(&self) -> &str {
		self.name.as_str()
	}
}

impl BookName {
	#[cfg(feature = "gui")]
	pub fn new(name: String, index: usize) -> Self
	{
		BookName { name, index }
	}
	pub fn name(&self) -> &String {
		&self.name
	}
}

impl Clone for BookName {
	fn clone(&self) -> Self {
		BookName { name: self.name.clone(), index: self.index }
	}
}

// for non pack file as a container with single book
pub struct DummyContainer {
	filenames: Vec<BookName>,
}

impl Container for DummyContainer {
	fn inner_book_names(&self) -> &Vec<BookName> {
		&self.filenames
	}

	fn book_content(&mut self, _inner_index: usize) -> Result<BookContent> {
		Ok(BookContent::File(self.filenames[0].name.clone()))
	}
}

pub enum BookContent {
	File(String),
	Buf(Vec<u8>),
}

pub fn load_container(container_manager: &ContainerManager, reading: &ReadingInfo) -> Result<Box<dyn Container>> {
	let container = container_manager.open(&reading.filename)?;
	let book_names = container.inner_book_names();
	if book_names.len() == 0 {
		return Err(anyhow!("No content supported."));
	}
	Ok(container)
}

pub fn load_book(container_manager: &ContainerManager, container: &mut Box<dyn Container>, reading: &mut ReadingInfo) -> Result<Box<dyn Book>> {
	let book = container_manager.load_book(container, reading.inner_book, reading.chapter)?;
	let lines = book.lines();
	if reading.line >= lines.len() {
		reading.line = lines.len() - 1;
	}
	let chars = lines[reading.line].len();
	if reading.position >= chars {
		reading.position = 0;
	}
	Ok(book)
}
