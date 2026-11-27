//! Android start-up and the bridge to the Java side (`android/java`).
//!
//! Rust calls instance methods on the running `MainActivity`, which avoids
//! looking up app classes from native threads (their class loader cannot see
//! them). Java calls back through the `native` methods at the bottom.
//!
//! Gotcha: `ndk_context` holds the Application, not the activity, so the
//! activity comes from `AndroidApp::activity_as_ptr` instead.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, Ordering};

use cpz_core::player::Command;
use jni::objects::{JClass, JObject, JString};
use jni::{Env, EnvUnowned, JValue, JavaVM, jni_sig, jni_str};
use tokio::sync::{mpsc, oneshot};

/// Where Supabase sends the browser after GitHub. The activity's intent
/// filter catches it; it must also be listed in Supabase's Redirect URLs.
pub const REDIRECT: &str = "uk.capzay.music://auth/callback";

static REDIRECT_WAITER: Mutex<Option<oneshot::Sender<String>>> = Mutex::new(None);
/// A global reference to the running `MainActivity`, from android-activity.
static ACTIVITY: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
static COMMANDS: Mutex<Option<mpsc::UnboundedSender<Command>>> = Mutex::new(None);

#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info,cpz_core=info"))
        .with(paranoid_android::layer("cpz-music"))
        .init();
    std::panic::set_hook(Box::new(|info| tracing::error!("panic: {info}")));

    // `internal_data_path` is the app's files/ folder; cache/ sits beside it
    // and is what Android clears when it runs short of space.
    let data = app
        .internal_data_path()
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp/cpz-music"));
    let cache = data
        .parent()
        .map(|parent| parent.join("cache"))
        .unwrap_or_else(|| data.join("cache"));

    ACTIVITY.store(app.activity_as_ptr(), Ordering::Release);
    if let Err(err) = slint::android::init(app) {
        tracing::error!("could not start Slint: {err}");
        return;
    }
    if let Err(err) = crate::run(crate::Paths { data, cache }) {
        tracing::error!("the app stopped: {err:#}");
    }
    now_playing("", "", false);
    ACTIVITY.store(std::ptr::null_mut(), Ordering::Release);
}

/// Runs `call` with the activity, on whatever thread this is.
fn with_activity<T>(call: impl FnOnce(&mut Env, &JObject) -> jni::errors::Result<T>) -> Option<T> {
    let raw = ACTIVITY.load(Ordering::Acquire);
    if raw.is_null() {
        return None;
    }
    // SAFETY: android-activity fills in the process's VM before `android_main`
    // runs, and the activity reference stays valid while it runs.
    let vm = unsafe { JavaVM::from_raw(ndk_context::android_context().vm().cast()) };
    let result = vm.attach_current_thread(|env| {
        let activity = unsafe { JObject::from_raw(env, raw.cast()) };
        // `JObject` does not delete the reference on drop, so the global one
        // stays with its owner.
        let result = call(env, &activity);
        if env.exception_check() {
            env.exception_describe();
            env.exception_clear();
        }
        result
    });
    result
        .inspect_err(|err| tracing::warn!("call into Java failed: {err}"))
        .ok()
}

/// Keeps the playback notification (and with it the foreground service that
/// lets music play with the screen off) in step with the player. An empty
/// title removes it.
pub fn now_playing(title: &str, artist: &str, playing: bool) {
    with_activity(|env, activity| {
        let title = env.new_string(title)?;
        let artist = env.new_string(artist)?;
        env.call_method(
            activity,
            jni_str!("nowPlaying"),
            jni_sig!("(Ljava/lang/String;Ljava/lang/String;Z)V"),
            &[
                JValue::Object(&title),
                JValue::Object(&artist),
                JValue::Bool(playing),
            ],
        )?;
        Ok(())
    });
}

/// Sends the app to the background, as Back does on the first screen of
/// other music apps. Playback carries on.
pub fn move_to_back() {
    with_activity(|env, activity| {
        env.call_method(activity, jni_str!("moveToBack"), jni_sig!("()V"), &[])?;
        Ok(())
    });
}

/// The phone's model, such as "Pixel 9", for the device list.
pub fn device_name() -> Option<String> {
    with_activity(|env, activity| {
        let name = env.call_method(
            activity,
            jni_str!("deviceName"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?;
        let name = JString::cast_local(env, name.l()?)?;
        name.try_to_string(env)
    })
    .filter(|name| !name.trim().is_empty())
}

/// Hands a downloaded APK to Android's package installer, which asks the
/// user and then replaces the running app.
pub fn install_apk(path: &Path) -> anyhow::Result<()> {
    let path = path.to_string_lossy().into_owned();
    with_activity(|env, activity| {
        let path = env.new_string(&path)?;
        env.call_method(
            activity,
            jni_str!("installApk"),
            jni_sig!("(Ljava/lang/String;)Z"),
            &[JValue::Object(&path)],
        )?
        .z()
    })
    .filter(|started| *started)
    .map(|_| ())
    .ok_or_else(|| anyhow::anyhow!("Android would not start the installer"))
}

/// The next sign-in redirect the activity receives. Replaces any earlier wait.
pub fn next_redirect() -> oneshot::Receiver<String> {
    let (tx, rx) = oneshot::channel();
    *REDIRECT_WAITER.lock().unwrap() = Some(tx);
    rx
}

/// Commands from the notification, the lock screen and headset buttons.
pub fn commands() -> mpsc::UnboundedReceiver<Command> {
    let (tx, rx) = mpsc::unbounded_channel();
    *COMMANDS.lock().unwrap() = Some(tx);
    rx
}

fn string_arg(env: &mut EnvUnowned, value: &JString) -> Option<String> {
    env.with_env(|env| value.try_to_string(env))
        .resolve::<jni::errors::LogErrorAndDefault>()
        .into()
}

#[unsafe(no_mangle)]
extern "system" fn Java_uk_capzay_music_MainActivity_nativeCommand<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    command: JString<'caller>,
) {
    let Some(command) = string_arg(&mut env, &command) else {
        return;
    };
    let command = match command.as_str() {
        "play" => Command::Play { track: None },
        "pause" => Command::Pause,
        "toggle" => Command::Toggle,
        "next" => Command::Next,
        "prev" => Command::Prev,
        other => {
            tracing::warn!("unknown command from Java: {other}");
            return;
        }
    };
    if let Some(tx) = COMMANDS.lock().unwrap().as_ref() {
        let _ = tx.send(command);
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_uk_capzay_music_MainActivity_nativeDeepLink<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    url: JString<'caller>,
) {
    let Some(url) = string_arg(&mut env, &url) else {
        return;
    };
    if let Some(waiter) = REDIRECT_WAITER.lock().unwrap().take() {
        let _ = waiter.send(url);
    }
}
