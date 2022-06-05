extern crate core;
#[macro_use]
extern crate markup5ever;

use std::cmp::Ordering;
use std::collections::HashMap;
#[cfg(feature = "gui")]
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use std::env;
use anyhow::{anyhow, Result};
use clap::Parser;
use cursive::theme::{Error, load_theme_file, load_toml, Theme};
use dirs::{cache_dir, config_dir};
use rust_embed::RustEmbed;
use serde_derive::{Deserialize, Serialize};
use toml;

use crate::book::BookLoader;
use crate::common::Position;
use crate::container::ContainerManager;

mod terminal;
mod common;
mod list;
mod book;
mod html_convertor;
mod container;
mod controller;
#[cfg(feature = "gui")]
mod gui;

const TBR_BOOK_ENV_KEY: &str = "TBR_BOOK";

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
	#[cfg(feature = "gui")]
	#[clap(short, long, help = "Using terminal to read e-book, by default if gui exists, tbr will using gui view.")]
	terminal: bool,
	filename: Option<String>,
}

#[derive(RustEmbed)]
#[folder = "assets/"]
#[prefix = ""]
#[include = "*.toml"]
#[include = "*.svg"]
#[include = "*.ttc"]
pub struct Asset;

pub struct ThemeEntry(String, Theme);

impl Eq for ThemeEntry {}

impl PartialEq<Self> for ThemeEntry {
	fn eq(&self, other: &Self) -> bool {
		self.0.eq(&other.0)
	}
}

impl PartialOrd<Self> for ThemeEntry {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		self.0.partial_cmp(&other.0)
	}
}

impl Ord for ThemeEntry {
	fn cmp(&self, other: &Self) -> Ordering {
		self.0.cmp(&other.0)
	}
}

#[derive(Serialize, Deserialize)]
pub struct ReadingInfo {
	filename: String,
	inner_book: usize,
	chapter: usize,
	line: usize,
	position: usize,
	ts: u64,
}

impl ReadingInfo {
	pub(crate) fn new(filename: &str) -> Self {
		ReadingInfo {
			filename: String::from(filename),
			inner_book: 0,
			chapter: 0,
			line: 0,
			position: 0,
			ts: 0,
		}
	}
	pub(crate) fn with_last_chapter(mut self) -> Self {
		self.chapter = usize::MAX;
		self
	}
	pub(crate) fn with_inner_book(mut self, inner_book: usize) -> Self {
		self.inner_book = inner_book;
		self
	}

	fn now() -> u64 {
		SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap()
			.as_secs()
	}
}

impl Clone for ReadingInfo {
	fn clone(&self) -> Self {
		ReadingInfo {
			filename: self.filename.clone(),
			inner_book: self.inner_book,
			chapter: self.chapter,
			line: self.line,
			position: self.position,
			ts: ReadingInfo::now(),
		}
	}
}

#[derive(Serialize, Deserialize)]
#[cfg(feature = "gui")]
pub struct GuiConfiguration {
	fonts: HashSet<PathBuf>,
	font_size: u8,
}

#[cfg(feature = "gui")]
impl Default for GuiConfiguration
{
	fn default() -> Self {
		GuiConfiguration { fonts: HashSet::new(), font_size: 20 }
	}
}

#[derive(Serialize, Deserialize)]
pub struct Configuration {
	render_type: String,
	#[serde(default)]
	search_pattern: String,
	current: Option<String>,
	theme_name: String,
	history: Vec<ReadingInfo>,
	themes: HashMap<String, PathBuf>,
	#[cfg(feature = "gui")]
	#[serde(default)]
	gui: GuiConfiguration,
	#[serde(skip)]
	config_file: PathBuf,
}

impl Configuration {
	pub fn save(&self) -> Result<()>
	{
		let text = toml::to_string(self)?;
		fs::write(&self.config_file, text)?;
		Ok(())
	}
}

fn main() -> Result<()> {
	let cli = Cli::parse();
	let mut config_dir = match config_dir() {
		None => return Err(anyhow!("Can not find config dir.")),
		Some(x) => x,
	};
	config_dir.push("ter");
	let mut cache_dir = match cache_dir() {
		None => return Err(anyhow!("Can not find cache dir.")),
		Some(x) => x,
	};
	let mut config_file = config_dir.clone();
	config_file.push("ter.toml");
	cache_dir.push("ter");
	let mut themes_dir = config_dir.clone();
	themes_dir.push("themes");
	let filename = cli.filename.or(env::var(TBR_BOOK_ENV_KEY).ok());
	let (configuration, theme_entries) = load_config(filename, config_file, &themes_dir, &cache_dir)?;
	#[cfg(feature = "gui")]
	if !cli.terminal {
		return gui::start(configuration, theme_entries);
	}
	terminal::start(configuration, theme_entries)?;
	Ok(())
}

fn file_path(filename: Option<String>) -> Option<String> {
	if filename.is_none() {
		return None;
	}
	let filename = filename.unwrap();
	let filepath = PathBuf::from(filename);
	if !filepath.exists() {
		return None;
	}
	if !filepath.is_file() {
		return None;
	}
	if let Ok(filepath) = fs::canonicalize(filepath) {
		let filename = filepath.as_os_str().to_str().unwrap().to_string();
		Some(filename)
	} else {
		None
	}
}

fn load_config(filename: Option<String>, config_file: PathBuf, themes_dir: &PathBuf, cache_dir: &PathBuf) -> Result<(Configuration, Vec<ThemeEntry>)> {
	let (configuration, mut theme_entries) =
		if config_file.as_path().is_file() {
			let string = fs::read_to_string(&config_file)?;
			let mut configuration: Configuration = toml::from_str(&string)?;
			configuration.current = file_path(filename);
			let mut idx = 0 as usize;
			let mut found_current = false;
			// remove non-exists history
			while idx < configuration.history.len() {
				let name = &configuration.history[idx].filename;
				let path = PathBuf::from(&name);
				if !path.exists() {
					configuration.history.remove(idx);
				} else {
					if !found_current {
						if let Some(current) = &configuration.current {
							if name.eq(current) {
								found_current = true;
							}
						}
					}
					idx = idx + 1;
				}
			}
			if configuration.current.is_none() && configuration.history.len() > 0 {
				let ri = configuration.history.last().unwrap();
				configuration.current = Some(ri.filename.clone());
			}
			let mut theme_entries = vec![];
			for (name, path) in &configuration.themes {
				let mut theme_file = themes_dir.clone();
				theme_file.push(path);
				let theme = process_theme_result(load_theme_file(theme_file))?;
				theme_entries.push(ThemeEntry(name.clone(), theme));
			}
			configuration.config_file = config_file;
			(configuration, theme_entries)
		} else {
			let themes_map = HashMap::from([
				("dark".to_string(), PathBuf::from("dark.toml")),
				("bright".to_string(), PathBuf::from("bright.toml")),
			]);
			let theme_entries = create_default_theme_files(&themes_map, themes_dir)?;
			fs::create_dir_all(cache_dir)?;
			let filepath = file_path(filename);

			(Configuration {
				render_type: String::from("xi"),
				search_pattern: String::from(""),
				current: filepath,
				history: vec![],
				theme_name: String::from("dark"),
				themes: themes_map,
				#[cfg(feature = "gui")]
				gui: Default::default(),
				config_file,
			}, theme_entries)
		};
	theme_entries.sort();
	return Ok((configuration, theme_entries));
}

fn process_theme_result(result: Result<Theme, Error>) -> Result<Theme> {
	match result {
		Ok(theme) => Ok(theme),
		Err(e) => Err(anyhow!(match e {
					Error::Io(e) => e.to_string(),
					Error::Parse(e) => e.to_string(),
				}))
	}
}

fn create_default_theme_files(themes_map: &HashMap<String, PathBuf>, themes_dir: &PathBuf) -> Result<Vec<ThemeEntry>> {
	let mut theme_entries: Vec<ThemeEntry> = vec![];
	for (name, filepath) in themes_map {
		let filename = filepath.to_str().unwrap();
		let utf8 = Asset::get(filename).unwrap();
		let str = std::str::from_utf8(utf8.data.as_ref())?;
		let theme = process_theme_result(load_toml(str))?;
		theme_entries.push(ThemeEntry(name.clone(), theme));
		fs::create_dir_all(themes_dir)?;
		let mut theme_file = themes_dir.clone();
		theme_file.push(filename);
		fs::write(theme_file, str)?;
	}
	Ok(theme_entries)
}
