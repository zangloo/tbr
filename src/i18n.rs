use anyhow::Result;
use std::borrow::Cow;
use std::collections::HashMap;
use anyhow::anyhow;
use fluent::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;
use crate::Asset;

pub struct I18n {
	bundles: HashMap<String, FluentBundle<FluentResource>>,
	locale_list: Vec<(String, String)>,
	locale: String,
}

impl I18n
{
	pub fn new(locale: &str) -> Result<Self>
	{
		let mut bundles = HashMap::new();
		let mut locale_list = vec![];
		for file in Asset::iter() {
			if file.starts_with("i18n/") && file.ends_with(".ftl") {
				let content: Cow<'static, [u8]> = Asset::get(file.as_ref()).unwrap().data;
				let text = String::from_utf8(content.to_vec()).unwrap();
				let res = FluentResource::try_new(text).expect(&format!("Failed to parse an FTL string: {}", file));

				let name = &file["i18n/".len()..file.len() - 4];
				let langid_en: LanguageIdentifier = name.parse().expect(&format!("Parsing fluent failed: {}", file));
				let mut bundle = FluentBundle::new(vec![langid_en]);
				bundle.add_resource(res).expect(&format!("Failed to add FTL resources to the bundle for : {}", name));
				let locale_name = bundle_msg(&bundle, "title", None).expect(&format!("No title defined in : {file}"));
				locale_list.push((name.to_string(), locale_name.to_string()));
				bundles.insert(name.to_string(), bundle);
			}
		}
		if !bundles.contains_key(locale) {
			return Err(anyhow!("No locale defined: {}", locale));
		}
		Ok(I18n { bundles, locale: locale.to_string(), locale_list })
	}

	pub fn set_locale(&mut self, locale: &str) -> Result<()>
	{
		if !self.bundles.contains_key(locale) {
			return Err(anyhow!("No locale defined: {}", locale));
		}
		self.locale = locale.to_string();
		Ok(())
	}

	pub fn msg(&self, key: &str) -> Cow<str>
	{
		let bundle = self.bundles.get(&self.locale).unwrap();
		bundle_msg(bundle, key, None).expect(&format!("No {key} defined in {}", self.locale))
	}

	pub fn args_msg<'a>(&self, key: &str, msg_args: Vec<(&'a str, impl Into<FluentValue<'a>>)>) -> String
	{
		let bundle = self.bundles.get(&self.locale).unwrap();
		let mut args = FluentArgs::new();
		for (name, value) in msg_args {
			args.set(name, value);
		}
		let msg = bundle_msg(bundle, key, Some(&args)).expect(&format!("No {key} defined in {}", self.locale));
		msg.to_string()
	}

	pub fn locales(&self) -> &Vec<(String, String)>
	{
		&self.locale_list
	}
}

fn bundle_msg<'a>(bundle: &'a FluentBundle<FluentResource>, key: &str, args: Option<&'a FluentArgs>) -> Option<Cow<'a, str>>
{
	let message = bundle.get_message(key)?;
	let pattern = message.value()?;
	let mut errors = vec![];
	let text = bundle.format_pattern(pattern, args, &mut errors);
	Some(text)
}