//! What the screens show, kept on the UI thread. Background work hands its
//! results here through `slint::invoke_from_event_loop`.

use std::cell::RefCell;
use std::rc::Rc;

use cpz_core::playlists::LocalPlaylist;
use cpz_core::realtime::HubState;
use cpz_core::types::{AlbumSummary, ArtistCard, ArtistHit, PlayerTrack, RepeatMode};
use slint::{ComponentHandle, Image, Model, ModelRc, VecModel};

use crate::art::Pixels;
use crate::{
    AlbumItem, App, AppWindow, ArtistItem, DeviceItem, GuestItem, Playback, PlaylistItem,
    QueueItem, StatTrack, TrackItem,
};

pub fn time(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    let (hours, minutes, secs) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

/// "48 min", or "1 hr 12 min" for something long.
pub fn length(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

/// "340 MB", "1.2 GB".
pub fn bytes(count: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = count as f64 / MB;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

pub fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// The lists a page shows, and the models the UI draws them from. A model is
/// changed in place, so the UI only redraws the rows that changed.
#[derive(Default)]
pub struct View {
    pub albums: Rc<VecModel<AlbumItem>>,
    pub artists: Rc<VecModel<ArtistItem>>,
    pub album_tracks: Rc<VecModel<TrackItem>>,
    pub artist_albums: Rc<VecModel<AlbumItem>>,
    pub artist_tracks: Rc<VecModel<TrackItem>>,
    pub search_tracks: Rc<VecModel<TrackItem>>,
    pub search_albums: Rc<VecModel<AlbumItem>>,
    pub search_artists: Rc<VecModel<ArtistItem>>,
    pub playlists: Rc<VecModel<PlaylistItem>>,
    pub playlist_tracks: Rc<VecModel<TrackItem>>,
    /// Each playlist row's album cover, row for row with `playlist_tracks`.
    pub playlist_covers: Rc<VecModel<Image>>,
    pub downloads: Rc<VecModel<TrackItem>>,
    pub queue: Rc<VecModel<QueueItem>>,
    pub devices: Rc<VecModel<DeviceItem>>,
    pub stats_tracks: Rc<VecModel<StatTrack>>,
    pub stats_artists: Rc<VecModel<ArtistItem>>,
    pub guests: Rc<VecModel<GuestItem>>,

    queue_ids: Vec<i32>,
    /// Queue indices in the order the queue panel lists them.
    queue_rows: Vec<usize>,
    current: Option<i32>,
    playback_album: Option<i32>,
}

thread_local! {
    static VIEW: RefCell<Option<View>> = const { RefCell::new(None) };
}

/// Runs `change` with the view, on the UI thread.
pub fn with<R>(change: impl FnOnce(&mut View) -> R) -> R {
    VIEW.with(|view| {
        change(
            view.borrow_mut()
                .as_mut()
                .expect("the view is installed at startup"),
        )
    })
}

pub fn install(ui: &AppWindow) {
    let view = View::default();
    let app = ui.global::<App>();
    app.set_albums(ModelRc::from(view.albums.clone()));
    app.set_artists(ModelRc::from(view.artists.clone()));
    app.set_album_tracks(ModelRc::from(view.album_tracks.clone()));
    app.set_artist_albums(ModelRc::from(view.artist_albums.clone()));
    app.set_artist_tracks(ModelRc::from(view.artist_tracks.clone()));
    app.set_search_tracks(ModelRc::from(view.search_tracks.clone()));
    app.set_search_albums(ModelRc::from(view.search_albums.clone()));
    app.set_search_artists(ModelRc::from(view.search_artists.clone()));
    app.set_playlists(ModelRc::from(view.playlists.clone()));
    app.set_playlist_tracks(ModelRc::from(view.playlist_tracks.clone()));
    app.set_playlist_covers(ModelRc::from(view.playlist_covers.clone()));
    app.set_downloads(ModelRc::from(view.downloads.clone()));
    app.set_stats_tracks(ModelRc::from(view.stats_tracks.clone()));
    app.set_stats_artists(ModelRc::from(view.stats_artists.clone()));
    app.set_guests(ModelRc::from(view.guests.clone()));
    let playback = ui.global::<Playback>();
    playback.set_queue(ModelRc::from(view.queue.clone()));
    playback.set_devices(ModelRc::from(view.devices.clone()));
    VIEW.with(|slot| *slot.borrow_mut() = Some(view));
}

pub fn album_item(album: &AlbumSummary, art: Option<Pixels>) -> AlbumItem {
    AlbumItem {
        id: album.id,
        title: album.title.as_str().into(),
        artist: album.artist.as_str().into(),
        year: album.year.map(|y| y.to_string()).unwrap_or_default().into(),
        art: art.map(Image::from_rgb8).unwrap_or_default(),
    }
}

/// The album whose cover stands in for a playlist: the first one that has art.
pub fn cover_album(playlist: &LocalPlaylist) -> Option<i32> {
    playlist
        .tracks
        .iter()
        .find(|t| t.album.has_artwork)
        .map(|t| t.album.id)
}

pub fn playlist_detail(playlist: &LocalPlaylist, with_length: bool) -> String {
    let mut detail = plural(playlist.tracks.len(), "song", "songs");
    let total: f64 = playlist.tracks.iter().filter_map(|t| t.duration).sum();
    if with_length && total > 0.0 {
        detail = format!("{detail} · {}", length(total));
    }
    detail
}

pub fn playlist_item(
    playlist: &LocalPlaylist,
    with_length: bool,
    art: Option<Pixels>,
) -> PlaylistItem {
    PlaylistItem {
        id: playlist.uuid.as_str().into(),
        name: playlist.name.as_str().into(),
        detail: playlist_detail(playlist, with_length).into(),
        album_id: cover_album(playlist).unwrap_or(0),
        art: art.map(Image::from_rgb8).unwrap_or_default(),
    }
}

pub fn artist_item(artist: &ArtistCard) -> ArtistItem {
    ArtistItem {
        id: artist.id,
        name: artist.name.as_str().into(),
        detail: format!(
            "{} · {}",
            plural(artist.albums as usize, "album", "albums"),
            plural(artist.tracks as usize, "song", "songs")
        )
        .into(),
    }
}

pub fn artist_hit(artist: &ArtistHit) -> ArtistItem {
    ArtistItem {
        id: artist.id,
        name: artist.name.as_str().into(),
        detail: "Artist".into(),
    }
}

/// `number` is the track number, or the position in the list when there is
/// none, as in search results.
pub fn track_item(
    track: &PlayerTrack,
    position: usize,
    numbered: bool,
    current: Option<i32>,
    offline: bool,
) -> TrackItem {
    let number = if numbered {
        track
            .track_number
            .map_or_else(|| (position + 1).to_string(), |n| n.to_string())
    } else {
        (position + 1).to_string()
    };
    TrackItem {
        id: track.id,
        title: track.title.as_str().into(),
        artist: track.artist.name.as_str().into(),
        album: track.album.title.as_str().into(),
        album_id: track.album.id,
        artist_id: track.artist.id,
        number: number.into(),
        duration: track.duration.map(time).unwrap_or_default().into(),
        current: current == Some(track.id),
        offline,
    }
}

impl View {
    fn track_models(&self) -> [&Rc<VecModel<TrackItem>>; 5] {
        [
            &self.album_tracks,
            &self.artist_tracks,
            &self.search_tracks,
            &self.playlist_tracks,
            &self.downloads,
        ]
    }

    /// A track was downloaded or removed: update its tick wherever it is listed.
    pub fn mark_offline(&mut self, track_id: i32, offline: bool) {
        for model in self.track_models() {
            for row in 0..model.row_count() {
                let mut item = model.row_data(row).expect("row is in range");
                if item.id == track_id && item.offline != offline {
                    item.offline = offline;
                    model.set_row_data(row, item);
                }
            }
        }
        for row in 0..self.queue.row_count() {
            let mut item = self.queue.row_data(row).expect("row is in range");
            if item.track.id == track_id && item.track.offline != offline {
                item.track.offline = offline;
                self.queue.set_row_data(row, item);
            }
        }
    }

    /// Which page's list is the queue, so its play button can show pause and
    /// pause it. Call after the queue or a page's list changes.
    pub fn sync_source(&self, ui: &AppWindow) {
        let same = |model: &Rc<VecModel<TrackItem>>| {
            !self.queue_ids.is_empty()
                && model.row_count() == self.queue_ids.len()
                && model.iter().zip(&self.queue_ids).all(|(t, id)| t.id == *id)
        };
        let source = [
            ("album", &self.album_tracks),
            ("artist", &self.artist_tracks),
            ("playlist", &self.playlist_tracks),
            ("downloads", &self.downloads),
        ]
        .into_iter()
        .find(|(_, model)| same(model))
        .map_or("", |(name, _)| name);
        let app = ui.global::<App>();
        if app.get_playing_source() != source {
            app.set_playing_source(source.into());
        }
    }

    /// Moves the "playing" highlight in every list when the track changes.
    fn highlight(&mut self, current: Option<i32>) {
        if current == self.current {
            return;
        }
        self.current = current;
        for model in self.track_models() {
            for row in 0..model.row_count() {
                let mut item = model.row_data(row).expect("row is in range");
                let now = current == Some(item.id);
                if item.current != now {
                    item.current = now;
                    model.set_row_data(row, item);
                }
            }
        }
    }

    /// A cover arrived: show it wherever that album appears.
    pub fn art(&mut self, ui: &AppWindow, album_id: i32, pixels: Pixels) {
        let image = Image::from_rgb8(pixels);
        for model in [&self.albums, &self.artist_albums, &self.search_albums] {
            for row in 0..model.row_count() {
                let mut item = model.row_data(row).expect("row is in range");
                if item.id == album_id {
                    item.art = image.clone();
                    model.set_row_data(row, item);
                }
            }
        }
        for row in 0..self.playlists.row_count() {
            let mut item = self.playlists.row_data(row).expect("row is in range");
            if item.album_id == album_id {
                item.art = image.clone();
                self.playlists.set_row_data(row, item);
            }
        }
        for row in 0..self.playlist_covers.row_count() {
            let on_album = self
                .playlist_tracks
                .row_data(row)
                .is_some_and(|t| t.album_id == album_id);
            if on_album {
                self.playlist_covers.set_row_data(row, image.clone());
            }
        }
        for row in 0..self.queue.row_count() {
            let mut item = self.queue.row_data(row).expect("row is in range");
            if item.track.album_id == album_id {
                item.art = image.clone();
                self.queue.set_row_data(row, item);
            }
        }
        for row in 0..self.stats_tracks.row_count() {
            let mut item = self.stats_tracks.row_data(row).expect("row is in range");
            if item.album_id == album_id {
                item.art = image.clone();
                self.stats_tracks.set_row_data(row, item);
            }
        }
        let app = ui.global::<App>();
        let mut album = app.get_album();
        if album.id == album_id {
            album.art = image.clone();
            app.set_album(album);
        }
        let mut playlist = app.get_playlist();
        if playlist.album_id == album_id {
            playlist.art = image.clone();
            app.set_playlist(playlist);
        }
        if self.playback_album == Some(album_id) {
            ui.global::<Playback>().set_art(image);
        }
    }

    /// Mirrors the hub onto the player bar, queue and device list. Called a few
    /// times a second, so only what changed is touched.
    pub fn hub(
        &mut self,
        ui: &AppWindow,
        state: &HubState,
        offline: impl Fn(i32) -> bool,
        // The cover if it is already in memory; otherwise it is fetched and
        // arrives through `art` below.
        art: impl Fn(i32) -> Option<Pixels>,
    ) {
        let playback = ui.global::<Playback>();
        let player = &state.player;
        let current = player.current();

        playback.set_has_track(current.is_some());
        if let Some(track) = current {
            playback.set_title(track.title.as_str().into());
            playback.set_artist(track.artist.name.as_str().into());
            playback.set_album_title(track.album.title.as_str().into());
            playback.set_album_id(track.album.id);
            if self.playback_album != Some(track.album.id) {
                self.playback_album = Some(track.album.id);
                let pixels = track
                    .album
                    .has_artwork
                    .then(|| art(track.album.id))
                    .flatten();
                playback.set_art(pixels.map(Image::from_rgb8).unwrap_or_default());
            }
        }
        let duration = player
            .duration
            .or(current.and_then(|t| t.duration))
            .unwrap_or(0.0);
        playback.set_playing(player.playing);
        playback.set_loading(player.loading);
        playback.set_position(player.position as f32);
        playback.set_duration(duration as f32);
        playback.set_position_text(time(player.position).into());
        playback.set_duration_text(time(duration).into());
        playback.set_buffered(player.buffered);
        playback.set_error(player.error.clone().unwrap_or_default().into());
        playback.set_shuffle(player.queue.shuffle);
        playback.set_repeat(match player.queue.repeat {
            RepeatMode::Off => 0,
            RepeatMode::All => 1,
            RepeatMode::One => 2,
        });
        playback.set_volume(player.volume);
        playback.set_queue_index(player.queue.index as i32);

        let ids: Vec<i32> = player.queue.tracks.iter().map(|t| t.id).collect();
        // What is playing and what comes after it, in play order: the shuffled
        // order when this device is the one playing (a remote has no way to know
        // the other device's order, so it lists them as queued). Songs already
        // played are left out.
        let tracks = &player.queue.tracks;
        let index = player.queue.index;
        let order = &player.queue.order;
        let rows: Vec<usize> = if state.is_active && order.len() == tracks.len() {
            let pos = order.iter().position(|&i| i == index).unwrap_or(0);
            order[pos..].to_vec()
        } else {
            (index.min(tracks.len())..tracks.len()).collect()
        };
        if ids != self.queue_ids || rows != self.queue_rows {
            let items: Vec<QueueItem> = rows
                .iter()
                .enumerate()
                .map(|(row, &i)| (row, i, &tracks[i]))
                .map(|(row, i, t)| QueueItem {
                    index: i as i32,
                    track: track_item(t, row, false, None, offline(t.id)),
                    art: t
                        .album
                        .has_artwork
                        .then(|| art(t.album.id))
                        .flatten()
                        .map(Image::from_rgb8)
                        .unwrap_or_default(),
                })
                .collect();
            self.queue.set_vec(items);
            self.queue_ids = ids;
            self.queue_rows = rows;
            self.sync_source(ui);
        }
        self.highlight(current.map(|t| t.id));

        let active_name = state
            .devices
            .iter()
            .find(|d| Some(&d.device_id) == state.active_id.as_ref())
            .map(|d| d.name.clone())
            .unwrap_or_default();
        playback.set_remote(!state.is_active);
        playback.set_active_device(active_name.into());
        playback.set_connected(state.connected);
        let devices: Vec<DeviceItem> = state
            .devices
            .iter()
            .map(|d| DeviceItem {
                id: d.device_id.as_str().into(),
                name: d.name.as_str().into(),
                platform: d.platform.as_str().into(),
                active: Some(&d.device_id) == state.active_id.as_ref(),
                me: d.device_id == state.me,
            })
            .collect();
        if devices.len() != self.devices.row_count()
            || devices
                .iter()
                .enumerate()
                .any(|(i, d)| self.devices.row_data(i).as_ref() != Some(d))
        {
            self.devices.set_vec(devices);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_times_and_lengths() {
        assert_eq!(time(0.0), "0:00");
        assert_eq!(time(65.9), "1:05");
        assert_eq!(time(3725.0), "1:02:05");
        assert_eq!(time(-3.0), "0:00");
        assert_eq!(length(2880.0), "48 min");
        assert_eq!(length(3600.0), "1 hr");
        assert_eq!(length(4320.0), "1 hr 12 min");
        assert_eq!(plural(1, "song", "songs"), "1 song");
        assert_eq!(plural(3, "song", "songs"), "3 songs");
        assert_eq!(bytes(5 * 1024 * 1024 + 300_000), "5.3 MB");
        assert_eq!(bytes(340 * 1024 * 1024), "340 MB");
        assert_eq!(bytes(1288 * 1024 * 1024), "1.3 GB");
    }
}
