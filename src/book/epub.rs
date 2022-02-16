use std::io::{Cursor, Read, Seek};

use anyhow::{Error, Result};
use epub::doc::{EpubDoc, NavPoint};

use crate::book::{Book, InvalidChapterError, Loader};
use crate::html_convertor::html_lines;

pub struct EpubBook<R: Read + Seek> {
	doc: EpubDoc<R>,
	chapter: usize,
	title: String,
	lines: Vec<String>,
}

pub struct EpubLoader {}

fn get_nav_point<R: Read + Seek>(epub_doc: &EpubDoc<R>, chapter: usize) -> Option<&NavPoint> {
	epub_doc.toc.get(chapter)
}

fn load_chapter<R: Read + Seek>(epub_doc: &mut EpubDoc<R>, chapter: usize) -> Result<(String, Vec<String>)> {
	let single = match get_nav_point(epub_doc, chapter) {
		Some(s) => s,
		None => return Err(Error::new(InvalidChapterError {})),
	};
	let resource_path = single.content.clone();
	let title = single.label.clone();
	let content = epub_doc.get_resource_by_path(resource_path)?;
	let lines = html_lines(content)?;
	Ok((title, lines))
}

impl Loader for EpubLoader {
	fn support(&self, filename: &str) -> bool {
		filename.to_lowercase().ends_with(".epub")
	}

	fn load_file(&self, filename: &str, chapter: usize) -> Result<Box<dyn Book>> {
		let doc = EpubDoc::new(filename)?;
		self.do_load(doc, chapter)
	}

	fn load_buf(&self, _filename: &str, content: Vec<u8>, chapter: usize) -> Result<Box<dyn Book>>
	{
		let doc = EpubDoc::from_reader(Cursor::new(content))?;
		self.do_load(doc, chapter)
	}
}

impl EpubLoader {
	fn do_load<R: 'static + Read + Seek>(&self, mut doc: EpubDoc<R>, mut chapter: usize) -> Result<Box<dyn Book>> {
		let chapters = doc.toc.len();
		if chapter >= chapters {
			chapter = chapters - 1;
		}
		let (title, lines) = load_chapter(&mut doc, chapter)?;
		let book = EpubBook { doc, chapter, title, lines };
		Result::Ok(Box::new(book))
	}
}

impl<R: Read + Seek> Book for EpubBook<R> {
	fn chapter_count(&self) -> usize {
		self.doc.toc.len()
	}

	fn set_chapter(&mut self, chapter: usize) -> Result<()> {
		let (title, lines) = load_chapter(&mut self.doc, chapter)?;
		self.chapter = chapter;
		self.title = title;
		self.lines = lines;
		Ok(())
	}

	fn current_chapter(&self) -> usize {
		self.chapter
	}

	fn title(&self) -> Option<&String> {
		Some(&self.title)
	}

	fn chapter_title(&self, chapter: usize) -> Option<&String> {
		Some(&get_nav_point(&self.doc, chapter)?.label)
	}

	fn lines(&self) -> &Vec<String> {
		&self.lines
	}
}
