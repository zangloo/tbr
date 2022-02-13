use std::fs::File;

use anyhow::{anyhow, Error, Result};
use epub::doc::{EpubDoc, NavPoint};

use crate::book::{Book, InvalidChapterError, Loader};
use crate::html_convertor::html_lines;

pub struct EpubBook {
	doc: EpubDoc<File>,
	chapter: usize,
	title: String,
	lines: Vec<String>,
}

pub struct EpubLoader {}

fn get_nav_point(epub_doc: &EpubDoc<File>, chapter: usize) -> Option<&NavPoint> {
	epub_doc.toc.get(chapter)
}

fn load_chapter(epub_doc: &mut EpubDoc<File>, chapter: usize) -> Result<(String, Vec<String>)> {
	let single = match get_nav_point(epub_doc, chapter) {
		Some(s) => s,
		None => return Err(Error::new(InvalidChapterError{})),
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

	fn load(&self, filename: &String, chapter: usize) -> Result<Box<dyn Book>> {
		let mut doc = EpubDoc::new(filename)?;
		let (title, lines) = load_chapter(&mut doc, chapter)?;
		let book = EpubBook { doc, chapter, title, lines };
		Result::Ok(Box::new(book))
	}
}

impl Book for EpubBook {
	fn chapter_count(&self) -> usize {
		self.doc.toc.len()
	}

	fn set_chapter(&mut self, chapter: usize) -> Result<()> {
		let single = match get_nav_point(&self.doc, chapter) {
			Some(s) => s,
			None => return Err(anyhow!("Invalid chapter: {}", chapter)),
		};
		let resource_path = single.content.clone();
		self.title = single.label.clone();
		let content = self.doc.get_resource_by_path(resource_path)?;
		self.lines = html_lines(content)?;
		self.chapter = chapter;
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
