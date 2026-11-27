use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use lofty::file::TaggedFile;
use lofty::prelude::*;

use crate::paths::is_inside;

#[derive(Debug, Default)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub year: Option<i32>,
    pub track_number: Option<i32>,
    pub disc_number: i32,
    pub duration: Option<f64>,
    pub bitrate: Option<i32>,
    pub sample_rate: Option<i32>,
    pub artwork: Option<Artwork>,
}

#[derive(Debug)]
pub struct Artwork {
    pub data: Vec<u8>,
    pub mime_type: String,
}

fn is_disc_folder(name: &str) -> bool {
    let lower = name.to_lowercase();
    let Some(rest) = ["cd", "disc", "disk"]
        .iter()
        .find_map(|word| lower.strip_prefix(word))
    else {
        return false;
    };
    let digits = rest.trim_start_matches(|c: char| c.is_whitespace() || "._-".contains(c));
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// The folder standing in for the album: the file's own, or its parent when the
/// file sits in a CD2-style disc subfolder, so both discs resolve alike.
fn album_dir_of(file_path: &Path) -> &Path {
    let dir = file_path.parent().unwrap_or(file_path);
    if dir
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_disc_folder)
        && let Some(parent) = dir.parent().filter(|parent| parent.file_name().is_some())
    {
        return parent;
    }
    dir
}

/// Album name for a file with no ALBUM tag.
///
/// "Unknown Album" is a terrible fallback here: albums are keyed by title and
/// album artist, so every untagged file by the same artist would collapse into
/// one row holding tracks from unrelated records. The containing folder is nearly
/// always the album.
pub fn album_from_path(file_path: &Path) -> String {
    album_dir_of(file_path).file_name().map_or_else(
        || "Unknown Album".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Artist for a file with no ARTIST tag, from the folder above the album.
///
/// An album whose tracks are only partly tagged would otherwise split into two
/// rows, one filed under "Unknown Artist". None once the walk reaches the library
/// root, whose name is a music folder rather than an artist.
pub fn artist_from_path(file_path: &Path, root: &Path) -> Option<String> {
    let artist_dir = album_dir_of(file_path).parent()?;
    if !is_inside(root, artist_dir) {
        return None;
    }
    Some(artist_dir.file_name()?.to_string_lossy().into_owned())
}

const COVER_NAMES: [&str; 6] = ["cover", "folder", "front", "album", "albumart", "artwork"];

fn cover_type(extension: &str) -> Option<&'static str> {
    match extension {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        _ => None,
    }
}

/// Cover art sitting next to the tracks, for rips that carry no embedded picture.
/// Candidates are sorted so a folder holding more than one always yields the same
/// answer, however the filesystem happens to list it.
fn folder_cover(dir: &Path) -> Option<Artwork> {
    let mut candidates: Vec<(PathBuf, &str)> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let path = entry.path();
            let stem = path.file_stem()?.to_str()?.to_lowercase();
            let mime = cover_type(&path.extension()?.to_str()?.to_lowercase())?;
            COVER_NAMES.contains(&stem.as_str()).then_some((path, mime))
        })
        .collect();
    candidates.sort_by_key(|(path, _)| path.to_string_lossy().to_lowercase());

    // An unreadable candidate does not rule out a later one.
    candidates.into_iter().find_map(|(path, mime)| {
        let data = fs::read(path).ok()?;
        Some(Artwork {
            data,
            mime_type: mime.to_owned(),
        })
    })
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.is_empty())
}

fn read_tagged(file_path: &Path) -> Result<Option<TaggedFile>> {
    match lofty::read_from_path(file_path) {
        Ok(tagged) => Ok(Some(tagged)),
        // A format the parser has no support for, such as WMA, is still a real
        // track. It stays in the library, named from its path.
        Err(err) if err.is_unknown_format() => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// Blocking: parses the file. Call from a blocking thread.
pub fn extract_metadata(file_path: &Path, root: &Path) -> Result<TrackMetadata> {
    let tagged = read_tagged(file_path)?;
    let tag = tagged
        .as_ref()
        .and_then(|file| file.primary_tag().or_else(|| file.first_tag()));
    let properties = tagged.as_ref().map(|file| file.properties());

    let text = |value: Option<std::borrow::Cow<'_, str>>| non_empty(value.map(|v| v.into_owned()));
    let title = tag.and_then(|t| text(t.title()));
    let artist = tag.and_then(|t| text(t.artist()));
    let album = tag.and_then(|t| text(t.album()));
    let album_artist = tag
        .and_then(|t| t.get_string(ItemKey::AlbumArtist))
        .and_then(|value| non_empty(Some(value.to_owned())));

    let embedded = tag
        .and_then(|t| t.pictures().first())
        .map(|picture| Artwork {
            data: picture.data().to_vec(),
            mime_type: picture
                .mime_type()
                .map_or_else(|| "image/jpeg".to_owned(), |mime| mime.as_str().to_owned()),
        });
    let artwork = embedded.or_else(|| folder_cover(album_dir_of(file_path)));

    let folder_artist = artist_from_path(file_path, root);
    let duration = properties
        .map(|p| p.duration().as_secs_f64())
        .filter(|secs| *secs > 0.0);
    let bitrate = properties.and_then(|p| p.audio_bitrate().or(p.overall_bitrate()));

    Ok(TrackMetadata {
        title: title.unwrap_or_else(|| {
            file_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        }),
        artist: artist
            .clone()
            .or_else(|| folder_artist.clone())
            .unwrap_or_else(|| "Unknown Artist".to_owned()),
        // Album artist matters for compilations, where every track has a different
        // artist but they all belong to one album.
        album_artist: album_artist
            .or(artist)
            .or(folder_artist)
            .unwrap_or_else(|| "Unknown Artist".to_owned()),
        album: album.unwrap_or_else(|| album_from_path(file_path)),
        year: tag.and_then(|t| t.date()).map(|date| i32::from(date.year)),
        track_number: tag
            .and_then(|t| t.track())
            .and_then(|n| i32::try_from(n).ok()),
        disc_number: tag
            .and_then(|t| t.disk())
            .and_then(|n| i32::try_from(n).ok())
            .unwrap_or(1),
        duration,
        bitrate: bitrate
            .filter(|rate| *rate > 0)
            .and_then(|rate| i32::try_from(rate).ok()),
        sample_rate: properties
            .and_then(|p| p.sample_rate())
            .and_then(|rate| i32::try_from(rate).ok()),
        artwork,
    })
}

pub fn mime_for_file(file_path: &Path) -> Option<&'static str> {
    let extension = file_path.extension()?.to_str()?.to_lowercase();
    match extension.as_str() {
        "mp3" => Some("audio/mpeg"),
        "flac" => Some("audio/flac"),
        "m4a" => Some("audio/mp4"),
        "aac" => Some("audio/aac"),
        "ogg" | "opus" => Some("audio/ogg"),
        "wav" => Some("audio/wav"),
        "wma" => Some("audio/x-ms-wma"),
        "aiff" => Some("audio/aiff"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn album(path: &str) -> String {
        album_from_path(path.as_ref())
    }

    fn artist(path: &str) -> Option<String> {
        artist_from_path(path.as_ref(), "/music".as_ref())
    }

    #[test]
    fn uses_the_containing_folder_for_the_album() {
        assert_eq!(
            album("/music/Artist Two/First Album/01 Cedar.flac"),
            "First Album"
        );
    }

    #[test]
    fn climbs_out_of_a_disc_subfolder() {
        assert_eq!(
            album("/music/Artist One/Second Album/CD1/01 Birch.flac"),
            "Second Album"
        );
        assert_eq!(
            album("/music/Artist One/Second Album/CD 2/01 Amber.flac"),
            "Second Album"
        );
    }

    #[test]
    fn recognises_disc_folder_spellings() {
        for folder in ["Disc 2", "disk_3", "cd-10", "CD2"] {
            assert_eq!(
                album(&format!("/music/Artist/Album/{folder}/track.mp3")),
                "Album"
            );
        }
    }

    #[test]
    fn keeps_a_folder_that_merely_starts_with_a_disc_like_word() {
        assert_eq!(
            album("/music/Artist/Discovery/01 One More Time.mp3"),
            "Discovery"
        );
    }

    #[test]
    fn takes_the_artist_from_the_folder_above_the_album() {
        assert_eq!(
            artist("/music/System Of A Down/Hypnotize/01 Attack.mp3").as_deref(),
            Some("System Of A Down")
        );
    }

    #[test]
    fn climbs_past_a_disc_subfolder_for_the_artist() {
        assert_eq!(
            artist("/music/Artist One/Second Album/CD2/01 Amber.flac").as_deref(),
            Some("Artist One")
        );
    }

    #[test]
    fn gives_up_at_the_library_root() {
        assert_eq!(artist("/music/Some Album/01 Track.mp3"), None);
        assert_eq!(artist("/music/01 Loose Track.mp3"), None);
    }

    #[test]
    fn ignores_a_root_that_only_looks_like_a_prefix() {
        assert_eq!(artist("/music-backup/Artist/Album/01.mp3"), None);
    }

    #[test]
    fn knows_every_supported_extension_case_insensitively() {
        assert_eq!(mime_for_file("/a/B.MP3".as_ref()), Some("audio/mpeg"));
        assert_eq!(mime_for_file("/a/b.opus".as_ref()), Some("audio/ogg"));
        assert_eq!(mime_for_file("/a/cover.jpg".as_ref()), None);
    }
}
