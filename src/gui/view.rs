use std::rc::Rc;

use glib::Object;
use gtk4::{CssProvider, EventControllerMotion, EventControllerScroll, EventControllerScrollFlags, gdk, GestureClick, GestureDrag, glib};
use gtk4::gdk::{Display, ModifierType};
use gtk4::pango::Layout as PangoContext;
use gtk4::prelude::{EventControllerExt, GestureDragExt, GestureExt, ObjectExt, WidgetExt};
use gtk4::Scrollable;
use gtk4::subclass::prelude::ObjectSubclassIsExt;

use crate::book::{Book, Line};
use crate::color::Colors;
use crate::common::Position;
use crate::config::ReadingInfo;
use crate::controller::{HighlightInfo, Render};
use crate::gui::font::UserFonts;
use crate::gui::HtmlFonts;
use crate::gui::math::{Pos2, pos2};
use crate::gui::render::RenderContext;

const MIN_TEXT_SELECT_DISTANCE: f32 = 4.0;

pub enum ScrollPosition {
	LineNext,
	LinePrev,
	PageNext,
	PagePrev,
	Begin,
	End,
	Position(f64),
}

pub enum ClickTarget {
	Link(usize, usize),
	ExternalLink(usize, usize),
	Image(usize, usize),
	Char(usize, usize),
	None,
}

glib::wrapper! {
    pub struct GuiView(ObjectSubclass<imp::GuiView>)
        @extends gtk4::Widget, gtk4::DrawingArea,
		@implements Scrollable
	;
}

impl Render<RenderContext> for GuiView {
	#[inline]
	fn book_loaded(&mut self, book: &dyn Book, reading: &ReadingInfo,
		context: &mut RenderContext)
	{
		self.imp().book_loaded(book, reading, &self.get_pango(), context);
	}

	#[inline]
	fn redraw(&mut self, book: &dyn Book, lines: &Vec<Line>, line: usize,
		offset: usize, highlight: &Option<HighlightInfo>, context: &mut RenderContext)
		-> Option<Position>
	{
		let next = self.imp().redraw(book, lines, line, offset, highlight, context, &self.get_pango());
		self.queue_draw();
		next
	}

	#[inline]
	fn prev_page(&mut self, book: &dyn Book, lines: &Vec<Line>,
		line: usize, offset: usize, context: &mut RenderContext) -> Position
	{
		self.imp().prev_page(book, lines, line, offset, &self.get_pango(), context)
	}

	#[inline]
	fn next_line(&mut self, book: &dyn Book, lines: &Vec<Line>,
		line: usize, offset: usize, context: &mut RenderContext) -> Position
	{
		self.imp().next_line(book, lines, line, offset, &self.get_pango(), context)
	}

	#[inline]
	fn prev_line(&mut self, book: &dyn Book, lines: &Vec<Line>,
		line: usize, offset: usize, context: &mut RenderContext) -> Position
	{
		self.imp().prev_line(book, lines, line, offset, &self.get_pango(), context)
	}

	#[inline]
	fn setup_highlight(&mut self, book: &dyn Book, lines: &Vec<Line>,
		line: usize, start: usize, context: &mut RenderContext) -> Position
	{
		self.imp().setup_highlight(book, lines, line, start, &self.get_pango(), context)
	}
}

impl GuiView {
	pub const WIDGET_NAME: &'static str = "book-view";
	pub const OPEN_LINK_SIGNAL: &'static str = "open-link";
	pub const OPEN_IMAGE_EXTERNAL_SIGNAL: &'static str = "open-image-external";
	pub const OPEN_LINK_EXTERNAL_SIGNAL: &'static str = "open-link-external";
	pub const SELECTING_TEXT_SIGNAL: &'static str = "select-text";
	pub const TEXT_SELECTED_SIGNAL: &'static str = "text-selected";
	pub const CLEAR_SELECTION_SIGNAL: &'static str = "clear-selection";
	pub const SCROLL_SIGNAL: &'static str = "scroll";
	pub const SELECT_WORD_SIGNAL: &'static str = "select-word";
	pub const SHOW_TITLE_SIGNAL: &'static str = "title";

	pub fn new(instance_name: &str, render_han: bool, book_fonts: Option<&HtmlFonts>,
		user_fonts: Rc<Option<UserFonts>>, render_context: &mut RenderContext) -> Self
	{
		let view: GuiView = Object::builder().build();
		view.set_vexpand(true);
		view.set_hexpand(true);
		view.set_widget_name(instance_name);
		view.set_focusable(true);
		view.set_focus_on_click(true);

		let imp = view.imp();
		let pango = &view.get_pango();
		imp.set_render_type(render_han, render_context);
		imp.set_fonts(book_fonts, user_fonts, pango, render_context);
		view
	}

	pub fn setup_gesture(&self)
	{
		let drag_gesture = GestureDrag::builder()
			.button(gdk::BUTTON_PRIMARY)
			.build();
		let view = self.clone();
		drag_gesture.connect_update(move |drag, seq| {
			if let Some(bp) = drag.start_point() {
				if let Some(ep) = drag.point(seq) {
					let from = pos2(bp.0 as f32, bp.1 as f32);
					let to = pos2(ep.0 as f32, ep.1 as f32);
					if let Some((from, to)) = view.calc_selection(from, to) {
						view.emit_by_name::<()>(GuiView::SELECTING_TEXT_SIGNAL, &[
							&(from.line as u64),
							&(from.offset as u64),
							&(to.line as u64),
							&(to.offset as u64),
						]);
					} else {
						view.emit_by_name::<()>(GuiView::CLEAR_SELECTION_SIGNAL, &[]);
					}
				}
			}
		});
		let view = self.clone();
		drag_gesture.connect_end(move |drag, seq| {
			if let Some(bp) = drag.start_point() {
				if let Some(ep) = drag.point(seq) {
					view.grab_focus();
					if bp != ep {
						let from = pos2(bp.0 as f32, bp.1 as f32);
						let to = pos2(ep.0 as f32, ep.1 as f32);
						if let Some((from, to)) = view.calc_selection(from, to) {
							view.emit_by_name::<()>(GuiView::TEXT_SELECTED_SIGNAL, &[
								&(from.line as u64),
								&(from.offset as u64),
								&(to.line as u64),
								&(to.offset as u64),
							]);
						}
					}
				}
			}
		});
		self.add_controller(drag_gesture);

		let gesture = GestureClick::new();
		let view = self.clone();
		gesture.connect_pressed(move |gesture, n_press, x, y| {
			if n_press == 1 {
				gesture.set_state(gtk4::EventSequenceState::Claimed);
				let pos = pos2(x as f32, y as f32);
				let imp = view.imp();
				let state = gesture.current_event_state();
				match imp.resolve_click(pos, state) {
					ClickTarget::Link(line, link_index) => view.emit_by_name::<()>(GuiView::OPEN_LINK_SIGNAL, &[
						&(line as u64),
						&(link_index as u64),
					]),
					ClickTarget::ExternalLink(line, link_index) => view.emit_by_name::<()>(GuiView::OPEN_LINK_EXTERNAL_SIGNAL, &[
						&(line as u64),
						&(link_index as u64),
					]),
					ClickTarget::Image(line, offset) => view.emit_by_name::<()>(GuiView::OPEN_IMAGE_EXTERNAL_SIGNAL, &[
						&(line as u64),
						&(offset as u64),
					]),
					ClickTarget::None | ClickTarget::Char(..) =>
						view.emit_by_name::<()>(GuiView::CLEAR_SELECTION_SIGNAL, &[]),
				}
			} else if n_press == 2 {
				gesture.set_state(gtk4::EventSequenceState::Claimed);
				let pos = pos2(x as f32, y as f32);
				let imp = view.imp();
				let state = gesture.current_event_state();
				if let ClickTarget::Char(line, offset) = imp.resolve_click(pos, state) {
					view.emit_by_name::<()>(GuiView::SELECT_WORD_SIGNAL, &[
						&(line as u64),
						&(offset as u64),
					]);
				}
			}
		});
		self.add_controller(gesture);

		let mouse_event = EventControllerMotion::new();
		let view = self.clone();
		mouse_event.connect_motion(move |motion, x, y| {
			update_mouse_pointer(&view, x as f32, y as f32, motion.current_event_state());
		});
		self.add_controller(mouse_event);

		if !self.scrollable() {
			let scroll_event = EventControllerScroll::new(EventControllerScrollFlags::VERTICAL);
			let view = self.clone();
			scroll_event.connect_scroll(move |_, _, y| {
				view.grab_focus();
				let delta = if y > 0. { 1 } else { -1 };
				view.emit_by_name::<()>(GuiView::SCROLL_SIGNAL, &[&delta]);
				glib::Propagation::Stop
			});
			self.add_controller(scroll_event);
		}
	}

	#[inline]
	pub fn get_pango(&self) -> PangoContext
	{
		let context = self.pango_context();
		let layout = PangoContext::new(&context);
		layout.set_width(300);
		layout.set_height(300);
		layout
	}

	#[inline]
	pub fn reload_render(&self, render_han: bool, render_context: &mut RenderContext)
	{
		self.imp().set_render_type(render_han, render_context);
	}

	#[inline]
	pub fn resized(&self, width: i32, height: i32, render_context: &mut RenderContext)
	{
		self.imp().resized(width, height, render_context);
	}

	#[inline]
	pub fn set_font_size(&self, font_size: u8, book_fonts: Option<&HtmlFonts>, render_context: &mut RenderContext)
	{
		self.imp().set_font_size(font_size, book_fonts, render_context, &self.get_pango());
	}

	#[inline]
	pub fn set_fonts(&self, book_fonts: Option<&HtmlFonts>, user_fonts: Rc<Option<UserFonts>>, render_context: &mut RenderContext)
	{
		self.imp().set_fonts(book_fonts, user_fonts, &self.get_pango(), render_context);
	}

	#[inline]
	pub fn set_custom_font(&self, custom_font: bool, book_fonts: Option<&HtmlFonts>, render_context: &mut RenderContext)
	{
		self.imp().set_custom_font(custom_font, book_fonts, &self.get_pango(), render_context);
	}

	#[inline(always)]
	pub fn scroll_pos(&self) -> f64
	{
		self.imp().scroll_pos().unwrap_or(0.)
	}

	#[inline(always)]
	pub fn scroll_to(&self, position: ScrollPosition)
	{
		self.imp().scroll_to(position);
		self.queue_draw();
	}

	#[inline(always)]
	pub fn calc_selection(&self, original_pos: Pos2, current_pos: Pos2)
		-> Option<(Position, Position)>
	{
		self.imp().calc_selection(original_pos, current_pos)
	}
}

mod imp {
	use std::cell::{Cell, RefCell};
	use std::cmp::min;
	use std::rc::Rc;
	use std::sync::OnceLock;

	use glib::Properties;
	use gtk4::{Adjustment, glib, graphene, Scrollable, ScrollablePolicy, Snapshot};
	use gtk4::gdk::ModifierType;
	use gtk4::glib::prelude::StaticType;
	use gtk4::glib::subclass::Signal;
	use gtk4::pango::Layout as PangoContext;
	use gtk4::prelude::*;
	use gtk4::subclass::drawing_area::DrawingAreaImpl;
	use gtk4::subclass::prelude::*;
	use indexmap::IndexSet;

	use crate::book::{Book, Line};
	use crate::common::Position;
	use crate::config::ReadingInfo;
	use crate::controller::HighlightInfo;
	use crate::gui::font::{HtmlFonts, UserFonts};
	use crate::gui::math::{Pos2, Rect};
	use crate::gui::render::{BlockBackgroundEntry, create_render, GuiRender, PointerPosition, RenderCell, RenderChar, RenderContext, RenderLine, ScrolledDrawData, ScrollRedrawMethod, TextDecoration};
	use crate::gui::view::{ClickTarget, MIN_TEXT_SELECT_DISTANCE, ScrollPosition};

	#[derive(Properties)]
	#[properties(wrapper_type = super::GuiView)]
	pub struct GuiView {
		#[property(override_interface = Scrollable, nullable, get, set = Self::set_vadjustment)]
		vadjustment: RefCell<Option<Adjustment>>,
		#[property(override_interface = Scrollable, nullable, get, set = Self::set_hadjustment)]
		hadjustment: RefCell<Option<Adjustment>>,
		#[property(override_interface = Scrollable, get, set, builder(ScrollablePolicy::Minimum))]
		hscroll_policy: Cell<ScrollablePolicy>,
		#[property(override_interface = Scrollable, get, set, builder(ScrollablePolicy::Minimum))]
		vscroll_policy: Cell<ScrollablePolicy>,
		#[property(get, set)]
		scrollable: Cell<bool>,
		render_han: Cell<bool>,
		data: RefCell<GuiViewData>,
		render: RefCell<Box<dyn GuiRender>>,
	}

	impl Default for GuiView {
		fn default() -> Self {
			GuiView {
				vadjustment: RefCell::new(None),
				hadjustment: RefCell::new(None),
				hscroll_policy: Cell::new(ScrollablePolicy::Minimum),
				vscroll_policy: Cell::new(ScrollablePolicy::Minimum),
				scrollable: Cell::new(false),
				render_han: Cell::new(false),
				data: RefCell::new(GuiViewData {
					render_rect: Rect::NOTHING,
					render_lines: vec![],
					block_backgrounds: vec![],
					block_borders: vec![],
					draw_data: None,
					font_family_names: None,
				}),
				render: RefCell::new(create_render(false)),
			}
		}
	}

	struct GuiViewData {
		render_rect: Rect,
		render_lines: Vec<RenderLine>,
		block_backgrounds: Vec<BlockBackgroundEntry>,
		block_borders: Vec<TextDecoration>,
		draw_data: Option<ScrolledDrawData>,
		font_family_names: Option<IndexSet<String>>,
	}

	#[glib::object_subclass]
	impl ObjectSubclass for GuiView {
		const NAME: &'static str = "BookView";
		type Type = super::GuiView;
		type ParentType = gtk4::DrawingArea;
		type Interfaces = (Scrollable,);

		fn class_init(clazz: &mut Self::Class) {
			clazz.set_css_name(super::GuiView::WIDGET_NAME);
		}
	}

	#[glib::derived_properties]
	impl ObjectImpl for GuiView {
		fn signals() -> &'static [Signal]
		{
			static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
			SIGNALS.get_or_init(|| {
				vec![
					Signal::builder(super::GuiView::OPEN_LINK_SIGNAL)
						.param_types([
							<u64>::static_type(),
							<u64>::static_type(),
						])
						.run_last()
						.build(),
					Signal::builder(super::GuiView::OPEN_LINK_EXTERNAL_SIGNAL)
						.param_types([
							<u64>::static_type(),
							<u64>::static_type(),
						])
						.run_last()
						.build(),
					Signal::builder(super::GuiView::OPEN_IMAGE_EXTERNAL_SIGNAL)
						.param_types([
							<u64>::static_type(),
							<u64>::static_type(),
						])
						.run_last()
						.build(),
					Signal::builder(super::GuiView::SELECTING_TEXT_SIGNAL)
						.param_types([
							<u64>::static_type(),
							<u64>::static_type(),
							<u64>::static_type(),
							<u64>::static_type(),
						])
						.run_last()
						.build(),
					Signal::builder(super::GuiView::TEXT_SELECTED_SIGNAL)
						.param_types([
							<u64>::static_type(),
							<u64>::static_type(),
							<u64>::static_type(),
							<u64>::static_type(),
						])
						.run_last()
						.build(),
					Signal::builder(super::GuiView::CLEAR_SELECTION_SIGNAL)
						.run_last()
						.build(),
					Signal::builder(super::GuiView::SELECT_WORD_SIGNAL)
						.param_types([
							<u64>::static_type(),
							<u64>::static_type(),
						])
						.run_last()
						.build(),
					Signal::builder(super::GuiView::SCROLL_SIGNAL)
						.param_types([
							<i32>::static_type(),
						])
						.run_last()
						.build(),
					Signal::builder(super::GuiView::SHOW_TITLE_SIGNAL)
						.param_types([
							<bool>::static_type(),
							<u64>::static_type(),
							<u64>::static_type(),
						])
						.run_last()
						.build(),
				]
			})
		}
	}

	impl WidgetImpl for GuiView {
		fn snapshot(&self, snapshot: &Snapshot)
		{
			let data = self.data.borrow();
			let obj = self.obj();
			let width = obj.width() as f32;
			let height = obj.height() as f32;
			let rect = graphene::Rect::new(0.0, 0.0, width, height);
			let cairo = snapshot.append_cairo(&rect);
			let render = self.render.borrow();
			let (render_lines, block_borders, block_backgrounds) = if let Some(draw_data) = &data.draw_data {
				let offset = &draw_data.offset;
				cairo.translate(offset.x as f64, offset.y as f64);
				(&data.render_lines[draw_data.range.clone()],
					&data.block_borders,
					&data.block_backgrounds)
			} else {
				(data.render_lines.as_slice(),
					&data.block_borders,
					&data.block_backgrounds)
			};
			render.draw(
				render_lines,
				block_borders,
				block_backgrounds,
				&data.font_family_names,
				&cairo,
				&self.obj().get_pango());
		}
	}

	impl DrawingAreaImpl for GuiView {}

	impl ScrollableImpl for GuiView {}

	impl GuiView {
		fn adjustment_value_handle(&self, adjustment: &Option<Adjustment>)
		{
			if let Some(adjustment) = &adjustment {
				let bv = self.obj().clone();
				adjustment.connect_value_changed(move |adjustment| {
					let imp = bv.imp();
					let mut data = imp.data.borrow_mut();
					let draw_data = imp.render.borrow().visible_scrolling(
						adjustment.value() as f32,
						adjustment.upper() as f32,
						&data.render_rect,
						&data.render_lines,
					);
					data.draw_data = draw_data;
					bv.queue_draw();
				});
			}
		}

		pub fn set_hadjustment(&self, adjustment: Option<Adjustment>)
		{
			self.adjustment_value_handle(&adjustment);
			self.hadjustment.replace(adjustment);
		}

		pub fn set_vadjustment(&self, adjustment: Option<Adjustment>)
		{
			self.adjustment_value_handle(&adjustment);
			self.vadjustment.replace(adjustment);
		}

		#[inline]
		fn adjustment<F, T>(&self, f: F) -> T
		where
			F: FnOnce(&Adjustment) -> T,
		{
			assert!(self.scrollable.get());
			let adjustment = if self.render_han.get() {
				&self.hadjustment
			} else {
				&self.vadjustment
			};
			let adjustment = adjustment.borrow();
			let adjustment = adjustment.as_ref()
				.expect("No adjustment for scrollable book view");
			f(&adjustment)
		}

		#[inline]
		pub(super) fn translate(&self, mouse_pos: &mut Pos2,
			render: &dyn GuiRender, render_rect: &Rect)
		{
			if self.render_han.get() {
				if let Some(adjustment) = &self.hadjustment.borrow().as_ref() {
					render.translate_mouse_pos(
						mouse_pos,
						render_rect,
						adjustment.value() as f32,
						adjustment.upper() as f32);
				}
			} else if let Some(adjustment) = &self.vadjustment.borrow().as_ref() {
				render.translate_mouse_pos(
					mouse_pos,
					render_rect,
					adjustment.value() as f32,
					adjustment.upper() as f32);
			};
		}

		pub(super) fn book_loaded(&self, book: &dyn Book, reading: &ReadingInfo,
			pango: &PangoContext, context: &mut RenderContext)
		{
			context.custom_font = reading.custom_font;
			context.custom_color = reading.custom_color;
			context.leading_chars = book.leading_space();
			let mut render = self.render.borrow_mut();
			render.image_cache_mut().clear();
			render.apply_font_modified(book.custom_fonts(), pango, context);
			render.reset_render_context(context);
		}

		pub(super) fn redraw(&self, book: &dyn Book, lines: &Vec<Line>, line: usize,
			offset: usize, highlight: &Option<HighlightInfo>, context: &mut RenderContext,
			pango: &PangoContext) -> Option<Position>
		{
			if self.scrollable.get() {
				self.full_redraw(book, lines, highlight, context, pango);
				None
			} else {
				let mut render = self.render.borrow_mut();
				let mut data = self.data.borrow_mut();
				sync_font_family_names(
					&mut data,
					book.font_family_names(),
					&mut render,
				);
				let (render_lines, block_borders, block_backgrounds, next) = render.gui_redraw(book, lines, line, offset, highlight,
					pango, context);
				data.render_lines = render_lines;
				data.block_borders = block_borders;
				data.block_backgrounds = block_backgrounds;
				next
			}
		}

		#[inline]
		pub(super) fn prev_page(&self, book: &dyn Book, lines: &Vec<Line>,
			line: usize, offset: usize, pango: &PangoContext, context: &mut RenderContext) -> Position
		{
			let mut render = self.render.borrow_mut();
			render.gui_prev_page(book, lines, line, offset, pango, context)
		}

		#[inline]
		pub(super) fn next_line(&self, book: &dyn Book, lines: &Vec<Line>,
			line: usize, offset: usize, pango: &PangoContext, context: &mut RenderContext) -> Position
		{
			let mut render = self.render.borrow_mut();
			render.gui_next_line(book, lines, line, offset, pango, context)
		}

		#[inline]
		pub(super) fn prev_line(&self, book: &dyn Book, lines: &Vec<Line>,
			line: usize, offset: usize, pango: &PangoContext, context: &mut RenderContext) -> Position
		{
			let mut render = self.render.borrow_mut();
			render.gui_prev_line(book, lines, line, offset, pango, context)
		}

		#[inline]
		pub(super) fn setup_highlight(&self, book: &dyn Book, lines: &Vec<Line>,
			line: usize, start: usize, pango: &PangoContext, context: &mut RenderContext) -> Position
		{
			let mut render = self.render.borrow_mut();
			render.gui_setup_highlight(book, lines, line, start, pango, context)
		}

		pub(super) fn full_redraw(&self, book: &dyn Book, lines: &[Line],
			highlight: &Option<HighlightInfo>,
			render_context: &mut RenderContext, pango: &PangoContext)
		{
			let view_size = render_context.max_page_size;
			render_context.max_page_size = f32::INFINITY;
			let mut render = self.render.borrow_mut();
			let (lines, block_borders, block_backgrounds, _) = render.gui_redraw(
				book, lines, 0, 0,
				highlight, pango, render_context);
			let sizing = render.scroll_size(render_context);
			render_context.max_page_size = view_size;
			sync_font_family_names(
				&mut self.data.borrow_mut(),
				book.font_family_names(),
				&mut render,
			);

			self.adjustment(|adjustment| {
				let value = match &render_context.scroll_redraw_method {
					ScrollRedrawMethod::ResetScroll => sizing.init_scroll_value as f64,
					ScrollRedrawMethod::NoResetScroll => adjustment.value(),
					ScrollRedrawMethod::ScrollTo(value) => *value,
				};
				let mut data = self.data.borrow_mut();
				data.render_lines = lines;
				data.block_borders = block_borders;
				data.block_backgrounds = block_backgrounds;
				let draw_data = render.visible_scrolling(
					value as f32, sizing.full_size,
					&render_context.render_rect, &data.render_lines);
				data.draw_data = draw_data;
				drop(data);
				drop(render);

				adjustment.configure(
					value,
					0.,
					sizing.full_size as f64,
					sizing.step_size as f64,
					sizing.page_size as f64,
					sizing.page_size as f64,
				);
			})
		}

		pub(super) fn scroll_pos(&self) -> Option<f64>
		{
			self.adjustment(|adjustment| Some(adjustment.value()))
		}

		pub(super) fn scroll_to(&self, position: ScrollPosition)
		{
			self.adjustment(|adjustment| {
				let value = match position {
					ScrollPosition::LineNext => adjustment.value() + adjustment.step_increment(),
					ScrollPosition::LinePrev => adjustment.value() - adjustment.step_increment(),
					ScrollPosition::PageNext => adjustment.value() + adjustment.page_increment(),
					ScrollPosition::PagePrev => adjustment.value() - adjustment.page_increment(),
					ScrollPosition::Begin => 0.,
					ScrollPosition::End => adjustment.upper(),
					ScrollPosition::Position(value) => value,
				};
				adjustment.set_value(value);
			});
		}

		#[inline(always)]
		pub(super) fn set_render_type(&self, render_han: bool, render_context: &mut RenderContext)
		{
			if self.scrollable.get() {
				self.adjustment(|adjustment| adjustment.configure(0., 0., 0., 0., 0., 0.));
			}
			let mut render = self.render.borrow_mut();
			if render.render_han() != render_han {
				*render = create_render(render_han);
			}
			self.render_han.replace(render_han);
			render.reset_render_context(render_context);
		}

		#[inline(always)]
		pub(super) fn set_fonts(&self, book_fonts: Option<&HtmlFonts>,
			user_fonts: Rc<Option<UserFonts>>, pango: &PangoContext,
			render_context: &mut RenderContext)
		{
			render_context.fonts = user_fonts;
			if !render_context.custom_font || book_fonts.is_none() {
				let mut render = self.render.borrow_mut();
				render.apply_font_modified(book_fonts, pango, render_context);
			}
		}

		pub(super) fn set_font_size(&self, font_size: u8, book_fonts: Option<&HtmlFonts>, render_context: &mut RenderContext,
			pango: &PangoContext)
		{
			render_context.font_size = font_size;
			let mut render = self.render.borrow_mut();
			render.apply_font_modified(book_fonts, pango, render_context);
		}

		pub(super) fn set_custom_font(&self, custom_font: bool, book_fonts: Option<&HtmlFonts>,
			pango: &PangoContext, render_context: &mut RenderContext)
		{
			render_context.custom_font = custom_font;
			if book_fonts.is_some() {
				let mut render = self.render.borrow_mut();
				render.apply_font_modified(book_fonts, pango, render_context);
			}
		}

		pub fn resized(&self, width: i32, height: i32, render_context: &mut RenderContext)
		{
			render_context.update_render_rect(width as f32, height as f32);

			let mut render = self.render.borrow_mut();
			render.reset_baseline(render_context);
			render.reset_render_context(render_context);
			let mut data = self.data.borrow_mut();
			data.render_rect = render_context.render_rect.clone();
		}

		pub fn calc_selection(&self, mut original_pos: Pos2, mut current_pos: Pos2)
			-> Option<(Position, Position)>
		{
			#[inline]
			fn offset_index(line: &RenderLine, offset: &PointerPosition) -> usize {
				match offset {
					PointerPosition::Head => line.first_offset(),
					PointerPosition::Exact(offset) => line.char_at_index(*offset).offset,
					PointerPosition::Tail => line.last_offset(),
				}
			}
			fn select_all(lines: &Vec<RenderLine>) -> (Position, Position)
			{
				let render_line = lines.first().unwrap();
				let from = Position::new(
					render_line.line(),
					render_line.first_offset(),
				);
				let render_line = lines.last().unwrap();
				let to = Position::new(
					render_line.line(),
					render_line.last_offset(),
				);
				(from, to)
			}

			fn head_to_exact(line: usize, offset: &PointerPosition, lines: &Vec<RenderLine>) -> (Position, Position) {
				let render_line = lines.first().unwrap();
				let from = Position::new(
					render_line.line(),
					render_line.first_offset(),
				);
				let render_line = &lines[line];
				let to = Position::new(
					render_line.line(),
					offset_index(render_line, offset),
				);
				(from, to)
			}
			fn exact_to_tail(line: usize, offset: &PointerPosition, lines: &Vec<RenderLine>) -> (Position, Position) {
				let render_line = &lines[line];
				let from = Position::new(
					render_line.line(),
					offset_index(render_line, offset),
				);
				let render_line = lines.last().unwrap();
				let to = Position::new(
					render_line.line(),
					render_line.last_offset(),
				);
				(from, to)
			}

			let data = self.data.borrow_mut();
			let lines = &data.render_lines;
			let line_count = lines.len();
			if line_count == 0 {
				return None;
			}
			let render = self.render.borrow_mut();
			let render_rect = &data.render_rect;
			self.translate(&mut original_pos, render.as_ref(), render_rect);
			self.translate(&mut current_pos, render.as_ref(), render_rect);

			if (original_pos.x - current_pos.x).abs() < MIN_TEXT_SELECT_DISTANCE
				&& (original_pos.y - current_pos.y).abs() < MIN_TEXT_SELECT_DISTANCE {
				return None;
			}
			let (line1, offset1) = render.pointer_pos(&original_pos, &data.render_lines, render_rect);
			let (line2, offset2) = render.pointer_pos(&current_pos, &data.render_lines, render_rect);

			let (from, to) = match line1 {
				PointerPosition::Head => match line2 {
					PointerPosition::Head => return None,
					PointerPosition::Exact(line2) => head_to_exact(line2, &offset2, lines),
					PointerPosition::Tail => select_all(lines),
				}
				PointerPosition::Exact(line1) => match line2 {
					PointerPosition::Head => head_to_exact(line1, &offset1, lines),
					PointerPosition::Exact(line2) => {
						let render_line = &lines[line1];
						let from = Position::new(
							render_line.line(),
							offset_index(render_line, &offset1),
						);
						let render_line = &lines[line2];
						let to = Position::new(
							render_line.line(),
							offset_index(render_line, &offset2),
						);
						(from, to)
					}
					PointerPosition::Tail => exact_to_tail(line1, &offset1, lines),
				}
				PointerPosition::Tail => match line2 {
					PointerPosition::Head => select_all(lines),
					PointerPosition::Exact(line2) => exact_to_tail(line2, &offset2, lines),
					PointerPosition::Tail => return None
				}
			};
			Some((from, to))
		}

		#[inline]
		pub(super) fn pointer_info<F, T>(&self, mut pointer_position: Pos2, f: F) -> Option<T>
		where
			F: FnOnce(&RenderLine, &RenderChar) -> Option<T>,
		{
			let data = self.data.borrow();
			let render_lines = &data.render_lines;
			let render = self.render.borrow();
			let render_rect = &data.render_rect;
			self.translate(&mut pointer_position, render.as_ref(), render_rect);
			if let (PointerPosition::Exact(line), PointerPosition::Exact(offset)) = render
				.pointer_pos(&pointer_position, render_lines, render_rect) {
				let render_line = render_lines.get(line)?;
				let dc = render_line.char_at_index(offset);
				f(render_line, dc)
			} else {
				None
			}
		}

		#[inline]
		pub fn resolve_click(&self, mouse_position: Pos2, state: ModifierType) -> ClickTarget
		{
			self.pointer_info(mouse_position, |line, dc| {
				let target = match dc.cell {
					RenderCell::Link(_, link_index) =>
						if state.eq(&(ModifierType::CONTROL_MASK)) {
							ClickTarget::ExternalLink(line.line(), link_index)
						} else {
							ClickTarget::Link(line.line(), link_index)
						},
					RenderCell::Image(_, link_index) =>
						if state.eq(&(ModifierType::CONTROL_MASK)) {
							ClickTarget::Image(line.line(), dc.offset)
						} else if let Some(link_index) = link_index {
							ClickTarget::Link(line.line(), link_index)
						} else {
							return None;
						}
					RenderCell::Char(_) =>
						ClickTarget::Char(line.line(), dc.offset),
				};
				Some(target)
			}).unwrap_or(ClickTarget::None)
		}

		#[cfg(not(windows))]
		pub fn pointer_cursor(&self, dc: &RenderChar, state: ModifierType) -> &str
		{
			let name = match dc.cell {
				RenderCell::Char(_) => None,
				RenderCell::Image(_, None) => if state.eq(&ModifierType::CONTROL_MASK) {
					Some("zoom-in")
				} else {
					None
				}
				RenderCell::Link(_, _) |
				RenderCell::Image(_, Some(_)) => Some("pointer"),
			};
			name.unwrap_or_else(|| {
				if self.render_han.get() {
					"vertical-text"
				} else {
					"text"
				}
			})
		}

		#[cfg(windows)]
		pub fn pointer_cursor(&self, dc: &RenderChar, state: ModifierType) -> &str
		{
			let name = match dc.cell {
				RenderCell::Char(_) => None,
				RenderCell::Image(_, None) => if state.eq(&ModifierType::CONTROL_MASK) {
					Some("pointer")
				} else {
					None
				}
				RenderCell::Link(_, _) |
				RenderCell::Image(_, Some(_)) => Some("pointer"),
			};
			name.unwrap_or("default")
		}
	}

	// with different name for family index, the cache with family invalid
	fn sync_font_family_names(data: &mut GuiViewData,
		font_family_names: Option<&IndexSet<String>>,
		render: &mut Box<dyn GuiRender>)
	{
		if let Some(curr) = font_family_names {
			if let Some(orig) = &data.font_family_names {
				let mut copy = false;
				let orig_len = orig.len();
				let curr_len = curr.len();
				let len = min(orig.len(), curr.len());
				for i in 0..len {
					if orig[i] != curr[i] {
						render.clear_cache_with_family();
						copy = true;
						break;
					}
				}
				if copy || orig_len < curr_len {
					data.font_family_names = Some(curr.clone());
				}
			} else {
				data.font_family_names = Some(curr.clone());
			}
		}
	}
}

pub fn init_css(colors: &Colors) -> CssProvider
{
	let css_provider = CssProvider::new();
	update_css(&css_provider, colors);
	gtk4::style_context_add_provider_for_display(
		&Display::default().expect("Could not connect to a display."),
		&css_provider,
		gtk4::STYLE_PROVIDER_PRIORITY_USER,
	);
	css_provider
}

#[inline]
pub fn update_css(css_provider: &CssProvider, colors: &Colors)
{
	let css = format!("{}#main {{background: {};}}", GuiView::WIDGET_NAME, &colors.background);
	css_provider.load_from_string(&css);
}

pub fn update_mouse_pointer(view: &GuiView, x: f32, y: f32, state: ModifierType)
{
	let pos = pos2(x, y);
	let imp = view.imp();
	let title_info = imp.pointer_info(pos, |render_line, render_char| {
		let cursor_name = imp.pointer_cursor(render_char, state);
		view.set_cursor_from_name(Some(cursor_name));
		if render_char.has_title {
			Some((render_line.line(), render_char.offset))
		} else {
			None
		}
	});
	if let Some((line, offset)) = title_info {
		view.emit_by_name::<()>(GuiView::SHOW_TITLE_SIGNAL, &[
			&true,
			&(line as u64),
			&(offset as u64),
		]);
	} else {
		view.emit_by_name::<()>(GuiView::SHOW_TITLE_SIGNAL, &[
			&false,
			&(0u64),
			&(0u64),
		]);
	}
}
