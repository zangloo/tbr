use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;
use anyhow::{anyhow, Result};
use elsa::FrozenMap;
use indexmap::IndexSet;

use crate::book::{Book, LoadingChapter, Line, Loader};
use crate::html_convertor::{html_content, html_str_content, HtmlContent};
use crate::common::{plain_text, TraceInfo};
use crate::config::{BookLoadingInfo, ReadingInfo};
use crate::frozen_map_get;
#[cfg(feature = "gui")]
use crate::gui::HtmlFonts;
use crate::xhtml::xhtml_to_html;

pub(crate) struct HtmlLoader {
	extensions: Vec<&'static str>,
}

pub(crate) struct HtmlBook {
	content: HtmlContent,
	font_families: IndexSet<String>,
	#[cfg(feature = "gui")]
	fonts: HtmlFonts,
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

	fn load_file(&self, _filename: &str, mut file: fs::File,
		_loading_chapter: LoadingChapter, loading: BookLoadingInfo)
		-> Result<(Box<dyn Book>, ReadingInfo)>
	{
		let filename = loading.filename();
		let path = PathBuf::from_str(filename)?;
		let cwd = path.parent()
			.ok_or(anyhow!("Failed get parent of {:#?}", path))?;
		let mut content: Vec<u8> = Vec::new();
		file.read_to_end(&mut content)?;
		let mut font_families = IndexSet::new();
		let mut text = plain_text(content, false)?;
		if filename.to_lowercase().ends_with(".xhtml") {
			text = xhtml_to_html(&text)?;
		}
		let stylesheets: FrozenMap<String, String> = Default::default();
		#[allow(unused)]
			let (content, mut font_faces) = html_str_content(&text, &mut font_families, Some(|path: &str| {
			let path = cwd.join(&path);
			let full_path = path.canonicalize().ok()?.to_str()?.to_string();
			frozen_map_get!(stylesheets, full_path, || {
				fs::read_to_string(&path).ok()
			})
		}))?;
		#[cfg(feature = "gui")]
			let book = {
			// make source url to full path
			// will used as key for access
			for face in &mut font_faces {
				for source in &mut face.sources {
					if let Some(full_path) = cwd.join(&source).to_str() {
						*source = full_path.to_string();
					}
				}
			}
			let mut fonts = HtmlFonts::new();
			fonts.reload(font_faces, |path| {
				let path = PathBuf::from_str(path).ok()?;
				let content = fs::read(path).ok()?;
				Some(content)
			});
			HtmlBook {
				content,
				font_families,
				fonts,
			}
		};
		#[cfg(not(feature = "gui"))]
			let book = HtmlBook {
			content,
			font_families,
		};
		let reading = book.get_reading(loading);
		Ok((
			Box::new(book),
			reading
		))
	}

	fn load_buf(&self, _filename: &str, content: Vec<u8>,
		_loading_chapter: LoadingChapter, loading: BookLoadingInfo)
		-> Result<(Box<dyn Book>, ReadingInfo)>
	{
		let mut font_families = IndexSet::new();
		let text = plain_text(content, false)?;
		let content = html_content(&text, &mut font_families)?;
		let book = HtmlBook {
			content,
			font_families,
			#[cfg(feature = "gui")]
			fonts: HtmlFonts::new(),
		};
		let reading = book.get_reading(loading);
		Ok((
			Box::new(book),
			reading,
		))
	}
}

impl Book for HtmlBook {
	#[inline]
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
	#[inline]
	fn color_customizable(&self) -> bool
	{
		true
	}

	#[cfg(feature = "gui")]
	#[inline]
	fn fonts_customizable(&self) -> bool
	{
		true
	}

	#[cfg(feature = "gui")]
	#[inline]
	fn custom_fonts(&self) -> Option<&HtmlFonts>
	{
		if self.fonts.has_faces() {
			Some(&self.fonts)
		} else {
			None
		}
	}
}

impl HtmlBook {
	#[inline]
	fn get_reading(&self, loading: BookLoadingInfo) -> ReadingInfo
	{
		loading.get_or_init(|reading|
			reading.custom_color = true
		)
	}
}