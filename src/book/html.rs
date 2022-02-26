use anyhow::Result;

use crate::book::{Book, Line, Loader};
use crate::html_convertor::{html_content, HtmlContent};
use crate::view::TraceInfo;

pub(crate) struct HtmlLoader {}

pub(crate) struct HtmlBook {
	content: HtmlContent,
}

impl HtmlLoader {
	pub(crate) fn support(filename: &str) -> bool {
		let filename = filename.to_lowercase();
		filename.ends_with(".html") || filename.ends_with(".htm")
	}
}

impl Loader for HtmlLoader {
	fn support(&self, filename: &str) -> bool {
		Self::support(filename)
	}

	fn load_buf(&self, _filename: &str, content: Vec<u8>, _chapter: usize) -> Result<Box<dyn Book>> {
		let content = html_content(content)?;
		Ok(Box::new(HtmlBook { content }))
	}
}

impl Book for HtmlBook {
	fn lines(&self) -> &Vec<Line> {
		&self.content.lines
	}

	fn link_position(&mut self, line: usize, link_index: usize) -> Option<TraceInfo> {
		let text = &self.content.lines.get(line)?;
		let link = text.link_at(link_index)?;
		let link_target = link.target.as_str();
		let mut split = link_target.split('#');
		split.next()?;
		let anchor = split.next()?;
		let position = self.content.id_map.get(anchor)?;
		Some(TraceInfo { chapter: 0, line: position.line, position: position.position })
	}
}