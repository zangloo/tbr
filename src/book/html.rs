use anyhow::Result;
use indexmap::IndexSet;

use crate::book::{Book, LoadingChapter, Line, Loader};
use crate::html_convertor::{html_content, HtmlContent};
use crate::common::TraceInfo;

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
		let extensions = vec![".html", ".htm"];
		HtmlLoader { extensions }
	}
}

impl Loader for HtmlLoader {
	fn extensions(&self) -> &Vec<&'static str>
	{
		&self.extensions
	}

	fn load_buf(&self, _filename: &str, content: Vec<u8>, _loading_chapter: LoadingChapter) -> Result<Box<dyn Book>>
	{
		let mut font_families = IndexSet::new();
		let content = html_content(content, &mut font_families)?;
		Ok(Box::new(HtmlBook { content, font_families: IndexSet::new() }))
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
}
