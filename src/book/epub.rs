use anyhow::{anyhow, bail, Result};
use elsa::FrozenMap;
use indexmap::IndexSet;
use serde_derive::Deserialize;
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::io::Read;
use std::io::Seek;
use std::path::PathBuf;
use std::str::FromStr;
use zip::ZipArchive;

use crate::book::{Book, ChapterError, ImageData, Line, Loader, LoadingChapter, TocInfo};
use crate::common::TraceInfo;
use crate::config::{BookLoadingInfo, ReadingInfo};
#[cfg(feature = "gui")]
use crate::gui::HtmlFonts;
#[cfg(feature = "gui")]
use crate::html_parser::BlockStyle;
use crate::html_parser::{HtmlContent, HtmlParseOptions, HtmlResolver};
use crate::list::ListIterator;
use crate::xhtml::xhtml_to_html;
use crate::{frozen_map_get, html_parser};

struct ManifestItem {
	#[allow(unused)]
	id: String,
	href: String,
	media_type: String,
	properties: Option<String>,
}

type ItemId = String;
type Manifest = HashMap<ItemId, ManifestItem>;
type Spine = Vec<ItemId>;

#[allow(unused)]
struct ContentOPF {
	pub title: String,
	pub author: Option<String>,
	pub language: String,
	pub manifest: Manifest,
	pub spine: Spine,
	pub toc_id: Option<String>,
}

struct NavPoint {
	#[allow(unused)]
	pub id: Option<String>,
	pub label: Option<String>,
	#[allow(unused)]
	pub play_order: Option<usize>,
	pub level: usize,
	pub src_file: Option<String>,
	pub src_anchor: Option<String>,
	first_chapter_index: usize,
}

type Chapter = HtmlContent;

trait EpubArchive {
	fn is_encrypted(&self) -> bool;
	fn content(&self, path: &str) -> Result<Vec<u8>>;
	fn string(&self, path: &str) -> Result<String>
	{
		let buf = self.content(path)?;
		Ok(String::from_utf8(buf)?)
	}
	fn exists(&self, path: &str) -> bool;
}

struct EpubZipArchive<R: Read + Seek> {
	zip: RefCell<ZipArchive<R>>,
}

impl<R: Read + Seek> EpubZipArchive<R> {
	#[inline]
	fn new(reader: R) -> Result<Self>
	{
		let zip = ZipArchive::new(reader)?;
		Ok(EpubZipArchive { zip: RefCell::new(zip) })
	}
}

impl<R: Read + Seek> EpubArchive for EpubZipArchive<R> {
	#[inline]
	fn is_encrypted(&self) -> bool
	{
		self.zip.borrow().file_names().find(|f| *f == "META-INF/encryption.xml").is_some()
	}

	fn content(&self, path: &str) -> Result<Vec<u8>>
	{
		match self.zip.borrow_mut().by_name(path) {
			Ok(mut file) => {
				let mut buf = vec![];
				file.read_to_end(&mut buf)?;
				Ok(buf)
			}
			Err(e) => Err(anyhow!("failed load {}: {}", path, e.to_string())),
		}
	}

	fn exists(&self, path: &str) -> bool
	{
		self.zip.borrow().index_for_name(path).is_some()
	}
}

struct EpubExtractedArchive {
	root: PathBuf,
}

impl EpubExtractedArchive {
	#[inline]
	fn new(filename: &str) -> Result<Self>
	{
		let mut root = PathBuf::from_str(filename)?;
		if !root.pop() {
			bail!("Invalid Extracted epub path");
		}
		if !root.pop() {
			bail!("Invalid Extracted epub path");
		}
		// needed?
		if !root.exists() {
			bail!("Extracted epub root not exists");
		}
		Ok(EpubExtractedArchive { root })
	}

	#[inline]
	fn target(&self, path: &str) -> PathBuf
	{
		let names = path.split(|c| c == '/');
		let mut target = self.root.clone();
		for name in names {
			target.push(name);
		}
		target
	}
}

impl EpubArchive for EpubExtractedArchive {
	#[inline]
	fn is_encrypted(&self) -> bool
	{
		self.exists("META-INF/encryption.xml")
	}

	#[inline]
	fn content(&self, path: &str) -> Result<Vec<u8>>
	{
		Ok(fs::read(self.target(path))?)
	}

	#[inline]
	fn string(&self, path: &str) -> Result<String>
	{
		Ok(fs::read_to_string(self.target(path))?)
	}

	#[inline]
	fn exists(&self, path: &str) -> bool
	{
		self.target(path).exists()
	}
}

struct EpubBook {
	archive: Box<dyn EpubArchive>,
	content_opf: ContentOPF,
	toc: Vec<NavPoint>,
	chapter_cache: HashMap<usize, Chapter>,
	css_cache: FrozenMap<String, String>,
	images: FrozenMap<String, Vec<u8>>,
	font_families: IndexSet<String>,
	chapter_index: usize,
	#[cfg(feature = "gui")]
	fonts: HtmlFonts,
	custom_style: Option<String>,
}

pub struct EpubLoader {
	extensions: Vec<&'static str>,
}

impl EpubLoader {
	#[inline]
	pub(crate) fn new() -> Self
	{
		let extensions = vec![".epub", ".xml"];
		EpubLoader { extensions }
	}
}

impl Loader for EpubLoader {
	fn extensions(&self) -> &Vec<&'static str>
	{
		&self.extensions
	}

	fn support(&self, filename: &str) -> bool
	{
		if filename.to_lowercase().ends_with(".epub") {
			return true;
		}
		if filename.ends_with("META-INF/container.xml") {
			return true;
		}
		false
	}

	#[inline]
	fn load_file(&self, filename: &str, file: fs::File,
		loading_chapter: LoadingChapter, loading: BookLoadingInfo)
		-> Result<(Box<dyn Book>, ReadingInfo)>
	{
		let archive: Box<dyn EpubArchive> = if filename.to_lowercase().ends_with(".epub") {
			Box::new(EpubZipArchive::new(file)?)
		} else {
			Box::new(EpubExtractedArchive::new(filename)?)
		};
		let reading = get_reading(loading);
		let book = EpubBook::new(archive, loading_chapter, &reading.custom_style)?;
		Ok((Box::new(book), reading))
	}

	fn load_buf(&self, filename: &str, content: Vec<u8>,
		loading_chapter: LoadingChapter, loading: BookLoadingInfo)
		-> Result<(Box<dyn Book>, ReadingInfo)>
	{
		if !filename.to_lowercase().ends_with(".epub") {
			bail!("Not support extracted epub in other container.")
		}
		let archive = EpubZipArchive::new(Cursor::new(content))?;
		let reading = get_reading(loading);
		let book = EpubBook::new(Box::new(archive), loading_chapter, &reading.custom_style)?;
		Ok((Box::new(book), reading))
	}
}

impl Book for EpubBook {
	#[inline]
	fn name(&self) -> Option<&str>
	{
		Some(&self.content_opf.title)
	}

	#[inline]
	fn chapter_count(&self) -> usize
	{
		self.content_opf.spine.len()
	}

	fn prev_chapter(&mut self) -> Result<Option<usize>>
	{
		let mut current = self.chapter_index;
		loop {
			if current == 0 {
				return Ok(None);
			} else {
				current -= 1;
				let chapter = self.load_chapter(current)?;
				let lines_count = chapter.lines().len();
				if lines_count > 0 {
					self.chapter_index = current;
					return Ok(Some(current));
				}
			}
		}
	}

	fn goto_chapter(&mut self, chapter_index: usize) -> Result<Option<usize>>
	{
		let mut current = chapter_index;
		let chapter_count = self.chapter_count();
		loop {
			if current >= chapter_count {
				return Ok(None);
			} else {
				let chapter = self.load_chapter(current)?;
				let lines_count = chapter.lines().len();
				if lines_count > 0 {
					self.chapter_index = current;
					return Ok(Some(current));
				}
			}
			current += 1;
		}
	}

	#[inline]
	fn current_chapter(&self) -> usize
	{
		self.chapter_index
	}

	fn title(&self, line: usize, offset: usize) -> Option<&str> {
		let toc_index = self.toc_index(line, offset);
		let toc = self.toc.get(toc_index)?;
		Some(toc_title(toc))
	}

	fn toc_index(&self, line: usize, offset: usize) -> usize
	{
		self.chapter_cache
			.get(&self.chapter_index)
			.map_or(0, |c| {
				let toc = &self.toc;
				let len = toc.len();
				if len == 0 {
					return 0;
				}
				let mut file_matched = None;
				let spine = &self.content_opf.spine[self.chapter_index];
				let manifest = &self.content_opf.manifest[spine];
				let chapter_href = &manifest.href;
				for toc_index in 0..len {
					let np = &toc[toc_index];
					match &np.src_file {
						Some(src_file) if chapter_href == src_file => {
							if let Some(anchor) = &np.src_anchor {
								if let Some(position) = c.id_position(anchor) {
									if position.line > line || (position.line == line && position.offset > offset) {
										break;
									}
								}
							}
							file_matched = Some(toc_index);
						}
						_ => if np.first_chapter_index <= self.chapter_index {
							file_matched = Some(toc_index);
						}
					}
				}
				if let Some(the_last_index_found) = file_matched {
					return the_last_index_found;
				}
				0
			})
	}

	fn toc_iterator(&self) -> Option<Box<dyn Iterator<Item=TocInfo> + '_>>
	{
		let iter = ListIterator::new(|index| {
			let toc = self.toc.get(index)?;
			Some(TocInfo { title: toc_title(toc), index, level: toc.level })
		});
		Some(Box::new(iter))
	}

	fn toc_position(&mut self, toc_index: usize) -> Option<TraceInfo>
	{
		let np = self.toc.get(toc_index)?;
		let src_file = np.src_file.as_ref()?.to_string();
		let src_anchor = np.src_anchor.clone();
		self.target_position(Some(&src_file), src_anchor)
	}

	#[inline]
	fn lines(&self) -> &Vec<Line>
	{
		&self.chapter_cache.get(&self.chapter_index).unwrap().lines()
	}

	fn link_position(&mut self, line: usize, link_index: usize) -> Option<TraceInfo>
	{
		let full_path = chapter_path(self.chapter_index, &self.content_opf).ok()?;
		let cwd = path_cwd(full_path);
		let chapter = self.chapter_cache.get(&self.chapter_index)?;
		let text = &chapter.lines().get(line)?;
		let link = text.link_at(link_index)?;
		let link_target = link.target;

		let mut target_split = link_target.split('#');
		let target_file = target_split.next()?;
		let target_anchor = target_split.next().and_then(|a| Some(String::from(a))).or(None);
		if target_file.is_empty() {
			self.target_position(None, target_anchor)
		} else {
			let path = concat_path_str(cwd, target_file)?;
			self.target_position(Some(&path), target_anchor)
		}
	}

	fn image<'h>(&'h self, href: &'h str) -> Option<ImageData<'h>>
	{
		if let Ok(path) = chapter_path(self.current_chapter(), &self.content_opf) {
			let cwd = path_cwd(path);
			let full_path = concat_path_str(cwd, href)?;
			let bytes = frozen_map_get!(self.images, full_path, true, ||{
				self.archive.content(&full_path).ok()
			})?;
			Some(ImageData::Borrowed((Cow::Owned(full_path), bytes)))
		} else {
			None
		}
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
	fn custom_fonts(&self) -> Option<&HtmlFonts> {
		if self.fonts.has_faces() {
			Some(&self.fonts)
		} else {
			None
		}
	}

	#[cfg(feature = "gui")]
	#[inline]
	fn style_customizable(&self) -> bool
	{
		true
	}

	#[cfg(feature = "gui")]
	#[inline]
	fn block_styles(&self) -> Option<&Vec<BlockStyle>>
	{
		self.chapter_cache
			.get(&self.current_chapter())?
			.block_styles()
	}
}

struct EpubResolver<'a> {
	cwd: PathBuf,
	archive: &'a dyn EpubArchive,
	css_cache: &'a FrozenMap<String, String>,
	custom_style: Option<&'a str>,
}

impl<'a> HtmlResolver for EpubResolver<'a>
{
	#[inline]
	fn cwd(&self) -> PathBuf
	{
		self.cwd.clone()
	}

	#[inline]
	fn resolve(&self, path: &PathBuf, sub: &str) -> PathBuf
	{
		let cwd = path.clone();
		concat_path(cwd, sub)
	}

	fn css(&self, sub: &str) -> Option<(PathBuf, &str)>
	{
		let mut full_path = concat_path(self.cwd.clone(), sub);
		let path = path_str(&full_path)?;
		let content = frozen_map_get!(self.css_cache, path, || {
			self.archive.string( &path).ok()
		})?;
		full_path.pop();
		Some((full_path, content))
	}

	fn custom_style(&self) -> Option<&str>
	{
		self.custom_style
	}
}

/// epub container.xml
#[derive(Deserialize)]
struct RootFile<'a> {
	#[serde(rename = "@full-path")]
	full_path: Cow<'a, str>,
	#[allow(unused)]
	#[serde(borrow, rename = "@media-type")]
	media_type: Cow<'a, str>,
}
#[derive(Deserialize)]
struct RootFiles<'a> {
	#[serde(borrow)]
	rootfile: Vec<RootFile<'a>>,
}
#[derive(Deserialize)]
struct EpubContainer<'a> {
	#[serde(borrow)]
	rootfiles: RootFiles<'a>,
}

/// epub content.opf
#[derive(Deserialize)]
struct EpubContentOpfMetadata<'a> {
	#[serde(borrow, default, rename = "title")]
	title: Vec<Cow<'a, str>>,
	#[serde(borrow, default, rename = "creator")]
	creator: Vec<Cow<'a, str>>,
	#[serde(borrow, rename = "language")]
	language: Option<Cow<'a, str>>,
}
#[derive(Deserialize)]
struct EpubContentOpfManifestItem<'a> {
	#[serde(borrow, rename = "@id")]
	id: Cow<'a, str>,
	#[serde(borrow, rename = "@media-type")]
	media_type: Cow<'a, str>,
	#[serde(borrow, rename = "@href")]
	href: Cow<'a, str>,
	#[serde(borrow, rename = "@properties")]
	properties: Option<Cow<'a, str>>,
}
#[derive(Deserialize)]
struct EpubContentOpfManifest<'a> {
	#[serde(borrow, rename = "item")]
	items: Vec<EpubContentOpfManifestItem<'a>>,
}
#[derive(Deserialize)]
struct EpubContentOpfSpineItem<'a> {
	#[serde(borrow, rename = "@idref")]
	idref: Cow<'a, str>,
}
#[derive(Deserialize)]
struct EpubContentOpfSpine<'a> {
	#[serde(borrow, rename = "@toc")]
	toc: Option<Cow<'a, str>>,
	#[serde(borrow, rename = "itemref")]
	itemrefs: Vec<EpubContentOpfSpineItem<'a>>,
}
#[derive(Deserialize)]
struct EpubContentOpf<'a> {
	#[serde(borrow)]
	metadata: EpubContentOpfMetadata<'a>,
	#[serde(borrow)]
	manifest: EpubContentOpfManifest<'a>,
	#[serde(borrow)]
	spine: EpubContentOpfSpine<'a>,
}

/// epub toc.ncx
#[derive(Deserialize)]
struct EpubContentNcxNavLabel<'a> {
	// #[serde(borrow, rename="text")]
	text: Cow<'a, str>,
}
#[derive(Deserialize)]
struct EpubContentNcxNavContent<'a> {
	#[serde(borrow, rename = "@src")]
	src: Cow<'a, str>,
}
#[derive(Deserialize)]
struct EpubContentNcxNavPoint<'a> {
	#[serde(borrow, rename = "@id")]
	id: Cow<'a, str>,
	#[serde(rename = "@playOrder")]
	play_order: Option<Cow<'a, str>>,
	#[serde(rename = "navLabel")]
	nav_label: Option<EpubContentNcxNavLabel<'a>>,
	#[serde(borrow)]
	content: EpubContentNcxNavContent<'a>,
	#[serde(borrow, rename = "navPoint")]
	nav_points: Option<Vec<EpubContentNcxNavPoint<'a>>>,
}
#[derive(Deserialize)]
struct EpubContentNcxNavMap<'a> {
	#[serde(borrow, rename = "navPoint")]
	nav_points: Vec<EpubContentNcxNavPoint<'a>>,
}
#[derive(Deserialize)]
struct EpubContentNcx<'a> {
	#[serde(borrow, rename = "navMap")]
	nav_map: EpubContentNcxNavMap<'a>,
}

/// epub3 navigation document
#[derive(Deserialize)]
struct EpubContentNcxNavA<'a> {
	#[serde(borrow, rename = "@href")]
	href: Option<Cow<'a, str>>,
	#[serde(borrow, rename = "@title")]
	title: Option<Cow<'a, str>>,
	#[serde(rename = "$value")]
	text: String,
}
#[derive(Deserialize)]
struct EpubContentNcxNavLi<'a> {
	#[serde(borrow)]
	a: Option<EpubContentNcxNavA<'a>>,
	#[serde(borrow)]
	span: Option<EpubContentNcxNavA<'a>>,
	#[serde(borrow, rename = "id")]
	ol: Option<EpubContentNcxNavOl<'a>>,
}
#[derive(Deserialize)]
struct EpubContentNcxNavOl<'a> {
	#[serde(borrow, rename = "li")]
	lis: Vec<EpubContentNcxNavLi<'a>>,
}
#[derive(Deserialize)]
struct Epub3NavDocNav<'a> {
	#[serde(borrow, rename = "@type")]
	nav_type: Cow<'a, str>,
	#[serde(borrow)]
	ol: Vec<EpubContentNcxNavOl<'a>>,
}
#[derive(Deserialize)]
struct Epub3NavDocBody<'a> {
	#[serde(borrow, rename = "nav")]
	navs: Vec<Epub3NavDocNav<'a>>,
}
#[derive(Deserialize)]
struct Epub3NavDoc<'a> {
	#[serde(borrow)]
	body: Epub3NavDocBody<'a>,
}

impl EpubBook {
	pub fn new(archive: Box<dyn EpubArchive>, loading_chapter: LoadingChapter,
		custom_style: &Option<String>) -> Result<Self>
	{
		if archive.is_encrypted() {
			return Err(anyhow!("Encrypted epub."));
		}
		let container_text = archive.string("META-INF/container.xml")?;
		let container = quick_xml::de::from_str::<EpubContainer>(&container_text)
			.map_err(|e| anyhow!("Malformatted container.xml file: {}", e.to_string()))?;
		let content_opf_path = &container
			.rootfiles
			.rootfile
			.get(0)
			.ok_or(anyhow!("invalid container.xml: no rootfile"))?
			.full_path;
		let content_opf_dir = match PathBuf::from(content_opf_path.as_ref()).parent() {
			Some(p) => p.to_path_buf(),
			None => PathBuf::new(),
		};
		let content_opf_text = archive.string(content_opf_path)?;
		let content_opf = quick_xml::de::from_str::<EpubContentOpf>(&content_opf_text)
			.map_err(|e| anyhow!("Malformatted content.opf file: {}", e.to_string()))?;
		let content_opf = setup_content_opf(content_opf, &content_opf_dir, archive.as_ref())?;

		let mut toc = match content_opf.manifest.get(content_opf.toc_id.as_ref().unwrap_or(&"ncx".to_string())) {
			Some(ManifestItem { href, .. }) => {
				let ncx_text = archive.string(href)?;
				let cwd = path_cwd(href);
				parse_ncx(&ncx_text, &cwd)?
			}
			None => {
				let mut toc = None;
				for (_id, item) in &content_opf.manifest {
					if let Some(properties) = &item.properties {
						if properties.contains("nav") {
							let nav_text = archive.string(&item.href)?;
							let cwd = path_cwd(&item.href);
							toc = Some(parse_nav_doc(&nav_text, &cwd)?);
							break;
						}
					}
				}
				toc.ok_or(anyhow!("Invalid content.opf file, no ncx or nav"))?
			}
		};

		let chapter_count = content_opf.spine.len();

		let mut chapter_index = 0;
		for np in &mut toc {
			if let Some(src_file) = &np.src_file {
				for i in chapter_index..chapter_count {
					let spine = &content_opf.spine[i];
					let manifest = &content_opf.manifest[spine];
					let chapter_href = &manifest.href;
					if chapter_href == src_file {
						np.first_chapter_index = i;
						chapter_index = i;
						break;
					}
				}
			}
		}

		let mut chapter_index = match loading_chapter {
			LoadingChapter::Index(index) => index,
			LoadingChapter::Last => chapter_count - 1,
		};
		if chapter_index >= chapter_count {
			chapter_index = chapter_count - 1;
		}
		let chapter_cache = HashMap::new();
		let mut book = EpubBook {
			archive,
			content_opf,
			toc,
			chapter_cache,
			chapter_index,
			css_cache: Default::default(),
			images: Default::default(),
			font_families: Default::default(),
			#[cfg(feature = "gui")]
			fonts: HtmlFonts::new(),
			custom_style: custom_style.clone(),
		};
		book.load_chapter(chapter_index)?;
		Ok(book)
	}

	fn load_chapter(&mut self, chapter_index: usize) -> Result<&Chapter>
	{
		let chapter = match self.chapter_cache.entry(chapter_index) {
			Entry::Occupied(o) => o.into_mut(),
			Entry::Vacant(v) => {
				let full_path = chapter_path(chapter_index, &self.content_opf)?;
				let cwd = path_cwd(full_path);
				let mut html_str = self.archive.string(full_path)?;
				if full_path.to_lowercase().ends_with(".xhtml") {
					html_str = xhtml_to_html(&html_str)?;
				}
				let mut resolve = EpubResolver {
					cwd,
					archive: self.archive.as_ref(),
					css_cache: &self.css_cache,
					custom_style: self.custom_style.as_ref().map(|s| s.as_ref()),
				};
				#[allow(unused)]
				let (html_content, mut font_faces) = html_parser::parse(HtmlParseOptions::new(&html_str)
					.with_font_family(&mut self.font_families)
					.with_resolver(&mut resolve))?;
				#[cfg(feature = "gui")]
				{
					self.fonts.reload(font_faces, |path| {
						let path_str = path_str(path)?;
						let content = self.archive.content(&path_str).ok()?;
						Some(content)
					});
				}
				v.insert(html_content)
			}
		};
		Ok(chapter)
	}

	fn target_position(&mut self, target_file: Option<&str>, target_anchor: Option<String>) -> Option<TraceInfo>
	{
		fn target_position_in_chapter(chapter_index: usize, chapter: &Chapter, target_anchor: &Option<String>) -> Option<TraceInfo> {
			if let Some(anchor) = target_anchor {
				if let Some(position) = chapter.id_position(anchor) {
					return Some(TraceInfo {
						chapter: chapter_index,
						line: position.line,
						offset: position.offset,
					});
				}
			}
			None
		}
		if let Some(target_file) = target_file {
			for (chapter_index, item_id) in self.content_opf.spine.iter().enumerate() {
				let manifest = self.content_opf.manifest.get(item_id)?;
				if target_file == manifest.href {
					let chapter = self.load_chapter(chapter_index).ok()?;
					return match target_position_in_chapter(chapter_index, chapter, &target_anchor) {
						Some(ti) => Some(ti),
						None => Some(TraceInfo {
							chapter: chapter_index,
							line: 0,
							offset: 0,
						})
					};
				}
			}
			None
		} else {
			let chapter_index = self.current_chapter();
			let chapter = self.load_chapter(chapter_index).ok()?;
			target_position_in_chapter(chapter_index, chapter, &target_anchor)
		}
	}
}

fn setup_nav_points(nav_points_element: &Vec<EpubContentNcxNavPoint>, level: usize, nav_points: &mut Vec<NavPoint>, cwd: &PathBuf)
{
	fn setup_point(point: &EpubContentNcxNavPoint, level: usize, cwd: &PathBuf) -> Option<NavPoint> {
		let id = Some(point.id.to_string());
		let label = point
			.nav_label
			.as_ref()
			.map(|l| l.text.to_string());
		let play_order = point
			.play_order
			.as_ref()
			.and_then(|po| po.parse().ok());
		let src = point.content.src.to_string();
		let mut src_split = src.split('#');
		let src_file = src_split.next()?;
		let src_file = concat_path_str(cwd.clone(), src_file)?;
		let src_file = Some(src_file);
		let src_anchor = src_split.next().and_then(|str| Some(String::from(str)));
		Some(NavPoint {
			id,
			label,
			play_order,
			level,
			src_file,
			src_anchor,
			first_chapter_index: 0,
		})
	}
	for point in nav_points_element {
		if let Some(p) = setup_point(point, level, &cwd) {
			nav_points.push(p);
			if let Some(points) = &point.nav_points {
				setup_nav_points(points, level + 1, nav_points, cwd)
			}
		}
	}
}

fn parse_ncx(text: &str, cwd: &PathBuf) -> Result<Vec<NavPoint>>
{
	let ncx: EpubContentNcx = quick_xml::de::from_str(text)
		.map_err(|e| anyhow!("Failed parse ncx: {}", e.to_string()))?;

	let mut nav_points = vec![];
	setup_nav_points(&ncx.nav_map.nav_points, 1, &mut nav_points, cwd);
	if nav_points.len() == 0 {
		Err(anyhow!("No NavPoints found"))
	} else {
		Ok(nav_points)
	}
}

fn parse_nav_doc(text: &str, cwd: &PathBuf) -> Result<Vec<NavPoint>>
{
	fn process(ol: EpubContentNcxNavOl, toc: &mut Vec<NavPoint>, level: usize, cwd: &PathBuf) -> Result<()>
	{
		for li in ol.lis {
			// li
			//     In this order:
			//         (span or a) [exactly 1]
			//         ol [conditionally required]
			let tag = if let Some(a) = li.a {
				a
			} else if let Some(span) = li.span {
				span
			} else {
				bail!("Navigation document entry with no text");
			};
			let label = if let Some(l) = tag.title {
				l.to_string()
			} else {
				tag.text
			};
			if label.len() == 0 {
				bail!("Navigation document entry with empty text");
			}
			let (src_file, src_anchor) = if let Some(href) = tag.href {
				// In the case of the toc nav, landmarks nav and page-list nav, it MUST resolve to an Top-level Content Document or fragment therein.
				let mut parts = href.split('#');
				let src_file = parts.next()
					.map(|a| a.to_string())
					.ok_or(anyhow!("Navigation document entry href not resolve to an Top-level Content Document or fragment therein"))?;
				let src_file = concat_path_str(cwd.clone(), &src_file);
				let src_anchor = parts.next().map(|a| a.to_string());
				(src_file, src_anchor)
			} else {
				(None, None)
			};

			toc.push(NavPoint {
				id: None,
				label: Some(label),
				play_order: None,
				level,
				src_file,
				src_anchor,
				first_chapter_index: 0,
			});
			if let Some(ol) = li.ol {
				process(ol, toc, level + 1, cwd)?;
			}
		}
		Ok(())
	}
	let doc: Epub3NavDoc = quick_xml::de::from_str(text)
		.map_err(|_e| anyhow!("Failed parse Navigation document"))?;
	let mut toc = vec![];
	for nav in doc.body.navs {
		if nav.nav_type == "toc" {
			for ol in nav.ol {
				process(ol, &mut toc, 1, cwd)?;
			}
			break;
		}
	}
	if toc.len() == 0 {
		Err(anyhow!("Navigation document with no entries"))
	} else {
		Ok(toc)
	}
}

fn setup_manifest(manifest: EpubContentOpfManifest, path: &PathBuf) -> Manifest
{
	let mut map = HashMap::new();
	for item in manifest.items {
		if let Some(href) = concat_path_str(path.clone(), &item.href) {
			let id = item.id.to_string();
			map.insert(id.clone(), ManifestItem {
				id,
				href,
				media_type: item.media_type.to_string(),
				properties: item.properties.map(|p| p.to_string()),
			});
		}
	}
	map
}

#[inline]
fn setup_spine(spine: EpubContentOpfSpine, manifest: &Manifest, archive: &dyn EpubArchive) -> (Spine, Option<String>)
{
	let mut chapters = vec![];
	for item in spine.itemrefs {
		let id = item.idref.to_string();
		if let Some(item) = manifest.get(&id) {
			if archive.exists(&item.href as &str) {
				chapters.push(id);
			}
		}
	}
	let toc_id = spine.toc.map(|toc| toc.to_string());
	(chapters, toc_id)
}

fn setup_content_opf(opf: EpubContentOpf, content_opf_dir: &PathBuf, archive: &dyn EpubArchive) -> Result<ContentOPF>
{
	let title = opf.metadata
		.title
		.get(0)
		.ok_or(anyhow!("No title defined"))?.to_string();
	let author = opf.metadata
		.creator
		.get(0)
		.map(|a| a.to_string());
	let language = opf.metadata.language.unwrap_or(Cow::Borrowed("")).to_string();

	let manifest = setup_manifest(opf.manifest, content_opf_dir);
	let (spine, toc_id) = setup_spine(opf.spine, &manifest, archive);
	Ok(ContentOPF {
		title,
		author,
		language,
		manifest,
		spine,
		toc_id,
	})
}

fn toc_title(nav_point: &NavPoint) -> &str {
	let label = match &nav_point.label {
		Some(label) => label,
		None => match &nav_point.src_file {
			Some(src_file) => src_file,
			None => "blank",
		}
	};
	label
}

fn chapter_path(chapter_index: usize, content_opf: &ContentOPF) -> Result<&str>
{
	let spine = content_opf.spine
		.get(chapter_index)
		.ok_or(ChapterError::anyhow("invalid index".to_string()))?;
	let item = content_opf.manifest
		.get(spine)
		.ok_or(ChapterError::anyhow(format!("Invalid ref id: {}", spine)))?;
	if item.media_type != "application/xhtml+xml" {
		return Err(ChapterError::anyhow(format!("Referenced content for {} is not valid.", spine)));
	}
	Ok(&item.href)
}

fn concat_path(mut path: PathBuf, mut sub_path: &str) -> PathBuf
{
	while sub_path.starts_with("../") {
		path.pop();
		sub_path = &sub_path[3..];
	}
	#[cfg(windows)]
	let sub_path = &sub_path.replace("/", "\\");
	path.push(sub_path);
	path
}

fn path_str(path: &PathBuf) -> Option<String>
{
	let str = path.to_str()?;
	#[cfg(windows)]
	return Some(str.replace("\\", "/"));
	#[cfg(not(windows))]
	Some(str.to_owned())
}

#[inline]
fn concat_path_str(path: PathBuf, sub_path: &str) -> Option<String>
{
	path_str(&concat_path(path, sub_path))
}

#[inline]
fn path_cwd(path: &str) -> PathBuf
{
	let mut cwd = PathBuf::from(path);
	cwd.pop();
	cwd
}

#[inline]
fn get_reading(loading: BookLoadingInfo) -> ReadingInfo
{
	#[cfg(not(feature = "gui"))]
	{ loading.get() }
	#[cfg(feature = "gui")]
	loading.get_or_init(|reading| {
		reading.custom_color = true;
		reading.custom_font = true;
	})
}
