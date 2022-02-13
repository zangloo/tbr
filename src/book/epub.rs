use std::fs::File;

use anyhow::{anyhow, Result};
use epub::doc::{EpubDoc, NavPoint};

use crate::book::{Book, Loader};
use crate::common::html_lines;

pub struct EpubBook {
	doc: EpubDoc<File>,
	chapter: usize,
	title: String,
	lines: Vec<String>,
}

pub struct EpubLoader {}

impl Loader for EpubLoader {
	fn support(&self, filename: &String) -> bool {
		filename.to_lowercase().ends_with(".epub")
	}

	fn load(&self, filename: &String) -> Result<Box<dyn Book>> {
		let doc = EpubDoc::new(filename)?;
		let book = EpubBook { doc, chapter: 0, title: "Not loaded".to_string(), lines: vec![] };
		Result::Ok(Box::new(book))
	}
}

impl Book for EpubBook {
	fn chapter_count(&self) -> usize {
		self.doc.toc.len()
	}

	fn set_chapter(&mut self, chapter: usize) -> Result<()> {
		let single = match self.get_nav_point(chapter) {
			Some(s) => s,
			None => return Err(anyhow!("Invalid chapter: {}", chapter)),
		};
		let resource_path = single.content.clone();
		self.title = single.label.clone();
		let content = &self.doc.get_resource_by_path(resource_path)?;
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
		Some(&self.get_nav_point(chapter)?.label)
	}

	fn lines(&self) -> &Vec<String> {
		&self.lines
	}
}

impl EpubBook {
	fn get_nav_point(&self, chapter: usize) -> Option<&NavPoint> {
		self.doc.toc.get(chapter)
	}
}