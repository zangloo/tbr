// copy from egui/emath

use std::fmt::{Display, Formatter};
use std::ops::{Add, Sub};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec2 {
	pub x: f32,
	pub y: f32,
}

#[inline(always)]
pub const fn vec2(x: f32, y: f32) -> Vec2 {
	Vec2 { x, y }
}

#[inline(always)]
pub const fn pos2(x: f32, y: f32) -> Pos2 {
	Pos2 { x, y }
}

impl Vec2 {
	pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
	pub const INFINITY: Self = Vec2 { x: f32::INFINITY, y: f32::INFINITY };

	pub fn new(x: f32, y: f32) -> Self
	{
		Vec2 { x, y }
	}
}

impl Add for Vec2 {
	type Output = Vec2;

	#[inline(always)]
	fn add(self, rhs: Vec2) -> Vec2 {
		Vec2 {
			x: self.x + rhs.x,
			y: self.y + rhs.y,
		}
	}
}

impl Sub for Vec2 {
	type Output = Vec2;

	#[inline(always)]
	fn sub(self, rhs: Vec2) -> Vec2 {
		Vec2 {
			x: self.x - rhs.x,
			y: self.y - rhs.y,
		}
	}
}

impl Display for Vec2 {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result
	{
		write!(f, "{{x: {}, y: {}}}", self.x, self.y)
	}
}

pub type Pos2 = Vec2;

#[derive(Clone, Debug, PartialEq)]
pub struct Rect {
	pub min: Pos2,
	pub max: Pos2,
}

impl Rect {
	pub const EVERYTHING: Self = Self {
		min: pos2(-f32::INFINITY, -f32::INFINITY),
		max: pos2(f32::INFINITY, f32::INFINITY),
	};

	pub const NOTHING: Self = Self {
		min: pos2(f32::INFINITY, f32::INFINITY),
		max: pos2(-f32::INFINITY, -f32::INFINITY),
	};

	#[inline(always)]
	pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self
	{
		Rect {
			min: pos2(x, y),
			max: pos2(x + width, y + height),
		}
	}

	#[inline(always)]
	pub const fn from_min_max(min: Pos2, max: Pos2) -> Self {
		Rect { min, max }
	}

	/// left-top corner plus a size (stretching right-down).
	#[inline(always)]
	pub fn from_min_size(min: Pos2, size: Vec2) -> Self {
		Rect {
			max: min + size,
			min,
		}
	}

	#[inline(always)]
	pub fn size(&self) -> Vec2 {
		self.max - self.min
	}

	#[inline(always)]
	pub fn width(&self) -> f32 {
		self.max.x - self.min.x
	}

	#[inline(always)]
	pub fn height(&self) -> f32 {
		self.max.y - self.min.y
	}

	/// `min.x`
	#[inline(always)]
	pub fn left(&self) -> f32 {
		self.min.x
	}

	/// `max.x`
	#[inline(always)]
	pub fn right(&self) -> f32 {
		self.max.x
	}

	/// `min.y`
	#[inline(always)]
	pub fn top(&self) -> f32 {
		self.min.y
	}

	/// `max.y`
	#[inline(always)]
	pub fn bottom(&self) -> f32 {
		self.max.y
	}

	#[inline(always)]
	pub fn contains(&self, p: &Pos2) -> bool {
		self.min.x <= p.x && p.x <= self.max.x && self.min.y <= p.y && p.y <= self.max.y
	}
}

impl Display for Rect {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result
	{
		write!(f, "{{min: {}, max: {}}}", self.min, self.max)
	}
}