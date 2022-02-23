extern crate core;
#[macro_use]
extern crate markup5ever;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use clap::Parser;
use cursive::theme::{Error, load_theme_file, load_toml, Theme};
use dirs::{cache_dir, config_dir};
use rust_embed::RustEmbed;
use serde_derive::{Deserialize, Serialize};
use toml;

use crate::book::BookLoader;
use crate::container::ContainerManager;
use crate::view::ReverseInfo;

mod controller;
mod view;
mod common;
mod list;
mod book;
mod html_convertor;
mod container;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
	#[clap(short, long)]
	debug: bool,
	filename: Option<String>,
}

#[derive(RustEmbed)]
#[folder = "assets/"]
#[prefix = ""]
#[include = "*.toml"]
struct Asset;

struct ThemeEntry(String, Theme);

#[derive(Serialize, Deserialize)]
pub struct ReadingInfo {
	filename: String,
	inner_book: usize,
	chapter: usize,
	line: usize,
	position: usize,
	ts: u64,
	#[serde(skip)]
	reverse: Option<ReverseInfo>,
}

impl ReadingInfo {
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
			reverse: None,
		}
	}
}

#[derive(Serialize, Deserialize)]
pub struct Configuration {
	render_type: String,
	search_pattern: Option<String>,
	current: String,
	theme_name: String,
	history: Vec<ReadingInfo>,
	themes: HashMap<String, PathBuf>,
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
	let (configuration, theme_entries) = load_config(cli.filename, &config_file, &themes_dir, &cache_dir)?;
	let configuration = controller::start(configuration, theme_entries)?;
	save_config(configuration, config_file)?;
	Ok(())
}

fn file_path(filename: String) -> Result<String> {
	let filepath = PathBuf::from(filename);
	if !filepath.exists() {
		return Err(anyhow!("{} is not exists.", filepath.to_str().unwrap()));
	}
	if !filepath.is_file() {
		return Err(anyhow!("{} is not a file.", filepath.to_str().unwrap()));
	}
	let filepath = fs::canonicalize(filepath)?;
	let filename = filepath.as_os_str().to_str().unwrap().to_string();
	Ok(filename)
}

fn load_config(filename: Option<String>, config_file: &PathBuf, themes_dir: &PathBuf, cache_dir: &PathBuf) -> Result<(Configuration, Vec<ThemeEntry>)> {
	let (configuration, theme_entries) =
		if config_file.as_path().is_file() {
			let string = fs::read_to_string(config_file)?;
			let mut configuration: Configuration = toml::from_str(&string)?;
			let with_filename = filename.is_some();
			if with_filename {
				let filepath = file_path(filename.unwrap())?;
				configuration.current = filepath;
			}
			let mut idx = 0 as usize;
			let mut found_current = false;
			while idx < configuration.history.len() {
				let name = &configuration.history[idx].filename;
				let path = PathBuf::from(&name);
				if !path.exists() {
					configuration.history.remove(idx);
				} else {
					if !found_current && name.eq(&configuration.current) {
						found_current = true;
					}
					idx = idx + 1;
				}
			}
			if !with_filename {
				if configuration.history.len() == 0 {
					let path = PathBuf::from(&configuration.current);
					if !path.exists() {
						return Err(anyhow!("No file to open."));
					}
				} else if !found_current {
					// last reading book not exists
					let ri = configuration.history.first().unwrap();
					configuration.current = ri.filename.clone();
				}
			}
			let mut theme_entries = vec![];
			for (name, path) in &configuration.themes {
				let mut theme_file = themes_dir.clone();
				theme_file.push(path);
				let theme = process_theme_result(load_theme_file(theme_file))?;
				theme_entries.push(ThemeEntry(name.clone(), theme));
			}
			(configuration, theme_entries)
		} else {
			if filename.is_none() {
				return Err(anyhow!("No file to open."));
			}
			let themes_map = HashMap::from([
				("dark".to_string(), PathBuf::from("dark.toml")),
				("bright".to_string(), PathBuf::from("bright.toml")),
			]);
			let theme_entries = create_default_theme_files(&themes_map, themes_dir)?;
			fs::create_dir_all(cache_dir)?;
			let filename = filename.unwrap();
			let filepath = file_path(filename)?;
			(Configuration {
				render_type: String::from("xi"),
				search_pattern: Option::None,
				current: filepath,
				history: vec![],
				theme_name: String::from("dark"),
				themes: themes_map,
			}, theme_entries)
		};
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

fn save_config(configuration: Configuration, config_file: PathBuf) -> Result<()> {
	let text = toml::to_string(&configuration)?;
	fs::write(config_file, text)?;
	Ok(())
}
