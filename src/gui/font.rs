use std::fs::OpenOptions;
use std::io::Read;
use std::sync::Arc;
use ab_glyph::{Font, FontRef, OutlinedGlyph};
use anyhow::{anyhow, Result};
use fontdb::{Database, Query};
use indexmap::IndexMap;
use ouroboros::self_referencing;
use crate::config::PathConfig;
use crate::gui::DEFAULT_FONT_WEIGHT;

#[self_referencing]
pub struct Fonts {
	db: Database,
	#[borrows(db)]
	#[covariant]
	fonts: IndexMap<fontdb::ID, FontRef<'this>>,
}

impl Fonts {
	pub fn from_files(font_paths: &Vec<PathConfig>) -> Result<Option<Fonts>>
	{
		if font_paths.is_empty() {
			Ok(None)
		} else {
			let mut db = Database::new();
			for config in font_paths {
				if config.enabled {
					if let Ok(mut file) = OpenOptions::new()
						.read(true)
						.open(&config.path) {
						let mut buf = vec![];
						file.read_to_end(&mut buf)?;
						let source = fontdb::Source::Binary(Arc::new(buf));
						db.load_font_source(source);
					}
				}
			}
			if db.len() > 0 {
				let mut err = None;
				let fonts = FontsBuilder {
					db,
					fonts_builder: |db| {
						let mut fonts = IndexMap::new();
						for info in db.faces() {
							if let fontdb::Source::Binary(bytes) = &info.source {
								match FontRef::try_from_slice_and_index(bytes.as_ref().as_ref(), info.index) {
									Ok(font) => { fonts.insert(info.id, font); }
									Err(_) => err = Some(anyhow!("Error load font: {:#?}", info)),
								}
							}
						}
						fonts
					},
				}.build();
				if let Some(err) = err {
					Err(err)
				} else {
					Ok(Some(fonts))
				}
			} else {
				Ok(None)
			}
		}
	}

	pub fn query(&self, char: char, font_size: f32, font_weight: u8,
		font_family_names: Option<&str>) -> Option<(&FontRef, OutlinedGlyph)>
	{
		fn get_glyph(char: char, font_size: f32, font: &FontRef) -> Option<OutlinedGlyph>
		{
			if let Some(scale) = font.pt_to_px_scale(font_size) {
				let glyph = font.glyph_id(char);
				if glyph.0 != 0 {
					let glyph = glyph.with_scale(scale);
					let outlined = font.outline_glyph(glyph);
					if outlined.is_some() {
						return outlined;
					}
				}
			}
			None
		}
		let font_weight = match font_weight {
			1 |
			2 |
			3 |
			4 |
			5 |
			6 |
			7 |
			8 |
			9 => font_weight,
			_ => DEFAULT_FONT_WEIGHT,
		};
		self.with_db(|db| {
			let mut families = vec![];
			// without custom family and weight, using custom fonts
			if font_weight == DEFAULT_FONT_WEIGHT && font_family_names.is_none() {
				for (_, font) in self.borrow_fonts() {
					if let Some(outlined) = get_glyph(char, font_size, font) {
						return Some((font, outlined));
					}
				}
				return None;
			}
			if let Some(names) = font_family_names {
				for name in names.split(',') {
					let name = name.trim();
					families.push(fontdb::Family::Name(name));
				}
			}
			let query = Query {
				families: &families,
				weight: fontdb::Weight(font_weight as u16 * 100),
				stretch: Default::default(),
				style: Default::default(),
			};
			let id = db.query(&query)?;
			let font = self.borrow_fonts().get(&id)?;
			let outlined = get_glyph(char, font_size, font)?;
			Some((font, outlined))
		})
	}
}