use std::fs::OpenOptions;
use std::io::Read;
use std::sync::Arc;
use ab_glyph::{Font, FontRef, FontVec, OutlinedGlyph, Rect};
use anyhow::{anyhow, Result};
use fontdb::{Database, Query};
use indexmap::IndexMap;
use lightningcss::properties::font::GenericFontFamily;
use ouroboros::self_referencing;
use crate::config::PathConfig;
use crate::html_convertor::{FontWeight, HtmlFontFaceDesc};

pub trait Fonts {
	fn query(&self, char: char, font_size: f32, font_weight: &FontWeight,
		font_family_names: Option<&str>) -> Option<(OutlinedGlyph, Rect)>;
}

#[self_referencing]
pub struct UserFonts {
	db: Database,
	#[borrows(db)]
	#[covariant]
	fonts: IndexMap<fontdb::ID, FontRef<'this>>,
}

impl Fonts for UserFonts {
	fn query(&self, char: char, font_size: f32, font_weight: &FontWeight,
		font_family_names: Option<&str>) -> Option<(OutlinedGlyph, Rect)>
	{
		self.with_db(|db| {
			let mut families = vec![];
			// without custom family and weight, using custom fonts
			if font_weight.is_default() && font_family_names.is_none() {
				for (_, font) in self.borrow_fonts() {
					if let Some(outlined) = get_glyph(char, font_size, font) {
						let rect = font.glyph_bounds(outlined.glyph());
						return Some((outlined, rect));
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
				weight: fontdb::Weight(font_weight.outlined()),
				stretch: Default::default(),
				style: Default::default(),
			};
			let id = db.query(&query)?;
			let font = self.borrow_fonts().get(&id)?;
			let outlined = get_glyph(char, font_size, font)?;
			let rect = font.glyph_bounds(outlined.glyph());
			Some((outlined, rect))
		})
	}
}

fn create_user_fonts(db: Database) -> Result<Option<UserFonts>>
{
	if db.len() > 0 {
		let mut err = None;
		let fonts = UserFontsBuilder {
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

pub fn user_fonts(font_paths: &Vec<PathConfig>) -> Result<Option<UserFonts>>
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
		create_user_fonts(db)
	}
}

struct HtmlFontFace {
	family: String,
	refs: Vec<usize>,
}

pub struct HtmlFonts {
	fonts: Vec<(String, FontVec)>,
	faces: Vec<HtmlFontFace>,
}

impl HtmlFonts {
	#[inline]
	pub fn new() -> Self
	{
		HtmlFonts { fonts: vec![], faces: vec![] }
	}

	#[inline]
	pub fn has_faces(&self) -> bool
	{
		!self.faces.is_empty()
	}

	pub fn reload<F>(&mut self, font_faces: Vec<HtmlFontFaceDesc>, data_resolver: F)
		where F: Fn(&str) -> Option<Vec<u8>>
	{
		if font_faces.is_empty() {
			self.faces.clear();
			return;
		}
		if self.same_with(&font_faces) {
			return;
		}

		self.faces.clear();
		for face in font_faces {
			let mut refs = vec![];
			for source in face.sources {
				if let Err(idx) = self.fonts.binary_search_by(|(key, _)| key.cmp(&source)) {
					if let Some(content) = data_resolver(&source) {
						if let Ok(font) = FontVec::try_from_vec(content) {
							self.fonts.insert(idx, (source, font));
							for v in &mut refs {
								if *v >= idx {
									*v += 1;
								}
							}
							refs.push(idx)
						}
					}
				};
			}
			if !refs.is_empty() {
				let family = face.family;
				if let Err(idx) = self.faces.binary_search_by(|face| {
					face.family.as_str().cmp(&family)
				}) {
					self.faces.insert(idx, HtmlFontFace {
						family,
						refs,
					});
				}
			}
		}
	}

	#[inline]
	fn same_with(&self, font_faces: &Vec<HtmlFontFaceDesc>) -> bool
	{
		let len = font_faces.len();
		if self.faces.len() != len {
			return false;
		}
		for i in 0..len {
			let orig_face = &self.faces[i];
			let new_face = &font_faces[i];
			if orig_face.family != new_face.family {
				return false;
			}
			let refs_len = orig_face.refs.len();
			if refs_len != new_face.sources.len() {
				return false;
			}
			for j in 0..refs_len {
				if let Some(font) = self.fonts.get(orig_face.refs[j]) {
					if font.0 != new_face.sources[j] {
						return false;
					}
				} else {
					return false;
				}
			}
		}
		true
	}

	fn find(&self, char: char, font_size: f32, _font_weight: &FontWeight,
		font_family: &str) -> Option<(OutlinedGlyph, Rect)>
	{
		if let Ok(idx) = self.faces.binary_search_by(|face| {
			face.family.as_str().cmp(font_family)
		}) {
			if let Some((_, font)) = self.fonts.get(idx) {
				if let Some(outlined) = get_glyph(char, font_size, font) {
					let rect = font.glyph_bounds(outlined.glyph());
					return Some((outlined, rect));
				}
			}
		}
		None
	}
}

impl Fonts for HtmlFonts {
	fn query(&self, char: char, font_size: f32, font_weight: &FontWeight,
		font_family_names: Option<&str>) -> Option<(OutlinedGlyph, Rect)>
	{
		if let Some(names) = font_family_names {
			for name in names.split(',') {
				let name = name.trim();
				if let Some(found) = self.find(
					char, font_size, font_weight, name) {
					return Some(found);
				}
			}
		} else {
			let name = GenericFontFamily::Default.as_str();
			if let Some(found) = self.find(
				char, font_size, font_weight, name) {
				return Some(found);
			}
		}
		None
	}
}

fn get_glyph(char: char, font_size: f32, font: &impl Font) -> Option<OutlinedGlyph>
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
