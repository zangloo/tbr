use std::fs::OpenOptions;
use std::io::{Cursor, Read, Seek};
use std::path::PathBuf;
use anyhow::Result;

use crate::book::{Book, Chapter, Line, Loader};
use crate::book::epub::parser::EpubArchive;
use crate::view::TraceInfo;

mod parser;

pub struct EpubBook<R: Read + Seek> {
	doc: EpubArchive<R>,
	chapter: Chapter,
	chapter_path: PathBuf,
}

pub struct EpubLoader {}

impl Loader for EpubLoader {
	fn support(&self, filename: &str) -> bool {
		filename.to_lowercase().ends_with(".epub")
	}

	fn load_file(&self, filename: &str, chapter: usize) -> Result<Box<dyn Book>> {
		let file = OpenOptions::new().read(true).open(filename)?;
		let doc = EpubArchive::new(file)?;
		self.do_load(doc, chapter)
	}

	fn load_buf(&self, _filename: &str, content: Vec<u8>, chapter: usize) -> Result<Box<dyn Book>>
	{
		let doc = EpubArchive::new(Cursor::new(content))?;
		self.do_load(doc, chapter)
	}
}

impl EpubLoader {
	fn do_load<R: 'static + Read + Seek>(&self, mut doc: EpubArchive<R>, mut chapter: usize) -> Result<Box<dyn Book>> {
		let chapters = doc.toc.len();
		if chapter >= chapters {
			chapter = chapters - 1;
		}
		let (chapter, chapter_path) = doc.load_chapter(chapter)?;
		let book = EpubBook { doc, chapter, chapter_path };
		Result::Ok(Box::new(book))
	}
}

impl<'a, R: Read + Seek> Book for EpubBook<R> {
	fn chapter_count(&self) -> usize {
		self.doc.toc.len()
	}

	fn set_chapter(&mut self, chapter_index: usize) -> Result<()> {
		let (chapter, chapter_path) = self.doc.load_chapter(chapter_index)?;
		self.chapter = chapter;
		self.chapter_path = chapter_path;
		Ok(())
	}

	fn current_chapter(&self) -> usize {
		self.chapter.index
	}

	fn title(&self) -> Option<&String> {
		Some(&self.chapter.title)
	}

	fn chapter_title(&self, chapter: usize) -> Option<&String> {
		let np = self.doc.toc.get(chapter)?;
		let label = match &np.label {
			Some(label) => label,
			None => &np.src,
		};
		Some(&label)
	}

	fn lines(&self) -> &Vec<Line> {
		&self.chapter.lines
	}

	fn link_position(&self, mut link_target: &str) -> Option<TraceInfo> {
		let mut current_path = self.chapter_path.clone();
		current_path.pop();
		while link_target.starts_with("../") {
			current_path.pop();
			link_target = &link_target[3..];
		}
		current_path.push(link_target);
		let chapter = self.doc.target_location(current_path.to_str()?)?;
		Some(TraceInfo { chapter, line: 0, position: 0 })
	}
}
