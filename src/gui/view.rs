use std::rc::Rc;
use ab_glyph::FontVec;
use gtk4::{CssProvider, EventControllerMotion, EventControllerScroll, EventControllerScrollFlags, gdk, GestureDrag, glib};
use glib::Object;
use gtk4::Scrollable;
use gtk4::gdk::Display;
use gtk4::glib::ObjectExt;
use gtk4::pango::Layout as PangoContext;
use gtk4::prelude::{GestureDragExt, GestureExt, WidgetExt};
use gtk4::subclass::prelude::ObjectSubclassIsExt;
use crate::book::{Book, Line};
use crate::color::Color32;
use crate::common::Position;
use crate::controller::{HighlightInfo, Render};
use crate::gui::math::{Pos2, pos2};
use crate::gui::render::{RedrawMode, RenderContext};

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

glib::wrapper! {
    pub struct GuiView(ObjectSubclass<imp::GuiView>)
        @extends gtk4::Widget, gtk4::DrawingArea,
		@implements Scrollable
	;
}

impl Render<RenderContext> for GuiView {
	fn book_loaded(&mut self, book: &dyn Book, context: &mut RenderContext)
	{
		self.imp().book_loaded(book, context);
	}

	#[inline]
	fn redraw(&mut self, book: &dyn Book, lines: &Vec<Line>, line: usize,
		offset: usize, highlight: &Option<HighlightInfo>, context: &mut RenderContext)
		-> Option<Position>
	{
		let position = match &context.redraw_mode {
			RedrawMode::NoScroll => self.imp().redraw(book, lines, line, offset, highlight, context, &self.get_pango()),
			_ => {
				self.imp().full_redraw(book, lines, highlight, context, &self.get_pango());
				None
			}
		};
		self.queue_draw();
		position
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
	pub const WIDGET_NAME: &str = "book-view";
	pub const OPEN_LINK_SIGNAL: &str = "open-link";
	pub const SELECTING_TEXT_SIGNAL: &str = "select-text";
	pub const TEXT_SELECTED_SIGNAL: &str = "text-selected";
	pub const CLEAR_SELECTION_SIGNAL: &str = "clear-selection";
	pub const SCROLL_SIGNAL: &str = "scroll";

	pub fn new(instance_name: &str, render_han: bool, fonts: Rc<Option<Vec<FontVec>>>,
		render_context: &mut RenderContext) -> Self
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
		imp.set_fonts(fonts, pango, render_context);
		view
	}

	pub fn setup_gesture<'a, F>(&self, scrollable: bool, link_resolver: F)
		where F: Fn(&Self, &Pos2) -> Option<(usize, usize)> + Clone + 'static
	{
		let drag_gesture = GestureDrag::builder()
			.button(gdk::BUTTON_PRIMARY)
			.build();
		let view = self.clone();
		drag_gesture.connect_update(move |drag, seq| {
			if let Some(bp) = drag.start_point() {
				if let Some(ep) = drag.point(seq) {
					let imp = view.imp();
					let mut from = pos2(bp.0 as f32, bp.1 as f32);
					let mut to = pos2(ep.0 as f32, ep.1 as f32);
					imp.translate(&mut from);
					imp.translate(&mut to);
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
		let lr = link_resolver.clone();
		drag_gesture.connect_end(move |drag, seq| {
			if let Some(bp) = drag.start_point() {
				if let Some(ep) = drag.point(seq) {
					view.grab_focus();
					let imp = view.imp();
					if bp == ep {
						let mut pos = pos2(bp.0 as f32, bp.1 as f32);
						imp.translate(&mut pos);
						if let Some((line, link_index)) = lr(&view, &pos) {
							view.emit_by_name::<()>(GuiView::OPEN_LINK_SIGNAL, &[
								&(line as u64),
								&(link_index as u64),
							]);
						} else {
							view.emit_by_name::<()>(GuiView::CLEAR_SELECTION_SIGNAL, &[]);
						}
					} else {
						let mut from = pos2(bp.0 as f32, bp.1 as f32);
						let mut to = pos2(ep.0 as f32, ep.1 as f32);
						imp.translate(&mut from);
						imp.translate(&mut to);
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

		let mouse_event = EventControllerMotion::new();
		let view = self.clone();
		let lr = link_resolver.clone();
		mouse_event.connect_motion(move |_, x, y| {
			let mut pos = pos2(x as f32, y as f32);
			let imp = view.imp();
			imp.translate(&mut pos);
			let cursor_name = if let Some(_) = lr(&view, &pos) {
				"pointer"
			} else if imp.is_han_render() {
				"vertical-text"
			} else {
				"text"
			};
			view.set_cursor_from_name(Some(cursor_name))
		});
		self.add_controller(mouse_event);

		if !scrollable {
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

	pub fn set_font_size(&self, font_size: u8, render_context: &mut RenderContext)
	{
		self.imp().set_font_size(font_size, render_context, &self.get_pango());
	}

	pub fn set_fonts(&self, fonts: Rc<Option<Vec<FontVec>>>, render_context: &mut RenderContext)
	{
		self.imp().set_fonts(fonts, &self.get_pango(), render_context);
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

	#[inline(always)]
	pub fn link_resolve(&self, mouse_position: &Pos2, lines: &Vec<Line>) -> Option<(usize, usize)>
	{
		self.imp().link_resolve(mouse_position, lines)
	}
}

mod imp {
	use std::cell::{Cell, RefCell};
	use std::rc::Rc;
	use ab_glyph::FontVec;
	use glib::Properties;
	use gtk4::prelude::*;
	use gtk4::{Adjustment, glib, graphene, Scrollable, ScrollablePolicy, Snapshot};
	use gtk4::glib::once_cell::sync::Lazy;
	use gtk4::glib::StaticType;
	use gtk4::glib::subclass::Signal;
	use gtk4::pango::Layout as PangoContext;
	use gtk4::subclass::drawing_area::DrawingAreaImpl;
	use gtk4::subclass::prelude::*;
	use crate::book::{Book, Line};
	use crate::common::Position;
	use crate::controller::HighlightInfo;
	use crate::gui::math::{Pos2, Rect};
	use crate::gui::render::{create_render, GuiRender, GuiViewScrollDirection, PointerPosition, RedrawMode, RenderContext, RenderLine};
	use crate::gui::view::{MIN_TEXT_SELECT_DISTANCE, ScrollPosition};

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
				data: RefCell::new(GuiViewData {
					render_rect: Rect::NOTHING,
					render_lines: vec![],
					direction: GuiViewScrollDirection::None,
					scroll_size: 0.,
				}),
				render: RefCell::new(create_render(false)),
			}
		}
	}

	struct GuiViewData {
		render_rect: Rect,
		render_lines: Vec<RenderLine>,
		direction: GuiViewScrollDirection,
		scroll_size: f32,
	}

	#[glib::object_subclass]
	impl ObjectSubclass for GuiView {
		const NAME: &'static str = "BookView";
		type Type = super::GuiView;
		type ParentType = gtk4::DrawingArea;
		type Interfaces = (Scrollable, );

		fn class_init(clazz: &mut Self::Class) {
			clazz.set_css_name(super::GuiView::WIDGET_NAME);
		}
	}

	#[glib::derived_properties]
	impl ObjectImpl for GuiView {
		fn signals() -> &'static [Signal]
		{
			static SIGNALS: Lazy<Vec<Signal>> = Lazy::new(|| {
				vec![
					Signal::builder(super::GuiView::OPEN_LINK_SIGNAL)
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
					Signal::builder(super::GuiView::SCROLL_SIGNAL)
						.param_types([
							<i32>::static_type(),
						])
						.run_last()
						.build(),
				]
			});
			SIGNALS.as_ref()
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
			let render_lines = self.adjustment(
				|adjustment| {
					let (pos, lines) = render.visible_scrolling(
						adjustment.value() as f32,
						adjustment.upper() as f32,
						&data.render_rect,
						&data.render_lines);
					cairo.translate(pos.x as f64, pos.y as f64);
					lines
				}
			).unwrap_or(&data.render_lines);
			render.draw(
				render_lines,
				&cairo,
				&self.obj().get_pango());
		}
	}

	impl DrawingAreaImpl for GuiView {}

	impl ScrollableImpl for GuiView {}

	impl GuiView {
		pub fn set_hadjustment(&self, adjustment: Option<Adjustment>)
		{
			if let Some(adjustment) = &adjustment {
				let bv = self.obj().clone();
				adjustment.connect_value_changed(move |_| bv.queue_draw());
			}
			self.hadjustment.replace(adjustment);
		}
		pub fn set_vadjustment(&self, adjustment: Option<Adjustment>)
		{
			if let Some(adjustment) = &adjustment {
				let bv = self.obj().clone();
				adjustment.connect_value_changed(move |_| bv.queue_draw());
			}
			self.vadjustment.replace(adjustment);
		}

		#[inline]
		fn adjustment<F, T>(&self, f: F) -> Option<T>
			where F: FnOnce(&Adjustment) -> T
		{
			let direction = &self.data.borrow().direction;
			let adjustment = match direction {
				GuiViewScrollDirection::Horizontal => &self.hadjustment,
				GuiViewScrollDirection::Vertical => &self.vadjustment,
				GuiViewScrollDirection::None => return None,
			};
			if let Some(adjustment) = adjustment.borrow().as_ref() {
				Some(f(adjustment))
			} else {
				None
			}
		}

		#[inline]
		pub(super) fn translate(&self, mouse_pos: &mut Pos2)
		{
			match self.data.borrow().direction {
				GuiViewScrollDirection::Horizontal => if let Some(adjustment) = &self.hadjustment.borrow().as_ref() {
					self.render.borrow().translate_mouse_pos(
						mouse_pos,
						&self.data.borrow().render_rect,
						adjustment.value() as f32,
						adjustment.upper() as f32);
				}
				GuiViewScrollDirection::Vertical => if let Some(adjustment) = &self.vadjustment.borrow().as_ref() {
					self.render.borrow().translate_mouse_pos(
						mouse_pos,
						&self.data.borrow().render_rect,
						adjustment.value() as f32,
						adjustment.upper() as f32);
				}
				GuiViewScrollDirection::None => {}
			};
		}

		pub(super) fn book_loaded(&self, book: &dyn Book, context: &mut RenderContext)
		{
			context.leading_chars = book.leading_space();
			let mut render = self.render.borrow_mut();
			render.reset_render_context(context);
		}

		pub(super) fn redraw(&self, book: &dyn Book, lines: &Vec<Line>, line: usize,
			offset: usize, highlight: &Option<HighlightInfo>, context: &mut RenderContext,
			pango: &PangoContext) -> Option<Position>
		{
			let mut render = self.render.borrow_mut();
			let (render_lines, next) = render.gui_redraw(book, lines, line, offset, highlight,
				pango, context);
			let mut data = self.data.borrow_mut();
			data.render_lines = render_lines;
			next
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
			let orig_size = render_context.max_page_size;
			render_context.max_page_size = f32::INFINITY;
			let mut render = self.render.borrow_mut();
			let (lines, _) = render.gui_redraw(
				book, lines, 0, 0,
				highlight, pango, render_context);
			let sizing = render.scroll_size(render_context);
			render_context.max_page_size = orig_size;

			let mut data = self.data.borrow_mut();
			data.direction = sizing.direction;
			data.scroll_size = sizing.full_size;
			data.render_lines = lines;
			drop(data);
			self.adjustment(|adjustment| {
				let value = match render_context.redraw_mode {
					RedrawMode::ResetScroll => sizing.init_scroll_value as f64,
					RedrawMode::NoResetScroll => adjustment.value(),
					RedrawMode::ScrollTo(value) => value,
					RedrawMode::NoScroll => panic!("should not be here"),
				};
				adjustment.configure(
					value,
					0.,
					sizing.full_size as f64,
					sizing.step_size as f64,
					sizing.page_size as f64,
					sizing.page_size as f64,
				);
			});
		}

		pub(super) fn scroll_pos(&self) -> Option<f64>
		{
			self.adjustment(|adjustment| adjustment.value())
		}

		pub(super) fn scroll_to(&self, position: ScrollPosition)
		{
			self.adjustment(|adjustment| {
				match position {
					ScrollPosition::LineNext => adjustment.set_value(adjustment.value() + adjustment.step_increment()),
					ScrollPosition::LinePrev => adjustment.set_value(adjustment.value() - adjustment.step_increment()),
					ScrollPosition::PageNext => adjustment.set_value(adjustment.value() + adjustment.page_increment()),
					ScrollPosition::PagePrev => adjustment.set_value(adjustment.value() - adjustment.page_increment()),
					ScrollPosition::Begin => adjustment.set_value(0.),
					ScrollPosition::End => adjustment.set_value(adjustment.upper()),
					ScrollPosition::Position(value) => adjustment.set_value(value),
				}
			});
		}

		#[inline(always)]
		pub(super) fn is_han_render(&self) -> bool
		{
			self.render.borrow().render_han()
		}

		#[inline(always)]
		pub(super) fn set_render_type(&self, render_han: bool, render_context: &mut RenderContext)
		{
			self.adjustment(|adjustment| adjustment.configure(0., 0., 0., 0., 0., 0.));
			let mut render = self.render.borrow_mut();
			if render.render_han() != render_han {
				*render = create_render(render_han);
			}
			render.reset_render_context(render_context);
		}

		#[inline(always)]
		pub(super) fn set_fonts(&self, fonts: Rc<Option<Vec<FontVec>>>, pango: &PangoContext,
			render_context: &mut RenderContext)
		{
			let mut render = self.render.borrow_mut();
			render_context.fonts = fonts;
			render.apply_font_modified(pango, render_context);
		}

		pub(super) fn set_font_size(&self, font_size: u8, render_context: &mut RenderContext,
			pango: &PangoContext)
		{
			render_context.font_size = font_size;
			let mut render = self.render.borrow_mut();
			render.apply_font_modified(pango, render_context);
		}

		pub fn resized(&self, width: i32, height: i32, render_context: &mut RenderContext)
		{
			let width = width as f32;
			let height = height as f32;
			let measure_x = render_context.default_font_measure.x;
			let measure_y = render_context.default_font_measure.y;
			let x_margin = measure_x / 2.0;
			let y_margin = measure_y / 2.0;
			render_context.render_rect = Rect::new(x_margin, y_margin, width - measure_x, height - measure_y);

			let mut render = self.render.borrow_mut();
			render.reset_baseline(render_context);
			render.reset_render_context(render_context);
			let mut data = self.data.borrow_mut();
			data.render_rect = render_context.render_rect.clone();
		}

		pub fn calc_selection(&self, original_pos: Pos2, current_pos: Pos2)
			-> Option<(Position, Position)>
		{
			#[inline]
			fn offset_index(line: &RenderLine, offset: &PointerPosition) -> usize {
				match offset {
					PointerPosition::Head => line.chars.first().map_or(0, |dc| dc.offset),
					PointerPosition::Exact(offset) => line.chars[*offset].offset,
					PointerPosition::Tail => line.chars.last().map_or(0, |dc| dc.offset),
				}
			}
			fn select_all(lines: &Vec<RenderLine>) -> (Position, Position)
			{
				let render_line = lines.first().unwrap();
				let from = Position::new(
					render_line.line,
					render_line.chars.first().map_or(0, |dc| dc.offset),
				);
				let render_line = lines.last().unwrap();
				let to = Position::new(
					render_line.line,
					render_line.chars.last().map_or(0, |dc| dc.offset),
				);
				(from, to)
			}

			fn head_to_exact(line: usize, offset: &PointerPosition, lines: &Vec<RenderLine>) -> (Position, Position) {
				let render_line = lines.first().unwrap();
				let from = Position::new(
					render_line.line,
					render_line.chars.first().map_or(0, |dc| dc.offset),
				);
				let render_line = &lines[line];
				let to = Position::new(
					render_line.line,
					offset_index(render_line, offset),
				);
				(from, to)
			}
			fn exact_to_tail(line: usize, offset: &PointerPosition, lines: &Vec<RenderLine>) -> (Position, Position) {
				let render_line = &lines[line];
				let from = Position::new(
					render_line.line,
					offset_index(render_line, offset),
				);
				let render_line = lines.last().unwrap();
				let to = Position::new(
					render_line.line,
					render_line.chars.last().map_or(0, |dc| dc.offset),
				);
				(from, to)
			}

			let data = self.data.borrow_mut();
			let render = self.render.borrow_mut();
			let render_rect = &data.render_rect;
			let lines = &data.render_lines;
			let line_count = lines.len();
			if line_count == 0 {
				return None;
			}
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
							render_line.line,
							offset_index(render_line, &offset1),
						);
						let render_line = &lines[line2];
						let to = Position::new(
							render_line.line,
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

		pub fn link_resolve(&self, mouse_position: &Pos2, lines: &Vec<Line>) -> Option<(usize, usize)>
		{
			let data = self.data.borrow_mut();
			for line in &data.render_lines {
				if let Some(dc) = line.char_at_pos(mouse_position) {
					if let Some(link_index) = lines[line.line].link_iter(true, |link| {
						if link.range.contains(&dc.offset) {
							(true, Some(link.index))
						} else {
							(false, None)
						}
					}) {
						return Some((line.line, link_index));
					}
				}
			}
			None
		}
	}
}

pub fn init_css(name: &str, background: &Color32) -> CssProvider
{
	let css_provider = CssProvider::new();
	update_css(&css_provider, name, background);
	gtk4::style_context_add_provider_for_display(
		&Display::default().expect("Could not connect to a display."),
		&css_provider,
		gtk4::STYLE_PROVIDER_PRIORITY_USER,
	);
	css_provider
}

#[inline]
pub fn update_css(css_provider: &CssProvider, name: &str, background: &Color32)
{
	let css = format!("{}#{} {{background: {};}}", GuiView::WIDGET_NAME, name, background);
	css_provider.load_from_data(&css);
}
