//! Connects the screens to cpz-core: what each button does, and getting the
//! answers back onto the screen.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpz_core::api::{Api, ApiError, fetch_config};
use cpz_core::audio::Output;
use cpz_core::auth::{
    Credentials, LOOPBACK_PORT, Pkce, Session, SupabaseAuth, authorize_url, loopback_redirect,
    receive_code,
};
use cpz_core::downloads::Downloads;
use cpz_core::listens::Listens;
use cpz_core::player::{Command, Player, PlayerDeps};
use cpz_core::realtime::{Channel, ChannelConfig, Hub, Identity, PLAYER_TOPIC};
use cpz_core::store::Store;
use cpz_core::types::{AlbumSummary, ArtistCard, ClientConfig, PlayerTrack};
use slint::{ComponentHandle, Weak};

use crate::art::Art;
use crate::view::{self, album_item, artist_hit, artist_item, length, plural, track_item};
use crate::{AlbumItem, App, AppWindow, Playback};

const SERVER_KEY: &str = "server";
const CONFIG_KEY: &str = "client-config";
const SESSION_KEY: &str = "session";
const SEARCH_DELAY: Duration = Duration::from_millis(250);
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(600);

pub struct Paths {
    pub data: PathBuf,
    pub cache: PathBuf,
}

/// Everything that needs a signed-in session.
pub struct Services {
    pub api: Api,
    pub hub: Hub,
    pub downloads: Arc<Downloads>,
    pub art: Arc<Art>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    Albums,
    Artists,
    Album(i32),
    Artist(i32),
    Search,
}

impl Route {
    fn page(self) -> &'static str {
        match self {
            Self::Albums => "albums",
            Self::Artists => "artists",
            Self::Album(_) => "album",
            Self::Artist(_) => "artist",
            Self::Search => "search",
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
}

impl Lists {
    fn get(&self, source: &str) -> &[PlayerTrack] {
        match source {
            "album" => &self.album,
            "artist" => &self.artist,
            "search" => &self.search,
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

        let services = Arc::new(Services {
            api: api.clone(),
            hub,
            downloads,
            art,
        });
        *self.shared.services.lock().unwrap() = Some(services.clone());
        tokio::spawn(async move { listens.flush(&api).await });

        self.watch_hub(&services);
        let ctx = self.clone();
        self.ui(move |_| ctx.go_top(Route::Albums));
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
                            |album| ctx2.request_art(&art, album),
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
        *self.shared.routes.lock().unwrap() = vec![route];
        self.show(route);
    }

    fn open(&self, route: Route) {
        let mut routes = self.shared.routes.lock().unwrap();
        if routes.last() != Some(&route) {
            routes.push(route);
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
            routes.last().copied()
        };
        if let Some(route) = route {
            self.show(route);
        }
    }

    fn show(&self, route: Route) {
        let generation = self.shared.page_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let can_go_back = self.shared.routes.lock().unwrap().len() > 1;
        self.ui(move |ui| {
            let app = ui.global::<App>();
            app.set_page(route.page().into());
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
                    view::with(|v| v.album_tracks.set_vec(rows));
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
                    view::with(|v| v.artist_tracks.set_vec(rows));
                    app.set_busy(false);
                });
                self.show_albums(&services, artist.albums, generation, AlbumList::Artist);
            }
            // Results come from typing, not from opening the page.
            Route::Search => {}
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
        if let Some(route) = self.shared.routes.lock().unwrap().last().copied() {
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

    // ── Wiring ──────────────────────────────────────────────────────────────

    pub fn wire(&self, ui: &AppWindow) {
        let app = ui.global::<App>();
        let ctx = self.clone();
        app.on_navigate(move |page| match page.as_str() {
            "albums" => ctx.go_top(Route::Albums),
            "artists" => ctx.go_top(Route::Artists),
            "search" => ctx.go_top(Route::Search),
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
        app.on_retry(move || ctx.retry());

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
