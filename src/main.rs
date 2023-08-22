#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

extern crate core;
#[macro_use]
extern crate markup5ever;

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
#[cfg(feature = "i18n")]
use crate::i18n::I18n;

mod terminal;
mod common;
mod list;
mod book;
mod html_convertor;
mod container;
mod controller;
#[cfg(feature = "gui")]
mod gui;
#[cfg(feature = "i18n")]
mod i18n;
mod color;
#[cfg(feature = "open")]
mod open;

const TBR_BOOK_ENV_KEY: &str = "TBR_BOOK";

#[macro_export]
macro_rules! description {
    () => ( "Terminal ebook reader," )
}
#[macro_export]
macro_rules! version {
    () => ( env!("CARGO_PKG_VERSION") )
}
#[macro_export]
macro_rules! version_string {
    () => ( concat!(description!(), " v", version!()) )
}
#[macro_export]
macro_rules! package_name {
    () => ( env!("CARGO_PKG_NAME") )
}

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
#[include = "*.ftl"]
#[include = "*.png"]
struct Asset;

#[derive(Serialize, Deserialize)]
pub struct ReadingInfo {
	filename: String,
	inner_book: usize,
	chapter: usize,
	line: usize,
	position: usize,
	#[serde(default)]
	custom_color: bool,
	ts: u64,
}

impl ReadingInfo {
	#[inline]
	pub(crate) fn new(filename: &str) -> Self
	{
		ReadingInfo {
			filename: String::from(filename),
			inner_book: 0,
			chapter: 0,
			line: 0,
			position: 0,
			custom_color: true,
			ts: 0,
		}
	}
	#[inline]
	pub(crate) fn with_last_chapter(mut self) -> Self
	{
		self.chapter = usize::MAX;
		self
	}
	#[inline]
	pub(crate) fn with_inner_book(mut self, inner_book: usize) -> Self
	{
		self.inner_book = inner_book;
		self
	}
	#[inline]
	#[allow(unused)]
	pub(crate) fn no_custom_color(mut self) -> Self
	{
		self.custom_color = false;
		self
	}
	#[inline]
	#[allow(unused)]
	pub(crate) fn pos(&self) -> Position
	{
		Position::new(self.line, self.position)
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
			custom_color: self.custom_color,
			ts: ReadingInfo::now(),
		}
	}
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct PathConfig {
	enabled: bool,
	path: PathBuf,
}

#[derive(Serialize, Deserialize, Clone)]
#[cfg(feature = "gui")]
struct GuiConfiguration {
	fonts: Vec<PathConfig>,
	font_size: u8,
	sidebar_size: u32,
	#[serde(default = "default_locale")]
	lang: String,
	dictionaries: Vec<PathConfig>,
	#[serde(default)]
	cache_dict: bool,
}

#[cfg(feature = "gui")]
impl Default for GuiConfiguration
{
	fn default() -> Self {
		GuiConfiguration {
			fonts: vec![],
			font_size: 20,
			sidebar_size: 300,
			lang: default_locale(),
			dictionaries: vec![],
			cache_dict: false,
		}
	}
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Configuration {
	render_han: bool,
	current: Option<String>,
	history: Vec<ReadingInfo>,
	dark_theme: bool,
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

#[derive(Clone)]
pub struct Themes {
	bright: Theme,
	dark: Theme,
}

impl Themes {
	fn get(&self, dark: bool) -> &Theme
	{
		if dark {
			&self.dark
		} else {
			&self.bright
		}
	}
}

pub enum BookToOpen {
	None,
	Cmd(String),
	Env(String),
}

impl BookToOpen {
	#[inline]
	fn name(&self) -> Option<&str>
	{
		match self {
			BookToOpen::None => None,
			BookToOpen::Cmd(name) => Some(name),
			BookToOpen::Env(name) => Some(name)
		}
	}
}

fn main() -> Result<()> {
	let cli = Cli::parse();
	let config_dir = match config_dir() {
		None => return Err(anyhow!("Can not find config dir.")),
		Some(x) => x.join(package_name!()),
	};
	let cache_dir = match cache_dir() {
		None => return Err(anyhow!("Can not find cache dir.")),
		Some(x) => x.join(package_name!()),
	};
	let config_file = config_dir.join("tbr.toml");
	let filename = cli.filename
		.map_or_else(
			|| env::var(TBR_BOOK_ENV_KEY).map_or(BookToOpen::None, |name| {
				BookToOpen::Env(name)
			}),
			|name| BookToOpen::Cmd(name));
	#[allow(unused_mut)]
		let (mut configuration, mut themes) = load_config(&filename, config_file, &config_dir, &cache_dir)?;
	#[cfg(feature = "gui")]
	if !cli.terminal {
		if let Some((c, t)) = gui::start(configuration, themes, filename)? {
			configuration = c;
			themes = t;
		} else {
			return Ok(());
		}
	}
	terminal::start(configuration, themes)?;
	Ok(())
}

fn file_path(filename: &str) -> Option<String> {
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

fn load_config(filename: &BookToOpen, config_file: PathBuf, themes_dir: &PathBuf, cache_dir: &PathBuf) -> Result<(Configuration, Themes)> {
	let (configuration, themes) =
		if config_file.as_path().is_file() {
			let string = fs::read_to_string(&config_file)?;
			let mut configuration: Configuration = toml::from_str(&string)?;
			if let Some(filename) = filename.name() {
				configuration.current = file_path(filename);
			}
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
			let theme_file = themes_dir.join("dark.toml");
			let dark = process_theme_result(load_theme_file(theme_file))?;
			let theme_file = themes_dir.join("bright.toml");
			let bright = process_theme_result(load_theme_file(theme_file))?;
			let themes = Themes { dark, bright };
			configuration.config_file = config_file;
			(configuration, themes)
		} else {
			let themes = create_default_theme_files(themes_dir)?;
			fs::create_dir_all(cache_dir)?;
			let filepath = filename.name()
				.map_or(None, |filename| file_path(filename));

			(Configuration {
				render_han: false,
				current: filepath,
				history: vec![],
				dark_theme: false,
				#[cfg(feature = "gui")]
				gui: Default::default(),
				config_file,
			}, themes)
		};
	return Ok((configuration, themes));
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

fn create_default_theme_files(themes_dir: &PathBuf) -> Result<Themes>
{
	fs::create_dir_all(themes_dir)?;

	let utf8 = Asset::get("dark.toml").unwrap();
	let str = std::str::from_utf8(utf8.data.as_ref())?;
	let dark = process_theme_result(load_toml(str))?;
	let theme_file = themes_dir.join("dark.toml");
	fs::write(theme_file, str)?;

	let utf8 = Asset::get("bright.toml").unwrap();
	let str = std::str::from_utf8(utf8.data.as_ref())?;
	let bright = process_theme_result(load_toml(str))?;
	let theme_file = themes_dir.join("bright.toml");
	fs::write(theme_file, str)?;

	Ok(Themes { dark, bright })
}

#[inline]
#[cfg(feature = "i18n")]
fn default_locale() -> String
{
	use sys_locale::get_locale;
	get_locale().unwrap_or_else(|| String::from(i18n::DEFAULT_LOCALE))
}
