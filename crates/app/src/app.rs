//! Connects the screens to cpz-core: what each button does, and getting the
//! answers back onto the screen.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpz_core::api::{Api, ApiError, ClientRelease, fetch_config};
use cpz_core::audio::Output;
use cpz_core::auth::{
    Credentials, LOOPBACK_PORT, Pkce, Session, SupabaseAuth, authorize_url, loopback_redirect,
    receive_code,
};
use cpz_core::downloads::Downloads;
use cpz_core::listens::Listens;
use cpz_core::player::{Command, Player, PlayerDeps};
use cpz_core::playlists::Playlists;
use cpz_core::realtime::{Channel, ChannelConfig, Hub, Identity, PLAYER_TOPIC};
use cpz_core::store::Store;
use cpz_core::types::{AlbumSummary, ArtistCard, ClientConfig, PlayerTrack};
use slint::{ComponentHandle, Image, Weak};

use crate::art::{Art, Pixels};
use crate::view::{
    self, album_item, artist_hit, artist_item, bytes, cover_album, length, playlist_item, plural,
    track_item,
};
use crate::{AlbumItem, App, AppWindow, Playback, PlaylistItem};

const SERVER_KEY: &str = "server";
const CONFIG_KEY: &str = "client-config";
const SESSION_KEY: &str = "session";
const SEARCH_DELAY: Duration = Duration::from_millis(250);
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(600);
/// Edits sync straight away; this catches ones made on other devices.
const PLAYLIST_SYNC_EVERY: Duration = Duration::from_secs(60);
/// How often to ask the server whether a newer build of the app is out.
const UPDATE_CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

pub struct Paths {
    pub data: PathBuf,
    pub cache: PathBuf,
}

/// Everything that needs a signed-in session.
pub struct Services {
    pub api: Api,
    pub hub: Hub,
    pub downloads: Arc<Downloads>,
    pub playlists: Arc<Playlists>,
    pub art: Arc<Art>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Route {
    Albums,
    Artists,
    Album(i32),
    Artist(i32),
    Search,
    Playlists,
    /// By uuid.
    Playlist(String),
    Downloads,
}

impl Route {
    fn page(&self) -> &'static str {
        match self {
            Self::Albums => "albums",
            Self::Artists => "artists",
            Self::Album(_) => "album",
            Self::Artist(_) => "artist",
            Self::Search => "search",
            Self::Playlists => "playlists",
            Self::Playlist(_) => "playlist",
            Self::Downloads => "downloads",
        }
    }
}

/// The track lists behind whatever pages are open, so a tap on row `i` knows
/// what to queue.
#[derive(Default)]
struct Lists {
    album: Vec<PlayerTrack>,
    artist: Vec<PlayerTrack>,
    search: Vec<PlayerTrack>,
    playlist: Vec<PlayerTrack>,
    downloads: Vec<PlayerTrack>,
}

impl Lists {
    fn get(&self, source: &str) -> &[PlayerTrack] {
        match source {
            "album" => &self.album,
            "artist" => &self.artist,
            "search" => &self.search,
            "playlist" => &self.playlist,
            "downloads" => &self.downloads,
            _ => &[],
        }
    }
}

struct Shared {
    store: Arc<Store>,
    paths: Paths,
    http: reqwest::Client,
    services: Mutex<Option<Arc<Services>>>,
    routes: Mutex<Vec<Route>>,
    /// Bumped on every page change, so a slow answer for a page already left
    /// is dropped instead of overwriting the one on screen.
    page_generation: AtomicU64,
    search_generation: AtomicU64,
    lists: Mutex<Lists>,
    albums: Mutex<Option<Vec<AlbumSummary>>>,
    artists: Mutex<Option<Vec<ArtistCard>>>,
    album_artist: Mutex<Option<i32>>,
    sign_in: Mutex<Option<tokio::task::AbortHandle>>,
    /// Shared by every download, so three albums queued at once still only
    /// fetch two songs at a time.
    download_slots: Arc<tokio::sync::Semaphore>,
    progress: Mutex<Progress>,
    /// A newer build the server offered, until it is installed.
    update: Mutex<Option<ClientRelease>>,
    /// Installed: start the new build once this one has closed.
    restart: AtomicBool,
}

/// Downloads asked for since the last batch finished.
#[derive(Default)]
struct Progress {
    total: usize,
    done: usize,
    failed: usize,
}

impl Progress {
    fn status(&self) -> String {
        if self.done + self.failed < self.total {
            format!(
                "Downloading {} of {}",
                self.done + self.failed + 1,
                self.total
            )
        } else if self.failed > 0 {
            format!(
                "{} failed to download",
                plural(self.failed, "song", "songs")
            )
        } else {
            String::new()
        }
    }
}

#[derive(Clone)]
pub struct Ctx {
    ui: Weak<AppWindow>,
    shared: Arc<Shared>,
}

fn platform() -> &'static str {
    if cfg!(target_os = "android") {
        "Android"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else {
        "Linux"
    }
}

/// The machine's name, which is what people recognise in a device list.
fn device_name() -> String {
    let host = std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/proc/sys/kernel/hostname").ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());
    host.unwrap_or_else(|| format!("{} device", platform()))
}

fn describe(err: &ApiError) -> String {
    match err {
        ApiError::Network(_) => {
            "Can't reach the server. Check your connection and try again.".into()
        }
        ApiError::SignedOut => "You've been signed out.".into(),
        ApiError::Status(status) => format!("The server answered {status}."),
        ApiError::Io(err) => err.to_string(),
    }
}

/// Accepts "music.example.com" as well as a full URL.
pub fn normalise_server(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let url = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };
    let parsed = url::Url::parse(&url).ok()?;
    matches!(parsed.scheme(), "http" | "https").then(|| url)
}

impl Ctx {
    pub fn new(ui: &AppWindow, store: Arc<Store>, paths: Paths) -> Self {
        Self {
            ui: ui.as_weak(),
            shared: Arc::new(Shared {
                store,
                paths,
                http: cpz_core::http::client(),
                services: Mutex::new(None),
                routes: Mutex::new(Vec::new()),
                page_generation: AtomicU64::new(0),
                search_generation: AtomicU64::new(0),
                lists: Mutex::new(Lists::default()),
                albums: Mutex::new(None),
                artists: Mutex::new(None),
                album_artist: Mutex::new(None),
                sign_in: Mutex::new(None),
                download_slots: Arc::new(tokio::sync::Semaphore::new(2)),
                progress: Mutex::new(Progress::default()),
                update: Mutex::new(None),
                restart: AtomicBool::new(false),
            }),
        }
    }

    /// Runs `change` on the UI thread, if the window is still open.
    fn ui(&self, change: impl FnOnce(&AppWindow) + Send + 'static) {
        let weak = self.ui.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                change(&ui);
            }
        });
    }

    fn services(&self) -> Option<Arc<Services>> {
        self.shared.services.lock().unwrap().clone()
    }

    fn send(&self, command: Command) {
        if let Some(services) = self.services() {
            services.hub.send(command);
        }
    }

    pub fn restart_requested(&self) -> bool {
        self.shared.restart.load(Ordering::Relaxed)
    }

    pub fn shutdown(&self) {
        self.shared.services.lock().unwrap().take();
    }

    // ── Start-up and signing in ─────────────────────────────────────────────

    /// Picks up where the last run left off: a saved server and session go
    /// straight to the library, even offline.
    pub fn boot(&self) {
        let store = &self.shared.store;
        let server: Option<String> = store.get(SERVER_KEY).ok().flatten();
        let config: Option<ClientConfig> = store.get(CONFIG_KEY).ok().flatten();
        let session: Option<Session> = store.get(SESSION_KEY).ok().flatten();

        let initial = server
            .clone()
            .or_else(|| std::env::var("CPZ_SERVER_URL").ok())
            .unwrap_or_default();
        self.ui(move |ui| ui.global::<App>().set_server_url(initial.into()));

        match (server, config, session) {
            (Some(server), Some(config), Some(session)) => {
                let ctx = self.clone();
                tokio::spawn(async move { ctx.start(server, config, session).await });
            }
            (Some(_), Some(_), None) => self.ui(|ui| ui.global::<App>().set_page("signin".into())),
            _ => self.ui(|ui| ui.global::<App>().set_page("setup".into())),
        }
    }

    fn connect(&self, raw: String) {
        let Some(server) = normalise_server(&raw) else {
            self.ui(|ui| {
                ui.global::<App>()
                    .set_setup_error("That doesn't look like a web address.".into())
            });
            return;
        };
        let ctx = self.clone();
        self.ui(|ui| {
            let app = ui.global::<App>();
            app.set_busy(true);
            app.set_setup_error("".into());
        });
        tokio::spawn(async move {
            let result = fetch_config(&ctx.shared.http, &server).await;
            let store = ctx.shared.store.clone();
            ctx.ui(move |ui| {
                let app = ui.global::<App>();
                app.set_busy(false);
                match result {
                    Ok(config) => {
                        let _ = store.set(SERVER_KEY, &server);
                        let _ = store.set(CONFIG_KEY, &config);
                        app.set_server_url(server.into());
                        app.set_page("signin".into());
                    }
                    Err(ApiError::Status(_)) => app.set_setup_error(
                        "That server answered, but it isn't a Cpz Music server, or it's an older version.".into(),
                    ),
                    Err(err) => app.set_setup_error(describe(&err).into()),
                }
            });
        });
    }

    fn sign_in(&self) {
        let store = &self.shared.store;
        let (Some(server), Some(config)) = (
            store.get::<String>(SERVER_KEY).ok().flatten(),
            store.get::<ClientConfig>(CONFIG_KEY).ok().flatten(),
        ) else {
            self.ui(|ui| ui.global::<App>().set_page("setup".into()));
            return;
        };
        self.ui(|ui| {
            let app = ui.global::<App>();
            app.set_signing_in(true);
            app.set_setup_error("".into());
        });

        let ctx = self.clone();
        let task = tokio::spawn(async move {
            let result = ctx.sign_in_flow(&config).await;
            match result {
                Ok(session) => {
                    let _ = ctx.shared.store.set(SESSION_KEY, &session);
                    ctx.ui(|ui| ui.global::<App>().set_signing_in(false));
                    ctx.start(server, config, session).await;
                }
                Err(message) => ctx.ui(move |ui| {
                    let app = ui.global::<App>();
                    app.set_signing_in(false);
                    app.set_setup_error(message.into());
                }),
            }
        });
        if let Some(previous) = self
            .shared
            .sign_in
            .lock()
            .unwrap()
            .replace(task.abort_handle())
        {
            previous.abort();
        }
    }

    async fn sign_in_flow(&self, config: &ClientConfig) -> Result<Session, String> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", LOOPBACK_PORT))
            .await
            .map_err(|_| {
                "Another sign-in is already waiting. Close it and try again.".to_owned()
            })?;
        let pkce = Pkce::new();
        let url = authorize_url(config, &loopback_redirect(LOOPBACK_PORT), &pkce);
        webbrowser::open(&url).map_err(|err| format!("Couldn't open a browser: {err}"))?;

        let code = tokio::time::timeout(SIGN_IN_TIMEOUT, receive_code(listener))
            .await
            .map_err(|_| "Sign-in timed out.".to_owned())?
            .map_err(|err| format!("Sign-in failed: {err}"))?;
        SupabaseAuth::new(self.shared.http.clone(), config.clone())
            .exchange(&code, &pkce.verifier)
            .await
            .map_err(|err| format!("Sign-in failed: {err}"))
    }

    fn cancel_sign_in(&self) {
        if let Some(task) = self.shared.sign_in.lock().unwrap().take() {
            task.abort();
        }
        self.ui(|ui| ui.global::<App>().set_signing_in(false));
    }

    fn sign_out(&self) {
        let services = self.shared.services.lock().unwrap().take();
        let _ = self.shared.store.remove(SESSION_KEY);
        *self.shared.albums.lock().unwrap() = None;
        *self.shared.artists.lock().unwrap() = None;
        if let Some(services) = services {
            tokio::spawn(async move { services.api.credentials().sign_out().await });
        }
        self.ui(|ui| ui.global::<App>().set_page("signin".into()));
    }

    /// Opens everything a signed-in session needs and shows the library.
    async fn start(&self, server: String, config: ClientConfig, session: Session) {
        let store = self.shared.store.clone();
        let http = self.shared.http.clone();

        let persist = store.clone();
        let ctx = self.clone();
        let credentials = Credentials::new(
            SupabaseAuth::new(http.clone(), config.clone()),
            Some(session),
            Box::new(move |session| match session {
                Some(session) => {
                    let _ = persist.set(SESSION_KEY, session);
                }
                // Refused by the auth server: back to the sign-in screen.
                None => {
                    let _ = persist.remove(SESSION_KEY);
                    ctx.ui(|ui| ui.global::<App>().set_page("signin".into()));
                }
            }),
        );
        let api = Api::new(http.clone(), &server, credentials.clone());

        // Stamps the owner flag the player channel needs. Refused means this is
        // not the library's owner; unreachable is fine, the app works offline.
        match api.claim_owner().await {
            Err(ApiError::Status(status)) if status.as_u16() == 401 => {
                let _ = store.remove(SESSION_KEY);
                self.ui(|ui| {
                    let app = ui.global::<App>();
                    app.set_page("signin".into());
                    app.set_setup_error("That GitHub account doesn't own this library.".into());
                });
                return;
            }
            Err(err) => tracing::warn!("could not stamp the owner flag: {err}"),
            Ok(()) => {}
        }

        let downloads = Arc::new(Downloads::new(
            store.clone(),
            self.shared.paths.data.join("downloads"),
        ));
        let listens = Arc::new(Listens::new(store.clone()));
        let art = Art::new(http, &server, self.shared.paths.cache.join("art"));
        let player = Player::start(PlayerDeps {
            api: api.clone(),
            store: store.clone(),
            downloads: downloads.clone(),
            listens: listens.clone(),
            cache_dir: self.shared.paths.cache.join("stream"),
            output: Output::Device,
        });

        let mut identity = Identity::load(&store, platform());
        if store.get::<String>("device-name").ok().flatten().is_none() {
            identity.name = device_name();
        }
        let channel = Channel::spawn(
            ChannelConfig {
                supabase: config,
                topic: PLAYER_TOPIC.into(),
                presence_key: identity.id.clone(),
            },
            credentials,
        );
        let hub = Hub::start(player, Some(channel), identity, store);

        let playlists = Arc::new(Playlists::load(self.shared.store.clone()));
        let services = Arc::new(Services {
            api: api.clone(),
            hub,
            downloads,
            playlists,
            art,
        });
        *self.shared.services.lock().unwrap() = Some(services.clone());
        tokio::spawn(async move { listens.flush(&api).await });

        self.watch_hub(&services);
        self.watch_updates(&services);
        self.refresh_playlists();
        let ctx = self.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(PLAYLIST_SYNC_EVERY);
            loop {
                every.tick().await;
                // Stops once signed out, when these services are gone.
                if !ctx
                    .services()
                    .is_some_and(|current| Arc::ptr_eq(&current, &services))
                {
                    return;
                }
                ctx.sync_playlists_now(&services).await;
            }
        });
        let ctx = self.clone();
        self.ui(move |_| ctx.go_top(Route::Albums));
    }

    // ── Updates ─────────────────────────────────────────────────────────────

    /// Asks the server for a newer build now and every few hours, while these
    /// services last.
    fn watch_updates(&self, services: &Arc<Services>) {
        if !crate::update::enabled() {
            return;
        }
        let ctx = self.clone();
        let services = services.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(UPDATE_CHECK_EVERY);
            loop {
                every.tick().await;
                if !ctx
                    .services()
                    .is_some_and(|current| Arc::ptr_eq(&current, &services))
                {
                    return;
                }
                match crate::update::check(&services.api).await {
                    Ok(Some(release)) => {
                        let version = release.version.clone();
                        *ctx.shared.update.lock().unwrap() = Some(release);
                        ctx.ui(move |ui| {
                            let app = ui.global::<App>();
                            app.set_update_version(version.into());
                            app.set_update_error("".into());
                        });
                    }
                    Ok(None) => {}
                    Err(err) => tracing::debug!("update check failed: {err}"),
                }
            }
        });
    }

    /// Downloads and installs the offered build, then closes the app so
    /// `main` can start the new one.
    fn install_update(&self) {
        let Some(services) = self.services() else {
            return;
        };
        let Some(release) = self.shared.update.lock().unwrap().clone() else {
            return;
        };
        let ctx = self.clone();
        let scratch = self.shared.paths.cache.clone();
        self.ui(|ui| {
            let app = ui.global::<App>();
            app.set_update_busy(true);
            app.set_update_error("".into());
        });
        tokio::spawn(async move {
            let _ = tokio::fs::create_dir_all(&scratch).await;
            match crate::update::install(&services.api, &release, &scratch).await {
                Ok(()) => {
                    tracing::info!("updated to {}, restarting", release.version);
                    ctx.shared.restart.store(true, Ordering::Relaxed);
                    let _ = slint::invoke_from_event_loop(|| {
                        let _ = slint::quit_event_loop();
                    });
                }
                Err(err) => {
                    tracing::warn!("update failed: {err:#}");
                    let message = format!("{err:#}");
                    ctx.ui(move |ui| {
                        let app = ui.global::<App>();
                        app.set_update_busy(false);
                        app.set_update_error(message.into());
                    });
                }
            }
        });
    }

    fn watch_hub(&self, services: &Arc<Services>) {
        let mut changes = services.hub.subscribe();
        let ctx = self.clone();
        let (downloads, art) = (services.downloads.clone(), services.art.clone());
        tokio::spawn(async move {
            loop {
                let state = changes.borrow_and_update().clone();
                let (downloads, art, ctx2) = (downloads.clone(), art.clone(), ctx.clone());
                ctx.ui(move |ui| {
                    view::with(|v| {
                        v.hub(
                            ui,
                            &state,
                            |id| downloads.is_downloaded(id),
                            |album| {
                                let pixels = art.get(album);
                                if pixels.is_none() {
                                    ctx2.request_art(&art, album);
                                }
                                pixels
                            },
                        );
                    });
                });
                if changes.changed().await.is_err() {
                    return;
                }
            }
        });
    }

    fn request_art(&self, art: &Arc<Art>, album_id: i32) {
        let ctx = self.clone();
        art.request(album_id, move |id, pixels| {
            ctx.ui(move |ui| view::with(|v| v.art(ui, id, pixels)));
        });
    }

    // ── Pages ───────────────────────────────────────────────────────────────

    fn go_top(&self, route: Route) {
        *self.shared.routes.lock().unwrap() = vec![route.clone()];
        self.show(route);
    }

    fn open(&self, route: Route) {
        let mut routes = self.shared.routes.lock().unwrap();
        if routes.last() != Some(&route) {
            routes.push(route.clone());
        }
        drop(routes);
        self.show(route);
    }

    fn back(&self) {
        let route = {
            let mut routes = self.shared.routes.lock().unwrap();
            if routes.len() > 1 {
                routes.pop();
            }
            routes.last().cloned()
        };
        if let Some(route) = route {
            self.show(route);
        }
    }

    fn show(&self, route: Route) {
        let generation = self.shared.page_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let can_go_back = self.shared.routes.lock().unwrap().len() > 1;
        let page = route.page();
        self.ui(move |ui| {
            let app = ui.global::<App>();
            app.set_page(page.into());
            app.set_can_go_back(can_go_back);
            app.set_error("".into());
            app.set_busy(false);
            ui.global::<Playback>().set_show_queue(false);
        });
        let ctx = self.clone();
        tokio::spawn(async move { ctx.load(route, generation).await });
    }

    fn current(&self, generation: u64) -> bool {
        self.shared.page_generation.load(Ordering::Acquire) == generation
    }

    /// Shows a failure on the page, if it is still the one on screen.
    fn failed(&self, generation: u64, err: ApiError) {
        if !self.current(generation) {
            return;
        }
        let offline = matches!(err, ApiError::Network(_));
        let not_found = matches!(err, ApiError::Status(status) if status.as_u16() == 404);
        let message = describe(&err);
        self.ui(move |ui| {
            let app = ui.global::<App>();
            app.set_busy(false);
            app.set_offline(offline);
            // A missing album or artist has its own empty state.
            app.set_error(if not_found { "".into() } else { message.into() });
        });
    }

    fn reached(&self) {
        self.ui(|ui| ui.global::<App>().set_offline(false));
    }

    fn busy(&self) {
        self.ui(|ui| ui.global::<App>().set_busy(true));
    }

    async fn load(&self, route: Route, generation: u64) {
        let Some(services) = self.services() else {
            return;
        };
        let api = &services.api;
        match route {
            Route::Albums => {
                let cached = self.shared.albums.lock().unwrap().clone();
                let albums = match cached {
                    Some(albums) => albums,
                    None => {
                        self.busy();
                        match api.albums().await {
                            Ok(albums) => {
                                self.reached();
                                *self.shared.albums.lock().unwrap() = Some(albums.clone());
                                albums
                            }
                            Err(err) => return self.failed(generation, err),
                        }
                    }
                };
                self.show_albums(&services, albums, generation, AlbumList::Library);
            }
            Route::Artists => {
                let cached = self.shared.artists.lock().unwrap().clone();
                let artists = match cached {
                    Some(artists) => artists,
                    None => {
                        self.busy();
                        match api.artists().await {
                            Ok(artists) => {
                                self.reached();
                                *self.shared.artists.lock().unwrap() = Some(artists.clone());
                                artists
                            }
                            Err(err) => return self.failed(generation, err),
                        }
                    }
                };
                let items: Vec<_> = artists.iter().map(artist_item).collect();
                self.ui(move |ui| {
                    view::with(|v| v.artists.set_vec(items));
                    ui.global::<App>().set_busy(false);
                });
            }
            Route::Album(id) => {
                self.ui(|ui| {
                    ui.global::<App>().set_album(AlbumItem::default());
                    view::with(|v| v.album_tracks.set_vec(Vec::new()));
                });
                self.busy();
                let album = match api.album(id).await {
                    Ok(album) => album,
                    Err(err) => return self.failed(generation, err),
                };
                self.reached();
                if !self.current(generation) {
                    return;
                }
                *self.shared.album_artist.lock().unwrap() = Some(album.artist.id);
                let total: f64 = album.tracks.iter().filter_map(|t| t.duration).sum();
                let mut detail = vec![plural(album.tracks.len(), "song", "songs")];
                if total > 0.0 {
                    detail.push(length(total));
                }
                if let Some(year) = album.year {
                    detail.insert(0, year.to_string());
                }
                let summary = AlbumSummary {
                    id: album.id,
                    title: album.title.clone(),
                    year: album.year,
                    artist: album.artist.name.clone(),
                    has_artwork: album.has_artwork,
                };
                let pixels = services.art.get(album.id);
                let has_artwork = summary.has_artwork;
                let album_id = summary.id;
                let rows = self.track_rows(&services, &album.tracks, true);
                self.shared.lists.lock().unwrap().album = album.tracks;
                self.ui(move |ui| {
                    let app = ui.global::<App>();
                    app.set_album(album_item(&summary, pixels));
                    app.set_album_detail(detail.join(" · ").into());
                    view::with(|v| {
                        v.album_tracks.set_vec(rows);
                        v.sync_source(ui);
                    });
                    app.set_busy(false);
                });
                if has_artwork {
                    self.request_art(&services.art, album_id);
                }
            }
            Route::Artist(id) => {
                self.ui(|ui| {
                    ui.global::<App>().set_artist_name("".into());
                    view::with(|v| {
                        v.artist_albums.set_vec(Vec::new());
                        v.artist_tracks.set_vec(Vec::new());
                    });
                });
                self.busy();
                let artist = match api.artist(id).await {
                    Ok(artist) => artist,
                    Err(err) => return self.failed(generation, err),
                };
                self.reached();
                if !self.current(generation) {
                    return;
                }
                let detail = format!(
                    "{} · {}",
                    plural(artist.albums.len(), "album", "albums"),
                    plural(artist.tracks.len(), "song", "songs")
                );
                let rows = self.track_rows(&services, &artist.tracks, false);
                self.shared.lists.lock().unwrap().artist = artist.tracks;
                let name = artist.name.clone();
                self.ui(move |ui| {
                    let app = ui.global::<App>();
                    app.set_artist_name(name.into());
                    app.set_artist_detail(detail.into());
                    view::with(|v| {
                        v.artist_tracks.set_vec(rows);
                        v.sync_source(ui);
                    });
                    app.set_busy(false);
                });
                self.show_albums(&services, artist.albums, generation, AlbumList::Artist);
            }
            // Results come from typing, not from opening the page.
            Route::Search => {}
            // Playlists live on the device, so there is nothing to wait for.
            Route::Playlists | Route::Playlist(_) => self.refresh_playlists(),
            Route::Downloads => self.refresh_downloads(),
        }
    }

    fn track_rows(
        &self,
        services: &Services,
        tracks: &[PlayerTrack],
        numbered: bool,
    ) -> Vec<crate::TrackItem> {
        let current = services.hub.state().player.current().map(|t| t.id);
        tracks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                track_item(
                    t,
                    i,
                    numbered,
                    current,
                    services.downloads.is_downloaded(t.id),
                )
            })
            .collect()
    }

    fn show_albums(
        &self,
        services: &Arc<Services>,
        albums: Vec<AlbumSummary>,
        generation: u64,
        list: AlbumList,
    ) {
        if !self.current(generation) && list != AlbumList::Search {
            return;
        }
        // Built on the UI thread: an `AlbumItem` holds an image, which cannot
        // cross threads, though the pixels behind it can.
        let wanted: Vec<i32> = albums
            .iter()
            .filter(|a| a.has_artwork && services.art.get(a.id).is_none())
            .map(|a| a.id)
            .collect();
        let pairs: Vec<_> = albums
            .into_iter()
            .map(|a| {
                let pixels = services.art.get(a.id);
                (a, pixels)
            })
            .collect();
        self.ui(move |ui| {
            let items: Vec<AlbumItem> = pairs
                .into_iter()
                .map(|(a, pixels)| album_item(&a, pixels))
                .collect();
            view::with(|v| match list {
                AlbumList::Library => v.albums.set_vec(items),
                AlbumList::Artist => v.artist_albums.set_vec(items),
                AlbumList::Search => v.search_albums.set_vec(items),
            });
            ui.global::<App>().set_busy(false);
        });
        for id in wanted {
            self.request_art(&services.art, id);
        }
    }

    fn search(&self, text: String) {
        let generation = self.shared.search_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let query = text.trim().to_owned();
        if query.is_empty() {
            self.shared.lists.lock().unwrap().search.clear();
            self.ui(|_| {
                view::with(|v| {
                    v.search_tracks.set_vec(Vec::new());
                    v.search_albums.set_vec(Vec::new());
                    v.search_artists.set_vec(Vec::new());
                });
            });
            return;
        }
        let ctx = self.clone();
        tokio::spawn(async move {
            // Typing fast sends one search, for what was typed last.
            tokio::time::sleep(SEARCH_DELAY).await;
            let latest = || ctx.shared.search_generation.load(Ordering::Acquire) == generation;
            if !latest() {
                return;
            }
            let Some(services) = ctx.services() else {
                return;
            };
            ctx.busy();
            let results = match services.api.search(&query).await {
                Ok(results) => results,
                Err(err) => {
                    let message = describe(&err);
                    ctx.ui(move |ui| {
                        let app = ui.global::<App>();
                        app.set_busy(false);
                        app.set_error(message.into());
                    });
                    return;
                }
            };
            if !latest() {
                return;
            }
            ctx.reached();
            let rows = ctx.track_rows(&services, &results.tracks, false);
            let artists: Vec<_> = results.artists.iter().map(artist_hit).collect();
            ctx.shared.lists.lock().unwrap().search = results.tracks;
            ctx.ui(move |_| {
                view::with(|v| {
                    v.search_tracks.set_vec(rows);
                    v.search_artists.set_vec(artists);
                });
            });
            ctx.show_albums(&services, results.albums, 0, AlbumList::Search);
        });
    }

    fn retry(&self) {
        *self.shared.albums.lock().unwrap() = None;
        *self.shared.artists.lock().unwrap() = None;
        let route = self.shared.routes.lock().unwrap().last().cloned();
        if let Some(route) = route {
            self.show(route);
        }
    }

    fn play_list(&self, source: &str, start: usize) {
        let tracks = self.shared.lists.lock().unwrap().get(source).to_vec();
        if start < tracks.len() {
            self.send(Command::SetQueue { tracks, start });
        }
    }

    fn shuffle_list(&self, source: &str) {
        let Some(services) = self.services() else {
            return;
        };
        let tracks = self.shared.lists.lock().unwrap().get(source).to_vec();
        if tracks.is_empty() {
            return;
        }
        let start = fastrand::usize(..tracks.len());
        services.hub.send(Command::SetQueue { tracks, start });
        if !services.hub.state().player.queue.shuffle {
            services.hub.send(Command::ToggleShuffle);
        }
    }

    // ── Playlists ───────────────────────────────────────────────────────────

    fn open_playlist_id(&self) -> Option<String> {
        match self.shared.routes.lock().unwrap().last() {
            Some(Route::Playlist(id)) => Some(id.clone()),
            _ => None,
        }
    }

    /// Redraws the playlists list, and the open playlist if there is one, from
    /// what is on the device. Needs no network, so it works offline.
    fn refresh_playlists(&self) {
        let Some(services) = self.services() else {
            return;
        };
        let art = &services.art;
        let docs = services.playlists.list();
        let rows: Vec<_> = docs
            .iter()
            .map(|doc| {
                (
                    doc.clone(),
                    cover_album(doc).and_then(|album| art.get(album)),
                )
            })
            .collect();
        let open = self
            .open_playlist_id()
            .and_then(|id| services.playlists.find(&id))
            .map(|doc| {
                let tracks = self.track_rows(&services, &doc.tracks, false);
                let pixels = cover_album(&doc).and_then(|album| art.get(album));
                // Each song's own album cover, beside it in the list.
                let covers: Vec<Option<Pixels>> =
                    doc.tracks.iter().map(|t| art.get(t.album.id)).collect();
                (doc, tracks, pixels, covers)
            });
        self.shared.lists.lock().unwrap().playlist = open
            .as_ref()
            .map(|(doc, _, _, _)| doc.tracks.clone())
            .unwrap_or_default();
        let mut wanted: Vec<i32> = docs.iter().filter_map(cover_album).collect();
        if let Some((doc, ..)) = &open {
            wanted.extend(
                doc.tracks
                    .iter()
                    .filter(|t| t.album.has_artwork)
                    .map(|t| t.album.id),
            );
        }
        wanted.sort_unstable();
        wanted.dedup();
        wanted.retain(|album| art.get(*album).is_none());

        self.ui(move |ui| {
            let app = ui.global::<App>();
            let items: Vec<PlaylistItem> = rows
                .into_iter()
                .map(|(doc, pixels)| playlist_item(&doc, false, pixels))
                .collect();
            view::with(|v| v.playlists.set_vec(items));
            match open {
                Some((doc, tracks, pixels, covers)) => {
                    app.set_playlist(playlist_item(&doc, true, pixels));
                    let covers: Vec<Image> = covers
                        .into_iter()
                        .map(|p| p.map(Image::from_rgb8).unwrap_or_default())
                        .collect();
                    view::with(|v| {
                        v.playlist_covers.set_vec(covers);
                        v.playlist_tracks.set_vec(tracks);
                        v.sync_source(ui);
                    });
                }
                None => {
                    app.set_playlist(PlaylistItem::default());
                    view::with(|v| {
                        v.playlist_covers.set_vec(Vec::new());
                        v.playlist_tracks.set_vec(Vec::new());
                        v.sync_source(ui);
                    });
                }
            }
        });
        for album in wanted {
            self.request_art(art, album);
        }
    }

    async fn sync_playlists_now(&self, services: &Arc<Services>) {
        match services.playlists.sync(&services.api).await {
            Ok(()) => self.refresh_playlists(),
            // Kept on the device and sent next time.
            Err(err) => tracing::debug!("playlists not synced: {err}"),
        }
    }

    fn sync_playlists(&self) {
        let Some(services) = self.services() else {
            return;
        };
        let ctx = self.clone();
        tokio::spawn(async move { ctx.sync_playlists_now(&services).await });
    }

    /// Changes playlists on the device at once, then tells the server.
    fn edit_playlists(&self, change: impl FnOnce(&Playlists)) {
        let Some(services) = self.services() else {
            return;
        };
        change(&services.playlists);
        self.refresh_playlists();
        self.sync_playlists();
    }

    fn create_playlist(&self, name: &str) {
        let Some(services) = self.services() else {
            return;
        };
        let Some(uuid) = services.playlists.create(name) else {
            return;
        };
        self.sync_playlists();
        self.open(Route::Playlist(uuid));
    }

    /// Named after what it was made from, as other music apps do. Stays on the
    /// current page: the point was to file these songs away, not to leave.
    fn new_playlist_with(&self, name: &str, tracks: Vec<PlayerTrack>) {
        self.edit_playlists(|playlists| {
            if let Some(uuid) = playlists.create(name) {
                playlists.add_tracks(&uuid, &tracks);
            }
        });
    }

    fn rename_playlist(&self, name: &str) {
        let Some(id) = self.open_playlist_id() else {
            return;
        };
        self.edit_playlists(|playlists| {
            playlists.rename(&id, name);
        });
    }

    fn delete_playlist(&self) {
        let Some(id) = self.open_playlist_id() else {
            return;
        };
        self.edit_playlists(|playlists| {
            playlists.delete(&id);
        });
        self.go_top(Route::Playlists);
    }

    fn track_action(&self, source: &str, index: usize, action: &str, arg: &str) {
        let track = self
            .shared
            .lists
            .lock()
            .unwrap()
            .get(source)
            .get(index)
            .cloned();
        let Some(track) = track else { return };
        let playlist = (source == "playlist")
            .then(|| self.open_playlist_id())
            .flatten();
        match (action, playlist) {
            ("play-next", _) => self.send(Command::PlayNext { track }),
            ("queue", _) => self.send(Command::Append { track }),
            ("add-to", _) => self.edit_playlists(|playlists| {
                playlists.add_tracks(arg, &[track]);
            }),
            ("new-playlist", _) => self.new_playlist_with(&track.title.clone(), vec![track]),
            ("download", _) => self.download_tracks(vec![track]),
            ("remove-download", _) => self.remove_download(track.id),
            ("album", _) => self.open(Route::Album(track.album.id)),
            ("artist", _) => self.open(Route::Artist(track.artist.id)),
            ("up", Some(id)) if index > 0 => self.edit_playlists(|playlists| {
                playlists.move_track(&id, index, index - 1);
            }),
            ("down", Some(id)) => self.edit_playlists(|playlists| {
                playlists.move_track(&id, index, index + 1);
            }),
            ("remove", Some(id)) => self.edit_playlists(|playlists| {
                playlists.remove_track(&id, index);
            }),
            _ => {}
        }
    }

    /// The album, artist or playlist page's own menu, for all of its songs.
    fn list_action(&self, action: &str, source: &str, name: String) {
        let tracks = self.shared.lists.lock().unwrap().get(source).to_vec();
        if tracks.is_empty() {
            return;
        }
        if action == "queue" {
            for track in tracks {
                self.send(Command::Append { track });
            }
        } else if action == "download" {
            self.download_tracks(tracks);
        } else if action == "new-playlist" {
            self.new_playlist_with(&name, tracks);
        } else if let Some(uuid) = action.strip_prefix("add-to:") {
            self.edit_playlists(|playlists| {
                playlists.add_tracks(uuid, &tracks);
            });
        }
    }

    // ── Downloads ───────────────────────────────────────────────────────────

    fn refresh_downloads(&self) {
        let Some(services) = self.services() else {
            return;
        };
        let saved = services.downloads.list();
        let tracks: Vec<PlayerTrack> = saved.iter().map(|d| d.track.clone()).collect();
        let size: u64 = saved.iter().map(|d| d.bytes).sum();
        let detail = format!(
            "{} · {}",
            plural(tracks.len(), "song", "songs"),
            bytes(size)
        );
        let rows = self.track_rows(&services, &tracks, false);
        self.shared.lists.lock().unwrap().downloads = tracks;
        self.ui(move |ui| {
            ui.global::<App>().set_downloads_detail(detail.into());
            view::with(|v| {
                v.downloads.set_vec(rows);
                v.sync_source(ui);
            });
        });
    }

    fn show_progress(&self) {
        let status = self.shared.progress.lock().unwrap().status();
        self.ui(move |ui| ui.global::<App>().set_download_status(status.into()));
    }

    fn download_tracks(&self, tracks: Vec<PlayerTrack>) {
        let Some(services) = self.services() else {
            return;
        };
        let wanted: Vec<PlayerTrack> = tracks
            .into_iter()
            .filter(|t| !services.downloads.is_downloaded(t.id))
            .collect();
        if wanted.is_empty() {
            return;
        }
        {
            let mut progress = self.shared.progress.lock().unwrap();
            // A finished batch's failures are old news once a new one starts.
            if progress.done + progress.failed >= progress.total {
                *progress = Progress::default();
            }
            progress.total += wanted.len();
        }
        self.show_progress();

        for track in wanted {
            let (ctx, services) = (self.clone(), services.clone());
            let slots = self.shared.download_slots.clone();
            tokio::spawn(async move {
                let _slot = slots.acquire().await;
                let result = services.downloads.download(&services.api, &track).await;
                let finished = {
                    let mut progress = ctx.shared.progress.lock().unwrap();
                    match &result {
                        Ok(()) => progress.done += 1,
                        Err(err) => {
                            tracing::warn!("could not download {}: {err}", track.title);
                            progress.failed += 1;
                        }
                    }
                    progress.done + progress.failed >= progress.total
                };
                if result.is_ok() {
                    let id = track.id;
                    ctx.ui(move |_| view::with(|v| v.mark_offline(id, true)));
                }
                ctx.show_progress();
                if finished && ctx.shared.routes.lock().unwrap().last() == Some(&Route::Downloads) {
                    ctx.refresh_downloads();
                }
            });
        }
    }

    fn remove_download(&self, track_id: i32) {
        let Some(services) = self.services() else {
            return;
        };
        services.downloads.remove(track_id);
        self.ui(move |_| view::with(|v| v.mark_offline(track_id, false)));
        self.refresh_downloads();
    }

    fn clear_downloads(&self) {
        let Some(services) = self.services() else {
            return;
        };
        let ids: Vec<i32> = services
            .downloads
            .list()
            .iter()
            .map(|d| d.track.id)
            .collect();
        services.downloads.clear();
        self.ui(move |_| {
            view::with(|v| {
                for id in ids {
                    v.mark_offline(id, false);
                }
            });
        });
        self.refresh_downloads();
    }

    // ── Wiring ──────────────────────────────────────────────────────────────

    pub fn wire(&self, ui: &AppWindow) {
        let app = ui.global::<App>();
        let ctx = self.clone();
        app.on_navigate(move |page| match page.as_str() {
            "albums" => ctx.go_top(Route::Albums),
            "artists" => ctx.go_top(Route::Artists),
            "search" => ctx.go_top(Route::Search),
            "playlists" => ctx.go_top(Route::Playlists),
            "downloads" => ctx.go_top(Route::Downloads),
            "album-artist" => {
                let artist = *ctx.shared.album_artist.lock().unwrap();
                if let Some(id) = artist {
                    ctx.open(Route::Artist(id));
                }
            }
            "setup" => ctx.ui(|ui| ui.global::<App>().set_page("setup".into())),
            _ => {}
        });
        let ctx = self.clone();
        app.on_back(move || ctx.back());
        let ctx = self.clone();
        app.on_open_album(move |id| ctx.open(Route::Album(id)));
        let ctx = self.clone();
        app.on_open_artist(move |id| ctx.open(Route::Artist(id)));
        let ctx = self.clone();
        app.on_play_list(move |source, index| ctx.play_list(&source, index.max(0) as usize));
        let ctx = self.clone();
        app.on_shuffle_list(move |source| ctx.shuffle_list(&source));
        let ctx = self.clone();
        app.on_search(move |text| ctx.search(text.to_string()));
        let ctx = self.clone();
        app.on_connect(move |url| ctx.connect(url.to_string()));
        let ctx = self.clone();
        app.on_sign_in(move || ctx.sign_in());
        let ctx = self.clone();
        app.on_cancel_sign_in(move || ctx.cancel_sign_in());
        let ctx = self.clone();
        app.on_sign_out(move || ctx.sign_out());
        let ctx = self.clone();
        app.on_install_update(move || ctx.install_update());
        app.set_version(crate::update::VERSION.into());
        let ctx = self.clone();
        app.on_retry(move || ctx.retry());
        let ctx = self.clone();
        app.on_track_action(move |source, index, action, arg| {
            ctx.track_action(&source, index.max(0) as usize, &action, &arg);
        });
        let ctx = self.clone();
        let weak = ui.as_weak();
        app.on_album_action(move |action, source| {
            // A playlist made from a whole page is named after it.
            let name = weak.upgrade().map_or_else(String::new, |ui| {
                let app = ui.global::<App>();
                match source.as_str() {
                    "album" => app.get_album().title.to_string(),
                    "artist" => app.get_artist_name().to_string(),
                    "playlist" => format!("{} (copy)", app.get_playlist().name),
                    _ => String::new(),
                }
            });
            ctx.list_action(&action, &source, name);
        });
        let ctx = self.clone();
        app.on_open_playlist(move |id| ctx.open(Route::Playlist(id.to_string())));
        let ctx = self.clone();
        app.on_create_playlist(move |name| ctx.create_playlist(&name));
        let ctx = self.clone();
        app.on_rename_playlist(move |name| ctx.rename_playlist(&name));
        let ctx = self.clone();
        app.on_delete_playlist(move || ctx.delete_playlist());
        let ctx = self.clone();
        app.on_clear_downloads(move || ctx.clear_downloads());

        let playback = ui.global::<Playback>();
        let ctx = self.clone();
        playback.on_toggle(move || ctx.send(Command::Toggle));
        let ctx = self.clone();
        playback.on_next(move || ctx.send(Command::Next));
        let ctx = self.clone();
        playback.on_prev(move || ctx.send(Command::Prev));
        let ctx = self.clone();
        let weak = ui.as_weak();
        playback.on_seek(move |fraction| {
            let duration = weak
                .upgrade()
                .map_or(0.0, |ui| ui.global::<Playback>().get_duration());
            ctx.send(Command::Seek {
                time: f64::from(fraction * duration),
            });
        });
        let ctx = self.clone();
        playback.on_set_volume(move |volume| ctx.send(Command::SetVolume { volume }));
        let ctx = self.clone();
        playback.on_toggle_shuffle(move || ctx.send(Command::ToggleShuffle));
        let ctx = self.clone();
        playback.on_cycle_repeat(move || ctx.send(Command::CycleRepeat));
        let ctx = self.clone();
        playback.on_jump_to(move |index| {
            ctx.send(Command::JumpTo {
                index: index.max(0) as usize,
            })
        });
        let ctx = self.clone();
        playback.on_remove(move |index| {
            ctx.send(Command::Remove {
                index: index.max(0) as usize,
            })
        });
        let ctx = self.clone();
        playback.on_claim(move || {
            if let Some(services) = ctx.services() {
                services.hub.claim();
            }
        });
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AlbumList {
    Library,
    Artist,
    Search,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_bare_hosts_and_refuses_junk() {
        assert_eq!(
            normalise_server("music.example.com/").as_deref(),
            Some("https://music.example.com")
        );
        assert_eq!(
            normalise_server(" http://pi.local:3006 ").as_deref(),
            Some("http://pi.local:3006")
        );
        assert_eq!(normalise_server(""), None);
        assert_eq!(normalise_server("ftp://x"), None);
        assert_eq!(normalise_server("not a url at all"), None);
    }
}
