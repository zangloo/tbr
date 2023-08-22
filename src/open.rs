use std::{env, fs};
use std::path::PathBuf;
use anyhow::Result;
use rand::distributions::Alphanumeric;
use rand::Rng;

pub struct Opener {
	files: Vec<PathBuf>,
}

impl Default for Opener {
	fn default() -> Self
	{
		Opener { files: vec![] }
	}
}

impl Opener {
	pub fn open_image(&mut self, path: &str, bytes: &[u8]) -> Result<()>
	{
		if let Some(ext_idx) = path.rfind('.') {
			let ext = &path[ext_idx..];
			let tmp_dir = env::temp_dir();
			let tmp_file_path = create_tmp(&tmp_dir, ext, bytes)?;
			open::that(&tmp_file_path)?;
			self.files.push(tmp_file_path);
		}
		Ok(())
	}

	pub fn open_link(&mut self, url: &str) -> Result<()>
	{
		if url.starts_with("http://") || url.starts_with("https://") {
			open::that(url)?;
		}
		Ok(())
	}

	/// impl Drop not called on exit, so need call this manually
	pub fn cleanup(&mut self)
	{
		for tmp in self.files.drain(..) {
			if let Err(err) = fs::remove_file(&tmp) {
				eprint!("Failed delete temp file: {:#?}: {}", tmp, err.to_string());
			}
		}
	}
}

fn create_tmp(tmp_dir: &PathBuf, ext: &str, bytes: &[u8]) -> Result<PathBuf>
{
	loop {
		let name: String = rand::thread_rng()
			.sample_iter(Alphanumeric)
			.take(8)
			.map(char::from)
			.collect();
		let tmp_file = tmp_dir.join(format!("tbr-{name}{ext}"));
		if !tmp_file.exists() {
			fs::write(&tmp_file, bytes)?;
			break Ok(tmp_file);
		}
	}
}
