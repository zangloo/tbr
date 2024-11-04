use crate::book::BookLoader;
use crate::container::{BookContent, BookName, Container, ContainerLoader};
use anyhow::{anyhow, Result};
use std::fs;
use std::fs::ReadDir;
use std::path::PathBuf;
use std::str::FromStr;
use lexical_sort::{natural_lexical_cmp, StringSort};

pub(crate) struct FolderLoader {}

impl ContainerLoader for FolderLoader {
	fn accept(&self, filename: &str) -> bool
	{
		PathBuf::from_str(filename)
			.map_or(false, |path| path.is_dir())
	}

	fn open(&self, filename: &str, book_loader: &BookLoader) -> Result<Box<dyn Container>>
	{
		let root = PathBuf::from_str(filename)?;
		let dir = fs::read_dir(filename)?;
		let mut files = vec![];
		let mut names = vec![];
		load_folder(dir, &root, &mut files, &mut names, book_loader)?;
		names.string_sort_unstable(natural_lexical_cmp);
		let filename = filename.to_owned();
		Ok(Box::new(FolderContainer { filename, files, names }))
	}
}

fn load_folder(dir: ReadDir, root: &PathBuf, files: &mut Vec<PathBuf>, names: &mut Vec<BookName>,
	book_loader: &BookLoader) -> Result<()>
{
	for entry in dir {
		let entry = entry?;
		let path = entry.path();
		if path.is_file() {
			let name = path
				.strip_prefix(root)?
				.to_str()
				.ok_or(anyhow!("Failed get path string: {:#?}", path))?;
			if book_loader.support(name) {
				let idx = files.len();
				names.push(BookName { name: name.to_owned(), index: idx });
				files.push(path);
			}
		} else {
			load_folder(path.read_dir()?, root, files, names, book_loader)?;
		}
	}
	Ok(())
}

struct FolderContainer {
	filename: String,
	files: Vec<PathBuf>,
	names: Vec<BookName>,
}

impl Container for FolderContainer {
	fn filename(&self) -> &str
	{
		&self.filename
	}

	fn inner_book_names(&self) -> Option<&Vec<BookName>>
	{
		Some(&self.names)
	}

	fn book_content(&mut self, inner_index: usize) -> Result<BookContent>
	{
		let bn = self.names.get(inner_index)
			.ok_or(anyhow!("No book at {inner_index}"))?;
		let path = self.files.get(bn.index)
			.ok_or(anyhow!("No file at {}", bn.index))?;
		if path.is_file() {
			Ok(BookContent::Path(path.clone()))
		} else {
			Err(anyhow!("Book path is a folder: {:#?}",path))
		}
	}
}