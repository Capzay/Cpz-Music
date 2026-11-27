//! Album covers: fetched once, kept on disk, decoded and shrunk off the UI thread.
//!
//! Full-size covers are often 1500px or more, so three hundred of them decoded
//! as they are would take gigabytes. Each is shrunk to thumbnail size before it
//! reaches the UI, and only that copy stays in memory.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use slint::{Rgb8Pixel, SharedPixelBuffer};
use tokio::sync::Semaphore;

/// Big enough for the album page's hero cover on a HiDPI screen.
const SIZE: u32 = 480;

pub type Pixels = SharedPixelBuffer<Rgb8Pixel>;

pub struct Art {
    http: reqwest::Client,
    server: String,
    dir: PathBuf,
    ready: Mutex<HashMap<i32, Pixels>>,
    /// Being fetched, or known to have no cover.
    skip: Mutex<HashSet<i32>>,
    limit: Semaphore,
}

impl Art {
    pub fn new(http: reqwest::Client, server: &str, dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            http,
            server: server.trim_end_matches('/').to_owned(),
            dir,
            ready: Mutex::new(HashMap::new()),
            skip: Mutex::new(HashSet::new()),
            limit: Semaphore::new(6),
        })
    }

    /// The cover if it is already in memory.
    pub fn get(&self, album_id: i32) -> Option<Pixels> {
        self.ready.lock().unwrap().get(&album_id).cloned()
    }

    /// Fetches a cover in the background and calls `done` with it, unless it is
    /// already on its way or there is none.
    pub fn request(
        self: &Arc<Self>,
        album_id: i32,
        done: impl FnOnce(i32, Pixels) + Send + 'static,
    ) {
        if let Some(pixels) = self.get(album_id) {
            done(album_id, pixels);
            return;
        }
        if !self.skip.lock().unwrap().insert(album_id) {
            return;
        }
        let art = self.clone();
        tokio::spawn(async move {
            let _permit = art.limit.acquire().await;
            // On failure the id stays in `skip`: no cover, or a broken one, is not
            // asked for again this run.
            if let Some(pixels) = art.load(album_id).await {
                art.ready.lock().unwrap().insert(album_id, pixels.clone());
                art.skip.lock().unwrap().remove(&album_id);
                done(album_id, pixels);
            }
        });
    }

    async fn load(&self, album_id: i32) -> Option<Pixels> {
        let path = self.dir.join(format!("{album_id}"));
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(_) => {
                let url = format!("{}/api/artwork/{album_id}", self.server);
                let response = self.http.get(url).send().await.ok()?;
                if !response.status().is_success() {
                    return None;
                }
                let bytes = response.bytes().await.ok()?.to_vec();
                let _ = tokio::fs::create_dir_all(&self.dir).await;
                let _ = tokio::fs::write(&path, &bytes).await;
                bytes
            }
        };
        tokio::task::spawn_blocking(move || decode(&bytes))
            .await
            .ok()?
    }
}

pub fn decode(bytes: &[u8]) -> Option<Pixels> {
    let image = image::load_from_memory(bytes).ok()?;
    let thumb = if image.width() > SIZE || image.height() > SIZE {
        image.thumbnail(SIZE, SIZE)
    } else {
        image
    };
    let rgb = thumb.to_rgb8();
    Some(SharedPixelBuffer::clone_from_slice(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
    ))
}
