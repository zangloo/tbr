use std::borrow::BorrowMut;
use std::io::{Cursor, Read, Seek, SeekFrom};

use anyhow::{anyhow, bail, Result};
use encoding_rs::Encoding;

use crate::book::{Book, LoadingChapter, Line, Loader, TocInfo};
use crate::common::{decode_text, detect_charset, txt_lines};
use crate::list::ListIterator;
use crate::common::TraceInfo;
use crate::config::{BookLoadingInfo, ReadingInfo};

///
// http://www.haodoo.net/?M=hd&P=mPDB22
//
//  機子及作業系統越來越多，我不可能逐一撰寫閱讀軟體，因而特將uPDB及PDB檔詳細規格公布如下，方便有興趣、有時間、能寫程式的讀友，為新機種撰寫閱讀軟體。唯一的請求是：您撰寫閱讀軟體的目的不是圖利，而是造福讀友，讓讀友們可免費使用。謝謝。
//
//     PDB是源自Palm作業系統的一個單一檔案，簡易資料庫。
//     每一個PDB檔含N筆不定長度的資料(record)。
//     PDB檔最前面當然要有個Header，定義本資料庫的特性。
//     因資料長度非固定，無法計算位置。所以Header之後，是各筆資料所在的位置，可以用來讀資料及計算每筆資料的長度。
//     之後，就是一筆一筆的資料，沒什麼大學問可言。
//
//     檔案的前78個bytes，是Header[0..77]：
//         Header[0..34]舊版是放書名，新版是放作者。可以不理。
//         Header[35]是2，舊版是1。可以不理。
//         Header[36..43]是為Palm而加的兩個日期，可以不理。
//         Header[44..59]都是0。可以不理。
//         Header[60..63]是"BOOK"。可以不理。
//         Header[64..67]是判別的關鍵，PDB是"MTIT"，uPDB是"MTIU"。
//         Header[68..75]都是0。可以不理。
//         Header[76..77]是record數，N(章數)加2(目錄及書籤)。
//
//     每筆資料的起始位置及屬性，依Palm的規格是8個bytes，前4個bytes是位置，後4個bytes是0。一共有 (N+2) * 8 bytes。
//
//     第一筆資料定義書的屬性，是8個空白字元、書名、章數及目錄：
//         (PDB檔)
//         8個空白btyes，可以不理；
//         之後接書名是Big5碼，後接三個ESC(即27)；
//         之後接章數(ASCII string)，後接一個ESC；
//         之後接目錄，各章之標題是以ESC分隔。
//         (uPDB檔)
//         8個空白btyes，可以不理；
//         之後接書名是Unicode碼，後接三個ESC(即27,0)；
//         之後接章數(ASCII string)，後接一個ESC (27, 0)；
//         之後接目錄，各章之標題是以CR(13,0) NL(10,0) 分隔。
//
//     再來是N筆資料，每筆是一章的內容，PDB檔是Big5碼(是null-terminated string，最後一個byte是0)，uPDB檔是Unicode碼。
//
//     第N+2筆資料是書籤，預設是-1。可以不理。

pub(crate) struct HaodooLoader {
	extensions: Vec<&'static str>,
}

const HEADER_LENGTH: usize = 78;
const PDB_ID: &str = "MTIT";
const UPDB_ID: &str = "MTIU";
const PALMDOC_ID: &str = "REAd";
const PDB_SEPARATOR: [u8; 1] = [0x1b];
const UPDB_TITLE_SEPARATOR: [u8; 4] = [0x0d, 0x00, 0x0a, 0x00];
const UPDB_ESCAPE_SEPARATOR: [u8; 2] = [0x1b, 0x00];

const RECODES_COUNT_OFFSET: usize = 76;
const ID_OFFSET: usize = 64;
const ID_LENGTH: usize = 4;

//"★★★★★★★以下內容★★︽本版︾★★無法顯示★★★★★★★";
const ENCRYPT_MARK: [u8; 70] = [0xA1, 0xB9, 0xA1, 0xB9, 0xA1, 0xB9, 0xA1, 0xB9, 0xA1, 0xB9, 0xA1, 0xB9, 0x0D, 0x0A, 0xA1, 0xB9, 0xA5, 0x48, 0xA4, 0x55, 0xA4, 0xBA, 0xAE, 0x65, 0xA1, 0xB9, 0x0D, 0x0A, 0xA1, 0xB9, 0xA1, 0x6F, 0xA5, 0xBB, 0xAA, 0xA9, 0xA1, 0x70, 0xA1, 0xB9, 0x0D, 0x0A, 0xA1, 0xB9, 0xB5, 0x4C, 0xAA, 0x6B, 0xC5, 0xE3, 0xA5, 0xDC, 0xA1, 0xB9, 0x0D, 0x0A, 0xA1, 0xB9, 0xA1, 0xB9, 0xA1, 0xB9, 0xA1, 0xB9, 0xA1, 0xB9, 0xA1, 0xB9, 0x0D, 0x0A];
const ENCRYPT_MARK_LENGTH: usize = ENCRYPT_MARK.len();

enum PDBType {
	PDB { encode: &'static Encoding },
	UPDB { encode: &'static Encoding },
	PalmDoc,
}

impl HaodooLoader {
	pub(crate) fn new() -> Self {
		let extensions = vec![".pdb", ".updb"];
		HaodooLoader { extensions }
	}
}

impl Loader for HaodooLoader {
	fn extensions(&self) -> &Vec<&'static str> {
		&self.extensions
	}

	fn load_file(&self, _filename: &str, file: std::fs::File,
		loading_chapter: LoadingChapter, loading: BookLoadingInfo)
		-> Result<(Box<dyn Book>, ReadingInfo)>
	{
		Ok((Box::new(HaodooBook::new(file, loading_chapter)?), loading.get()))
	}

	fn load_buf(&self, _filename: &str, content: Vec<u8>,
		chapter_position: LoadingChapter, loading: BookLoadingInfo)
		-> Result<(Box<dyn Book>, ReadingInfo)>
	{
		Ok((
			Box::new(HaodooBook::new(Cursor::new(content), chapter_position)?),
			loading.get()
		))
	}
}

struct HaodooBook<R: Read + Seek> {
	reader: R,
	book_type: PDBType,
	record_offsets: Vec<usize>,
	encrypt_chapter_index: Option<usize>,
	chapters: Vec<Chapter>,
	chapter_index: usize,
}

struct Chapter {
	title: String,
	lines: Option<Vec<Line>>,
}

impl<R: Read + Seek + 'static> HaodooBook<R> {
	pub fn new(reader: R, loading_chapter: LoadingChapter) -> Result<Self> {
		let mut book = parse_header(reader)?;
		book.load_toc()?;
		book.chapter_index = match loading_chapter {
			LoadingChapter::Index(index) => index,
			LoadingChapter::Last => book.chapters.len() - 1,
		};
		book.goto_chapter(book.chapter_index)?;
		Ok(book)
	}
}

fn parse_header<R: Read + Seek>(mut reader: R) -> Result<HaodooBook<R>> {
	let mut header = [0u8; HEADER_LENGTH];
	reader.read_exact(&mut header).expect("Invalid header");

	let book_id = String::from_utf8_lossy(&header[ID_OFFSET..ID_OFFSET + ID_LENGTH]).to_string();
	let book_type = match book_id.as_str() {
		PDB_ID => PDBType::PDB { encode: &encoding_rs::BIG5 },
		UPDB_ID => PDBType::UPDB { encode: &encoding_rs::UTF_16LE },
		PALMDOC_ID => PDBType::PalmDoc,
		_ => return Err(anyhow!("Invalid book id: {}", book_id)),
	};
	//line records count
	let record_count = read_u16(&header, RECODES_COUNT_OFFSET);

	//read all records offset
	let mut record_offsets = Vec::with_capacity(record_count);
	let record_buffer_len = 8 * record_count;
	let mut record_buffer = vec![0; record_buffer_len];
	reader.read_exact(record_buffer.borrow_mut()).expect("Invalid header");
	for index in 0..record_count {
		record_offsets.push(read_u32(&record_buffer, index << 3))
	}
	Ok(HaodooBook {
		reader,
		book_type,
		record_offsets,
		encrypt_chapter_index: None,
		chapters: vec![],
		chapter_index: 0,
	})
}

impl<R: Read + Seek + 'static> Book for HaodooBook<R> {
	fn chapter_count(&self) -> usize {
		if matches!(self.book_type, PDBType::PalmDoc) {
			1
		} else {
			self.chapters.len()
		}
	}

	fn goto_chapter(&mut self, chapter_index: usize) -> Result<Option<usize>> {
		match self.chapters.get(chapter_index) {
			Some(Chapter { lines: Some(_), .. }) => {
				self.chapter_index = chapter_index;
				Ok(Some(chapter_index))
			}
			Some(Chapter { lines: None, .. }) => {
				let lines = self.load_chapter(chapter_index)?;
				self.chapters[chapter_index].lines = Some(lines);
				self.chapter_index = chapter_index;
				Ok(Some(chapter_index))
			}
			None => Ok(None),
		}
	}

	fn current_chapter(&self) -> usize {
		self.chapter_index
	}

	fn title(&self, _line: usize, _offset: usize) -> Option<&str> {
		if matches!(self.book_type, PDBType::PalmDoc) {
			None
		} else {
			Some(&self.chapters.get(self.chapter_index)?.title)
		}
	}

	fn toc_index(&self, _line: usize, _offset: usize) -> usize {
		self.chapter_index
	}

	fn toc_iterator(&self) -> Option<Box<dyn Iterator<Item=TocInfo> + '_>>
	{
		if matches!(self.book_type, PDBType::PalmDoc) {
			return None;
		}
		let iter = ListIterator::new(|index| {
			let chapter = self.chapters.get(index)?;
			Some(TocInfo { title: &chapter.title, index, level: 1 })
		});
		Some(Box::new(iter))
	}

	fn toc_position(&mut self, toc_index: usize) -> Option<TraceInfo> {
		Some(TraceInfo { chapter: toc_index, line: 0, offset: 0 })
	}

	fn lines(&self) -> &Vec<Line> {
		match self.chapters.get(self.chapter_index) {
			Some(Chapter { lines: Some(lines), .. }) => lines,
			Some(Chapter { lines: None, .. })
			| None => panic!("chapter not loaded before using."),
		}
	}
}

impl<R: Read + Seek> HaodooBook<R> {
	fn read_record(&mut self, record_index: usize) -> Result<Vec<u8>> {
		let record_count = self.record_offsets.len();
		if record_index >= record_count {
			return Err(anyhow!("invalid record index: {}", record_index));
		}
		// Seek to the start of the given record
		let read_start = self.record_offsets[record_index];
		self.reader.seek(SeekFrom::Start(read_start as u64))?;

		let buf = if record_index == (record_count - 1) {
			// The last record in the DB occupies the rest of the space in the file.
			let mut buf = vec![];
			self.reader.read_to_end(&mut buf)?;
			buf
		} else {
			// Record is not the last so its lineCount can be computed from the
			// starting offset of the following record.
			let record_size = self.record_offsets[record_index + 1] - read_start;
			let mut buf = vec![0; record_size];
			self.reader.read_exact(buf.borrow_mut())?;
			buf
		};
		Ok(buf)
	}

	#[inline]
	fn parse_toc(&mut self, record: &[u8], encode: &'static Encoding, escape: &[u8], title_splitter: &[u8], record_tail: usize) -> Result<()>
	{
		let mut position = 8 + record[8..]
			.windows(escape.len())
			.position(|window| window == escape)
			.ok_or(anyhow!("Failed parse toc"))?;
		position += 3 * escape.len();
		position += escape.len() + record[position..]
			.windows(escape.len())
			.position(|window| window == escape)
			.ok_or(anyhow!("Failed parse toc"))?;
		// titles here
		while let Some(offset) = record[position..]
			.windows(title_splitter.len())
			.position(|window| window == title_splitter) {
			let next_position = position + offset;
			let title = String::from(encode.decode(&record[position..next_position]).0);
			self.chapters.push(Chapter { title, lines: None });
			position = next_position + title_splitter.len();
		}
		if position < record.len() - 1 {
			let end = record.len() - record_tail;
			self.chapters.push(Chapter {
				title: String::from(encode.decode(&record[position..end]).0),
				lines: None,
			});
		}
		Ok(())
	}
	fn load_toc(&mut self) -> Result<()> {
		let record = self.read_record(0)?;
		match self.book_type {
			PDBType::PDB { encode } => {
				self.parse_toc(&record, encode, &PDB_SEPARATOR, &PDB_SEPARATOR, 1)?;
				let encrypt_record_index = self.record_offsets.len() / 2;
				let mut encrypt_record = self.read_record(encrypt_record_index)?;
				let chapter_index = encrypt_record_index - 1;
				let mut offset = 0;
				self.encrypt_chapter_index = loop {
					if ENCRYPT_MARK[offset] != encrypt_record[offset] {
						break None;
					}
					offset += 1;
					if offset == ENCRYPT_MARK_LENGTH {
						break Some(chapter_index);
					}
				};
				let offset = if self.encrypt_chapter_index.is_some() {
					decrypt_pdb(&mut encrypt_record);
					ENCRYPT_MARK_LENGTH
				} else {
					0
				};
				let text = encode.decode(&mut encrypt_record[offset..]).0.to_string();
				let lines = txt_lines(&text);
				if let Some(chapter) = self.chapters.get_mut(chapter_index) {
					chapter.lines = Some(lines);
				} else {
					bail!("Corrupted document.")
				}
			}
			PDBType::UPDB { encode } => {
				self.parse_toc(&record, encode, &UPDB_ESCAPE_SEPARATOR, &UPDB_TITLE_SEPARATOR, 0)?;
			}
			PDBType::PalmDoc => {
				let compression = record[1] == 2;
				let text_count = read_u16(&record, 8);
				let mut buf = vec![];
				for index in 1..=text_count {
					let mut record = self.read_record(index)?;
					if compression {
						decompress_palm_doc(record, &mut buf);
					} else {
						buf.append(&mut record);
					}
				}
				let encoding = detect_charset(&buf, false);
				let text = decode_text(buf, encoding)?;
				let lines = txt_lines(&text);
				self.chapters.push(Chapter { title: String::from("None"), lines: Some(lines) });
			}
		}
		Ok(())
	}

	fn load_chapter(&mut self, chapter_index: usize) -> Result<Vec<Line>> {
		let mut record = self.read_record(chapter_index + 1)?;
		let text = match self.book_type {
			PDBType::PDB { encode, .. } => {
				if let Some(encrypt_chapter_index) = self.encrypt_chapter_index {
					if encrypt_chapter_index <= chapter_index {
						decrypt_pdb(&mut record);
					}
				}
				encode.decode(&mut record)
			}
			PDBType::UPDB { encode, .. } => {
				encode.decode(&mut record)
			}
			PDBType::PalmDoc => {
				panic!("no way")
			}
		}.0.to_string();
		Ok(txt_lines(&text))
	}
}

#[inline]
fn read_u16(buf: &[u8], offset: usize) -> usize {
	((buf[offset] as usize) << 8) | (buf[offset + 1] as usize)
}

#[inline]
fn read_u32(buf: &[u8], offset: usize) -> usize {
	((buf[offset] as usize) << 24)
		| ((buf[offset + 1] as usize) << 16)
		| ((buf[offset + 2] as usize) << 8)
		| (buf[offset + 3] as usize)
}

#[inline]
fn decrypt_pdb(record: &mut [u8]) {
	let mut i = 0;
	let length = record.len();
	loop {
		if record[i] >= 128 {
			i += 1;
			if i >= length {
				break;
			}
			if record[i] == 0 {
				record[i] = 127;
			} else {
				record[i] -= 1;
			}
		}
		i += 1;
		if i >= length {
			break;
		}
	}
}

fn decompress_palm_doc(data: Vec<u8>, output: &mut Vec<u8>)
{
	let mut slice = data.as_slice();
	loop {
		slice = match slice {
			[0x00, rest @ ..] => {
				output.push(0);
				rest
			}
			[len @ 0x01..=0x08, rest @ ..] => {
				for i in 0..*len as usize {
					output.push(rest[i]);
				}
				&rest[*len as usize..]
			}
			[c @ 0x09..=0x7f, rest @ ..] => {
				output.push(*c);
				rest
			}
			[c1 @ 0x80..=0xbf, rest @ ..] => {
				let c2 = rest[0];
				let distance = ((((*c1 as u32) << 8) | c2 as u32) >> 3) & 0x07ff;
				let length = 3 + (c2 & 0x07);
				let copy_from = output.len() - distance as usize;
				for idx in copy_from..copy_from + length as usize {
					output.push(output[idx]);
				}
				&rest[1..]
			}
			[c @ 0xc0..=0xff, rest @ ..] => {
				output.push(b' ');
				output.push(*c ^ 0x80);
				rest
			}
			[] => break,
		};
	}
}
