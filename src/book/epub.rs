use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::io::Read;
use std::io::Seek;
use std::path::PathBuf;
use anyhow::{anyhow, Result};
use elsa::FrozenMap;
use indexmap::IndexSet;
use strip_bom::StripBom;
use xmltree::{Element, XMLNode};
use zip::ZipArchive;

use crate::book::{Book, LoadingChapter, ChapterError, Line, Loader, TocInfo};
use crate::html_convertor::html_str_content;
use crate::list::ListIterator;
use crate::common::{Position, TraceInfo};

struct ManifestItem {
	#[allow(dead_code)]
	id: String,
	href: String,
	media_type: String,
	#[allow(dead_code)]
	properties: Option<String>,
}

type ItemId = String;
type Manifest = HashMap<ItemId, ManifestItem>;
type Spine = Vec<ItemId>;

#[allow(dead_code)]
struct ContentOPF {
	pub title: String,
	pub author: Option<String>,
	pub language: String,
	pub manifest: Manifest,
	pub spine: Spine,
	pub toc_id: Option<String>,
}

struct NavPoint {
	#[allow(dead_code)]
	pub id: Option<String>,
	pub label: Option<String>,
	#[allow(dead_code)]
	pub play_order: Option<usize>,
	pub level: usize,
	pub src_file: Option<String>,
	pub src_anchor: Option<String>,
	first_chapter_index: usize,
}

struct Chapter {
	lines: Vec<Line>,
	id_map: HashMap<String, Position>,
}

struct EpubBook<R: Read + Seek> {
	zip: RefCell<ZipArchive<R>>,
	content_opf: ContentOPF,
	toc: Vec<NavPoint>,
	chapter_cache: HashMap<usize, Chapter>,
	css_cache: HashMap<String, String>,
	images: FrozenMap<String, Vec<u8>>,
	font_families: IndexSet<String>,
	chapter_index: usize,
}

pub struct EpubLoader {
	extensions: Vec<&'static str>,
}

impl EpubLoader {
	#[inline]
	pub(crate) fn new() -> Self
	{
		let extensions = vec![".epub"];
		EpubLoader { extensions }
	}
}

impl Loader for EpubLoader {
	fn extensions(&self) -> &Vec<&'static str> {
		&self.extensions
	}

	#[inline]
	fn load_file(&self, _filename: &str, file: std::fs::File, loading_chapter: LoadingChapter) -> Result<Box<dyn Book>>
	{
		Ok(Box::new(EpubBook::new(file, loading_chapter)?))
	}

	fn load_buf(&self, _filename: &str, content: Vec<u8>, loading_chapter: LoadingChapter) -> Result<Box<dyn Book>>
	{
		Ok(Box::new(EpubBook::new(Cursor::new(content), loading_chapter)?))
	}
}

impl<'a, R: Read + Seek + 'static> Book for EpubBook<R> {
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
				let lines_count = chapter.lines.len();
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
				let lines_count = chapter.lines.len();
				if lines_count > 0 {
					self.chapter_index = current;
					return Ok(Some(current));
				}
			}
			current += 1;
		}
	}

	#[inline]
	fn current_chapter(&self) -> usize {
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
							file_matched = Some(toc_index);
							if let Some(anchor) = &np.src_anchor {
								if let Some(position) = c.id_map.get(anchor) {
									if position.line > line || (position.line == line && position.offset > offset) {
										break;
									}
								}
							}
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
			Some(TocInfo{title:toc_title(toc), index, level: toc.level })
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
		&self.chapter_cache.get(&self.chapter_index).unwrap().lines
	}

	fn link_position(&mut self, line: usize, link_index: usize) -> Option<TraceInfo>
	{
		let full_path = chapter_path(self.chapter_index, &self.content_opf).ok()?;
		let cwd = path_cwd(full_path);
		let chapter = self.chapter_cache.get(&self.chapter_index)?;
		let text = &chapter.lines.get(line)?;
		let link = text.link_at(link_index)?;
		let link_target = link.target;

		let mut target_split = link_target.split('#');
		let target_file = target_split.next()?;
		let target_anchor = target_split.next().and_then(|a| Some(String::from(a))).or(None);
		if target_file.is_empty() {
			self.target_position(None, target_anchor)
		} else {
			let path = concat_path(cwd, target_file)?;
			self.target_position(Some(&path), target_anchor)
		}
	}

	fn image<'h>(&self, href: &'h str) -> Option<(Cow<'h, str>, &[u8])>
	{
		if let Ok(path) = chapter_path(self.current_chapter(), &self.content_opf) {
			let cwd = path_cwd(path);
			let full_path = concat_path(cwd, href)?;
			let bytes = if let Some(bytes) = self.images.get(&full_path) {
				bytes
			} else {
				let bytes = zip_content(&mut self.zip.borrow_mut(), &full_path).ok()?;
				self.images.insert(full_path.clone(), bytes)
			};
			Some((Cow::Owned(full_path), bytes))
		} else {
			None
		}
	}

	#[inline]
	fn font_family_names(&self) -> Option<&IndexSet<String>>
	{
		Some(&self.font_families)
	}
}

impl<R: Read + Seek + 'static> EpubBook<R> {
	pub fn new(reader: R, loading_chapter: LoadingChapter) -> Result<Self>
	{
		let mut zip = ZipArchive::new(reader)?;
		if is_encrypted(&zip) {
			return Err(anyhow!("Encrypted epub."));
		}
		let container_text = zip_string(&mut zip, "META-INF/container.xml")?;
		let container = Element::parse(container_text.strip_bom().as_bytes())?;
		let rootfiles = container.get_child("rootfiles").ok_or(anyhow!("invalid container.xml: no rootfiles"))?;
		let rootfile = rootfiles.get_child("rootfile").ok_or(anyhow!("invalid container.xml: no rootfile"))?;
		let content_opf_path = rootfile.attributes.get("full-path").ok_or(anyhow!("invalid container.xml: no full-path"))?;
		let content_opf_dir = match PathBuf::from(&content_opf_path).parent() {
			Some(p) => p.to_path_buf(),
			None => PathBuf::new(),
		};
		let content_opf_text = zip_string(&mut zip, &content_opf_path)?;
		let content_opf = parse_content_opf(&content_opf_text, &content_opf_dir, &zip)
			.ok_or(anyhow!("Malformatted content.opf file"))?;

		let css_cache = load_cache(&mut zip, &content_opf.manifest);

		let mut toc = match content_opf.manifest.get(content_opf.toc_id.as_ref().unwrap_or(&"ncx".to_string())) {
			Some(ManifestItem { href, .. }) => {
				let ncx_text = zip_string(&mut zip, href)?;
				let cwd = path_cwd(href);
				parse_ncx(&ncx_text, &cwd)?
			}
			None => {
				let mut toc = None;
				for (_id, item) in &content_opf.manifest {
					if let Some(properties) = &item.properties {
						if properties.contains("nav") {
							let nav_text = zip_string(&mut zip, &item.href)?;
							let cwd = path_cwd(&item.href);
							toc = Some(parse_nav_doc(&nav_text, &cwd)?);
							break;
						}
					}
				}
				if let Some(toc) = toc {
					toc
				} else {
					return Err(anyhow!("Invalid content.opf file, no ncx or nav"));
				}
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
			zip: RefCell::new(zip),
			content_opf,
			toc,
			chapter_cache,
			chapter_index,
			css_cache,
			images: Default::default(),
			font_families: Default::default(),
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
				let html_str = zip_string(&mut self.zip.borrow_mut(), full_path)?;
				let css_cache = &self.css_cache;
				let html_content = html_str_content(&html_str, &mut self.font_families, Some(|path: String| {
					let full_path = concat_path(cwd.clone(), &path)?;
					let content = css_cache.get(&full_path)?;
					Some(content)
				}))?;
				let chapter = Chapter {
					lines: html_content.lines,
					id_map: html_content.id_map,
				};
				v.insert(chapter)
			}
		};
		Ok(chapter)
	}

	fn target_position(&mut self, target_file: Option<&str>, target_anchor: Option<String>) -> Option<TraceInfo>
	{
		fn target_position_in_chapter(chapter_index: usize, chapter: &Chapter, target_anchor: &Option<String>) -> Option<TraceInfo> {
			if let Some(anchor) = target_anchor {
				if let Some(position) = chapter.id_map.get(anchor) {
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

#[inline]
fn zip_string<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<String>
{
	let buf = zip_content(zip, name)?;
	Ok(String::from_utf8(buf)?)
}

#[inline]
fn zip_content<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<Vec<u8>>
{
	match zip.by_name(name) {
		Ok(mut file) => {
			let mut buf = vec![];
			file.read_to_end(&mut buf)?;
			Ok(buf)
		}
		Err(e) => Err(anyhow!("failed load {}: {}", name, e.to_string())),
	}
}


fn load_cache<R: Read + Seek>(zip: &mut ZipArchive<R>, manifest: &Manifest) -> HashMap<String, String>
{
	let mut css_cache = HashMap::new();
	for (_, item) in manifest {
		if item.media_type == "text/css" {
			if let Ok(content) = zip_string(zip, &item.href) {
				css_cache.insert(item.href.clone(), content);
			}
			continue;
		}
	}
	css_cache
}

fn parse_nav_points(nav_points_element: &Element, level: usize, nav_points: &mut Vec<NavPoint>, cwd: &PathBuf)
{
	fn parse_element(el: &Element, level: usize, cwd: &PathBuf) -> Option<NavPoint> {
		let id = Some(el.attributes.get("id")?.to_string());
		let play_order: Option<usize> = el
			.attributes
			.get("playOrder")
			.and_then(|po| po.parse().ok());
		let src = el.get_child("content")?.attributes.get("src")?.to_string();
		let mut src_split = src.split('#');
		let src_file = src_split.next()?;
		let src_file = concat_path(cwd.clone(), src_file)?;
		let src_file = Some(src_file);
		let src_anchor = src_split.next().and_then(|str| Some(String::from(str)));
		let label = el
			.get_child("navLabel")
			.and_then(|el| el.get_child("text"))
			.and_then(|el| el.get_text())
			.map(|s| s.to_string());
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
	nav_points_element
		.children
		.iter()
		.filter_map(|node| {
			if let Some(el) = node.as_element() {
				if el.name == "navPoint" {
					return Some(el);
				}
			}
			None
		})
		.for_each(|el| {
			if let Some(np) = parse_element(el, level, cwd) {
				nav_points.push(np);
				parse_nav_points(el, level + 1, nav_points, cwd);
			}
		});
}

fn parse_ncx(text: &str, cwd: &PathBuf) -> Result<Vec<NavPoint>>
{
	let ncx = Element::parse(text.strip_bom().as_bytes())
		.map_err(|_e| anyhow!("Failed parse ncx"))?;
	let nav_map = ncx
		.get_child("navMap")
		.ok_or_else(|| anyhow!("Missing navMap"))?;
	let mut nav_points = vec![];
	parse_nav_points(nav_map, 1, &mut nav_points, cwd);
	if nav_points.len() == 0 {
		Err(anyhow!("Could not parse NavPoints"))
	} else {
		Ok(nav_points)
	}
}

/// parse Navigation document
/// according to https://www.w3.org/publishing/epub3/epub-packages.html#sec-package-nav-def
fn parse_nav_doc(text: &str, cwd: &PathBuf) -> Result<Vec<NavPoint>>
{
	fn search_nav(element: &Element) -> Option<&Element>
	{
		for child in &element.children {
			if let Some(element) = child.as_element() {
				if element.name == "nav" && element.attributes.get("type").map_or(false, |t| t == "toc") {
					return Some(element);
				}
				let option = search_nav(element);
				if option.is_some() {
					return option;
				}
			}
		}
		None
	}
	fn process(children: &Vec<XMLNode>, toc: &mut Vec<NavPoint>, level: usize, cwd: &PathBuf) -> Result<()>
	{
		for child in children {
			match child {
				XMLNode::Element(Element { name, children, .. }) if name == "li" => {
					// li
					//     In this order:
					//         (span or a) [exactly 1]
					//         ol [conditionally required]
					let a = children.get(0).ok_or(anyhow!("Invalid entry in Navigation document"))?;
					let a = a.as_element().ok_or(anyhow!("Invalid entry node in Navigation document"))?;
					let label = match a.attributes.get("title") {
						Some(title) => title.clone(),
						None => a.get_text()
							.ok_or(anyhow!("Navigation document entry with no text"))?
							.to_string()
					};
					if label.len() == 0 {
						return Err(anyhow!("Navigation document entry with empty text"));
					}
					let (src_file, src_anchor) = if let Some(href) = a.attributes.get("href") {
						// In the case of the toc nav, landmarks nav and page-list nav, it MUST resolve to an Top-level Content Document or fragment therein.
						let mut parts = href.split('#');
						let src_file = parts.next()
							.map(|a| a.to_string())
							.ok_or(anyhow!("Navigation document entry href not resolve to an Top-level Content Document or fragment therein"))?;
						let src_file = concat_path(cwd.clone(), &src_file);
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
					if let Some(node) = children.get(1) {
						match node {
							XMLNode::Element(Element { name, children, .. }) if name == "ol" =>
								process(children, toc, level + 1, cwd)?,
							_ => {}
						}
					}
				}
				_ => {}
			}
		}
		Ok(())
	}
	let document = Element::parse(text.strip_bom().as_bytes())
		.map_err(|_e| anyhow!("Failed parse Navigation document"))?;
	let body = document.get_child("body").ok_or(anyhow!("Navigation document without body"))?;
	let nav = search_nav(body).ok_or(anyhow!("Navigation document without nav of toc"))?;
	let mut toc = vec![];
	for child in &nav.children {
		match child {
			XMLNode::Element(Element { name, children, .. }) if name == "ol" => {
				process(children, &mut toc, 1, cwd)?;
				break;
			}
			_ => {}
		}
	}
	if toc.len() == 0 {
		Err(anyhow!("Navigation document with no entries"))
	} else {
		Ok(toc)
	}
}

fn parse_manifest(manifest: &Element, path: &PathBuf) -> Manifest
{
	manifest
		.children
		.iter()
		.filter_map(|node| {
			if let Some(el) = node.as_element() {
				if el.name == "item" {
					let id = el.attributes.get("id")?.to_string();
					let href = el.attributes.get("href")?;
					let href = concat_path(path.clone(), href)?;
					return Some((
						id.clone(),
						ManifestItem {
							id,
							href,
							media_type: el.attributes.get("media-type")?.to_string(),
							properties: el.attributes.get("properties").map(|s| s.to_string()),
						},
					));
				}
			}
			None
		})
		.collect::<HashMap<ItemId, ManifestItem>>()
}

#[inline]
fn parse_spine<R: Read + Seek>(spine: &Element, manifest: &Manifest, zip: &ZipArchive<R>) -> Option<(Spine, Option<String>)>
{
	let file_names: HashSet<&str> = zip.file_names().collect();
	let chapters = spine.children
		.iter()
		.filter_map(|node| {
			if let Some(el) = node.as_element() {
				if el.name == "itemref" {
					let id = el.attributes.get("idref")?.to_string();
					let item = manifest.get(&id)?;
					if file_names.contains(&item.href as &str) {
						return Some(id);
					}
				}
			}
			None
		})
		.collect();
	let toc_id = spine.attributes.get("toc").map(|id| id.clone());
	Some((chapters, toc_id))
}

fn parse_content_opf<R: Read + Seek>(text: &str, content_opf_dir: &PathBuf, zip: &ZipArchive<R>) -> Option<ContentOPF>
{
	let package = Element::parse(text.strip_bom().as_bytes()).ok()?;
	let metadata = package.get_child("metadata")?;
	let manifest = package.get_child("manifest")?;
	let spine = package.get_child("spine")?;
	let title = metadata.get_child("title")?.get_text()?.to_string();
	let author = metadata
		.get_child("creator")
		.map(|el| el.get_text())
		.flatten()
		.map(|s| s.to_string());
	let language = metadata.get_child("language")
		.map_or(String::new(), |e| e.get_text()
			.map_or(String::new(), |s| s.to_string()));
	let manifest = parse_manifest(manifest, content_opf_dir);
	let (spine, toc_id) = parse_spine(spine, &manifest, zip)?;
	Some(ContentOPF {
		title,
		author,
		language,
		manifest,
		spine,
		toc_id,
	})
}

#[inline]
fn is_encrypted<R: Read + Seek>(zip: &ZipArchive<R>) -> bool
{
	zip.file_names().find(|f| *f == "META-INF/encryption.xml").is_some()
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

fn concat_path(mut path: PathBuf, mut sub_path: &str) -> Option<String>
{
	while sub_path.starts_with("../") {
		path.pop();
		sub_path = &sub_path[3..];
	}
	#[cfg(windows)]
		let sub_path = &sub_path.replace("/", "\\");
	path.push(sub_path);
	let str = path.to_str()?;
	#[cfg(windows)]
	return Some(str.replace("\\", "/"));
	#[cfg(not(windows))]
	Some(str.to_owned())
}

#[inline]
fn path_cwd(path: &str) -> PathBuf
{
	let mut cwd = PathBuf::from(path);
	cwd.pop();
	cwd
}