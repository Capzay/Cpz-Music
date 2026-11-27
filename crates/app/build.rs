use std::path::PathBuf;
use std::process::Command;

fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("the UI compiles");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        windows_resources();
    }
}

/// The icon Explorer and the taskbar show for the .exe, and the name and
/// version in its Properties and in Task Manager. Compiled with llvm-windres
/// (`WINDRES` to use another) into an object the linker adds to the binary.
fn windows_resources() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = manifest.join("../../packaging/icon.ico");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let part = |name: &str| std::env::var(name).unwrap();
    let numbers = format!(
        "{},{},{},0",
        part("CARGO_PKG_VERSION_MAJOR"),
        part("CARGO_PKG_VERSION_MINOR"),
        part("CARGO_PKG_VERSION_PATCH")
    );
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numbers}
PRODUCTVERSION {numbers}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Capzay"
      VALUE "FileDescription", "Cpz Music"
      VALUE "FileVersion", "{version}"
      VALUE "ProductName", "Cpz Music"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "cpz-music.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "/"),
    );
    let rc_path = out.join("app.rc");
    std::fs::write(&rc_path, rc).unwrap();
    let object = out.join("app-resources.o");

    let windres = std::env::var("WINDRES").unwrap_or_else(|_| "llvm-windres".into());
    let status = Command::new(&windres)
        .args(["--target", "pe-x86-64", "-O", "coff", "-i"])
        .arg(&rc_path)
        .arg("-o")
        .arg(&object)
        .status()
        .unwrap_or_else(|err| {
            panic!("could not run {windres} (install llvm, or set WINDRES): {err}")
        });
    assert!(
        status.success(),
        "{windres} failed on {}",
        rc_path.display()
    );

    // Only the executable: a library has nowhere to put an icon.
    println!("cargo:rustc-link-arg-bins={}", object.display());
    println!("cargo:rerun-if-changed={}", icon.display());
    println!("cargo:rerun-if-env-changed=WINDRES");
}
