use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;
use anyhow::Result;
use elsa::FrozenMap;
use indexmap::IndexSet;

use crate::book::{Book, LoadingChapter, Line, Loader};
use crate::html_convertor::{html_content, html_str_content, HtmlContent};
use crate::common::{plain_text, TraceInfo};
use crate::frozen_map_get;
use crate::xhtml::xhtml_to_html;

pub(crate) struct HtmlLoader {
	extensions: Vec<&'static str>,
}

pub(crate) struct HtmlBook {
	content: HtmlContent,
	font_families: IndexSet<String>,
}

impl HtmlLoader {
	pub(crate) fn new() -> Self
	{
		let extensions = vec![".html", ".htm", ".xhtml"];
		HtmlLoader { extensions }
	}
}

impl Loader for HtmlLoader {
	fn extensions(&self) -> &Vec<&'static str>
	{
		&self.extensions
	}

	fn load_file(&self, filename: &str, mut file: fs::File, _loading_chapter: LoadingChapter) -> Result<Box<dyn Book>>
	{
		let path = PathBuf::from_str(filename)?;
		let cwd = path.parent();
		let mut content: Vec<u8> = Vec::new();
		file.read_to_end(&mut content)?;
		let mut font_families = IndexSet::new();
		let mut text = plain_text(content, false)?;
		if filename.to_lowercase().ends_with(".xhtml") {
			text = xhtml_to_html(&text)?;
		}
		let stylesheets: FrozenMap<String, String> = Default::default();
		let content = html_str_content(&text, &mut font_families, Some(|path: &str| {
			let path = cwd?.join(&path);
			let full_path = path.canonicalize().ok()?.to_str()?.to_string();
			frozen_map_get!(stylesheets, full_path, || {
				fs::read_to_string(&path).ok()
			})
		}))?;
		Ok(Box::new(HtmlBook { content, font_families }))
	}

	fn load_buf(&self, _filename: &str, content: Vec<u8>, _loading_chapter: LoadingChapter) -> Result<Box<dyn Book>>
	{
		let mut font_families = IndexSet::new();
		let text = plain_text(content, false)?;
		let content = html_content(&text, &mut font_families)?;
		Ok(Box::new(HtmlBook { content, font_families }))
	}
}

impl Book for HtmlBook {
	fn lines(&self) -> &Vec<Line>
	{
		&self.content.lines
	}

	fn link_position(&mut self, line: usize, link_index: usize) -> Option<TraceInfo>
	{
		let text = &self.content.lines.get(line)?;
		let link = text.link_at(link_index)?;
		let link_target = link.target;
		let mut split = link_target.split('#');
		split.next()?;
		let anchor = split.next()?;
		let position = self.content.id_map.get(anchor)?;
		Some(TraceInfo { chapter: 0, line: position.line, offset: position.offset })
	}

	#[inline]
	fn font_family_names(&self) -> Option<&IndexSet<String>>
	{
		Some(&self.font_families)
	}

	#[cfg(feature = "gui")]
	fn with_custom_color(&self) -> bool
	{
		true
	}
}
