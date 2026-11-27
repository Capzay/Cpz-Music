//! `cargo android`: builds the Android app, no Gradle.
//!
//!   cargo android            signed release APK for phones (arm64) → dist/
//!   cargo android install    that, then installed and started over adb
//!   cargo android emulator   debug APK for an x86_64 emulator, installed and started
//!
//! The steps, which are all Gradle ever did here:
//! 1. cargo builds crates/app as libcpz_app.so, linked with the NDK's clang.
//! 2. aapt2 compiles android/res and links it with android/AndroidManifest.xml.
//! 3. javac and d8 turn android/java (and aapt2's R.java) into classes.dex.
//! 4. Those go in one zip, which zipalign lines up and apksigner signs.
//!
//! Needs the Android SDK with build-tools, a platform and the NDK (ANDROID_HOME,
//! default ~/Android/Sdk), a JDK (JAVA_HOME, default JDK 21), and the rustup
//! target. Release builds are signed with ~/.config/cpz-music/android-release.jks,
//! made on first use: back it up, phones only accept updates signed with it.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

type Result<T = ()> = std::result::Result<T, String>;

const APP_ID: &str = "uk.capzay.music";
const MIN_SDK: u32 = 29;

struct Target {
    triple: &'static str,
    abi: &'static str,
    release: bool,
}

const PHONE: Target = Target {
    triple: "aarch64-linux-android",
    abi: "arm64-v8a",
    release: true,
};
const EMULATOR: Target = Target {
    triple: "x86_64-linux-android",
    abi: "x86_64",
    release: false,
};

fn main() {
    let command = std::env::args().nth(1).unwrap_or_default();
    let result = match command.as_str() {
        "" | "build" => build(&PHONE).map(|_| ()),
        "install" => build(&PHONE).and_then(|apk| install(&apk)),
        "emulator" => build(&EMULATOR).and_then(|apk| install(&apk)),
        _ => Err("usage: cargo android [build|install|emulator]".into()),
    };
    if let Err(err) = result {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn build(target: &Target) -> Result<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(text)?;
    let sdk = Sdk::find()?;
    let version = app_version(&root)?;
    let profile = if target.release { "release" } else { "debug" };
    let work = root
        .join("target/android")
        .join(format!("{}-{profile}", target.abi));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(text)?;

    // 1. The Rust app.
    eprintln!("building cpz-app for {}", target.triple);
    let upper = target.triple.to_uppercase().replace('-', "_");
    let lower = target.triple.replace('-', "_");
    let clang = sdk
        .ndk_bin
        .join(format!("{}{MIN_SDK}-clang", target.triple));
    let mut cargo = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cargo
        .current_dir(&root)
        .args([
            "rustc",
            "-p",
            "cpz-app",
            "--lib",
            "--crate-type",
            "cdylib",
            "--target",
            target.triple,
        ])
        .env(format!("CARGO_TARGET_{upper}_LINKER"), &clang)
        // For the C in ring and SQLite.
        .env(format!("CC_{lower}"), &clang)
        .env(format!("AR_{lower}"), sdk.ndk_bin.join("llvm-ar"))
        // Slint's build script compiles a small Java helper.
        .env("ANDROID_HOME", &sdk.home)
        .env("ANDROID_JAR", &sdk.android_jar)
        .env("JAVA_HOME", &sdk.java_home);
    if target.release {
        cargo.arg("--release");
    }
    run(&mut cargo)?;
    let library = root
        .join("target")
        .join(target.triple)
        .join(profile)
        .join("libcpz_app.so");

    // 2. Resources and manifest.
    let android = root.join("android");
    let resources = work.join("resources.zip");
    run(Command::new(sdk.tool("aapt2"))
        .args(["compile", "--dir"])
        .arg(android.join("res"))
        .arg("-o")
        .arg(&resources))?;
    let base = work.join("base.apk");
    let generated = work.join("gen");
    run(Command::new(sdk.tool("aapt2"))
        .arg("link")
        .arg("-o")
        .arg(&base)
        .arg("-I")
        .arg(&sdk.android_jar)
        .arg("--manifest")
        .arg(android.join("AndroidManifest.xml"))
        .arg("--java")
        .arg(&generated)
        .args(["--min-sdk-version", &MIN_SDK.to_string()])
        .args(["--target-sdk-version", &sdk.platform.to_string()])
        .args(["--version-name", &version])
        .args(["--version-code", &version_code(&version)?.to_string()])
        .arg(&resources))?;

    // 3. Java.
    let classes = work.join("classes");
    let mut sources = java_files(&android.join("java"))?;
    sources.extend(java_files(&generated)?);
    run(Command::new(sdk.java_home.join("bin/javac"))
        .args(["--release", "11", "-nowarn", "-classpath"])
        .arg(&sdk.android_jar)
        .arg("-d")
        .arg(&classes)
        .args(&sources))?;
    let dex = work.join("dex");
    fs::create_dir_all(&dex).map_err(text)?;
    run(sdk
        .java_command(sdk.tool("d8"))
        .args(["--release", "--min-api", &MIN_SDK.to_string(), "--lib"])
        .arg(&sdk.android_jar)
        .arg("--output")
        .arg(&dex)
        .args(class_files(&classes)?))?;

    // 4. One zip, aligned and signed.
    let unaligned = work.join("unaligned.apk");
    package(
        &base,
        &dex.join("classes.dex"),
        &library,
        target.abi,
        &unaligned,
    )?;
    let aligned = work.join("aligned.apk");
    // -P 16: native libraries on 16 KB pages, so they load straight from the APK.
    run(Command::new(sdk.tool("zipalign"))
        .args(["-P", "16", "-f", "4"])
        .arg(&unaligned)
        .arg(&aligned))?;

    let dist = root.join("dist");
    fs::create_dir_all(&dist).map_err(text)?;
    let apk = if target.release {
        dist.join(format!("cpz-music-{version}.apk"))
    } else {
        dist.join(format!("cpz-music-{version}-{}-debug.apk", target.abi))
    };
    let key = if target.release {
        release_key(&sdk)?
    } else {
        debug_key()?
    };
    run(sdk
        .java_command(sdk.tool("apksigner"))
        .arg("sign")
        .arg("--ks")
        .arg(&key.store)
        .args(["--ks-key-alias", key.alias])
        .arg("--ks-pass")
        .arg(&key.password)
        .arg("--out")
        .arg(&apk)
        .arg(&aligned))?;
    let _ = fs::remove_file(apk.with_extension("apk.idsig"));
    eprintln!("{}", apk.display());
    Ok(apk)
}

/// aapt2's APK (manifest and resources) plus the code: the dex compressed, the
/// library stored so Android can map it without unpacking.
fn package(base: &Path, dex: &Path, library: &Path, abi: &str, out: &Path) -> Result {
    let mut input = ZipArchive::new(File::open(base).map_err(text)?).map_err(text)?;
    let mut output = ZipWriter::new(File::create(out).map_err(text)?);
    for i in 0..input.len() {
        output
            .raw_copy_file(input.by_index(i).map_err(text)?)
            .map_err(text)?;
    }
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    output.start_file("classes.dex", deflated).map_err(text)?;
    io::copy(&mut File::open(dex).map_err(text)?, &mut output).map_err(text)?;
    let stored = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(fs::metadata(library).map_err(text)?.len() >= u32::MAX as u64);
    output
        .start_file(format!("lib/{abi}/libcpz_app.so"), stored)
        .map_err(text)?;
    io::copy(&mut File::open(library).map_err(text)?, &mut output).map_err(text)?;
    output.finish().map_err(text)?;
    Ok(())
}

fn install(apk: &Path) -> Result {
    run(Command::new("adb").args(["install", "-r"]).arg(apk))?;
    run(Command::new("adb").args([
        "shell",
        "am",
        "start",
        "-n",
        &format!("{APP_ID}/.MainActivity"),
    ]))
}

// ── The SDK ─────────────────────────────────────────────────────────────────

struct Sdk {
    home: PathBuf,
    build_tools: PathBuf,
    android_jar: PathBuf,
    platform: u32,
    ndk_bin: PathBuf,
    java_home: PathBuf,
}

impl Sdk {
    fn find() -> Result<Self> {
        let home = std::env::var_os("ANDROID_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join("Android/Sdk"));
        let build_tools = newest(&home.join("build-tools"))?;
        let platform_dir = newest(&home.join("platforms"))?;
        let platform = numbers(&platform_dir)[0];
        let ndk_bin = newest(&home.join("ndk"))?.join("toolchains/llvm/prebuilt/linux-x86_64/bin");
        let java_home = std::env::var_os("JAVA_HOME")
            .map(PathBuf::from)
            .or_else(|| Some(PathBuf::from("/usr/lib/jvm/java-21-openjdk")).filter(|p| p.exists()))
            .unwrap_or_else(|| PathBuf::from("/usr/lib/jvm/default"));
        Ok(Self {
            android_jar: platform_dir.join("android.jar"),
            home,
            build_tools,
            platform,
            ndk_bin,
            java_home,
        })
    }

    fn tool(&self, name: &str) -> PathBuf {
        self.build_tools.join(name)
    }

    /// d8 and apksigner are scripts that run whatever `java` is on PATH.
    fn java_command(&self, program: PathBuf) -> Command {
        let path = std::env::var("PATH").unwrap_or_default();
        let mut command = Command::new(program);
        command.env(
            "PATH",
            format!("{}:{path}", self.java_home.join("bin").display()),
        );
        command
    }
}

/// The entry in `dir` with the highest version number, such as build-tools/36.0.0.
fn newest(dir: &Path) -> Result<PathBuf> {
    fs::read_dir(dir)
        .map_err(|err| format!("{}: {err}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| !numbers(path).is_empty())
        .max_by_key(|path| numbers(path))
        .ok_or_else(|| format!("nothing installed in {}", dir.display()))
}

fn numbers(path: &Path) -> Vec<u32> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    name.split(|c: char| !c.is_ascii_digit())
        .filter_map(|part| part.parse().ok())
        .collect()
}

// ── Signing ─────────────────────────────────────────────────────────────────

struct Key {
    store: PathBuf,
    alias: &'static str,
    /// In apksigner's form, such as `file:/path` or `pass:secret`.
    password: String,
}

fn release_key(sdk: &Sdk) -> Result<Key> {
    let dir = home_dir().join(".config/cpz-music");
    let store = dir.join("android-release.jks");
    let password = dir.join("android-release.password");
    if !store.exists() {
        fs::create_dir_all(&dir).map_err(text)?;
        let mut bytes = [0u8; 18];
        File::open("/dev/urandom")
            .and_then(|mut random| io::Read::read_exact(&mut random, &mut bytes))
            .map_err(text)?;
        let secret: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        fs::write(&password, format!("{secret}\n")).map_err(text)?;
        run(Command::new(sdk.java_home.join("bin/keytool"))
            .args(["-genkeypair", "-keystore"])
            .arg(&store)
            .args([
                "-alias",
                "cpz",
                "-keyalg",
                "RSA",
                "-keysize",
                "4096",
                "-validity",
                "36500",
            ])
            .args(["-dname", "CN=Cpz Music"])
            .arg("-storepass:file")
            .arg(&password)
            .arg("-keypass:file")
            .arg(&password))?;
        eprintln!(
            "made a signing key: {} (back it up with its password file)",
            store.display()
        );
    }
    // Older copies held `storePassword=…`; apksigner wants the bare password.
    let contents = fs::read_to_string(&password).map_err(text)?;
    if let Some(bare) = contents.strip_prefix("storePassword=") {
        fs::write(&password, bare).map_err(text)?;
    }
    Ok(Key {
        store,
        alias: "cpz",
        password: format!("file:{}", password.display()),
    })
}

/// The SDK's shared debug key, so debug builds install over each other.
fn debug_key() -> Result<Key> {
    let store = [".config/.android/debug.keystore", ".android/debug.keystore"]
        .iter()
        .map(|path| home_dir().join(path))
        .find(|path| path.exists())
        .ok_or("no debug.keystore in ~/.config/.android or ~/.android")?;
    Ok(Key {
        store,
        alias: "androiddebugkey",
        password: "pass:android".into(),
    })
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn app_version(root: &Path) -> Result<String> {
    let manifest = fs::read_to_string(root.join("crates/app/Cargo.toml")).map_err(text)?;
    manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .map(str::to_owned)
        .ok_or_else(|| "no version in crates/app/Cargo.toml".into())
}

/// 1.2.3 → 1002003, so every release counts higher than the last.
fn version_code(version: &str) -> Result<u32> {
    let parts: Vec<u32> = version
        .split('.')
        .map(|part| {
            part.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
        })
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| format!("version {version} is not x.y.z"))?;
    match parts[..] {
        [major, minor, patch] => Ok(major * 1_000_000 + minor * 1_000 + patch),
        _ => Err(format!("version {version} is not x.y.z")),
    }
}

fn java_files(dir: &Path) -> Result<Vec<PathBuf>> {
    files_with(dir, "java")
}

fn class_files(dir: &Path) -> Result<Vec<PathBuf>> {
    files_with(dir, "class")
}

fn files_with(dir: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).map_err(|err| format!("{}: {err}", dir.display()))? {
        let path = entry.map_err(text)?.path();
        if path.is_dir() {
            found.extend(files_with(&path, extension)?);
        } else if path.extension().is_some_and(|ext| ext == extension) {
            found.push(path);
        }
    }
    Ok(found)
}

fn run(command: &mut Command) -> Result {
    let status = command
        .status()
        .map_err(|err| format!("could not run {:?}: {err}", command.get_program()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{:?} failed ({status})", command.get_program()))
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn text(err: impl std::fmt::Display) -> String {
    err.to_string()
}
