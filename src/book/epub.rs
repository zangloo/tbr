use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{Cursor, Read, Seek};
use std::path::PathBuf;

use anyhow::{anyhow, Error, Result};
use regex::Regex;
use xmltree::Element;
use zip::ZipArchive;

use crate::book::{Book, InvalidChapterError, Line, Loader};
use crate::html_convertor::html_str_content;
use crate::view::{Position, TraceInfo};

struct ManifestItem {
	#[allow(dead_code)]
	id: String,
	href: String,
	#[allow(dead_code)]
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
	#[allow(dead_code)]
	index: usize,
	path: String,
	#[allow(dead_code)]
	title: String,
	lines: Vec<Line>,
	id_map: HashMap<String, Position>,
}

struct EpubBook<R: Read + Seek> {
	zip: ZipArchive<R>,
	#[allow(dead_code)]
	content_opf_dir: PathBuf,
	#[allow(dead_code)]
	content_opf: ContentOPF,
	pub toc: Vec<NavPoint>,
	chapter_cache: HashMap<usize, Chapter>,
	chapter_index: usize,
}

pub struct EpubLoader {}

impl Loader for EpubLoader {
	fn support(&self, filename: &str) -> bool {
		filename.to_lowercase().ends_with(".epub")
	}

	fn load_file(&self, filename: &str, chapter_index: usize) -> Result<Box<dyn Book>> {
		let file = OpenOptions::new().read(true).open(filename)?;
		Ok(Box::new(EpubBook::new(file, chapter_index)?))
	}

	fn load_buf(&self, _filename: &str, content: Vec<u8>, chapter_index: usize) -> Result<Box<dyn Book>>
	{
		Ok(Box::new(EpubBook::new(Cursor::new(content), chapter_index)?))
	}
}

impl<'a, R: Read + Seek> Book for EpubBook<R> {
	fn chapter_count(&self) -> usize {
		self.toc.len()
	}

	fn set_chapter(&mut self, chapter_index: usize) -> Result<()> {
		load_chapter(&mut self.zip, &self.toc, &self.content_opf_dir, chapter_index, &mut self.chapter_cache)?;
		self.chapter_index = chapter_index;
		Ok(())
	}

	fn current_chapter(&self) -> usize {
		self.chapter_index
	}

	fn title(&self) -> Option<&String> {
		self.chapter_title(self.chapter_index)
	}

	fn chapter_title(&self, chapter_index: usize) -> Option<&String> {
		self.chapter_title(chapter_index)
	}

	fn lines(&self) -> &Vec<Line> {
		&self.chapter_cache.get(&self.chapter_index).unwrap().lines
	}

	fn link_position(&mut self, line: usize, link_index: usize) -> Option<TraceInfo> {
		let chapter = self.chapter_cache.get(&self.chapter_index).unwrap();
		let text = &chapter.lines.get(line)?;
		let link = text.link_at(link_index)?;
		let mut link_target = link.target.as_str();

		let mut current_path = PathBuf::from(&chapter.path);
		current_path.pop();
		while link_target.starts_with("../") {
			current_path.pop();
			link_target = &link_target[3..];
		}
		current_path.push(link_target);
		let target = current_path.to_str()?;
		let mut target_split = target.split('#');
		let target_file = target_split.next()?;
		let target_anchor = target_split.next().and_then(|a| Some(String::from(a))).or(None);
		for (chapter_index, np) in self.toc.iter().enumerate() {
			if target_file == np.src_file {
				if target_anchor == np.src_anchor {
					return Some(TraceInfo {
						chapter: chapter_index,
						line: 0,
						position: 0,
					});
				}
				if let Some(anchor) = &target_anchor {
					if load_chapter(&mut self.zip, &self.toc, &self.content_opf_dir, chapter_index, &mut self.chapter_cache).is_err() {
						return None;
					}
					let chapter = self.chapter_cache.get(&chapter_index)?;
					if let Some(position) = chapter.id_map.get(anchor) {
						return Some(TraceInfo {
							chapter: chapter_index,
							line: position.line,
							position: position.position,
						});
					}
				}
			}
		}
		None
	}
}

impl<'a, R: Read + Seek> EpubBook<R> {
	pub fn new(reader: R, mut chapter_index: usize) -> Result<Self> {
		let mut zip = ZipArchive::new(reader)?;
		if is_encrypted(&zip) {
			return Err(anyhow!("Encrypted epub."));
		}
		let container_text = zip_content(&mut zip, "META-INF/container.xml")?;
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
		let content_opf_text = zip_content(&mut zip, &content_opf_path)?;
		let content_opf = parse_content_opf(&content_opf_text)
			.ok_or(anyhow!("Malformatted content.opf file"))?;

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
		let ncx_text = zip_content(&mut zip, &ncx_path)?;
		let toc = parse_ncx(&ncx_text)?;

		let chapter_count = toc.len();
		if chapter_index >= chapter_count {
			chapter_index = chapter_count - 1;
		}
		let mut chapter_cache = HashMap::new();
		load_chapter(&mut zip, &toc, &content_opf_dir, chapter_index, &mut chapter_cache)?;
		Ok(EpubBook {
			zip,
			content_opf_dir,
			content_opf,
			toc,
			chapter_cache,
			chapter_index,
		})
	}

	pub fn chapter_title(&self, chapter_index: usize) -> Option<&String> {
		let np = self.toc.get(chapter_index)?;
		let label = match &np.label {
			Some(label) => label,
			None => &np.src_file,
		};
		Some(&label)
	}
}

fn zip_content<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<String> {
	let mut buf = vec![];
	zip.by_name(name)?.read_to_end(&mut buf)?;
	Ok(String::from_utf8(buf)?)
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
	let ncx = xmltree::Element::parse(text.as_bytes())
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

pub fn parse_spine(spine: &Element) -> Option<Spine> {
	Some(
		spine
			.children
			.iter()
			.filter_map(|node| {
				if let Some(el) = node.as_element() {
					if el.name == "itemref" {
						let id = el.attributes.get("idref")?.to_string();
						return Some(id);
					}
				}
				None
			})
			.collect(),
	)
}

fn parse_content_opf(text: &str) -> Option<ContentOPF> {
	let package = xmltree::Element::parse(text.as_bytes()).ok()?;
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
	let spine = parse_spine(spine)?;
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

fn load_chapter<R: Read + Seek>(zip: &mut ZipArchive<R>, toc: &Vec<NavPoint>,
	content_opf_dir: &PathBuf, chapter_index: usize, chapter_cache: &mut HashMap<usize, Chapter>,
) -> Result<()> {
	if chapter_cache.contains_key(&chapter_index) {
		return Ok(());
	}
	let np = toc.get(chapter_index).ok_or(Error::new(InvalidChapterError {}))?;
	let src_file = &np.src_file;
	let mut full_path = content_opf_dir.clone();
	full_path.push(src_file);
	let full_path = full_path.into_os_string().into_string().unwrap();
	let html_str = zip_content(zip, &full_path)?;
	let mut html_content = html_str_content(&html_str)?;
	let html_lines = &mut html_content.lines;
	let all_id_map = &mut html_content.id_map;

	// load all chapter in this html file to cache
	for index in (0..toc.len()).rev() {
		let np = toc.get(index).unwrap();
		if np.src_file != *src_file {
			continue;
		}
		let start_anchor = &np.src_anchor;
		let stop_anchor = if let Some(np2) = toc.get(index + 1) {
			let next_src = &np2.src_file;
			if next_src == src_file {
				&np2.src_anchor
			} else {
				&None
			}
		} else {
			&None
		};
		let start_index = match start_anchor {
			Some(id) => if let Some(position) = all_id_map.get(id) {
				position.line
			} else {
				0
			}
			None => 0,
		};
		let end_index = match stop_anchor {
			Some(id) => if let Some(position) = all_id_map.get(id) {
				position.line
			} else {
				html_lines.len()
			}
			None => html_lines.len(),
		};
		let lines = html_lines.drain(start_index..end_index).collect::<Vec<Line>>();
		let mut id_map = HashMap::new();
		all_id_map.retain(|id, position| {
			if position.line >= start_index && position.line < end_index {
				let line = position.line - start_index;
				id_map.insert(id.clone(), Position::new(line, position.position));
				false
			} else {
				true
			}
		});
		let title = match &np.label {
			Some(label) => label,
			None => &np.src_file,
		}.clone();
		let chapter = Chapter { index, path: src_file.clone(), title, lines, id_map };
		chapter_cache.insert(index, chapter);
	}
	assert_eq!(html_lines.len(), 0);
	Ok(())
}
