mod imp;
mod han;
mod xi;

pub use imp::GuiRender;
pub use imp::PointerPosition;
pub use imp::RenderContext;
pub use imp::RenderLine;
pub use imp::RenderCell;
pub use imp::RenderChar;
pub use imp::ScrollRedrawMethod;
pub use imp::ScrolledDrawData;

use imp::{*};

use crate::gui::render::han::GuiHanRender;
use crate::gui::render::xi::GuiXiRender;

pub fn create_render(render_han: bool) -> Box<dyn GuiRender>
{
	if render_han {
		Box::new(GuiHanRender::new())
	} else {
		Box::new(GuiXiRender::new())
	}
}

