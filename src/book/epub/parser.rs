use std::{collections::HashMap, path::PathBuf};
use std::io::{Read, Seek};

use anyhow::{anyhow, Error, Result};
use regex::Regex;
use xmltree::Element;
use zip::ZipArchive;

use crate::book::InvalidChapterError;
use crate::html_convertor::html_str_lines;

pub struct ChapterInfo(pub String, pub Vec<String>);

pub struct ManifestItem {
	id: String,
	href: String,
	media_type: String,
	properties: Option<String>,
}

pub type ItemId = String;
pub type Manifest = HashMap<ItemId, ManifestItem>;
pub type Spine = Vec<ItemId>;

pub struct ContentOPF {
	pub title: String,
	pub author: Option<String>,
	pub language: String,
	pub manifest: Manifest,
	pub spine: Spine,
}

#[derive(PartialEq, Eq, Hash)]
pub struct NavPoint {
	pub id: String,
	pub label: Option<String>,
	pub play_order: Option<usize>,
	pub level: usize,
	pub src: String,
}

pub struct EpubArchive<R: Read + Seek> {
	zip: ZipArchive<R>,
	manifest_html_files: HashMap<String, String>,
	content_opf_dir: PathBuf,
	content_opf: ContentOPF,
	pub toc: Vec<NavPoint>,
}

impl<R: Read + Seek> EpubArchive<R> {
	pub fn new(reader: R) -> Result<Self> {
		let mut zip = ZipArchive::new(reader)?;
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

		// construct map filename -> content for all html files declared in manifest
		// let manifest_html_files: HashMap<String, String> = HashMap::new();
		let manifest_html_files: HashMap<String, String> = content_opf
			.manifest
			.values()
			.filter_map(|manifest_item| {
				if manifest_item.media_type == "application/xhtml+xml" {
					Some(manifest_item.href.to_string())
				} else {
					None
				}
			})
			.map(|filepath| {
				let mut full_path = content_opf_dir.clone();
				full_path.push(filepath.clone());
				let full_path = full_path.into_os_string().into_string().unwrap();
				zip_content(&mut zip, &full_path)
					.map(|content| (filepath, content))
			})
			.collect::<Result<HashMap<_, _>>>()?;

		Ok(EpubArchive {
			zip,
			manifest_html_files,
			content_opf_dir,
			content_opf,
			toc,
		})
	}

	pub fn load_chapter(&mut self, chapter: usize) -> Result<ChapterInfo> {
		let np = self.toc.get(chapter).ok_or(Error::new(InvalidChapterError {}))?;
		let resource_path = &np.src;

		let mut filepath = self.content_opf_dir.clone();
		filepath.push(resource_path.as_str());
		let text = zip_content(&mut self.zip, filepath.to_str().unwrap())?;
		let title = match &np.label {
			Some(label) => label.clone(),
			None => np.src.clone(),
		};
		let lines = html_str_lines(text)?;
		Ok(ChapterInfo(title, lines))
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
			src,
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