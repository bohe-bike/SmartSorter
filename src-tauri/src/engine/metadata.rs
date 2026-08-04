use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use exif::{In, Reader as ExifReader, Tag, Value};
use id3::{Tag as Id3Tag, TagLike};
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey, Tag as LoftyTag};
use lopdf::{Document, Object};
use quick_xml::events::Event;
use quick_xml::Reader;
use regex::Regex;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zip::ZipArchive;

use crate::engine::{executor, hasher};

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
    /// 首位参与艺术家，保留给需要单一作者值的既有调用方。
    pub artist: Option<String>,
    /// 音频/视频的参与艺术家，按标签中的原始顺序拆分。
    pub contributing_artists: Vec<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub composer: Option<String>,
    /// 嵌入式封面的精确内容哈希。仅用于归类辅助，不向前端传输封面数据。
    pub cover_art_hash: Option<String>,
}

pub fn get_media_type(path: &Path) -> Option<MediaType> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tif" | "tiff" | "webp" => Some(MediaType::Image),
        "mp3" | "flac" | "aac" | "m4a" | "ogg" | "wav" => Some(MediaType::Audio),
        "mp4" | "m4v" | "mov" | "mkv" | "avi" | "wmv" => Some(MediaType::Video),
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

/// 标签清洗范围。完整清洗仅保留作者字段和封面；仅修复创作者会保留其他现有标签。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagCleanupMode {
    Full,
    CreatorOnly,
}

impl TagCleanupMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_lowercase().as_str() {
            "full" => Ok(Self::Full),
            "creator_only" => Ok(Self::CreatorOnly),
            _ => Err("标签清洗模式无效，应为 full 或 creator_only".into()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::CreatorOnly => "creator_only",
        }
    }
}

/// 仅允许已经由 lofty 稳定支持读写的音视频容器参与标签清洗。
pub fn supports_tag_cleanup(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("mp3" | "flac" | "m4a" | "ogg" | "wav" | "mp4" | "m4v")
    )
}

pub fn validate_tag_cleanup_writable(path: &Path) -> Result<(), String> {
    if !supports_tag_cleanup(path) {
        return Err("该格式暂不支持安全标签清洗".into());
    }
    let tagged = Probe::open(path)
        .map_err(|error| format!("打开媒体文件失败: {}", error))?
        .options(ParseOptions::new())
        .read()
        .map_err(|error| format!("无法安全读取标签，已设为只读不可清洗: {}", error))?;
    let tag_type = tagged.primary_tag_type();
    if !tagged.supports_tag_type(tag_type) {
        return Err("该媒体容器不支持写入主标签".into());
    }
    Ok(())
}

/// 删除文件中的可写描述标签，保留封面，并写入统一的 Artist / AlbumArtist。
pub fn clean_tags_and_set_artist(
    path: &Path,
    backup: &Path,
    artist: &str,
    expected_hash: &str,
    mode: TagCleanupMode,
) -> Result<(), String> {
    if !supports_tag_cleanup(path) {
        return Err("该格式暂不支持安全标签清洗".into());
    }
    let artist =
        normalize_author(artist.to_string()).ok_or_else(|| "作者名称不能为空".to_string())?;
    if expected_hash.is_empty() {
        return Err("扫描时未能生成文件哈希，请重新扫描".into());
    }
    let original_cover_hash = extract_tagged_media_all(backup).cover_art_hash;
    let stage = sibling_work_path(path, "tag-stage")?;
    let rollback = sibling_work_path(path, "tag-rollback")?;
    executor::safe_copy(backup, &stage)?;

    let write_result = rewrite_tags_in_place(&stage, &artist, mode).and_then(|_| {
        let cleaned = extract_tagged_media_all(&stage);
        if cleaned.artist.as_deref() != Some(artist.as_str())
            || cleaned.album_artist.as_deref() != Some(artist.as_str())
        {
            return Err("标签写入后校验失败，Artist 或 AlbumArtist 不匹配".into());
        }
        if original_cover_hash.is_some() && cleaned.cover_art_hash != original_cover_hash {
            return Err("标签写入后校验失败，内嵌封面未被完整保留".into());
        }
        Ok(())
    });
    if let Err(error) = write_result {
        let _ = fs::remove_file(&stage);
        return Err(error);
    }

    let current_hash = match hasher::compute_sha256(path) {
        Ok(hash) => hash,
        Err(error) => {
            let _ = fs::remove_file(&stage);
            return Err(format!("验证待清洗文件 SHA-256 失败: {}", error));
        }
    };
    if current_hash != expected_hash {
        let _ = fs::remove_file(&stage);
        return Err("文件在扫描后发生变化，已拒绝用旧备份执行标签清洗".into());
    }

    if let Err(error) = fs::rename(path, &rollback) {
        let _ = fs::remove_file(&stage);
        return Err(format!("暂存原媒体文件失败: {}", error));
    }
    if let Err(error) = fs::rename(&stage, path) {
        let rollback_error = fs::rename(&rollback, path).err();
        let _ = fs::remove_file(&stage);
        return Err(match rollback_error {
            Some(rollback_error) => format!(
                "替换媒体文件失败: {}；恢复原文件也失败: {}",
                error, rollback_error
            ),
            None => format!("替换媒体文件失败，已恢复原文件: {}", error),
        });
    }
    let _ = fs::remove_file(&rollback);
    Ok(())
}

fn rewrite_tags_in_place(path: &Path, artist: &str, mode: TagCleanupMode) -> Result<(), String> {
    let mut tagged = Probe::open(path)
        .map_err(|error| format!("打开媒体文件失败: {}", error))?
        .options(ParseOptions::new())
        .read()
        .map_err(|error| format!("读取媒体标签失败: {}", error))?;
    let tag_type = tagged.primary_tag_type();
    if !tagged.supports_tag_type(tag_type) {
        return Err("该媒体容器不支持写入主标签".into());
    }

    match mode {
        TagCleanupMode::Full => {
            let pictures = tagged
                .tags()
                .iter()
                .flat_map(|tag| tag.pictures().iter().cloned())
                .collect::<Vec<_>>();
            let mut clean_tag = LoftyTag::new(tag_type);
            if !clean_tag.insert_text(ItemKey::TrackArtist, artist.to_string())
                || !clean_tag.insert_text(ItemKey::AlbumArtist, artist.to_string())
            {
                return Err("该媒体格式无法写入 Artist 或 AlbumArtist".into());
            }
            for picture in pictures {
                clean_tag.push_picture(picture);
            }
            tagged.clear();
            tagged.insert_tag(clean_tag);
        }
        TagCleanupMode::CreatorOnly => {
            let tag = tagged
                .primary_tag_mut()
                .ok_or_else(|| "该媒体文件没有可写入的主标签".to_string())?;
            tag.remove_key(&ItemKey::TrackArtist);
            tag.remove_key(&ItemKey::AlbumArtist);
            if !tag.insert_text(ItemKey::TrackArtist, artist.to_string())
                || !tag.insert_text(ItemKey::AlbumArtist, artist.to_string())
            {
                return Err("该媒体格式无法写入 Artist 或 AlbumArtist".into());
            }
        }
    }
    tagged
        .save_to_path(path, WriteOptions::default())
        .map_err(|error| format!("写入媒体标签失败: {}", error))
}

fn sibling_work_path(path: &Path, purpose: &str) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .ok_or_else(|| "无法确定媒体文件所在目录".to_string())?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| "媒体文件名无效".to_string())?;
    Ok(parent.join(format!(
        ".smartsorter-{}-{}-{}",
        purpose,
        Uuid::new_v4(),
        file_name
    )))
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
    let tagged = Probe::open(path)
        .ok()
        .and_then(|probe| probe.options(ParseOptions::new()).guess_file_type().ok())
        .and_then(|probe| probe.read().ok());

    let mut meta = MediaMetadata::default();

    if let Some(tagged) = tagged {
        for tag in tagged.tags() {
            if meta.cover_art_hash.is_none() {
                meta.cover_art_hash = tag
                    .get_picture_type(PictureType::CoverFront)
                    .or_else(|| tag.pictures().first())
                    .filter(|picture| !picture.data().is_empty())
                    .map(|picture| hash_cover_art(picture.data()));
            }
            append_tag_artists(&mut meta.contributing_artists, tag);
            if meta.album_artist.is_none() {
                if let Some(text) = tag.get_strings(&ItemKey::AlbumArtist).next() {
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
                if let Some(text) = tag.get_strings(&ItemKey::Composer).next() {
                    let s = text.to_string();
                    if !s.trim().is_empty() {
                        meta.composer = normalize_author(s);
                    }
                }
            }
        }
    }

    if needs_mp3_id3_fallback(path, &meta) {
        append_mp3_id3_fallback(path, &mut meta);
    }

    if needs_windows_shell_artist_fallback(&meta.contributing_artists) {
        append_windows_shell_artists(path, &mut meta.contributing_artists);
    }

    meta.artist = meta.contributing_artists.first().cloned();

    meta
}

fn append_tag_artists(artists: &mut Vec<String>, tag: &LoftyTag) {
    for artist_text in tag.get_strings(&ItemKey::TrackArtist) {
        append_contributing_artists(artists, artist_text);
    }
}

/// Lofty 会因部分无关的损坏 ID3 帧（例如非法时间戳）拒绝整个文件。
/// 归类只需要文本创作者字段时，对 MP3 再用宽容的 `id3` 读取器回退。
fn needs_mp3_id3_fallback(path: &Path, meta: &MediaMetadata) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mp3"))
        && (meta.contributing_artists.is_empty() || meta.album_artist.is_none())
}

fn append_mp3_id3_fallback(path: &Path, meta: &mut MediaMetadata) {
    let Ok(tag) = Id3Tag::read_from_path(path) else {
        return;
    };
    if let Some(artist) = tag.artist() {
        append_contributing_artists(&mut meta.contributing_artists, artist);
    }
    if meta.album_artist.is_none() {
        meta.album_artist = tag
            .album_artist()
            .and_then(|artist| normalize_author(artist.to_string()));
    }
}

fn needs_windows_shell_artist_fallback(artists: &[String]) -> bool {
    artists.is_empty()
        || (artists.len() == 1
            && artists[0]
                .chars()
                .any(|character| matches!(character, '/' | '／')))
}

#[cfg(target_os = "windows")]
fn append_windows_shell_artists(path: &Path, artists: &mut Vec<String>) {
    if let Ok(values) = read_windows_shell_artists(path) {
        let mut shell_artists = Vec::new();
        for value in values {
            append_contributing_artists(&mut shell_artists, &value);
        }
        if artists.is_empty() || shell_artists.len() > artists.len() {
            *artists = shell_artists;
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn append_windows_shell_artists(_path: &Path, _artists: &mut Vec<String>) {}

#[cfg(target_os = "windows")]
fn read_windows_shell_artists(path: &Path) -> Result<Vec<String>, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::EnhancedStorage::PKEY_Music_Artist;
    use windows::Win32::System::Com::StructuredStorage::{
        PropVariantClear, PropVariantGetElementCount, PropVariantGetStringElem,
    };
    use windows::Win32::System::Com::{
        CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::{
        IPropertyStore, SHGetPropertyStoreFromParsingName, GPS_DEFAULT,
    };

    struct ComInitialization(bool);
    impl Drop for ComInitialization {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };
    let _com = ComInitialization(initialized);
    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let store: IPropertyStore =
        unsafe { SHGetPropertyStoreFromParsingName(PCWSTR(wide_path.as_ptr()), None, GPS_DEFAULT) }
            .map_err(|error| format!("读取 Windows 媒体属性失败: {}", error))?;
    let mut property = unsafe { store.GetValue(&PKEY_Music_Artist) }
        .map_err(|error| format!("读取参与创作的艺术家失败: {}", error))?;

    let result = (|| {
        let count = unsafe { PropVariantGetElementCount(&property) };
        let mut values = Vec::new();
        for index in 0..count {
            let raw = unsafe { PropVariantGetStringElem(&property, index) }
                .map_err(|error| format!("转换参与创作的艺术家失败: {}", error))?;
            let value = unsafe { raw.to_string() }
                .map_err(|error| format!("转换参与创作的艺术家文本失败: {}", error));
            unsafe { CoTaskMemFree(Some(raw.0.cast())) };
            let value = value?;
            if !value.trim().is_empty() {
                values.push(value);
            }
        }
        Ok(values)
    })();
    let _ = unsafe { PropVariantClear(&mut property) };
    result
}

fn hash_cover_art(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn split_contributing_artists(value: &str) -> Vec<String> {
    let mut artists = Vec::new();
    static COLLABORATION_SEPARATOR: OnceLock<Regex> = OnceLock::new();
    let collaboration_separator = COLLABORATION_SEPARATOR.get_or_init(|| {
        Regex::new(r"(?i)\s+(?:feat(?:uring)?|ft)\.?\s+|\s*[/／]\s*")
            .expect("参与艺术家分隔正则必须有效")
    });
    for segment in collaboration_separator.split(value) {
        for part in segment.split([';', '；', '、', '|', '，', '\0', '\r', '\n']) {
            let Some(artist) = normalize_author(part.to_string()) else {
                continue;
            };
            if !artists
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(&artist))
            {
                artists.push(artist);
            }
        }
    }
    artists
}

fn append_contributing_artists(artists: &mut Vec<String>, value: &str) {
    for artist in split_contributing_artists(value) {
        if !artists
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&artist))
        {
            artists.push(artist);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn recognizes_documented_media_extensions() {
        assert_eq!(
            get_media_type(Path::new("cover.gif")),
            Some(MediaType::Image)
        );
        assert_eq!(
            get_media_type(Path::new("cover.bmp")),
            Some(MediaType::Image)
        );
        assert_eq!(
            get_media_type(Path::new("video.mkv")),
            Some(MediaType::Video)
        );
        assert_eq!(
            get_media_type(Path::new("video.avi")),
            Some(MediaType::Video)
        );
        assert_eq!(
            get_media_type(Path::new("video.wmv")),
            Some(MediaType::Video)
        );
    }

    #[test]
    fn splits_contributing_artists_in_tag_order() {
        assert_eq!(
            split_contributing_artists(" 作者A；频道名、嘉宾B | 作者A "),
            vec!["作者A", "频道名", "嘉宾B"]
        );
    }

    #[test]
    fn splits_multivalue_and_collaboration_artist_formats() {
        assert_eq!(
            split_contributing_artists("作者A\0频道名 / 嘉宾B feat. 嘉宾C，组合D\r\n作者A"),
            vec!["作者A", "频道名", "嘉宾B", "嘉宾C", "组合D"]
        );
    }

    #[test]
    fn splits_artist_slash_without_surrounding_whitespace() {
        assert_eq!(
            split_contributing_artists("作者A/TG@channel／嘉宾B"),
            vec!["作者A", "TG@channel", "嘉宾B"]
        );
    }

    #[test]
    fn splits_windows_contributing_artists_value() {
        assert_eq!(
            split_contributing_artists("黧落大总攻; TG@Jingluoasmr;"),
            vec!["黧落大总攻", "TG@Jingluoasmr"]
        );
    }

    #[test]
    fn collects_every_track_artist_item() {
        use lofty::tag::{ItemValue, TagItem, TagType};

        let mut tag = LoftyTag::new(TagType::Id3v2);
        assert!(tag.push(TagItem::new(
            ItemKey::TrackArtist,
            ItemValue::Text("作者A".into()),
        )));
        assert!(tag.push(TagItem::new(
            ItemKey::TrackArtist,
            ItemValue::Text("频道名".into()),
        )));
        let mut artists = Vec::new();

        append_tag_artists(&mut artists, &tag);

        assert_eq!(artists, vec!["作者A", "频道名"]);
    }

    #[test]
    fn requests_shell_fallback_for_empty_or_ambiguous_artist_values() {
        assert!(needs_windows_shell_artist_fallback(&[]));
        assert!(needs_windows_shell_artist_fallback(&["作者A/作者B".into()]));
        assert!(!needs_windows_shell_artist_fallback(&["ACDC".into()]));
        assert!(!needs_windows_shell_artist_fallback(&[
            "作者A".into(),
            "作者B".into(),
        ]));
    }

    #[test]
    fn parses_tag_cleanup_modes() {
        assert_eq!(TagCleanupMode::parse("full").unwrap(), TagCleanupMode::Full);
        assert_eq!(
            TagCleanupMode::parse("creator_only").unwrap(),
            TagCleanupMode::CreatorOnly
        );
        assert!(TagCleanupMode::parse("unknown").is_err());
    }

    #[test]
    fn limits_tag_cleanup_to_supported_containers() {
        assert!(supports_tag_cleanup(Path::new("track.mp3")));
        assert!(supports_tag_cleanup(Path::new("movie.mp4")));
        assert!(!supports_tag_cleanup(Path::new("movie.mkv")));
        assert!(!supports_tag_cleanup(Path::new("movie.mov")));
    }
}
