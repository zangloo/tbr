use std::fs;
use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use cursive::theme::{Error, load_theme_file, load_toml, Theme};
#[cfg(feature = "gui")]
use gtk4::Orientation;
use rusqlite::{Connection, Row};
use serde_derive::{Deserialize, Serialize};
#[cfg(feature = "i18n")]
use crate::i18n;
use crate::Asset;
use crate::terminal::Listable;

#[derive(Clone)]
pub struct ReadingInfo {
	row_id: i64,
	pub filename: String,
	pub inner_book: usize,
	pub chapter: usize,
	pub line: usize,
	pub position: usize,
	pub custom_color: bool,
	pub custom_font: bool,
	pub strip_empty_lines: bool,
	pub custom_style: Option<String>,
	pub font_size: u8,
}

impl ReadingInfo {
	#[inline]
	#[cfg(feature = "gui")]
	pub fn fake(filename: &str) -> Self
	{
		ReadingInfo {
			row_id: 0,
			filename: String::from(filename),
			inner_book: 0,
			chapter: 0,
			line: 0,
			position: 0,
			custom_color: false,
			custom_font: false,
			strip_empty_lines: false,
			custom_style: None,
			font_size: default_font_size(),
		}
	}

	#[inline]
	pub fn load_inner_book(&self, inner_book: usize) -> BookLoadingInfo
	{
		BookLoadingInfo::ChangeInnerBook(
			&self.filename,
			inner_book,
			self.row_id,
			self.custom_style.clone(),
			self.font_size)
	}

	#[inline]
	fn now() -> u64
	{
		SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap()
			.as_secs()
	}
}

impl Listable for ReadingInfo {
	fn title(&self) -> &str {
		&self.filename
	}

	fn id(&self) -> usize
	{
		let rowid = self.row_id;
		if rowid < 0 {
			0
		} else {
			rowid as usize
		}
	}
}

#[allow(unused)]
pub enum BookLoadingInfo<'a> {
	NewReading(&'a str, usize, usize, u8),
	ChangeInnerBook(&'a str, usize, i64, Option<String>, u8),
	History(ReadingInfo),
	Reload(ReadingInfo),
}

impl<'a> BookLoadingInfo<'a> {
	#[inline]
	pub fn filename(&self) -> &str
	{
		match self {
			BookLoadingInfo::NewReading(filename, ..) => filename,
			BookLoadingInfo::ChangeInnerBook(filename, ..) => filename,
			BookLoadingInfo::History(reading) | BookLoadingInfo::Reload(reading) => &reading.filename,
		}
	}

	#[inline]
	pub fn get(self) -> ReadingInfo
	{
		match self {
			BookLoadingInfo::NewReading(filename, inner_book, chapter, font_size) => ReadingInfo {
				row_id: 0,
				filename: filename.to_owned(),
				inner_book,
				chapter,
				line: 0,
				position: 0,
				custom_color: false,
				custom_font: false,
				strip_empty_lines: false,
				custom_style: None,
				font_size,
			},
			BookLoadingInfo::ChangeInnerBook(filename, inner_book, row_id, custom_style, font_size) =>
				ReadingInfo {
					row_id,
					filename: filename.to_owned(),
					inner_book,
					chapter: 0,
					line: 0,
					position: 0,
					custom_color: false,
					custom_font: false,
					strip_empty_lines: false,
					custom_style: custom_style.clone(),
					font_size,
				},
			BookLoadingInfo::History(reading) | BookLoadingInfo::Reload(reading) => reading,
		}
	}

	#[inline]
	pub fn get_or_init<F>(self, f: F) -> ReadingInfo
		where F: FnOnce(&mut ReadingInfo)
	{
		match self {
			BookLoadingInfo::NewReading(filename, inner_book, chapter, font_size) => {
				let mut reading = ReadingInfo {
					row_id: 0,
					filename: filename.to_owned(),
					inner_book,
					chapter,
					line: 0,
					position: 0,
					custom_color: false,
					custom_font: false,
					strip_empty_lines: false,
					custom_style: None,
					font_size,
				};
				f(&mut reading);
				reading
			}
			BookLoadingInfo::ChangeInnerBook(filename, inner_book, row_id, custom_style, font_size) => {
				let mut reading = ReadingInfo {
					row_id,
					filename: filename.to_owned(),
					inner_book,
					chapter: 0,
					line: 0,
					position: 0,
					custom_color: false,
					custom_font: false,
					strip_empty_lines: false,
					custom_style: custom_style.clone(),
					font_size,
				};
				f(&mut reading);
				reading
			}
			BookLoadingInfo::History(reading) | BookLoadingInfo::Reload(reading) => reading
		}
	}
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
pub struct PathConfig {
	pub enabled: bool,
	pub path: PathBuf,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[cfg(feature = "gui")]
#[serde(rename_all = "snake_case")]
pub enum SidebarPosition {
	Left,
	Top,
}

#[cfg(feature = "gui")]
impl Default for SidebarPosition {
	#[inline]
	fn default() -> Self
	{
		SidebarPosition::Left
	}
}

#[cfg(feature = "gui")]
impl SidebarPosition {
	#[inline]
	pub fn paned_orientation(&self) -> Orientation
	{
		match self {
			SidebarPosition::Left => Orientation::Horizontal,
			SidebarPosition::Top => Orientation::Vertical,
		}
	}
	#[inline]
	pub fn i18n_key(&self) -> &'static str
	{
		match self {
			SidebarPosition::Left => "sidebar-left",
			SidebarPosition::Top => "sidebar-top",
		}
	}
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[cfg(feature = "gui")]
pub struct GuiConfiguration {
	pub fonts: Vec<PathConfig>,
	#[serde(default = "default_font_size")]
	pub default_font_size: u8,
	#[serde(default = "default_font_size")]
	pub dict_font_size: u8,
	pub sidebar_size: u32,
	#[serde(default)]
	pub sidebar_position: SidebarPosition,
	#[serde(default = "default_locale")]
	pub lang: String,
	pub dictionaries: Vec<PathConfig>,
	pub cache_dict: bool,
	pub strip_empty_lines: bool,
	pub ignore_font_weight: bool,
	#[serde(default)]
	pub scroll_for_page: bool,
	#[serde(default)]
	pub select_by_dictionary: bool,
}

#[cfg(feature = "gui")]
impl Default for GuiConfiguration
{
	fn default() -> Self {
		GuiConfiguration {
			fonts: vec![],
			default_font_size: default_font_size(),
			dict_font_size: default_font_size(),
			sidebar_size: 300,
			sidebar_position: Default::default(),
			lang: default_locale(),
			dictionaries: vec![],
			cache_dict: false,
			strip_empty_lines: false,
			ignore_font_weight: false,
			scroll_for_page: false,
			select_by_dictionary: false,
		}
	}
}

pub struct Configuration {
	pub render_han: bool,
	pub dark_theme: bool,
	history: PathBuf,
	#[cfg(feature = "gui")]
	pub gui: GuiConfiguration,

	config_file: PathBuf,
	history_db: Connection,
	orig: RawConfig,
}

impl Configuration {
	pub fn save(&self) -> Result<()>
	{
		let raw_config = RawConfig {
			render_han: self.render_han,
			dark_theme: self.dark_theme,
			history: self.history.clone(),
			#[cfg(feature = "gui")]
			gui: self.gui.clone(),
		};
		if self.orig != raw_config {
			let text = toml::to_string(&raw_config)?;
			fs::write(&self.config_file, text)?;
		}
		Ok(())
	}

	fn map(row: &Row) -> rusqlite::Result<ReadingInfo>
	{
		Ok(ReadingInfo {
			row_id: row.get(0)?,
			filename: row.get(1)?,
			inner_book: row.get(2)?,
			chapter: row.get(3)?,
			line: row.get(4)?,
			position: row.get(5)?,
			custom_color: row.get(6)?,
			custom_font: row.get(7)?,
			strip_empty_lines: row.get(8)?,
			custom_style: row.get(9)?,
			font_size: row.get::<usize, Option<u8>>(10)?.
				unwrap_or(default_font_size()),
		})
	}

	pub fn history(&self, current: Option<&String>, filter_pattern: Option<&String>)
		-> Result<Vec<ReadingInfo>>
	{
		Ok(query(&self.history_db, 20, current, filter_pattern)?)
	}

	pub fn reading<'a>(&self, filename: &'a str) -> Result<BookLoadingInfo<'a>>
	{
		let mut stmt = self.history_db.prepare("
select row_id,
       filename,
       inner_book,
       chapter,
       line,
       position,
       custom_color,
       custom_font,
       strip_empty_lines,
       custom_style,
       font_size,
       ts
from history
where filename = ?
")?;
		let mut iter = stmt.query_map([filename], Configuration::map)?;
		if let Some(info) = iter.next() {
			Ok(BookLoadingInfo::History(info?))
		} else {
			#[cfg(feature = "gui")]
			{ Ok(BookLoadingInfo::NewReading(filename, 0, 0, self.gui.default_font_size)) }
			#[cfg(not(feature = "gui"))]
			{ Ok(BookLoadingInfo::NewReading(filename, 0, 0, default_font_size())) }
		}
	}

	pub fn reading_by_id(&self, row_id: i64) -> Result<ReadingInfo>
	{
		let mut stmt = self.history_db.prepare("
select row_id,
       filename,
       inner_book,
       chapter,
       line,
       position,
       custom_color,
       custom_font,
       strip_empty_lines,
       custom_style,
       font_size,
       ts
from history
where row_id = ?
")?;
		let mut iter = stmt.query_map([row_id], Configuration::map)?;
		if let Some(info) = iter.next() {
			Ok(info?)
		} else {
			panic!("Reading history not exists");
		}
	}

	pub fn save_reading(&self, reading: &mut ReadingInfo) -> Result<()>
	{
		let ts = ReadingInfo::now();
		if reading.row_id == 0 {
			self.history_db.execute("
insert into history (filename, inner_book, chapter, line, position,
                     custom_color, custom_font, strip_empty_lines,
                     custom_style, font_size, ts)
values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
", (&reading.filename, reading.inner_book, reading.chapter, reading.line,
				reading.position, reading.custom_color, reading.custom_font,
				reading.strip_empty_lines, &reading.custom_style,
				reading.font_size, ts))?;
			reading.row_id = self.history_db.last_insert_rowid();
		} else {
			self.history_db.execute("
update history
set filename          = ?,
    inner_book        = ?,
    chapter           = ?,
    line              = ?,
    position          = ?,
    custom_color      = ?,
    custom_font       = ?,
    strip_empty_lines = ?,
    custom_style      = ?,
    font_size         = ?,
    ts                = ?
where row_id = ?
", (&reading.filename, reading.inner_book, reading.chapter, reading.line,
				reading.position, reading.custom_color, reading.custom_font,
				reading.strip_empty_lines, &reading.custom_style,
				reading.font_size, ts, reading.row_id))?;
		}
		Ok(())
	}
}

#[derive(Clone)]
pub struct Themes {
	bright: Theme,
	dark: Theme,
}

impl Themes {
	pub fn get(&self, dark: bool) -> &Theme
	{
		if dark {
			&self.dark
		} else {
			&self.bright
		}
	}
}

pub(super) fn load_config(filename: Option<String>, config_file: PathBuf, config_dir: &PathBuf,
	cache_dir: &PathBuf) -> Result<(Option<String>, Configuration, Themes)>
{
	let (current, configuration, themes) =
		if config_file.as_path().is_file() {
			let string = fs::read_to_string(&config_file)?;
			let raw_config: RawConfig = toml::from_str(&string)?;
			let mut current = if let Some(filename) = &filename {
				file_path(filename)
			} else {
				None
			};
			let history_db = load_history_db(&raw_config.history)?;
			if current.is_none() {
				if let Some(latest_reading) = query(&history_db, 1, None, None)?.pop() {
					current = Some(latest_reading.filename);
				}
			}
			let theme_file = config_dir.join("dark.toml");
			let dark = process_theme_result(load_theme_file(theme_file))?;
			let theme_file = config_dir.join("bright.toml");
			let bright = process_theme_result(load_theme_file(theme_file))?;
			let themes = Themes { dark, bright };
			let orig = raw_config.clone();
			let configuration = Configuration {
				render_han: raw_config.render_han,
				dark_theme: raw_config.dark_theme,
				history: raw_config.history,
				#[cfg(feature = "gui")]
				gui: raw_config.gui,
				config_file,
				history_db,
				orig,
			};
			(current, configuration, themes)
		} else {
			let themes = create_default_theme_files(config_dir)?;
			fs::create_dir_all(cache_dir)?;
			let current = filename
				.map_or(None, |filename| file_path(&filename));
			let history = config_dir.join("history.sqlite");
			let history_db = load_history_db(&history)?;
			let orig = RawConfig {
				render_han: false,
				dark_theme: false,
				history: history.clone(),
				#[cfg(feature = "gui")]
				gui: Default::default(),
			};
			let text = toml::to_string(&orig)?;
			fs::write(&config_file, text)?;
			(current, Configuration {
				render_han: false,
				dark_theme: false,
				history,
				#[cfg(feature = "gui")]
				gui: Default::default(),

				config_file,
				history_db,
				orig,
			}, themes)
		};
	return Ok((current, configuration, themes));
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

#[inline]
#[cfg(feature = "i18n")]
fn default_locale() -> String
{
	use sys_locale::get_locale;
	get_locale().unwrap_or_else(|| String::from(i18n::DEFAULT_LOCALE))
}

#[inline]
fn default_font_size() -> u8
{
	20
}

const CURRENT_DB_VERSION: u16 = 2;

#[inline]
fn load_history_db(path: &PathBuf) -> Result<Connection>
{
	let connection = if !path.exists() {
		// init db
		let conn = Connection::open(path)?;
		conn.execute("
create table info ( version integer )
			", ())?;
		conn.execute("insert into info (version) values (?)", [CURRENT_DB_VERSION])?;
		conn.execute("
create table history
(
    row_id            integer primary key,
    filename          varchar,
    inner_book        unsigned big int,
    chapter           unsigned big int,
    line              unsigned big int,
    position          unsigned big int,
    custom_color      unsigned big int,
    custom_font       unsigned big int,
    strip_empty_lines unsigned big int,
    custom_style      varchar,
    font_size         unsigned big int,
    ts                unsigned big int,
    unique (filename)
)", ())?;
		conn
	} else {
		let connection = Connection::open(path)?;
		upgrade_db(&connection)?;
		connection
	};
	Ok(connection)
}

#[inline]
fn upgrade_db(connection: &Connection) -> Result<()>
{
	let mut stmt = connection.prepare("select version from info")?;
	let mut rows = stmt.query([])?;
	let row = rows.next()?;
	let version: u64 = if let Some(row) = row {
		row.get(0)?
	} else {
		0
	};
	if version == 0 {
		connection.execute("alter table history add custom_style varchar", [])?;
		connection.execute("insert into info (version) values (1)", [])?;
	}
	if version < 2 {
		connection.execute("alter table history add font_size unsigned big int", [])?;
		connection.execute("update info set version = 2", [])?;
	}
	Ok(())
}

fn query(conn: &Connection, limit: usize, exclude: Option<&String>,
	filter_pattern: Option<&String>) -> Result<Vec<ReadingInfo>>
{
	let mut stmt = conn.prepare("
select row_id,
       filename,
       inner_book,
       chapter,
       line,
       position,
       custom_color,
       custom_font,
       strip_empty_lines,
       custom_style,
       font_size,
       ts
from history
order by ts desc
")?;
	let iter = stmt.query_map([], Configuration::map)?;
	let mut list = vec![];
	for info in iter {
		let info = info?;
		let path = PathBuf::from_str(&info.filename)?;
		if !path.exists() {
			continue;
		}
		let filename = &info.filename;
		if let Some(exclude) = exclude {
			if exclude == filename {
				continue;
			}
		}
		if let Some(pattern) = filter_pattern {
			if match_filename(&filename, pattern).is_none() {
				continue;
			}
		}
		list.push(info);
		if list.len() >= limit {
			break;
		}
	}
	Ok(list)
}

pub fn match_filename(filename: &str, pattern: &str) -> Option<Vec<usize>>
{
	let mut vec = vec![];
	let mut name_iter = filename.chars().into_iter();
	let mut fi = 0;
	for pc in pattern.chars() {
		let pc = pc.to_ascii_lowercase();
		loop {
			let fc = name_iter.next()?.to_ascii_lowercase();
			if fc == pc {
				vec.push(fi);
				fi += 1;
				break;
			}
			fi += 1;
		}
	}
	Some(vec)
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct RawConfig {
	pub render_han: bool,
	pub dark_theme: bool,
	history: PathBuf,
	#[cfg(feature = "gui")]
	#[serde(default)]
	pub gui: GuiConfiguration,
}
