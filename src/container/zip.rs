use std::fs::{File, OpenOptions};
use std::io::Read;

use anyhow::Result;
use lexical_sort::{natural_lexical_cmp, StringSort};
use zip::ZipArchive;

use crate::BookLoader;
use crate::common::plain_text;
use crate::container::{BookContent, BookName, Container, ContainerLoader};

pub(crate) struct ZipLoader {}

impl ContainerLoader for ZipLoader {
	fn accept(&self, filename: &str) -> bool {
		let filename = filename.to_lowercase();
		filename.ends_with(".zip")
	}

	fn open(&self, filename: &str, book_loader: &BookLoader) -> Result<Box<dyn Container>>
	{
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
		let mut files = vec![];
		for (idx, name) in names.enumerate() {
			if book_loader.support(name) {
				files.push(BookName { name: String::from(name), index: idx });
			}
		}
		files.string_sort_unstable(natural_lexical_cmp);
		Ok(Box::new(ZipContainer { zip, files }))
	}
}

pub(crate) struct ZipContainer {
	zip: ZipArchive<File>,
	files: Vec<BookName>,
}

impl Container for ZipContainer {
	fn inner_book_names(&self) -> &Vec<BookName> {
		&self.files
	}

	fn book_content(&mut self, inner_index: usize) -> Result<BookContent> {
		let book_name = &self.files[inner_index];
		let mut zip_file = self.zip.by_index(book_name.index)?;
		let mut content = vec![];
		zip_file.read_to_end(&mut content)?;
		Ok(BookContent::Buf(content))
	}
}
