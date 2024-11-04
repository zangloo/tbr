use std::path::PathBuf;
use anyhow::{anyhow, Result};

use crate::book::{Book, LoadingChapter, EMPTY_CHAPTER_CONTENT};
use crate::BookLoader;
use crate::config::{BookLoadingInfo, ReadingInfo};
use crate::container::folder::FolderLoader;
use crate::container::zip::ZipLoader;

mod folder;
mod zip;

pub struct ContainerManager {
	pub book_loader: BookLoader,
	loaders: Vec<Box<dyn ContainerLoader>>,
}

impl Default for ContainerManager {
	fn default() -> Self
	{
		ContainerManager {
			book_loader: Default::default(),
			loaders: vec![
				Box::new(ZipLoader {}),
				Box::new(FolderLoader {})
			],
		}
	}
}

impl ContainerManager {
	pub fn open(&self, filename: &str) -> Result<Box<dyn Container>>
	{
		for loader in &self.loaders {
			if loader.accept(filename) {
				let book = loader.open(filename, &self.book_loader)?;
				return Ok(book);
			}
		}
		Ok(Box::new(DummyContainer::new(&filename)))
	}

	pub fn load_book(&self, container: &mut Box<dyn Container>, loading: BookLoadingInfo)
		-> Result<(Box<dyn Book>, ReadingInfo)>
	{
		let (book_index, chapter) = match &loading {
			BookLoadingInfo::NewReading(_, inner_book, chapter, _) => (*inner_book, *chapter),
			BookLoadingInfo::ChangeInnerBook(_, inner_book, _, _, _) => (*inner_book, 0),
			BookLoadingInfo::History(reading) | BookLoadingInfo::Reload(reading)
			=> (reading.inner_book, reading.chapter),
		};
		let book_name = if let Some(names) = container.inner_book_names() {
			match names.get(book_index) {
				Some(name) => name.name.clone(),
				None => return Err(anyhow!("Invalid book index: {}", book_index)),
			}
		} else {
			container.filename().to_owned()
		};
		let loading_chapter = if chapter == usize::MAX {
			LoadingChapter::Last
		} else {
			LoadingChapter::Index(chapter)
		};
		let content = container.book_content(book_index)?;
		let (book, reading) = self.book_loader.load(
			&book_name,
			content,
			loading_chapter,
			loading)?;
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
	fn filename(&self) -> &str;
	fn inner_book_names(&self) -> Option<&Vec<BookName>>;
	fn book_content(&mut self, inner_index: usize) -> Result<BookContent>;
	fn book_name(&self, inner_index: usize) -> &str
	{
		let filename = match self.inner_book_names() {
			None => self.filename(),
			Some(names) => match names.get(inner_index) {
				None => self.filename(),
				Some(bn) => &bn.name,
			}
		};
		title_for_filename(filename)
	}
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
	filename: String,
}

impl Container for DummyContainer {
	#[inline]
	fn filename(&self) -> &str
	{
		&self.filename
	}

	#[inline]
	fn inner_book_names(&self) -> Option<&Vec<BookName>>
	{
		None
	}

	#[inline]
	fn book_content(&mut self, _inner_index: usize) -> Result<BookContent>
	{
		Ok(BookContent::File(self.filename.clone()))
	}
}

impl DummyContainer {
	pub fn new(filename: &str) -> Self
	{
		DummyContainer {
			filename: filename.to_owned(),
		}
	}
}

#[inline]
#[allow(unused)]
pub fn title_for_filename(filename: &str) -> &str
{
	const SPLITTER: char = std::path::MAIN_SEPARATOR;
	filename.rfind(SPLITTER)
		.map_or_else(|| {
			filename
		}, |idx| {
			&filename[idx + 1..]
		})
}

pub enum BookContent {
	File(String),
	Path(PathBuf),
	Buf(Vec<u8>),
}

pub fn load_container(container_manager: &ContainerManager,
	filename: &str) -> Result<Box<dyn Container>>
{
	let container = container_manager.open(filename)?;
	let book_names = container.inner_book_names();
	if let Some(names) = book_names {
		if names.len() == 0 {
			return Err(anyhow!("No content supported."));
		}
	}
	Ok(container)
}

#[inline]
pub fn load_book(container_manager: &ContainerManager,
	container: &mut Box<dyn Container>, loading: BookLoadingInfo) -> Result<(Box<dyn Book>, ReadingInfo)> {
	container_manager.load_book(container, loading)
}
