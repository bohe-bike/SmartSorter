use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use exif::{In, Reader as ExifReader, Tag, Value};
use lofty::config::ParseOptions;
use lofty::file::TaggedFileExt;
use lofty::probe::Probe;
use lofty::tag::Accessor;
use lopdf::{Document, Object};
use quick_xml::events::Event;
use quick_xml::Reader;
use zip::ZipArchive;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Image,
    Audio,
    Video,
    Ebook,
}

/// 从文件中提取到的所有元数据字段
#[derive(Debug, Clone, Default)]
pub struct MediaMetadata {
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub composer: Option<String>,
}

pub fn get_media_type(path: &Path) -> Option<MediaType> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "tif" | "tiff" | "webp" => Some(MediaType::Image),
        "mp3" | "flac" | "aac" | "m4a" | "ogg" | "wav" => Some(MediaType::Audio),
        "mp4" | "m4v" | "mov" => Some(MediaType::Video),
        "epub" | "pdf" | "mobi" | "azw3" | "cbz" | "cbr" => Some(MediaType::Ebook),
        _ => None,
    }
}

pub fn media_type_name(media_type: MediaType) -> &'static str {
    match media_type {
        MediaType::Image => "image",
        MediaType::Audio => "audio",
        MediaType::Video => "video",
        MediaType::Ebook => "ebook",
    }
}

#[allow(dead_code)]
pub fn extract_author(path: &Path) -> Option<String> {
    match get_media_type(path)? {
        MediaType::Image => extract_image_author(path).and_then(normalize_author),
        MediaType::Audio | MediaType::Video => extract_tagged_media_all(path).artist,
        MediaType::Ebook => extract_ebook_author(path).and_then(normalize_author),
    }
}

/// 提取文件的所有元数据字段（artist, album_artist, album, composer）
pub fn extract_all_metadata(path: &Path) -> MediaMetadata {
    match get_media_type(path) {
        Some(MediaType::Image) => {
            let artist = extract_image_author(path).and_then(normalize_author);
            MediaMetadata {
                artist,
                ..Default::default()
            }
        }
        Some(MediaType::Audio) | Some(MediaType::Video) => extract_tagged_media_all(path),
        Some(MediaType::Ebook) => {
            let artist = extract_ebook_author(path).and_then(normalize_author);
            MediaMetadata {
                artist,
                ..Default::default()
            }
        }
        None => MediaMetadata::default(),
    }
}

pub fn media_type_label(path: &Path) -> Option<&'static str> {
    Some(media_type_name(get_media_type(path)?))
}

fn extract_image_author(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let exif = ExifReader::new().read_from_container(&mut reader).ok()?;

    if let Some(field) = exif.get_field(Tag::Artist, In::PRIMARY) {
        let value = field.display_value().with_unit(&exif).to_string();
        if !value.trim().is_empty() {
            return Some(value);
        }
    }

    let xp_comment = exif.get_field(Tag::UserComment, In::PRIMARY)?;
    match &xp_comment.value {
        Value::Ascii(values) => values
            .iter()
            .find_map(|value| String::from_utf8(value.clone()).ok()),
        Value::Byte(bytes) => {
            let utf16: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .take_while(|unit| *unit != 0)
                .collect();
            String::from_utf16(&utf16).ok()
        }
        _ => None,
    }
}

/// 提取音频/视频文件的所有元数据字段
fn extract_tagged_media_all(path: &Path) -> MediaMetadata {
    let tagged = match Probe::open(path)
        .ok()
        .and_then(|p| p.options(ParseOptions::new()).read().ok())
    {
        Some(t) => t,
        None => return MediaMetadata::default(),
    };

    let mut meta = MediaMetadata::default();

    for tag in tagged.tags() {
        if meta.artist.is_none() {
            if let Some(text) = tag.artist() {
                let s = text.to_string();
                if !s.trim().is_empty() {
                    meta.artist = normalize_author(s);
                }
            }
        }
        if meta.artist.is_none() {
            if let Some(text) = tag.get_string(&lofty::tag::ItemKey::TrackArtist) {
                let s = text.to_string();
                if !s.trim().is_empty() {
                    meta.artist = normalize_author(s);
                }
            }
        }
        if meta.album_artist.is_none() {
            if let Some(text) = tag.get_string(&lofty::tag::ItemKey::AlbumArtist) {
                let s = text.to_string();
                if !s.trim().is_empty() {
                    meta.album_artist = normalize_author(s);
                }
            }
        }
        if meta.album.is_none() {
            if let Some(text) = tag.album() {
                let s = text.to_string();
                if !s.trim().is_empty() {
                    meta.album = normalize_author(s);
                }
            }
        }
        if meta.composer.is_none() {
            if let Some(text) = tag.get_string(&lofty::tag::ItemKey::Composer) {
                let s = text.to_string();
                if !s.trim().is_empty() {
                    meta.composer = normalize_author(s);
                }
            }
        }
    }

    meta
}

fn extract_ebook_author(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    match ext.as_str() {
        "epub" => extract_epub_author(path),
        "pdf" => extract_pdf_author(path),
        "mobi" | "azw3" => extract_mobi_author(path),
        "cbz" | "cbr" => extract_cbz_author(path),
        _ => None,
    }
}

fn extract_epub_author(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut archive = ZipArchive::new(file).ok()?;

    let container_xml = {
        let mut container = archive.by_name("META-INF/container.xml").ok()?;
        let mut content = String::new();
        container.read_to_string(&mut content).ok()?;
        content
    };

    let opf_path = find_opf_path(&container_xml)?;
    let mut opf = archive.by_name(&opf_path).ok()?;
    let mut opf_content = String::new();
    opf.read_to_string(&mut opf_content).ok()?;
    find_dc_creator(&opf_content)
}

fn find_opf_path(container_xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(container_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf).ok()? {
            Event::Empty(event) | Event::Start(event) if event.name().as_ref() == b"rootfile" => {
                for attr in event.attributes().flatten() {
                    if attr.key.as_ref() == b"full-path" {
                        return Some(String::from_utf8_lossy(attr.value.as_ref()).to_string());
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    None
}

fn find_dc_creator(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut in_creator = false;

    loop {
        match reader.read_event_into(&mut buf).ok()? {
            Event::Start(event) => {
                let name = event.name();
                let raw = name.as_ref();
                in_creator = raw.ends_with(b"creator") || raw == b"dc:creator";
            }
            Event::Text(text) if in_creator => {
                return Some(String::from_utf8_lossy(text.as_ref()).to_string());
            }
            Event::End(_) => {
                in_creator = false;
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    None
}

fn extract_pdf_author(path: &Path) -> Option<String> {
    let document = Document::load(path).ok()?;
    let info_ref = match document.trailer.get(b"Info") {
        Ok(Object::Reference(reference)) => *reference,
        _ => return None,
    };
    let info = document.get_dictionary(info_ref).ok()?;
    let author = info.get(b"Author").ok()?;
    match author {
        Object::String(value, _) => Some(String::from_utf8_lossy(value).to_string()),
        Object::Name(value) => Some(String::from_utf8_lossy(value).to_string()),
        _ => None,
    }
}

fn extract_mobi_author(path: &Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&data);
    for marker in ["AUTHOR", "Creator", "creator"] {
        if let Some(index) = text.find(marker) {
            let slice = &text[index + marker.len()..];
            let candidate: String = slice
                .chars()
                .skip_while(|ch| !ch.is_alphanumeric())
                .take_while(|ch| *ch != '\0' && *ch != '\n' && *ch != '\r')
                .collect();
            if !candidate.trim().is_empty() {
                return Some(candidate.trim().to_string());
            }
        }
    }
    None
}

fn extract_cbz_author(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut archive = ZipArchive::new(file).ok()?;
    let mut comic_info = archive.by_name("ComicInfo.xml").ok()?;
    let mut xml = String::new();
    comic_info.read_to_string(&mut xml).ok()?;
    find_named_tag(&xml, b"Writer")
}

fn find_named_tag(xml: &str, tag_name: &[u8]) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut matched = false;

    loop {
        match reader.read_event_into(&mut buf).ok()? {
            Event::Start(event) => {
                matched = event.name().as_ref() == tag_name;
            }
            Event::Text(text) if matched => {
                return Some(String::from_utf8_lossy(text.as_ref()).to_string());
            }
            Event::End(_) => matched = false,
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    None
}

fn normalize_author(value: String) -> Option<String> {
    let normalized = value
        .replace('\u{0}', "")
        .trim()
        .trim_matches(['"', '\''].as_ref())
        .to_string();

    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}
