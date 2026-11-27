//! Installing and uninstalling the desktop app. The app is its own installer:
//! `--install` copies the running executable to where it belongs and adds it
//! to the app menu, `--uninstall` takes that away again. Installed copies sit
//! in a folder the user owns, so self-updates (see `update.rs`) keep working
//! without admin rights.
//!
//! Linux: ~/.local/bin/cpz-music, plus a menu entry and an icon in
//! ~/.local/share. The setup script from `scripts/package-client.sh` unpacks
//! the binary and runs `--install`.
//!
//! Windows: %LOCALAPPDATA%\Programs\Cpz Music, a Start menu shortcut and an
//! entry in Installed apps, whose Uninstall button runs `--uninstall`. The
//! setup .exe is this same program under a name with "setup" in it, which is
//! what makes it install instead of opening.
//!
//! Downloads, settings and the sign-in are kept on uninstall.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Also the Wayland app id, which is how the menu entry finds its window.
pub const APP_ID: &str = "uk.capzay.music";
const NAME: &str = "Cpz Music";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Install,
    Uninstall,
}

/// What the command line, or on Windows the file's own name, asks for.
pub fn requested() -> Option<Action> {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("--install") => return Some(Action::Install),
        Some("--uninstall") => return Some(Action::Uninstall),
        _ => {}
    }
    let exe = std::env::current_exe().ok()?;
    let stem = exe.file_stem()?.to_string_lossy().to_lowercase();
    (cfg!(target_os = "windows") && stem.contains("setup")).then_some(Action::Install)
}

/// Copies `from` to `to` through a temporary file beside it, so a running
/// copy of the old version is never half overwritten.
fn place(from: &Path, to: &Path) -> Result<()> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
    }
    let partial = to.with_extension("new");
    std::fs::copy(from, &partial)
        .with_context(|| format!("could not copy to {}", partial.display()))?;
    #[cfg(target_os = "windows")]
    if to.exists() {
        // Windows will not overwrite a running .exe, but it will rename one.
        let old = to.with_extension("old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(to, &old).context("could not move the old version aside")?;
    }
    std::fs::rename(&partial, to).with_context(|| format!("could not write {}", to.display()))?;
    Ok(())
}

fn write(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, contents).with_context(|| format!("could not write {}", path.display()))
}

// ── Linux ───────────────────────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
struct Places {
    bin: PathBuf,
    desktop: PathBuf,
    icon: PathBuf,
}

#[cfg(not(target_os = "windows"))]
fn places() -> Result<Places> {
    let home = dirs::home_dir().context("no home folder")?;
    let bin = std::env::var_os("XDG_BIN_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/bin"));
    let data = dirs::data_dir().unwrap_or_else(|| home.join(".local/share"));
    Ok(Places {
        bin: bin.join("cpz-music"),
        desktop: data.join(format!("applications/{APP_ID}.desktop")),
        icon: data.join(format!("icons/hicolor/scalable/apps/{APP_ID}.svg")),
    })
}

#[cfg(not(target_os = "windows"))]
fn desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={NAME}\n\
         Comment=Play your music library\n\
         Exec=\"{}\"\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=AudioVideo;Audio;Player;\n\
         StartupWMClass={APP_ID}\n",
        exe.display()
    )
}

#[cfg(not(target_os = "windows"))]
pub fn run(action: Action) -> Result<()> {
    let places = places()?;
    match action {
        Action::Install => {
            let me = std::env::current_exe()?;
            if me != places.bin {
                place(&me, &places.bin)?;
            }
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&places.bin, std::fs::Permissions::from_mode(0o755))?;
            write(&places.icon, include_bytes!("../../../packaging/icon.svg"))?;
            write(&places.desktop, desktop_entry(&places.bin).as_bytes())?;
            println!(
                "Installed {NAME} {} to {}.\nOpen it from your app launcher. It updates itself from your server.",
                crate::update::VERSION,
                places.bin.display()
            );
            let on_path = std::env::var_os("PATH").is_some_and(|path| {
                std::env::split_paths(&path).any(|dir| Some(dir.as_path()) == places.bin.parent())
            });
            if !on_path {
                println!(
                    "To run it from a terminal too, add {} to your PATH.",
                    places.bin.parent().unwrap_or(Path::new("")).display()
                );
            }
        }
        Action::Uninstall => {
            for path in [&places.bin, &places.desktop, &places.icon] {
                match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => {
                        return Err(err)
                            .with_context(|| format!("could not remove {}", path.display()));
                    }
                }
            }
            let data = dirs::data_dir().map(|dir| dir.join("cpz-music"));
            println!(
                "Removed {NAME}. Your downloads and settings are still in {}; delete that folder to remove them too.",
                data.map_or_else(
                    || "~/.local/share/cpz-music".into(),
                    |d| d.display().to_string()
                )
            );
        }
    }
    Ok(())
}

// ── Windows ─────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\CpzMusic";

#[cfg(target_os = "windows")]
fn install_dir() -> Result<PathBuf> {
    Ok(dirs::data_local_dir()
        .context("no local app data folder")?
        .join("Programs")
        .join(NAME))
}

#[cfg(target_os = "windows")]
fn shortcut() -> Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no app data folder")?
        .join(r"Microsoft\Windows\Start Menu\Programs")
        .join(format!("{NAME}.lnk")))
}

#[cfg(target_os = "windows")]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// A plain Windows message box. Returns whether Yes or OK was pressed.
#[cfg(target_os = "windows")]
fn ask(text: &str, buttons: u32) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{IDOK, IDYES, MessageBoxW};
    let (text, title) = (wide(text), wide(NAME));
    let answer =
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), buttons) };
    answer == IDYES || answer == IDOK
}

#[cfg(target_os = "windows")]
fn tell(text: &str, error: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_ICONINFORMATION, MB_OK};
    ask(
        text,
        MB_OK
            | if error {
                MB_ICONERROR
            } else {
                MB_ICONINFORMATION
            },
    );
}

#[cfg(target_os = "windows")]
fn register(dir: &Path, exe: &Path, icon: &Path) -> Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(UNINSTALL_KEY)?;
    let size_kb = std::fs::metadata(exe).map(|m| m.len() / 1024).unwrap_or(0) as u32;
    key.set_value("DisplayName", &NAME)?;
    key.set_value("DisplayVersion", &crate::update::VERSION)?;
    key.set_value("Publisher", &"Capzay")?;
    key.set_value("DisplayIcon", &icon.display().to_string())?;
    key.set_value("InstallLocation", &dir.display().to_string())?;
    key.set_value(
        "UninstallString",
        &format!("\"{}\" --uninstall", exe.display()),
    )?;
    key.set_value("NoModify", &1u32)?;
    key.set_value("NoRepair", &1u32)?;
    key.set_value("EstimatedSize", &size_kb)?;
    Ok(())
}

/// After a self-update, Installed apps should show the new version.
#[cfg(target_os = "windows")]
pub fn refresh_version() {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    let installed = install_dir().ok().map(|dir| dir.join("cpz-music.exe"));
    if std::env::current_exe().ok() != installed {
        return;
    }
    if let Ok(key) =
        RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(UNINSTALL_KEY, KEY_SET_VALUE)
    {
        let _ = key.set_value("DisplayVersion", &crate::update::VERSION);
    }
}

#[cfg(target_os = "windows")]
fn install() -> Result<()> {
    let dir = install_dir()?;
    let exe = dir.join("cpz-music.exe");
    let icon = dir.join("cpz-music.ico");
    place(&std::env::current_exe()?, &exe)?;
    write(&icon, include_bytes!("../../../packaging/icon.ico"))?;

    let mut link = mslnk::ShellLink::new(&exe).context("could not make the shortcut")?;
    link.set_name(Some(NAME.into()));
    link.set_working_dir(Some(dir.display().to_string()));
    link.set_icon_location(Some(icon.display().to_string()));
    let lnk = shortcut()?;
    if let Some(parent) = lnk.parent() {
        std::fs::create_dir_all(parent)?;
    }
    link.create_lnk(&lnk)
        .context("could not add the Start menu shortcut")?;

    register(&dir, &exe, &icon).context("could not add it to Installed apps")?;
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn run(action: Action) -> Result<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONQUESTION, MB_YESNO};
    match action {
        Action::Install => {
            let prompt = format!(
                "Install {NAME} {}?\n\nIt goes in your own user folder, so no admin rights are needed, and it keeps itself up to date.",
                crate::update::VERSION
            );
            if !ask(&prompt, MB_YESNO | MB_ICONQUESTION) {
                return Ok(());
            }
            if let Err(err) = install() {
                tell(&format!("Couldn't install {NAME}: {err:#}"), true);
                return Err(err);
            }
            if ask(
                &format!("{NAME} is installed. You'll find it in the Start menu.\n\nOpen it now?"),
                MB_YESNO | MB_ICONQUESTION,
            ) {
                let exe = install_dir()?.join("cpz-music.exe");
                std::process::Command::new(exe).spawn()?;
            }
        }
        Action::Uninstall => {
            if !ask(
                &format!("Remove {NAME}?\n\nYour downloads and settings are kept."),
                MB_YESNO | MB_ICONQUESTION,
            ) {
                return Ok(());
            }
            use winreg::RegKey;
            use winreg::enums::HKEY_CURRENT_USER;
            let _ = std::fs::remove_file(shortcut()?);
            let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(UNINSTALL_KEY);
            let dir = install_dir()?;
            if std::env::current_exe()?.starts_with(&dir) {
                // The running .exe cannot delete itself; self-replace hands that
                // to a helper that finishes once this process has exited.
                self_replace::self_delete_outside_path(&dir).context("could not remove the app")?;
            }
            let _ = std::fs::remove_dir_all(&dir);
            tell(&format!("{NAME} has been removed."), false);
        }
    }
    Ok(())
}
