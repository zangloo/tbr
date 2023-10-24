use anyhow::{anyhow, Result};

use crate::book::{Book, LoadingChapter, EMPTY_CHAPTER_CONTENT};
use crate::BookLoader;
use crate::config::{BookLoadingInfo, ReadingInfo};
use crate::container::zip::ZipLoader;

mod zip;

pub struct ContainerManager {
	pub book_loader: BookLoader,
	zip_loader: ZipLoader,
}

impl Default for ContainerManager {
	fn default() -> Self {
		ContainerManager { zip_loader: ZipLoader {}, book_loader: Default::default() }
	}
}

impl ContainerManager {
	pub fn open(&self, filename: &str) -> Result<Box<dyn Container>>
	{
		if self.zip_loader.accept(filename) {
			self.zip_loader.open(filename, &self.book_loader)
		} else {
			Ok(Box::new(DummyContainer::new(&filename)))
		}
	}

	pub fn load_book(&self, container: &mut Box<dyn Container>, loading: BookLoadingInfo)
		-> Result<(Box<dyn Book>, ReadingInfo)>
	{
		let (book_index, chapter) = match &loading {
			BookLoadingInfo::NewReading(_, inner_book, chapter) => (*inner_book, *chapter),
			BookLoadingInfo::History(reading) => (reading.inner_book, reading.chapter),
		};
		let book_name = match container.inner_book_names().get(book_index) {
			Some(name) => name,
			None => return Err(anyhow!("Invalid book index: {}", book_index)),
		};
		let loading_chapter = if chapter == usize::MAX {
			LoadingChapter::Last
		} else {
			LoadingChapter::Index(chapter)
		};
		let filename = book_name.name().clone();
		let content = container.book_content(book_index)?;
		let (book, reading) = self.book_loader.load(&filename, content, loading_chapter, loading)?;
		let lines = &mut book.lines();
		let line_count = lines.len();
		if line_count == 0 {
			return Err(anyhow!(EMPTY_CHAPTER_CONTENT));
		}
		Ok((book, reading))
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

impl DummyContainer {
	pub fn new(filename: &str) -> Self
	{
		let filename = title_for_filename(filename);
		let filename = filename.to_owned();
		DummyContainer {
			filenames: vec![BookName::new(filename, 0)]
		}
	}
}

#[inline]
#[cfg(windows)]
pub fn title_for_filename(filename: &str) -> &str
{
	// remove windows path prefix "\\?\"
	if filename.starts_with(r#"\\?\"#) {
		&filename[4..]
	} else {
		filename
	}
}

#[inline]
#[cfg(not(windows))]
pub fn title_for_filename(filename: &str) -> &str
{
	filename
}

pub enum BookContent {
	File(String),
	Buf(Vec<u8>),
}

pub fn load_container(container_manager: &ContainerManager,
	filename: &str) -> Result<Box<dyn Container>>
{
	let container = container_manager.open(filename)?;
	let book_names = container.inner_book_names();
	if book_names.len() == 0 {
		return Err(anyhow!("No content supported."));
	}
	Ok(container)
}

#[inline]
pub fn load_book(container_manager: &ContainerManager,
	container: &mut Box<dyn Container>, loading: BookLoadingInfo) -> Result<(Box<dyn Book>, ReadingInfo)> {
	container_manager.load_book(container, loading)
}
