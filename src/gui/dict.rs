use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::str::FromStr;
use egui::{FontSelection, ImageButton, Key, Modifiers, Rect, RichText, TextEdit, TextStyle, Ui, Vec2, Widget};
use egui_extras::RetainedImage;
use elsa::FrozenMap;
use fancy_regex::{Regex, Captures};
use stardict::{StarDict, StarDictCachedSqlite, WordDefinition};
use crate::book::{Book, Colors, Line};
use crate::{Color32, package_name};
use crate::controller::{highlight_selection, HighlightInfo, Render};
use crate::gui::{ICON_SIZE, image};
use crate::gui::view::{GuiView, ViewAction};
use crate::html_convertor::{html_str_content, HtmlContent};
use crate::i18n::I18n;

const SYS_DICT_PATH: &str = "/usr/share/stardict/dic";
const USER_DICT_PATH_SUFFIXES: [&str; 2] = [
	".stardict",
	"dic",
];
const HTML_DEFINITION_HEAD: &str = "
<style type=\"text/css\">
	.dict-name {
	  color: blue;
	}
	.dict-word {
	}
</style>
<body>
";
const HTML_DEFINITION_TAIL: &str = "</body>";
const INJECT_REGEXP: &str = r#"(<[\\s]*img[^>]+src[\\s]*=[\\s]*")([^"]+)("[^>]*>)|((<[\\s]*u)([^>]*>)(((?!</u>).)*)(</u>))"#;

pub(super) struct DictionaryManager {
	book: DictionaryBook,
	view: GuiView,
	replacer: Regex,

	images: Rc<HashMap<String, RetainedImage>>,

	words: Vec<(String, Vec2)>,
	current_index: Option<usize>,
	current_word: String,
	changed: bool,
	highlight: Option<HighlightInfo>,
	offset: Vec2,

	pub inputting: bool,
}

pub(super) struct LookupResult {
	dict_name: String,
	definitions: Vec<WordDefinition>,
}

struct DictionaryBook {
	dictionaries: Vec<StarDictCachedSqlite>,
	cache: HashMap<String, Vec<LookupResult>>,
	resources: FrozenMap<String, Vec<u8>>,

	content: HtmlContent,
}

impl Book for DictionaryBook
{
	#[inline]
	fn lines(&self) -> &Vec<Line>
	{
		&self.content.lines
	}

	#[inline]
	fn leading_space(&self) -> usize
	{
		0
	}

	fn image(&self, href: &str) -> Option<(String, &[u8])>
	{
		if let Some(bytes) = self.resources.get(href) {
			return if bytes.is_empty() {
				None
			} else {
				Some((href.to_owned(), bytes))
			};
		}

		let (dict_name, href) = href.split_once(":")?;
		for dict in &self.dictionaries {
			if dict.dict_name() == dict_name {
				let bytes = dict.get_resource(href).ok()??;
				let bytes = self.resources.insert(href.to_owned(), bytes);
				return Some((href.to_owned(), bytes));
			}
		}
		self.resources.insert(href.to_owned(), vec![]);
		None
	}
}

impl DictionaryBook {
	fn lookup(&mut self, word: &str) -> Option<&Vec<LookupResult>>
	{
		let result = self.cache
			.entry(word.to_owned())
			.or_insert_with(|| {
				let mut result = vec![];
				for dict in &mut self.dictionaries {
					let dict_name = dict.dict_name().to_owned();
					if let Ok(Some(definitions)) = dict.lookup(word) {
						result.push(LookupResult {
							dict_name,
							definitions,
						});
					}
				}
				result
			});
		if result.len() == 0 {
			None
		} else {
			Some(result)
		}
	}
}

impl DictionaryManager {
	pub fn from(data_path: &Option<PathBuf>, render_type: &str,
		images: Rc<HashMap<String, RetainedImage>>) -> Self
	{
		let mut dictionaries = vec![];
		load_dictionaries(data_path, &mut dictionaries);
		let cache = HashMap::new();
		let book = DictionaryBook {
			dictionaries,
			cache,
			resources: FrozenMap::new(),
			content: HtmlContent {
				title: None,
				lines: vec![],
				id_map: Default::default(),
			},
		};
		let mut view = GuiView::new(render_type, create_colors());
		view.set_custom_color(true);
		DictionaryManager {
			book,
			view,
			replacer: Regex::new(INJECT_REGEXP).unwrap(),
			images,

			words: vec![],
			current_index: None,
			current_word: "".to_string(),
			changed: false,
			highlight: None,

			offset: Vec2::ZERO,
			inputting: false,
		}
	}

	#[inline]
	pub fn reload(&mut self, data_path: &Option<PathBuf>)
	{
		self.book.dictionaries.clear();
		self.book.cache.clear();
		load_dictionaries(data_path, &mut self.book.dictionaries);
	}

	pub fn render_toolbar(&mut self, font_size: f32, i18n: &I18n, ui: &mut Ui)
	{
		match self.current_index {
			Some(current_index) if current_index > 0 => {
				let word = RichText::from(
					&self.words[current_index - 1].0)
					.size(font_size);
				let left_id = image(&self.images, ui.ctx(), "backward.svg");
				if ImageButton::new(left_id, ICON_SIZE)
					.ui(ui)
					.on_hover_text_at_pointer(word
					)
					.clicked() {
					self.switch_word(current_index - 1);
				}
			}
			_ => {
				let left_disabled_id = image(&self.images, ui.ctx(), "backward_disabled.svg");
				let button = ImageButton::new(left_disabled_id, ICON_SIZE);
				ui.add_enabled(false, button);
			}
		}
		match self.current_index {
			Some(current_index) if current_index < self.words.len() - 1 => {
				let word = RichText::from(
					&self.words[current_index + 1].0)
					.size(font_size as f32);
				let right_id = image(&self.images, ui.ctx(), "forward.svg");
				if ImageButton::new(right_id, ICON_SIZE)
					.ui(ui)
					.on_hover_text_at_pointer(word)
					.clicked() {
					self.switch_word(current_index + 1);
				}
			}
			_ => {
				let right_disabled_id = image(&self.images, ui.ctx(), "forward_disabled.svg");
				let button = ImageButton::new(right_disabled_id, ICON_SIZE);
				ui.add_enabled(false, button);
			}
		}

		let dict_input = TextEdit::singleline(&mut self.current_word)
			.hint_text(i18n.msg("lookup-dictionary"))
			.font(FontSelection::Style(TextStyle::Heading));
		let response = ui.add(dict_input);
		if response.gained_focus() {
			self.inputting = true;
		}
		if self.inputting && response.ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter)) {
			self.push_dict_word();
			self.inputting = false;
		}
		if response.lost_focus() {
			self.inputting = false;
		}
		if response.clicked_elsewhere() {
			self.inputting = false;
		}
	}

	#[inline]
	fn switch_word(&mut self, new_index: usize)
	{
		if let Some(current_index) = self.current_index {
			self.words[current_index].1 = self.offset;
		}
		self.current_index = Some(new_index);
		self.current_word = self.words[new_index].0.clone();
		self.highlight = None;
		self.changed = true;
	}

	fn push_dict_word(&mut self)
	{
		if self.current_word.is_empty() {
			return;
		}

		let current_index = if let Some(mut current_index) = self.current_index {
			if self.current_word == self.words[current_index].0 {
				return;
			}
			self.words[current_index].1 = self.offset;
			current_index += 1;
			self.words.drain(current_index..);
			current_index
		} else {
			0
		};
		self.words.push((self.current_word.clone(), Vec2::ZERO));
		self.current_index = Some(current_index);
		self.changed = true;
		self.highlight = None;
	}

	#[inline]
	pub fn set_word(&mut self, new_word: String)
	{
		self.current_word = new_word;
		self.push_dict_word();
	}

	#[inline]
	pub fn reset_offset(&mut self) -> Option<Vec2>
	{
		return if self.changed {
			self.changed = false;
			if let Some(current_index) = self.current_index {
				Some(self.words[current_index].1)
			} else {
				Some(Vec2::ZERO)
			}
		} else {
			None
		};
	}

	#[inline]
	#[allow(unused)]
	pub fn reload_render(&mut self, render_type: &str)
	{
		self.view.reload_render(render_type);
	}

	#[inline]
	pub fn render(&mut self, view_rect: &Rect, font_size: u8,
		i18n: &I18n, ui: &mut Ui)
	{
		if let Some(current_index) = self.current_index {
			self.offset = Vec2::new(view_rect.min.x, view_rect.min.y);
			self.lookup_and_render(
				current_index,
				font_size,
				view_rect,
				i18n,
				ui);
		}
	}

	fn lookup_and_render(&mut self, current_index: usize, font_size: u8,
		view_port: &Rect, i18n: &I18n, ui: &mut Ui)
	{
		let word = &self.words[current_index].0;

		if let Some(orig_word) = &self.book.content.title {
			if orig_word == word {
				self.render_view(font_size, view_port, ui);
				return;
			}
		}
		if let Some(results) = self.book.lookup(word) {
			let mut text = String::from(HTML_DEFINITION_HEAD);
			for single in results {
				render_definition(single, &mut text, &self.replacer);
			}
			text.push_str(HTML_DEFINITION_TAIL);
			if let Ok(mut content) = html_str_content(&text, None::<fn(String) -> Option<&'static String>>) {
				content.title = Some(String::from(word));
				self.book.content = content;
				self.render_view(font_size, view_port, ui);
				return;
			} else {
				for single in results {
					render_definition_text(ui, single, font_size as f32);
				}
			}
		} else {
			let msg = i18n.msg("dictionary-no-definition");
			ui.label(RichText::from(msg.as_ref())
				.text_style(TextStyle::Heading)
				.color(Color32::RED)
				.strong());
		}
	}

	fn render_view(&mut self, font_size: u8, view_port: &Rect, ui: &mut Ui)
	{
		let (_, mut redraw, action) = self.view.show(
			ui,
			font_size,
			&self.book,
			true,
			Some(view_port.clone()));
		match action {
			ViewAction::Goto(line, link_index) => {
				if let Some(line) = self.book.lines().get(line) {
					if let Some(link) = line.link_at(link_index) {
						self.set_word(link.target.trim().to_owned());
					}
				}
			}
			ViewAction::SelectText(from, to) => {
				if let Some((from, to)) = self.view.calc_selection(from, to) {
					self.highlight = self.book.range_highlight(from, to);
				} else {
					self.highlight = None;
				}
				redraw = true;
			}
			ViewAction::TextSelectedDone => if let Some(selected_text) = highlight_selection(&self.highlight) {
				ui.output_mut(|output| output.copied_text = selected_text.to_owned());
			}
			_ => {}
		}
		if redraw {
			self.view.redraw(&self.book, &self.book.lines(), 0, 0,
				&self.highlight, ui);
		}
		self.view.draw(ui);
	}
}

fn load_dictionaries(
	data_path: &Option<PathBuf>,
	dictionaries: &mut Vec<StarDictCachedSqlite>)
{
	#[cfg(not(windows))]
	if let Ok(sys_data_path) = PathBuf::from_str(SYS_DICT_PATH) {
		if sys_data_path.is_dir() {
			load_dictionaries_dir(&sys_data_path, dictionaries);
		}
	}

	let user_home = dirs::home_dir();
	if let Some(user_home) = user_home {
		let mut user_data_path = user_home;
		for suffix in USER_DICT_PATH_SUFFIXES {
			user_data_path = user_data_path.join(suffix);
		}
		if user_data_path.is_dir() {
			load_dictionaries_dir(&user_data_path, dictionaries);
		}
	}

	if let Some(custom_data_path) = data_path {
		if custom_data_path.is_dir() {
			load_dictionaries_dir(&custom_data_path, dictionaries);
		}
	}
}

fn load_dictionaries_dir(path: &PathBuf, dictionaries: &mut Vec<StarDictCachedSqlite>)
{
	if let Ok(read) = path.read_dir() {
		for entry in read {
			if let Ok(entry) = entry {
				if let Ok(dict) = stardict::with_sqlite(
					&entry.path(), package_name!()) {
					dictionaries.push(dict);
				}
			}
		}
	}
}

#[inline]
fn render_definition(result: &LookupResult, text: &mut String, replacer: &Regex)
{
	text.push_str(&format!("<h3 class=\"dict-name\">{}</h3>", result.dict_name));
	for definition in &result.definitions {
		text.push_str(&format!("<h3 class=\"dict-word\">{}</h3>", definition.word));
		for segment in &definition.segments {
			let content = if segment.types.contains('h') || segment.types.contains('g') {
				inject_definition(&segment.text, &result.dict_name, replacer)
			} else {
				html_escape::encode_text(&segment.text)
			};
			let html = str::replace(&content, "\n", "<br>");
			text.push_str(&html);
		}
	}
}

#[inline]
fn inject_definition<'a>(html: &'a str, dict_name: &str, replacer: &Regex) -> Cow<'a, str>
{
	replacer.replace_all(html, |caps: &Captures| {
		if let (Some(image1), Some(image2), Some(image3)) = (caps.get(1), caps.get(2), caps.get(3)) {
			format!("{}{}:{}{}", image1.as_str(), dict_name, image2.as_str(), image3.as_str())
		} else if let (Some(u1), Some(u2)) = (caps.get(6), caps.get(7)) {
			let text = u2.as_str();
			format!(r#"<a href="{}"{}{}</a>"#, text, u1.as_str(), text)
		} else {
			panic!("Internal error while inject dict html")
		}
	})
}

#[inline]
fn render_definition_text(ui: &mut Ui, result: &LookupResult, font_size: f32)
{
	ui.label(RichText::from(&result.dict_name)
		.color(Color32::BLUE)
		.text_style(TextStyle::Heading)
		.strong()
		.size(font_size)
	);
	ui.separator();
	for definition in &result.definitions {
		ui.label(RichText::from(&definition.word)
			.text_style(TextStyle::Heading)
			.strong()
			.size(font_size)
		);
		for segment in &definition.segments {
			ui.label(RichText::from(&segment.text)
				.size(font_size));
		}
	}
	ui.separator();
}

#[inline]
fn create_colors() -> Colors
{
	Colors {
		color: Color32::BLACK,
		background: Color32::LIGHT_GRAY,
		highlight: Color32::BLUE,
		highlight_background: Color32::YELLOW,
		link: Color32::BLUE,
	}
}

#[cfg(test)]
mod tests {
	use fancy_regex::Regex;
	use crate::gui::dict::{inject_definition, INJECT_REGEXP};

	#[test]
	fn inject()
	{
		let regex = Regex::new(INJECT_REGEXP).unwrap();
		let html = r#"(參見<span foreground="blue">迴</span>)<br/>huí<br/>ㄏㄨㄟˊ<br/>〔《廣韻》戶恢切，平灰，匣。〕<br/>“<span foreground="blue">違</span>”的被通假字。<br/><b>1.</b>旋轉；回旋。<br/> 《詩‧大雅‧雲漢》：“倬彼雲漢，昭回于天。”<br/>　<u>毛</u>傳：“回，轉也。”<br/>　<u>鄭玄</u>箋：“精光轉運於天。”<br/>　<u>晉</u><u>郭璞</u>《江賦》：“圓淵九回以懸騰，湓流雷呴而電激。”<br/>　<u>清</u><u>劉大櫆</u>《重修鳳山臺記》：“夫氣回于天，薀于地，匯于下，止于高。”<br/><b>2.</b>環繞；包圍。<br/>　<u>銀雀山</u><u>漢</u>墓竹簡《孫臏兵法‧雄牝城》：“營軍趣舍，毋回名水。”<br/>　<u>銀雀山</u><u>漢</u>墓竹簡《孫臏兵法‧五名五恭》：“出則擊之，不出則回之。”<br/>　<u>馬王堆</u><u>漢</u>墓帛書《戰國縱橫家書‧蘇秦謂陳軫章》：“<u>齊</u><u>宋</u>攻<u>魏</u>，<u>楚</u>回<u>雍氏</u>，<u>秦</u>敗<u>屈丐</u>。”<br/><b>3.</b>指周圍，四圍。<br/> 《三輔黃圖‧咸陽故城》：“<u>興樂宮</u>，<u>秦始皇</u>造，<u>漢</u>修飾之，周回二十餘里，<u>漢</u>太后居之。”<br/> 《水滸傳》第六十回：“周回一遭野水，四圍三面高崗。”<br/><b>4.</b>掉轉，轉到相反的方向；扭轉，改變事物的發展方向。<br/> 《楚辭‧離騷》：“回朕車以復路兮，及行迷之未遠。”<br/>　<u>唐</u><u>李白</u>《長干行》：“低頭向暗壁，千喚不一回。”<br/>　<u>宋</u><u>蘇軾</u>《潮州修韓文公廟記》：“故公之精誠，能開<u>衡山</u>之雲，而不能回<u>憲宗</u>之惑。”<br/>　<u>清</u><u>王士禛</u>《池北偶談‧談藝三‧燭雛》：“以滑稽回人主之怒，皆自<u>晏子</u>語得來。”<br/><b>5.</b>指變換方向、位置等。<br/>　<u>宋</u><u>歐陽修</u>《醉翁亭記》：“峰回路轉，有亭翼然。”<br/><b>6.</b>還，返回。<br/>　<u>唐</u><u>杜甫</u>《鄭駙馬池臺喜遇鄭廣文同飲》詩：“燃臍<u>郿塢</u>敗，握節<u>漢</u>臣回。”<br/> 《老殘游記》第十三回：“這時候，雲彩已經回了山，月亮很亮的。”<br/>　<u>魏巍</u>《東方》第三部第十一章：“﹝<u>陸希榮</u>﹞只好尷尬地回到原來的位子坐下來。”<br/><b>7.</b>猶醒。指睡後覺來。<br/>　<u>南唐</u><u>李璟</u>《攤破浣溪沙》詞：“細雨夢回雞塞遠，小樓吹徹玉笙寒。”<br/> 《金瓶梅詞話》第九三回：“剛合眼一場幽夢，猛驚回哭到天明。”<br/><b>8.</b>收回。<br/> 《新唐書‧李乂傳》：“若回所贖之貲，減方困之徭，其澤多矣。”<br/><b>9.</b>改變；變易。<br/> 《三國志‧魏志‧鍾會傳》：“百姓士民，安堵舊業，農不易畝，市不回肆，去累卵之危，就永安之福，豈不美與！”參見“<span foreground="blue">回變</span>”。<br/><b>10.</b>違逆；違背。<br/> 《詩‧大雅‧常武》：“<u>徐方</u>不回，王曰還歸。”<br/>　<u>鄭玄</u>箋：“回猶違也。”<br/>　<u>宋</u><u>蘇軾</u>《東坡志林‧趙高李斯》：“二人之不敢請，亦知<u>始皇</u>之鷙悍而不可回也。”<br/><b>11.</b>邪，邪僻。<br/> 《詩‧小雅‧鼓鐘》：“淑人君子，其德不回。”<br/>　<u>毛</u>傳：“回，邪也。”<br/>　<u>漢</u><u>班昭</u>《東征賦》：“好正直而不回兮，精誠通於神明。”<br/> 《周書‧王羆傳》：“<u>羆</u>輕侮權勢，守正不回，皆此類也。”<br/>　<u>清</u><u>錢謙益</u>《太僕寺少卿杜士全授中憲大夫贊治尹》：“自非秉心不回，邦之司直，其可與于茲選哉！”<br/><b>12.</b>迷惑；擾亂。<br/>　<u>漢</u><u>陸賈</u>《新語‧輔政》：“眾邪合黨，以回人君。”<br/> 《後漢書‧种暠傳》：“富貴不能回其慮，萬物不能擾其心。”<br/><b>13.</b>迂曲；曲折。<br/>　<u>晉</u><u>陸機</u>《答張士然》詩：“回渠繞曲陌，通波扶直阡。”<br/><b>14.</b>引申為屈服、委屈或冤屈。參見“<span foreground="blue">回遠</span>”、“<span foreground="blue">回從</span>”、“<span foreground="blue">回枉</span>”。<br/><b>15.</b>偏向，回護。<br/> 《國語‧晉語八》：“且<u>秦</u><u>楚</u>匹也，若之何其回於富也。乃均其祿。”<br/>　<u>韋昭</u>注：“回，曲也。”<br/><b>16.</b>回避，避讓。<br/>　<u>漢</u><u>劉向</u>《新序‧雜事》：“外舉不避仇讎，內舉不回親戚。”<br/> 《新唐書‧蕭倣傳》：“﹝<u>琢</u>﹞俄起為<u>壽州</u>團練使，<u>倣</u>劾奏<u>琢</u>無所回，時推其直。”<br/><b>17.</b>交易。買進。<br/> 《初刻拍案驚奇》卷八：“兩人一同上酒樓來，<u>陳大郎</u>便問酒保，打了幾角酒，回了一腿羊肉，又擺上些雞魚肉菜之類。”<br/> 《水滸傳》第九回：“當下<u>深</u>、<u>沖</u>、<u>超</u>、<u>霸</u>四人在村酒店中坐下，喚酒保買五七斤肉，打兩角酒來吃。回些麵來打餅。”<br/> 《老殘游記》第四回：“因強盜都有洋槍，鄉下洋槍沒有買處，也不敢買，所以從他們打鳥兒的回兩三枝土槍。”<br/><b>18.</b>指轉賣。<br/>　<u>元</u><u>姚守中</u>《粉蝶兒‧牛訴冤》曲：“好材兒賣與了鞋匠，破皮兒回與田夫。”參見“<span foreground="blue">回易</span>”。<br/><b>19.</b>答覆；回稟；告訴。<br/> 《二刻拍案驚奇》卷十一：“日後他來通消息時，好言回他。”<br/>　<u>清</u><u>李漁</u>《奈何天‧逼嫁》：“你為甚麼不當面回他？”<u>魯迅</u>《故事新編‧奔月》：“‘回老爺，’<u>王升</u>說，‘太太沒有到<u>姚</u>家去。’”<u>洪深</u>《趙閻王》第一幕：“去回排長<u>王老爺</u>，就說沒什麼大事。”<br/><b>20.</b>一方對另一方的行為舉措給以相同形式的回報，均謂之回。參見“<span foreground="blue">回禮</span>”、“<span foreground="blue">回電</span>”、“<span foreground="blue">回嘴</span>”。<br/><b>21.</b>請示；詢問。<br/> 《紅樓夢》第五五回：“<u>鳳姐兒</u>……想起什麼事來，就叫<u>平兒</u>去回<u>王夫人</u>。”<br/> 《老殘游記》第二回：“進得店去，茶房便來回道：‘客人，用什麼夜膳？’”<br/><b>22.</b>辭謝；拒絕。<br/> 《二刻拍案驚奇》卷十七：“<u>子中</u>笑道：‘……<u>聞舍人</u>因為自己已有姻親，﹝聘物﹞不好回得，乃為敝友轉定下了。’”<br/><b>23.</b>與一字連用，指短時間。猶會兒。<br/> 《金瓶梅詞話》第三回：“<u>西門慶</u>和婆子，一遞一句，說了一回。”<br/> 《紅樓夢》第四二回：“<u>寶玉</u>忙收了單子。大家又說了一回閑話兒。”<br/> 《老殘游記》第七回：“話說<u>老殘</u>與<u>申東造</u>議論<u>玉賢</u>正為有才，急於做官，所以喪天害理，至於如此，彼此嘆息了一回。”<br/><b>24.</b>量詞。次。<br/>　<u>唐</u>慕幽《柳》詩：“今古憑君一贈行，幾回折盡復重生。”<br/>　<u>宋</u><u>王安石</u>《送張公儀宰安豐》詩：“雁飛南北三兩回，回首湖山空夢亂。”<br/>　<u>魯迅</u>《兩地書‧致許廣平四》：“這回要先講‘兄’字的講義了。”<br/><b>25.</b>量詞。樁；件。<br/>　<u>魏巍</u>《東方》第一部第一章：“<u>老王</u>弄明白是怎麼回事，把臉一抹，哈哈大笑。”<br/>　<u>靳以</u>《一個中國姑娘》：“我看到了許許多多新鮮可愛的東西，有些我從來沒有看見過，有些我在<u>法國</u>看見過，可是那完全是另外一回事。”<br/><b>26.</b>量詞。小說的一章叫一回。如：《紅樓夢》第八回。<br/><b>27.</b>回族。參見“<span foreground="blue">回回</span>”。<br/><b>28.</b>姓。<br/>　<u>明</u>有<u>回滿住</u>。見《明史‧孝義傳序》。<br/><b>29.</b>同“<span foreground="blue">迴</span>”。"#;
		let injected_html = inject_definition(html, "段注說文解字", &regex);
		assert_eq!(injected_html, r#"(參見<span foreground="blue">迴</span>)<br/>huí<br/>ㄏㄨㄟˊ<br/>〔《廣韻》戶恢切，平灰，匣。〕<br/>“<span foreground="blue">違</span>”的被通假字。<br/><b>1.</b>旋轉；回旋。<br/> 《詩‧大雅‧雲漢》：“倬彼雲漢，昭回于天。”<br/>　<a href="毛">毛</a>傳：“回，轉也。”<br/>　<a href="鄭玄">鄭玄</a>箋：“精光轉運於天。”<br/>　<a href="晉">晉</a><a href="郭璞">郭璞</a>《江賦》：“圓淵九回以懸騰，湓流雷呴而電激。”<br/>　<a href="清">清</a><a href="劉大櫆">劉大櫆</a>《重修鳳山臺記》：“夫氣回于天，薀于地，匯于下，止于高。”<br/><b>2.</b>環繞；包圍。<br/>　<a href="銀雀山">銀雀山</a><a href="漢">漢</a>墓竹簡《孫臏兵法‧雄牝城》：“營軍趣舍，毋回名水。”<br/>　<a href="銀雀山">銀雀山</a><a href="漢">漢</a>墓竹簡《孫臏兵法‧五名五恭》：“出則擊之，不出則回之。”<br/>　<a href="馬王堆">馬王堆</a><a href="漢">漢</a>墓帛書《戰國縱橫家書‧蘇秦謂陳軫章》：“<a href="齊">齊</a><a href="宋">宋</a>攻<a href="魏">魏</a>，<a href="楚">楚</a>回<a href="雍氏">雍氏</a>，<a href="秦">秦</a>敗<a href="屈丐">屈丐</a>。”<br/><b>3.</b>指周圍，四圍。<br/> 《三輔黃圖‧咸陽故城》：“<a href="興樂宮">興樂宮</a>，<a href="秦始皇">秦始皇</a>造，<a href="漢">漢</a>修飾之，周回二十餘里，<a href="漢">漢</a>太后居之。”<br/> 《水滸傳》第六十回：“周回一遭野水，四圍三面高崗。”<br/><b>4.</b>掉轉，轉到相反的方向；扭轉，改變事物的發展方向。<br/> 《楚辭‧離騷》：“回朕車以復路兮，及行迷之未遠。”<br/>　<a href="唐">唐</a><a href="李白">李白</a>《長干行》：“低頭向暗壁，千喚不一回。”<br/>　<a href="宋">宋</a><a href="蘇軾">蘇軾</a>《潮州修韓文公廟記》：“故公之精誠，能開<a href="衡山">衡山</a>之雲，而不能回<a href="憲宗">憲宗</a>之惑。”<br/>　<a href="清">清</a><a href="王士禛">王士禛</a>《池北偶談‧談藝三‧燭雛》：“以滑稽回人主之怒，皆自<a href="晏子">晏子</a>語得來。”<br/><b>5.</b>指變換方向、位置等。<br/>　<a href="宋">宋</a><a href="歐陽修">歐陽修</a>《醉翁亭記》：“峰回路轉，有亭翼然。”<br/><b>6.</b>還，返回。<br/>　<a href="唐">唐</a><a href="杜甫">杜甫</a>《鄭駙馬池臺喜遇鄭廣文同飲》詩：“燃臍<a href="郿塢">郿塢</a>敗，握節<a href="漢">漢</a>臣回。”<br/> 《老殘游記》第十三回：“這時候，雲彩已經回了山，月亮很亮的。”<br/>　<a href="魏巍">魏巍</a>《東方》第三部第十一章：“﹝<a href="陸希榮">陸希榮</a>﹞只好尷尬地回到原來的位子坐下來。”<br/><b>7.</b>猶醒。指睡後覺來。<br/>　<a href="南唐">南唐</a><a href="李璟">李璟</a>《攤破浣溪沙》詞：“細雨夢回雞塞遠，小樓吹徹玉笙寒。”<br/> 《金瓶梅詞話》第九三回：“剛合眼一場幽夢，猛驚回哭到天明。”<br/><b>8.</b>收回。<br/> 《新唐書‧李乂傳》：“若回所贖之貲，減方困之徭，其澤多矣。”<br/><b>9.</b>改變；變易。<br/> 《三國志‧魏志‧鍾會傳》：“百姓士民，安堵舊業，農不易畝，市不回肆，去累卵之危，就永安之福，豈不美與！”參見“<span foreground="blue">回變</span>”。<br/><b>10.</b>違逆；違背。<br/> 《詩‧大雅‧常武》：“<a href="徐方">徐方</a>不回，王曰還歸。”<br/>　<a href="鄭玄">鄭玄</a>箋：“回猶違也。”<br/>　<a href="宋">宋</a><a href="蘇軾">蘇軾</a>《東坡志林‧趙高李斯》：“二人之不敢請，亦知<a href="始皇">始皇</a>之鷙悍而不可回也。”<br/><b>11.</b>邪，邪僻。<br/> 《詩‧小雅‧鼓鐘》：“淑人君子，其德不回。”<br/>　<a href="毛">毛</a>傳：“回，邪也。”<br/>　<a href="漢">漢</a><a href="班昭">班昭</a>《東征賦》：“好正直而不回兮，精誠通於神明。”<br/> 《周書‧王羆傳》：“<a href="羆">羆</a>輕侮權勢，守正不回，皆此類也。”<br/>　<a href="清">清</a><a href="錢謙益">錢謙益</a>《太僕寺少卿杜士全授中憲大夫贊治尹》：“自非秉心不回，邦之司直，其可與于茲選哉！”<br/><b>12.</b>迷惑；擾亂。<br/>　<a href="漢">漢</a><a href="陸賈">陸賈</a>《新語‧輔政》：“眾邪合黨，以回人君。”<br/> 《後漢書‧种暠傳》：“富貴不能回其慮，萬物不能擾其心。”<br/><b>13.</b>迂曲；曲折。<br/>　<a href="晉">晉</a><a href="陸機">陸機</a>《答張士然》詩：“回渠繞曲陌，通波扶直阡。”<br/><b>14.</b>引申為屈服、委屈或冤屈。參見“<span foreground="blue">回遠</span>”、“<span foreground="blue">回從</span>”、“<span foreground="blue">回枉</span>”。<br/><b>15.</b>偏向，回護。<br/> 《國語‧晉語八》：“且<a href="秦">秦</a><a href="楚">楚</a>匹也，若之何其回於富也。乃均其祿。”<br/>　<a href="韋昭">韋昭</a>注：“回，曲也。”<br/><b>16.</b>回避，避讓。<br/>　<a href="漢">漢</a><a href="劉向">劉向</a>《新序‧雜事》：“外舉不避仇讎，內舉不回親戚。”<br/> 《新唐書‧蕭倣傳》：“﹝<a href="琢">琢</a>﹞俄起為<a href="壽州">壽州</a>團練使，<a href="倣">倣</a>劾奏<a href="琢">琢</a>無所回，時推其直。”<br/><b>17.</b>交易。買進。<br/> 《初刻拍案驚奇》卷八：“兩人一同上酒樓來，<a href="陳大郎">陳大郎</a>便問酒保，打了幾角酒，回了一腿羊肉，又擺上些雞魚肉菜之類。”<br/> 《水滸傳》第九回：“當下<a href="深">深</a>、<a href="沖">沖</a>、<a href="超">超</a>、<a href="霸">霸</a>四人在村酒店中坐下，喚酒保買五七斤肉，打兩角酒來吃。回些麵來打餅。”<br/> 《老殘游記》第四回：“因強盜都有洋槍，鄉下洋槍沒有買處，也不敢買，所以從他們打鳥兒的回兩三枝土槍。”<br/><b>18.</b>指轉賣。<br/>　<a href="元">元</a><a href="姚守中">姚守中</a>《粉蝶兒‧牛訴冤》曲：“好材兒賣與了鞋匠，破皮兒回與田夫。”參見“<span foreground="blue">回易</span>”。<br/><b>19.</b>答覆；回稟；告訴。<br/> 《二刻拍案驚奇》卷十一：“日後他來通消息時，好言回他。”<br/>　<a href="清">清</a><a href="李漁">李漁</a>《奈何天‧逼嫁》：“你為甚麼不當面回他？”<a href="魯迅">魯迅</a>《故事新編‧奔月》：“‘回老爺，’<a href="王升">王升</a>說，‘太太沒有到<a href="姚">姚</a>家去。’”<a href="洪深">洪深</a>《趙閻王》第一幕：“去回排長<a href="王老爺">王老爺</a>，就說沒什麼大事。”<br/><b>20.</b>一方對另一方的行為舉措給以相同形式的回報，均謂之回。參見“<span foreground="blue">回禮</span>”、“<span foreground="blue">回電</span>”、“<span foreground="blue">回嘴</span>”。<br/><b>21.</b>請示；詢問。<br/> 《紅樓夢》第五五回：“<a href="鳳姐兒">鳳姐兒</a>……想起什麼事來，就叫<a href="平兒">平兒</a>去回<a href="王夫人">王夫人</a>。”<br/> 《老殘游記》第二回：“進得店去，茶房便來回道：‘客人，用什麼夜膳？’”<br/><b>22.</b>辭謝；拒絕。<br/> 《二刻拍案驚奇》卷十七：“<a href="子中">子中</a>笑道：‘……<a href="聞舍人">聞舍人</a>因為自己已有姻親，﹝聘物﹞不好回得，乃為敝友轉定下了。’”<br/><b>23.</b>與一字連用，指短時間。猶會兒。<br/> 《金瓶梅詞話》第三回：“<a href="西門慶">西門慶</a>和婆子，一遞一句，說了一回。”<br/> 《紅樓夢》第四二回：“<a href="寶玉">寶玉</a>忙收了單子。大家又說了一回閑話兒。”<br/> 《老殘游記》第七回：“話說<a href="老殘">老殘</a>與<a href="申東造">申東造</a>議論<a href="玉賢">玉賢</a>正為有才，急於做官，所以喪天害理，至於如此，彼此嘆息了一回。”<br/><b>24.</b>量詞。次。<br/>　<a href="唐">唐</a>慕幽《柳》詩：“今古憑君一贈行，幾回折盡復重生。”<br/>　<a href="宋">宋</a><a href="王安石">王安石</a>《送張公儀宰安豐》詩：“雁飛南北三兩回，回首湖山空夢亂。”<br/>　<a href="魯迅">魯迅</a>《兩地書‧致許廣平四》：“這回要先講‘兄’字的講義了。”<br/><b>25.</b>量詞。樁；件。<br/>　<a href="魏巍">魏巍</a>《東方》第一部第一章：“<a href="老王">老王</a>弄明白是怎麼回事，把臉一抹，哈哈大笑。”<br/>　<a href="靳以">靳以</a>《一個中國姑娘》：“我看到了許許多多新鮮可愛的東西，有些我從來沒有看見過，有些我在<a href="法國">法國</a>看見過，可是那完全是另外一回事。”<br/><b>26.</b>量詞。小說的一章叫一回。如：《紅樓夢》第八回。<br/><b>27.</b>回族。參見“<span foreground="blue">回回</span>”。<br/><b>28.</b>姓。<br/>　<a href="明">明</a>有<a href="回滿住">回滿住</a>。見《明史‧孝義傳序》。<br/><b>29.</b>同“<span foreground="blue">迴</span>”。"#);
		let html = r#"(迴,回)<br/>huí<br/>ㄏㄨㄟˊ<br/>〔《廣韻》戶恢切，平灰，匣。〕<br/>〔《廣韻》胡對切，去隊，匣。〕<br/><b>1.</b>掉轉；返回。<br/> 《楚辭‧離騷》：“迴朕車以復路兮，及行迷之未遠。”<br/>　<u>王逸</u>注：“迴，旋也。”<br/>　<u>南朝</u><u>宋</u><u>謝惠連</u>《隴西行》：“窮谷是處，考槃是營；千金不迴，百代傳名。”<br/> 《敦煌變文集‧秋胡變文》：“未及行至路傍，正見採桑而迴。”<br/> 《老殘游記》第八回：“車子就放在驢子旁邊，人卻倒迴走了數十步。”<br/><b>2.</b>旋轉；翻轉。<br/>　<u>漢</u><u>司馬遷</u>《報任少卿書》：“是以腸一日而九迴，居則忽忽若有所亡，出則不知其所往。”<br/>　<u>南朝</u><u>梁</u><u>王暕</u>《詠舞詩》：“從風迴綺袖，映日轉花鈿。”<br/>　<u>唐</u><u>李白</u>《大鵬賦》：“左迴右旋，倏陰忽明。”<br/> 《紅樓夢》第五回：“盼纖腰之楚楚兮，風迴雪舞。”<br/><b>3.</b>運轉；循環。<br/> 《呂氏春秋‧季冬》：“是月也，日窮于次，月窮於紀，星迴于天。”<br/>　<u>晉</u><u>盧諶</u>《贈劉琨》詩：“天地盈虛，寒暑周迴。”<br/><b>4.</b>重複某種動作或重現某種現象。<br/>　<u>北魏</u><u>賈思勰</u>《齊民要術‧作酢法》：“迴酒酢法：凡釀酒失所味醋者，或初好後動未壓者，皆宜迴作醋。”<br/>　<u>清</u><u>阮元</u>《小滄浪筆談》卷二：“澗草迴新綠，巖松發古春。”<br/><b>5.</b>環繞；圍繞。<br/>　<u>晉</u><u>張華</u>《博物志》卷四：“<u>始皇陵</u>在<u>驪山</u>之北，高數十丈，周迴六七里。”<br/>　<u>唐</u><u>姜晞</u>《龍池篇》：“靈沼縈迴邸第前，浴日涵春寫曙天。”<br/>　<u>唐</u><u>李白</u>《金陵》詩之二：“地擁<u>金陵</u>勢，城迴<u>江</u>水流。”<br/><b>6.</b>曲折，迂回。<br/>　<u>唐</u><u>杜甫</u>《野老》詩：“野老籬邊江岸迴，柴門不正逐江開。”<br/>　<u>仇兆鰲</u>注：“江岸回曲，其柴門不正設者，為逐江面而開也。”<br/>　<u>明</u><u>王思任</u>《將至京》詩：“平原獨茫茫，道路迴且長。”<br/>　<u>清</u><u>阮元</u>《小滄浪筆談》卷二：“湖平鏡揩，城迴帶曲，野氣沈村，林煙隱屋。”<br/>　<u>清</u><u>彭孫貽</u>《送陶子之淮上》詩之二：“<u>楊子</u>東迴通<u>白下</u>，<u>彭城</u>北望枕<u>黃流</u>。”<br/><b>7.</b>迂回難行。<br/> 《淮南子‧氾論訓》：“夫五行之山，固塞險阻之地也，使我德能覆之，則天下納其貢職者迴也。”<br/>　<u>高誘</u>注：“迴，迂難也。”<br/><b>8.</b>避讓；回避。<br/> 《晉書‧熊遠傳》：“時尚書<u>刁協</u>用事，眾皆憚之。<u><i>尚書郎</i></u><u>盧綝</u>將入直，遇<u>協</u>於大司馬門外。<br/>　<u>協</u>醉，使<u>綝</u>避之，<u>綝</u>不迴。”<br/>　<u>唐</u><u>陳子昂</u>《諫靈駕入京書》：“赴湯鑊而不迴，至誅夷而無悔。”<br/><b>9.</b>改易；轉變。<br/> 《北史‧骨儀傳》：“<u>開皇</u>初，為御史，處法平當，不為勢利所迴。”<br/>　<u>唐</u><u>劉餗</u>《隋唐嘉話》卷中：“<u>梁公</u>夫人至妒……夫人執心不迴。”<br/>　<u>清</u><u>陳康祺</u>《郎潛紀聞》卷一：“公退草疏，置之懷，閉閣自縊，冀以尸諫迴天聽也。”<br/><b>10.</b>收回成命。<br/>　<u>三國</u><u>魏</u><u>阮籍</u>《詣蔣公》：“補吏之召，非所克堪；乞迴謬恩，以光清舉。”<br/>　<u>南朝</u><u>梁</u><u>任昉</u>《為范尚書讓吏部封侯第一表》：“矜臣所乞，特迴寵命。”<br/>　<u>唐</u><u>韓愈</u>《為韋相公讓官表》：“伏乞特迴所授，以示至公之道，天下幸甚。”<br/><b>11.</b>謂把所得的封贈呈請改授他人。<br/> 《隋書‧李敏傳》：“<u>樂平公主</u>之將薨也，遺言於<u>煬帝</u>曰：‘妾無子息，唯有一女；不自憂死，但深憐之。今湯沐邑，乞迴與<u>敏</u>。’帝從之。”<br/>　<u>唐</u><u>劉肅</u>《大唐新語‧舉賢》：“﹝<u>李大亮</u>﹞言於<u>太宗</u>曰：‘臣有今日之榮貴，乃<u>張弼</u>之力也，乞迴臣之官爵以復之。’<u>太宗</u>即以<u>弼</u>為中郎。”<br/><b>12.</b>追述，回憶。<br/> 《北史‧恩幸傳‧王仲興》：“後與領軍<u>于勁</u>參機要，因自迴<u>馬圈</u>侍疾及入<u>金墉</u>功，遂封<u>上黨郡</u>開國公。”<br/><b>13.</b>邪，邪惡。參見“<span foreground="blue">迴邪</span>”。<br/><b>14.</b>量詞。表示動作的次數。<br/>　<u>唐</u><u>柳宗元</u>《同劉二十八哭呂衡州兼寄江陵李元二侍御》詩：“遙想<u>荊州</u>人物論，幾迴中夜惜<u>元龍</u>。”<br/>　<u>宋</u><u>陳與義</u>《對酒》詩：“<u>陳留</u>春色撩詩思，一日搜腸一百迴。”<br/> 《劉知遠諸宮調‧知遠別三娘太原投事》：“當此<u>李洪義</u>，遂側耳聽況兩迴三度。”<br/>　<u>元</u><u>尚仲賢</u>《氣英布》第一摺：“今番且過，這迴休再動干戈。”<br/><b>15.</b>副詞。相當於“再”、“又”、“復”。<br/> 《劉知遠諸宮調‧知遠投三娘與洪義廝打》：“嬌聲重問：‘我兒別後在和亡？’迴告<u>劉郎</u>：‘但對奴家聞早說。’”"#;
		let injected_html = inject_definition(html, "段注說文解字", &regex);
		assert_eq!(injected_html, r#"(迴,回)<br/>huí<br/>ㄏㄨㄟˊ<br/>〔《廣韻》戶恢切，平灰，匣。〕<br/>〔《廣韻》胡對切，去隊，匣。〕<br/><b>1.</b>掉轉；返回。<br/> 《楚辭‧離騷》：“迴朕車以復路兮，及行迷之未遠。”<br/>　<a href="王逸">王逸</a>注：“迴，旋也。”<br/>　<a href="南朝">南朝</a><a href="宋">宋</a><a href="謝惠連">謝惠連</a>《隴西行》：“窮谷是處，考槃是營；千金不迴，百代傳名。”<br/> 《敦煌變文集‧秋胡變文》：“未及行至路傍，正見採桑而迴。”<br/> 《老殘游記》第八回：“車子就放在驢子旁邊，人卻倒迴走了數十步。”<br/><b>2.</b>旋轉；翻轉。<br/>　<a href="漢">漢</a><a href="司馬遷">司馬遷</a>《報任少卿書》：“是以腸一日而九迴，居則忽忽若有所亡，出則不知其所往。”<br/>　<a href="南朝">南朝</a><a href="梁">梁</a><a href="王暕">王暕</a>《詠舞詩》：“從風迴綺袖，映日轉花鈿。”<br/>　<a href="唐">唐</a><a href="李白">李白</a>《大鵬賦》：“左迴右旋，倏陰忽明。”<br/> 《紅樓夢》第五回：“盼纖腰之楚楚兮，風迴雪舞。”<br/><b>3.</b>運轉；循環。<br/> 《呂氏春秋‧季冬》：“是月也，日窮于次，月窮於紀，星迴于天。”<br/>　<a href="晉">晉</a><a href="盧諶">盧諶</a>《贈劉琨》詩：“天地盈虛，寒暑周迴。”<br/><b>4.</b>重複某種動作或重現某種現象。<br/>　<a href="北魏">北魏</a><a href="賈思勰">賈思勰</a>《齊民要術‧作酢法》：“迴酒酢法：凡釀酒失所味醋者，或初好後動未壓者，皆宜迴作醋。”<br/>　<a href="清">清</a><a href="阮元">阮元</a>《小滄浪筆談》卷二：“澗草迴新綠，巖松發古春。”<br/><b>5.</b>環繞；圍繞。<br/>　<a href="晉">晉</a><a href="張華">張華</a>《博物志》卷四：“<a href="始皇陵">始皇陵</a>在<a href="驪山">驪山</a>之北，高數十丈，周迴六七里。”<br/>　<a href="唐">唐</a><a href="姜晞">姜晞</a>《龍池篇》：“靈沼縈迴邸第前，浴日涵春寫曙天。”<br/>　<a href="唐">唐</a><a href="李白">李白</a>《金陵》詩之二：“地擁<a href="金陵">金陵</a>勢，城迴<a href="江">江</a>水流。”<br/><b>6.</b>曲折，迂回。<br/>　<a href="唐">唐</a><a href="杜甫">杜甫</a>《野老》詩：“野老籬邊江岸迴，柴門不正逐江開。”<br/>　<a href="仇兆鰲">仇兆鰲</a>注：“江岸回曲，其柴門不正設者，為逐江面而開也。”<br/>　<a href="明">明</a><a href="王思任">王思任</a>《將至京》詩：“平原獨茫茫，道路迴且長。”<br/>　<a href="清">清</a><a href="阮元">阮元</a>《小滄浪筆談》卷二：“湖平鏡揩，城迴帶曲，野氣沈村，林煙隱屋。”<br/>　<a href="清">清</a><a href="彭孫貽">彭孫貽</a>《送陶子之淮上》詩之二：“<a href="楊子">楊子</a>東迴通<a href="白下">白下</a>，<a href="彭城">彭城</a>北望枕<a href="黃流">黃流</a>。”<br/><b>7.</b>迂回難行。<br/> 《淮南子‧氾論訓》：“夫五行之山，固塞險阻之地也，使我德能覆之，則天下納其貢職者迴也。”<br/>　<a href="高誘">高誘</a>注：“迴，迂難也。”<br/><b>8.</b>避讓；回避。<br/> 《晉書‧熊遠傳》：“時尚書<a href="刁協">刁協</a>用事，眾皆憚之。<a href="<i>尚書郎</i>"><i>尚書郎</i></a><a href="盧綝">盧綝</a>將入直，遇<a href="協">協</a>於大司馬門外。<br/>　<a href="協">協</a>醉，使<a href="綝">綝</a>避之，<a href="綝">綝</a>不迴。”<br/>　<a href="唐">唐</a><a href="陳子昂">陳子昂</a>《諫靈駕入京書》：“赴湯鑊而不迴，至誅夷而無悔。”<br/><b>9.</b>改易；轉變。<br/> 《北史‧骨儀傳》：“<a href="開皇">開皇</a>初，為御史，處法平當，不為勢利所迴。”<br/>　<a href="唐">唐</a><a href="劉餗">劉餗</a>《隋唐嘉話》卷中：“<a href="梁公">梁公</a>夫人至妒……夫人執心不迴。”<br/>　<a href="清">清</a><a href="陳康祺">陳康祺</a>《郎潛紀聞》卷一：“公退草疏，置之懷，閉閣自縊，冀以尸諫迴天聽也。”<br/><b>10.</b>收回成命。<br/>　<a href="三國">三國</a><a href="魏">魏</a><a href="阮籍">阮籍</a>《詣蔣公》：“補吏之召，非所克堪；乞迴謬恩，以光清舉。”<br/>　<a href="南朝">南朝</a><a href="梁">梁</a><a href="任昉">任昉</a>《為范尚書讓吏部封侯第一表》：“矜臣所乞，特迴寵命。”<br/>　<a href="唐">唐</a><a href="韓愈">韓愈</a>《為韋相公讓官表》：“伏乞特迴所授，以示至公之道，天下幸甚。”<br/><b>11.</b>謂把所得的封贈呈請改授他人。<br/> 《隋書‧李敏傳》：“<a href="樂平公主">樂平公主</a>之將薨也，遺言於<a href="煬帝">煬帝</a>曰：‘妾無子息，唯有一女；不自憂死，但深憐之。今湯沐邑，乞迴與<a href="敏">敏</a>。’帝從之。”<br/>　<a href="唐">唐</a><a href="劉肅">劉肅</a>《大唐新語‧舉賢》：“﹝<a href="李大亮">李大亮</a>﹞言於<a href="太宗">太宗</a>曰：‘臣有今日之榮貴，乃<a href="張弼">張弼</a>之力也，乞迴臣之官爵以復之。’<a href="太宗">太宗</a>即以<a href="弼">弼</a>為中郎。”<br/><b>12.</b>追述，回憶。<br/> 《北史‧恩幸傳‧王仲興》：“後與領軍<a href="于勁">于勁</a>參機要，因自迴<a href="馬圈">馬圈</a>侍疾及入<a href="金墉">金墉</a>功，遂封<a href="上黨郡">上黨郡</a>開國公。”<br/><b>13.</b>邪，邪惡。參見“<span foreground="blue">迴邪</span>”。<br/><b>14.</b>量詞。表示動作的次數。<br/>　<a href="唐">唐</a><a href="柳宗元">柳宗元</a>《同劉二十八哭呂衡州兼寄江陵李元二侍御》詩：“遙想<a href="荊州">荊州</a>人物論，幾迴中夜惜<a href="元龍">元龍</a>。”<br/>　<a href="宋">宋</a><a href="陳與義">陳與義</a>《對酒》詩：“<a href="陳留">陳留</a>春色撩詩思，一日搜腸一百迴。”<br/> 《劉知遠諸宮調‧知遠別三娘太原投事》：“當此<a href="李洪義">李洪義</a>，遂側耳聽況兩迴三度。”<br/>　<a href="元">元</a><a href="尚仲賢">尚仲賢</a>《氣英布》第一摺：“今番且過，這迴休再動干戈。”<br/><b>15.</b>副詞。相當於“再”、“又”、“復”。<br/> 《劉知遠諸宮調‧知遠投三娘與洪義廝打》：“嬌聲重問：‘我兒別後在和亡？’迴告<a href="劉郎">劉郎</a>：‘但對奴家聞早說。’”"#);
		let html = r#"<big><font color="blue">轉也。</font></big>淵、回水也。故顏回字子淵。毛詩傳曰。回、邪也。言回爲？之假借也。又曰。回、違也。亦謂假借也。？、衺也。見交部。<big><font color="blue">从囗。中象回轉之形。</font></big>中當作口。外爲大囗。內爲小口。皆回轉之形也。如天體在外左旋、日月五星在內右旋是也。戸恢切。十五部。"#;
		let injected_html = inject_definition(html, "段注說文解字", &regex);
		assert_eq!(injected_html, r#"<big><font color="blue">轉也。</font></big>淵、回水也。故顏回字子淵。毛詩傳曰。回、邪也。言回爲？之假借也。又曰。回、違也。亦謂假借也。？、衺也。見交部。<big><font color="blue">从囗。中象回轉之形。</font></big>中當作口。外爲大囗。內爲小口。皆回轉之形也。如天體在外左旋、日月五星在內右旋是也。戸恢切。十五部。"#);
		let html = r#"<img src="277A-01.png"><br><big><font color="blue">古文。</font></big>古文象一气回轉之形。"#;
		let injected_html = inject_definition(html, "段注說文解字", &regex);
		assert_eq!(injected_html, r#"<img src="段注說文解字:277A-01.png"><br><big><font color="blue">古文。</font></big>古文象一气回轉之形。"#);
	}
}