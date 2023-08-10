// copy from egui/ecolor

use std::fmt::{Display, Formatter};
use gtk4::cairo::Context as CairoContext;

#[derive(Clone, Debug)]
pub struct Color32(pub(crate) [u8; 4]);

#[allow(unused)]
impl Color32 {
	// Mostly follows CSS names:

	pub const TRANSPARENT: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 0);
	pub const BLACK: Color32 = Color32::from_rgb(0, 0, 0);
	pub const DARK_GRAY: Color32 = Color32::from_rgb(96, 96, 96);
	pub const GRAY: Color32 = Color32::from_rgb(160, 160, 160);
	pub const LIGHT_GRAY: Color32 = Color32::from_rgb(220, 220, 220);
	pub const WHITE: Color32 = Color32::from_rgb(255, 255, 255);

	pub const BROWN: Color32 = Color32::from_rgb(165, 42, 42);
	pub const DARK_RED: Color32 = Color32::from_rgb(0x8B, 0, 0);
	pub const RED: Color32 = Color32::from_rgb(255, 0, 0);
	pub const LIGHT_RED: Color32 = Color32::from_rgb(255, 128, 128);

	pub const YELLOW: Color32 = Color32::from_rgb(255, 255, 0);
	pub const LIGHT_YELLOW: Color32 = Color32::from_rgb(255, 255, 0xE0);
	pub const KHAKI: Color32 = Color32::from_rgb(240, 230, 140);

	pub const DARK_GREEN: Color32 = Color32::from_rgb(0, 0x64, 0);
	pub const GREEN: Color32 = Color32::from_rgb(0, 255, 0);
	pub const LIGHT_GREEN: Color32 = Color32::from_rgb(0x90, 0xEE, 0x90);

	pub const DARK_BLUE: Color32 = Color32::from_rgb(0, 0, 0x8B);
	pub const BLUE: Color32 = Color32::from_rgb(0, 0, 255);
	pub const LIGHT_BLUE: Color32 = Color32::from_rgb(0xAD, 0xD8, 0xE6);

	pub const GOLD: Color32 = Color32::from_rgb(255, 215, 0);

	pub const DEBUG_COLOR: Color32 = Color32::from_rgba_premultiplied(0, 200, 0, 128);

	/// An ugly color that is planned to be replaced before making it to the screen.
	pub const TEMPORARY_COLOR: Color32 = Color32::from_rgb(64, 254, 0);

	#[inline(always)]
	pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
		Self([r, g, b, 255])
	}

	#[inline(always)]
	pub const fn from_rgb_additive(r: u8, g: u8, b: u8) -> Self {
		Self([r, g, b, 0])
	}

	/// From `sRGBA` with premultiplied alpha.
	#[inline(always)]
	pub const fn from_rgba_premultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
		Self([r, g, b, a])
	}
	#[inline(always)]
	pub const fn is_opaque(&self) -> bool {
		self.a() == 255
	}

	#[inline(always)]
	pub const fn r(&self) -> u8 {
		self.0[0]
	}

	#[inline(always)]
	pub const fn g(&self) -> u8 {
		self.0[1]
	}

	#[inline(always)]
	pub const fn b(&self) -> u8 {
		self.0[2]
	}

	#[inline(always)]
	pub const fn a(&self) -> u8 {
		self.0[3]
	}

	/// Returns an additive version of self
	#[inline(always)]
	pub const fn additive(self) -> Self {
		let [r, g, b, _] = self.to_array();
		Self([r, g, b, 0])
	}

	/// Premultiplied RGBA
	#[inline(always)]
	pub const fn to_array(&self) -> [u8; 4] {
		[self.r(), self.g(), self.b(), self.a()]
	}

	/// Premultiplied RGBA
	#[inline(always)]
	pub const fn to_tuple(&self) -> (u8, u8, u8, u8) {
		(self.r(), self.g(), self.b(), self.a())
	}

	pub fn from_rgba_unmultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
		if a == 255 {
			Self::from_rgb(r, g, b) // common-case optimization
		} else if a == 0 {
			Self::TRANSPARENT // common-case optimization
		} else {
			let r_lin = linear_f32_from_gamma_u8(r);
			let g_lin = linear_f32_from_gamma_u8(g);
			let b_lin = linear_f32_from_gamma_u8(b);
			let a_lin = linear_f32_from_linear_u8(a);

			let r = gamma_u8_from_linear_f32(r_lin * a_lin);
			let g = gamma_u8_from_linear_f32(g_lin * a_lin);
			let b = gamma_u8_from_linear_f32(b_lin * a_lin);

			Self::from_rgba_premultiplied(r, g, b, a)
		}
	}

	#[inline(always)]
	pub fn apply(&self, cairo: &CairoContext)
	{
		cairo.set_source_rgba(
			self.r() as f64 / 255.,
			self.g() as f64 / 255.,
			self.b() as f64 / 255.,
			self.a() as f64 / 255.,
		)
	}
}

impl Display for Color32 {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result
	{
		write!(f, "#{:02x?}{:02x?}{:02x?}{:02x?}", self.0[0], self.0[1], self.0[2], self.0[3])
	}
}

pub fn linear_f32_from_gamma_u8(s: u8) -> f32 {
	if s <= 10 {
		s as f32 / 3294.6
	} else {
		((s as f32 + 14.025) / 269.025).powf(2.4)
	}
}

pub fn gamma_u8_from_linear_f32(l: f32) -> u8 {
	if l <= 0.0 {
		0
	} else if l <= 0.0031308 {
		fast_round(3294.6 * l)
	} else if l <= 1.0 {
		fast_round(269.025 * l.powf(1.0 / 2.4) - 14.025)
	} else {
		255
	}
}

#[inline(always)]
fn fast_round(r: f32) -> u8 {
	(r + 0.5).floor() as _ // rust does a saturating cast since 1.45
}

#[inline(always)]
pub fn linear_f32_from_linear_u8(a: u8) -> f32 {
	a as f32 / 255.0
}
