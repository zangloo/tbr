use std::env;
use std::process::Command;
use fancy_regex::Regex;

pub trait InputMethod {
	fn is_active(&self) -> bool;
	fn set_active(&mut self, active: Option<bool>, update_restore: bool);
}

#[cfg(windows)]
pub(crate) fn setup_im() -> Option<Box<dyn InputMethod>>
{
	None
}

#[cfg(unix)]
pub fn setup_im() -> Option<Box<dyn InputMethod>>
{
	fn detect_im() -> Option<impl InputMethod>
	{
		let re = Regex::new(r"^@im=(.+)$").unwrap();
		let xmodifiers = env::var("XMODIFIERS").ok()?;
		let captures = re.captures(&xmodifiers).ok()??;
		let im_name = captures.get(1)?;
		let im_name = im_name.as_str();
		if im_name == "fcitx" {
			Fcitx::new()
		} else {
			None
		}
	}
	let mut im = detect_im()?;
	im.set_active(Some(false), true);
	Some(Box::new(im))
}

#[cfg(unix)]
struct Fcitx {
	active: bool,
	version5: bool,
}

#[cfg(unix)]
impl Fcitx {
	fn detect_active(cmd: &str) -> Option<bool>
	{
		if let Ok(output) = Command::new(cmd).output() {
			Some(output.stdout.get(0) == Some(&b'2'))
		} else {
			None
		}
	}

	fn new() -> Option<Self>
	{
		if let Some(active) = Fcitx::detect_active("fcitx5-remote") {
			return Some(Fcitx { version5: true, active });
		} else if let Some(active) = Fcitx::detect_active("fcitx-remote") {
			return Some(Fcitx { version5: false, active });
		} else {
			None
		}
	}

	fn set_active_internal(&self, active: bool)
	{
		if self.version5 {
			run_command("fcitx5-remote", if active { &["-o"] } else { &["-c"] });
		} else {
			run_command("fcitx-remote", if active { &["-o"] } else { &["-c"] });
		}
	}
}

#[cfg(unix)]
impl InputMethod for Fcitx {
	fn is_active(&self) -> bool
	{
		let active = if self.version5 {
			Fcitx::detect_active("fcitx5-remote")
		} else {
			Fcitx::detect_active("fcitx-remote")
		};
		if let Some(active) = active {
			active
		} else {
			false
		}
	}

	fn set_active(&mut self, active: Option<bool>, update_restore: bool)
	{
		let is_active = self.is_active();
		if let Some(active) = active {
			if active != is_active {
				self.set_active_internal(active);
			}
		} else if self.active != is_active {
			self.set_active_internal(self.active)
		}
		if update_restore {
			self.active = is_active;
		}
	}
}

/// exec command and ignore error
#[cfg(unix)]
fn run_command(cmd: &str, args: &[&str]) -> bool
{
	if let Ok(mut child) = Command::new(cmd).args(args).spawn() {
		if let Ok(status) = child.wait() {
			return status.success();
		}
	}
	false
}