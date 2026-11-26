//! Draws a screen offscreen with sample data and saves it as a PNG, for checking
//! the UI without a server, a session or even a display:
//!
//!     cpz-music --screenshot albums out.png [1280x800]
//!
//! Screens: setup, signin, albums, album, artist, artists, search, queue,
//! devices, remote.

use std::rc::Rc;

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, Image, PhysicalSize, PlatformError, Rgb8Pixel, SharedPixelBuffer};

use crate::view::{self};
use crate::{AlbumItem, App, AppWindow, ArtistItem, DeviceItem, Playback, TrackItem};

struct Offscreen(Rc<MinimalSoftwareWindow>);

impl Platform for Offscreen {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
}

/// A cover made of a diagonal gradient, different per album.
fn cover(seed: u32) -> Image {
    let size = 160;
    let mut pixels = SharedPixelBuffer::<Rgb8Pixel>::new(size, size);
    let hue = seed.wrapping_mul(67) % 360;
    let base = |offset: u32| {
        (((hue + offset) % 360) as f32 / 360.0 * std::f32::consts::TAU).sin() * 0.5 + 0.5
    };
    let (r, g, b) = (base(0), base(120), base(240));
    for (i, pixel) in pixels.make_mut_slice().iter_mut().enumerate() {
        let (x, y) = ((i as u32 % size) as f32, (i as u32 / size) as f32);
        let t = (x + y) / (2.0 * size as f32);
        let shade = |c: f32| ((c * (1.0 - t) + 0.15 * t) * 220.0) as u8;
        *pixel = Rgb8Pixel {
            r: shade(r),
            g: shade(g),
            b: shade(b),
        };
    }
    Image::from_rgb8(pixels)
}

/// Sample data, generated rather than typed out: the screenshots only need
/// something of the right shape to lay out.
fn albums(count: usize) -> Vec<AlbumItem> {
    (0..count)
        .map(|i| AlbumItem {
            id: i as i32 + 1,
            title: format!("Album {}", i + 1).into(),
            artist: format!("Artist {}", i % 4 + 1).into(),
            year: (2000 + i).to_string().into(),
            // One without art, to show the placeholder.
            art: if i == 4 {
                Image::default()
            } else {
                cover(i as u32 + 3)
            },
        })
        .collect()
}

fn songs(count: usize, current: Option<usize>) -> Vec<TrackItem> {
    (0..count)
        .map(|i| TrackItem {
            id: i as i32 + 1,
            title: format!("Song {}", i + 1).into(),
            artist: "Artist 1".into(),
            album: "Album 1".into(),
            number: (i + 1).to_string().into(),
            duration: format!("{}:{:02}", 3 + i % 3, (i * 17) % 60).into(),
            current: current == Some(i),
            offline: i < 3,
        })
        .collect()
}

fn device(id: &str, name: &str, platform: &str, active: bool, me: bool) -> DeviceItem {
    DeviceItem {
        id: id.into(),
        name: name.into(),
        platform: platform.into(),
        active,
        me,
    }
}

fn fill(ui: &AppWindow, screen: &str) {
    let app = ui.global::<App>();
    let playback = ui.global::<Playback>();
    let remote = screen == "remote";
    view::with(|v| {
        v.albums.set_vec(albums(10));
        v.artists.set_vec(
            (0..6)
                .map(|i| ArtistItem {
                    id: i + 1,
                    name: format!("Artist {}", i + 1).into(),
                    detail: format!("{} albums · {} songs", i % 3 + 1, 10 + i * 7).into(),
                })
                .collect::<Vec<_>>(),
        );
        v.album_tracks.set_vec(songs(10, Some(6)));
        v.artist_tracks.set_vec(songs(10, Some(6)));
        v.artist_albums.set_vec(albums(2));
        v.search_tracks.set_vec(songs(3, None));
        v.search_albums.set_vec(albums(2));
        v.queue.set_vec(songs(10, None));
        v.devices.set_vec(vec![
            device("a", "This computer", "Linux", !remote, true),
            device("b", "Phone", "Android", remote, false),
            device("c", "Desktop", "Windows", false, false),
        ]);
    });

    app.set_album(AlbumItem {
        id: 1,
        title: "Album 1".into(),
        artist: "Artist 1".into(),
        year: "2000".into(),
        art: cover(3),
    });
    app.set_album_detail("2000 · 10 songs · 42 min".into());
    app.set_artist_name("Artist 1".into());
    app.set_artist_detail("2 albums · 10 songs".into());
    app.set_search_text("song".into());
    app.set_server_url("https://music.example.com".into());

    playback.set_has_track(true);
    playback.set_title("Song 7".into());
    playback.set_artist("Artist 1".into());
    playback.set_album_title("Album 1".into());
    playback.set_album_id(1);
    playback.set_art(cover(3));
    playback.set_playing(true);
    playback.set_position(97.0);
    playback.set_duration(290.0);
    playback.set_position_text("1:37".into());
    playback.set_duration_text("4:50".into());
    playback.set_buffered(0.7);
    playback.set_volume(0.7);
    playback.set_repeat(1);
    playback.set_queue_index(6);
    playback.set_connected(true);
    playback.set_remote(remote);
    playback.set_active_device("Phone".into());

    let page = match screen {
        "queue" | "devices" | "remote" => "album",
        other => other,
    };
    app.set_page(page.into());
    app.set_can_go_back(matches!(page, "album" | "artist"));
    playback.set_show_queue(screen == "queue");
    playback.set_show_devices(screen == "devices");
}

pub fn run(screen: &str, out: &str, size: &str) -> anyhow::Result<()> {
    let (width, height) = size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .unwrap_or((1280u32, 800u32));

    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Offscreen(window.clone())))
        .map_err(|err| anyhow::anyhow!("{err:?}"))?;
    let ui = AppWindow::new()?;
    view::install(&ui);
    fill(&ui, screen);
    window.set_size(PhysicalSize::new(width, height));
    ui.show()?;

    // Let layouts settle and any first-frame animation finish.
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        std::thread::sleep(std::time::Duration::from_millis(50));
        window.request_redraw();
        window.draw_if_needed(|_| {});
    }
    let mut buffer = vec![Rgb8Pixel::default(); (width * height) as usize];
    window.request_redraw();
    window.draw_if_needed(|renderer| {
        renderer.render(&mut buffer, width as usize);
    });
    let bytes: Vec<u8> = buffer.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
    image::save_buffer(out, &bytes, width, height, image::ExtendedColorType::Rgb8)?;
    println!("{out}");
    Ok(())
}
