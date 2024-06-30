#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

extern crate core;
#[macro_use]
extern crate markup5ever;

use std::env;
use anyhow::{anyhow, Result};
use clap::Parser;
use dirs::{cache_dir, config_dir};
use rust_embed::RustEmbed;

use crate::book::BookLoader;
use crate::common::Position;
use crate::config::load_config;
use crate::container::ContainerManager;
#[cfg(feature = "i18n")]
use crate::i18n::I18n;

mod terminal;
mod common;
mod list;
mod book;
mod html_parser;
mod container;
mod controller;
#[cfg(feature = "gui")]
mod gui;
#[cfg(feature = "i18n")]
mod i18n;
mod color;
#[cfg(feature = "open")]
mod open;
mod config;
mod xhtml;

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
	#[clap(
		short,
		long,
		help = "Using terminal to read e-book, by default if gui exists, tbr will using gui view."
	)]
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
			|| env::var(TBR_BOOK_ENV_KEY).map_or(None, |name| {
				Some(name)
			}),
			|name| Some(name));
	#[allow(unused_mut)]
		let (mut current, mut configuration) = load_config(
		filename,
		config_file,
		&config_dir,
		&cache_dir)?;
	#[cfg(feature = "gui")]
	if !cli.terminal {
		if let Some((curr, c)) = gui::start(current, configuration)? {
			current = curr;
			configuration = c;
		} else {
			return Ok(());
		}
	}
	terminal::start(current, configuration, config_dir)?;
	Ok(())
}
