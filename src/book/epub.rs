use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::io::Read;
use std::io::Seek;
use std::path::PathBuf;
use anyhow::{anyhow, Result};
use regex::Regex;
use strip_bom::StripBom;
use xmltree::Element;
use zip::result::ZipError;
use zip::ZipArchive;

use crate::book::{Book, LoadingChapter, ChapterError, Line, Loader};
use crate::html_convertor::html_str_content;
use crate::list::ListEntry;
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
}

#[allow(dead_code)]
struct NavPoint {
	pub id: String,
	pub label: Option<String>,
	pub play_order: Option<usize>,
	pub level: usize,
	pub src_file: String,
	pub src_anchor: Option<String>,
}

struct Chapter {
	path: String,
	title: String,
	lines: Vec<Line>,
	id_map: HashMap<String, Position>,
	toc_index: usize,
}

struct EpubBook<R: Read + Seek> {
	zip: ZipArchive<R>,
	#[allow(dead_code)]
	content_opf_dir: PathBuf,
	#[allow(dead_code)]
	content_opf: ContentOPF,
	toc: Vec<NavPoint>,
	chapter_cache: HashMap<usize, Chapter>,
	css_cache: HashMap<String, String>,
	images: HashMap<String, Vec<u8>>,
	chapter_index: usize,
}

pub struct EpubLoader {
	extensions: Vec<&'static str>,
}

impl EpubLoader {
	pub(crate) fn new() -> Self {
		let extensions = vec![".epub"];
		EpubLoader { extensions }
	}
}

impl Loader for EpubLoader {
	fn extensions(&self) -> &Vec<&'static str> {
		&self.extensions
	}

	fn load_file(&self, _filename: &str, file: std::fs::File, loading_chapter: LoadingChapter) -> Result<Box<dyn Book>> {
		Ok(Box::new(EpubBook::new(file, loading_chapter)?))
	}

	fn load_buf(&self, _filename: &str, content: Vec<u8>, loading_chapter: LoadingChapter) -> Result<Box<dyn Book>>
	{
		Ok(Box::new(EpubBook::new(Cursor::new(content), loading_chapter)?))
	}
}

impl<'a, R: Read + Seek> Book for EpubBook<R> {
	fn chapter_count(&self) -> usize {
		self.content_opf.spine.len()
	}

	fn prev_chapter(&mut self) -> Result<Option<usize>> {
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

	fn goto_chapter(&mut self, chapter_index: usize) -> Result<Option<usize>> {
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

	fn current_chapter(&self) -> usize {
		self.chapter_index
	}

	fn title(&self) -> Option<&String> {
		Some(&self.chapter_cache.get(&self.chapter_index)?.title)
	}

	fn toc_index(&self) -> usize {
		self.chapter_cache
			.get(&self.chapter_index)
			.map_or(0, |c| c.toc_index)
	}

	fn toc_list(&self) -> Option<Vec<ListEntry>> {
		let mut list = vec![];
		for (index, np) in self.toc.iter().enumerate() {
			let title = toc_title(np);
			list.push(ListEntry::new(title, index));
		}
		Some(list)
	}

	fn toc_position(&mut self, toc_index: usize) -> Option<TraceInfo> {
		let np = self.toc.get(toc_index)?;
		let src_file = np.src_file.clone();
		let src_anchor = np.src_anchor.clone();
		self.target_position(&src_file, src_anchor)
	}

	fn lines(&self) -> &Vec<Line> {
		&self.chapter_cache.get(&self.chapter_index).unwrap().lines
	}

	fn link_position(&mut self, line: usize, link_index: usize) -> Option<TraceInfo> {
		let chapter = self.chapter_cache.get(&self.chapter_index).unwrap();
		let text = &chapter.lines.get(line)?;
		let link = text.link_at(link_index)?;
		let link_target = link.target;

		let mut current_path = PathBuf::from(&chapter.path);
		current_path.pop();
		let current_path = concat_path(current_path, link_target);
		let target = current_path.to_str()?;
		let mut target_split = target.split('#');
		let target_file = target_split.next()?;
		let target_anchor = target_split.next().and_then(|a| Some(String::from(a))).or(None);
		self.target_position(target_file, target_anchor)
	}

	fn image(&self, href: &str) -> Option<(String, &Vec<u8>)> {
		if let Ok((_full_path, cwd, _src_path)) = chapter_path(self.current_chapter(), &self.content_opf, &self.content_opf_dir) {
			resolve(&cwd.clone(), href, &self.images)
		} else {
			None
		}
	}
}

impl<R: Read + Seek> EpubBook<R> {
	pub fn new(reader: R, loading_chapter: LoadingChapter) -> Result<Self>
	{
		let mut zip = ZipArchive::new(reader)?;
		if is_encrypted(&zip) {
			return Err(anyhow!("Encrypted epub."));
		}
		let container_text = zip_string(&mut zip, "META-INF/container.xml")?;
		// TODO: make this more robust
		let content_opf_re = Regex::new(r#"rootfile full-path="(\S*)""#).unwrap();

		let content_opf_path = match content_opf_re.captures(&container_text) {
			Some(captures) => captures.get(1).unwrap().as_str().to_string(),
			None => return Err(anyhow!("Malformatted/missing container.xml file")),
		};
		let content_opf_dir = match PathBuf::from(&content_opf_path).parent() {
			Some(p) => p.to_path_buf(),
			None => PathBuf::new(),
		};
		let content_opf_text = zip_string(&mut zip, &content_opf_path)?;
		let content_opf = parse_content_opf(&content_opf_text, &content_opf_dir, &zip)
			.ok_or(anyhow!("Malformatted content.opf file"))?;

		let (css_cache, images) = load_cache(&mut zip, &content_opf_dir, &content_opf.manifest);

		let mut nxc_path = content_opf_dir.clone();
		nxc_path.push(
			&content_opf
				.manifest
				.get("ncx")
				.ok_or(anyhow!("Malformatted content.opf file"))?
				.href,
		);
		// TODO: check if this would always work
		let ncx_path = nxc_path.into_os_string().into_string().unwrap();
		// println!("ncx path: {}", &ncx_path);
		let ncx_text = zip_string(&mut zip, &ncx_path)?;
		let toc = parse_ncx(&ncx_text)?;

		let chapter_count = content_opf.spine.len();
		let mut chapter_index = match loading_chapter {
			LoadingChapter::Index(index) => index,
			LoadingChapter::Last => chapter_count - 1,
		};
		if chapter_index >= chapter_count {
			chapter_index = chapter_count - 1;
		}
		let chapter_cache = HashMap::new();
		let mut book = EpubBook {
			zip,
			content_opf_dir,
			content_opf,
			toc,
			chapter_cache,
			chapter_index,
			css_cache,
			images,
		};
		book.load_chapter(chapter_index)?;
		Ok(book)
	}

	fn load_chapter(&mut self, chapter_index: usize) -> Result<&Chapter> {
		let chapter = match self.chapter_cache.entry(chapter_index) {
			Entry::Occupied(o) => o.into_mut(),
			Entry::Vacant(v) => {
				let (full_path, cwd, src_file) = chapter_path(chapter_index, &self.content_opf, &self.content_opf_dir)?;
				let html_str = zip_string(&mut self.zip, &full_path)?;
				let css_cache = &self.css_cache;
				let html_content = html_str_content(&html_str, Some(|path: String| {
					Some(resolve(&cwd, &path, css_cache)?.1)
				}))?;
				let toc_index = toc_index_for_chapter(chapter_index,
					&src_file, &html_content.id_map, &self.content_opf, &self.toc);
				let title = html_content.title
					.unwrap_or_else(|| toc_title(&self.toc[toc_index]).clone());
				let chapter = Chapter {
					path: src_file.clone(),
					title: String::from(title),
					lines: html_content.lines,
					id_map: html_content.id_map,
					toc_index,
				};
				v.insert(chapter)
			}
		};
		Ok(chapter)
	}

	fn target_position(&mut self, target_file: &str, target_anchor: Option<String>) -> Option<TraceInfo> {
		for (chapter_index, item_id) in self.content_opf.spine.iter().enumerate() {
			let manifest = self.content_opf.manifest.get(item_id)?;
			if target_file == manifest.href {
				let chapter = self.load_chapter(chapter_index).ok()?;
				if let Some(anchor) = &target_anchor {
					if let Some(position) = chapter.id_map.get(anchor) {
						return Some(TraceInfo {
							chapter: chapter_index,
							line: position.line,
							offset: position.offset,
						});
					}
				}
				return Some(TraceInfo {
					chapter: chapter_index,
					line: 0,
					offset: 0,
				});
			}
		}
		None
	}
}

#[inline]
fn zip_string<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<String> {
	let buf = zip_content(zip, name)?;
	Ok(String::from_utf8(buf)?)
}

#[inline]
fn zip_content<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<Vec<u8>> {
	match zip.by_name(name) {
		Ok(mut file) => {
			let mut buf = vec![];
			file.read_to_end(&mut buf)?;
			Ok(buf)
		}
		Err(ZipError::FileNotFound) => Err(anyhow!("{} not found in archive", name)),
		Err(e) => Err(anyhow!("failed load {}: {}", name, e.to_string())),
	}
}


fn load_cache<R: Read + Seek>(zip: &mut ZipArchive<R>, cwd: &PathBuf, manifest: &Manifest) -> (HashMap<String, String>, HashMap<String, Vec<u8>>)
{
	let mut css_cache = HashMap::new();
	#[cfg(not(feature = "gui"))]
		let images = HashMap::new();
	#[cfg(feature = "gui")]
		let mut images = HashMap::new();
	for (_, item) in manifest {
		if item.media_type == "text/css" {
			let full_path = concat_path(cwd.clone(), &item.href);
			if let Some(full_path) = full_path.to_str() {
				if let Ok(content) = zip_string(zip, full_path) {
					css_cache.insert(full_path.to_string(), content);
				}
			}
			continue;
		}
		#[cfg(feature = "gui")]
		if item.media_type.starts_with("image/") {
			let full_path = concat_path(cwd.clone(), &item.href);
			if let Some(full_path) = full_path.to_str() {
				if let Ok(content) = zip_content(zip, full_path) {
					images.insert(full_path.to_string(), content);
				}
			}
		}
	}
	(css_cache, images)
}

fn parse_nav_points(nav_points_element: &Element, level: usize, nav_points: &mut Vec<NavPoint>) {
	fn parse_element(el: &Element, level: usize) -> Option<NavPoint> {
		let id = el.attributes.get("id")?.to_string();
		let play_order: Option<usize> = el
			.attributes
			.get("playOrder")
			.and_then(|po| po.parse().ok());
		let src = el.get_child("content")?.attributes.get("src")?.to_string();
		let mut src_split = src.split('#');
		let src_file = String::from(src_split.next()?);
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
			if let Some(np) = parse_element(el, level) {
				nav_points.push(np);
				parse_nav_points(el, level + 1, nav_points);
			}
		});
}

fn parse_ncx(text: &str) -> Result<Vec<NavPoint>> {
	let ncx = Element::parse(text.strip_bom().as_bytes())
		.map_err(|_e| anyhow!("Invalid XML"))?;
	let nav_map = ncx
		.get_child("navMap")
		.ok_or_else(|| anyhow!("Missing navMap"))?;
	let mut nav_points = vec![];
	parse_nav_points(nav_map, 1, &mut nav_points);
	if nav_points.len() == 0 {
		Err(anyhow!("Could not parse NavPoints"))
	} else {
		Ok(nav_points)
	}
}

fn parse_manifest(manifest: &Element) -> Manifest {
	manifest
		.children
		.iter()
		.filter_map(|node| {
			if let Some(el) = node.as_element() {
				if el.name == "item" {
					let id = el.attributes.get("id")?.to_string();
					return Some((
						id.clone(),
						ManifestItem {
							id,
							href: el.attributes.get("href")?.to_string(),
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
fn parse_spine<R: Read + Seek>(spine: &Element, manifest: &Manifest, content_opf_dir: &PathBuf, zip: &ZipArchive<R>) -> Option<Spine> {
	let file_names: HashSet<&str> = zip.file_names().collect();
	Some(
		spine
			.children
			.iter()
			.filter_map(|node| {
				if let Some(el) = node.as_element() {
					if el.name == "itemref" {
						let id = el.attributes.get("idref")?.to_string();
						let item = manifest.get(&id)?;
						let item_path = concat_path(content_opf_dir.clone(), &item.href);
						let item_path = item_path.to_str()?;
						if file_names.contains(item_path) {
							return Some(id);
						}
					}
				}
				None
			})
			.collect(),
	)
}

fn parse_content_opf<R: Read + Seek>(text: &str, content_opf_dir: &PathBuf, zip: &ZipArchive<R>) -> Option<ContentOPF> {
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
	let language = metadata.get_child("language")?.get_text()?.to_string();
	let manifest = parse_manifest(manifest);
	let spine = parse_spine(spine, &manifest, content_opf_dir, zip)?;
	Some(ContentOPF {
		title,
		author,
		language,
		manifest,
		spine,
	})
}

fn is_encrypted<R: Read + Seek>(zip: &ZipArchive<R>) -> bool {
	zip.file_names().find(|f| *f == "META-INF/encryption.xml").is_some()
}

fn toc_index_for_chapter<'a>(chapter_index: usize, chapter_path: &str, id_map: &HashMap<String, Position>,
	opf: &ContentOPF, toc: &'a Vec<NavPoint>) -> usize {
	if toc.len() == 0 {
		return 0;
	}
	let mut file_matched = None;
	for current_chapter in (0..=chapter_index).rev() {
		for toc_index in 0..toc.len() {
			let np = &toc[toc_index];
			if current_chapter == chapter_index {
				if chapter_path == np.src_file {
					if let Some(anchor) = &np.src_anchor {
						if id_map.contains_key(anchor) {
							return toc_index;
						}
					} else {
						return toc_index;
					}
				}
			} else {
				let spine = &opf.spine[current_chapter];
				let manifest = &opf.manifest.get(spine).unwrap();
				if manifest.href == np.src_file {
					file_matched = Some(toc_index);
				}
			}
		}
		if let Some(the_last_index_found) = file_matched {
			return the_last_index_found;
		}
	}
	0
}

fn toc_title(nav_point: &NavPoint) -> &String {
	let label = match &nav_point.label {
		Some(label) => label,
		None => &nav_point.src_file,
	};
	label
}

fn resolve<'a, T>(cwd: &PathBuf, path: &str, cache: &'a HashMap<String, T>) -> Option<(String, &'a T)>
{
	let full_path = concat_path(cwd.clone(), path);
	let path_str = full_path.to_str()?;
	let content = cache.get(path_str)?;
	Some((path_str.to_string(), content))
}

fn chapter_path(chapter_index: usize, content_opf: &ContentOPF, content_opf_dir: &PathBuf) -> Result<(String, PathBuf, String)>
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
	let src_file = &item.href;
	let full_path = content_opf_dir.clone();
	let mut full_path = concat_path(full_path, src_file);
	let full_path_str = full_path.to_str().unwrap().to_string();
	full_path.pop();

	Ok((full_path_str, full_path, src_file.clone()))
}

fn concat_path(mut path: PathBuf, mut sub_path: &str) -> PathBuf
{
	while sub_path.starts_with("../") {
		path.pop();
		sub_path = &sub_path[3..];
	}
	path.push(sub_path);
	path
}