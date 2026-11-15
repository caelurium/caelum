//! Explicit per-user AppImage installation; never runs during ordinary startup.
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

const DESKTOP: &str = "dev.caelum.desktop.desktop";
const ICON: &[u8] = include_bytes!("../../assets/linux/dev.caelum.desktop.png");
const ENTRY: &str = include_str!("../../assets/linux/dev.caelum.desktop.desktop");

pub(super) fn run() -> Result<(), String> {
    let image = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .ok_or("Run the downloaded AppImage with --install")?;
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or("Set HOME or an absolute XDG_DATA_HOME before installing")?;
    let installed = install(&image, &data)?;
    for (program, args) in [
        (
            "update-desktop-database",
            vec![data.join("applications").into_os_string()],
        ),
        (
            "xdg-mime",
            vec![
                "default".into(),
                DESKTOP.into(),
                "x-scheme-handler/caelum".into(),
            ],
        ),
    ] {
        let status = Command::new(program)
            .args(args)
            .status()
            .map_err(|error| format!("{program}: {error}"))?;
        if !status.success() {
            return Err(format!("{program} failed: {status}"));
        }
    }
    println!(
        "Installed {} and registered Caelum in the application menu",
        installed.display()
    );
    Ok(())
}

fn install(source: &Path, data: &Path) -> Result<PathBuf, String> {
    let image = data.join("caelum/Caelum.AppImage");
    let command = desktop_command(&image)?;
    let directory = image.parent().expect("installation directory");
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    // Copy first, then atomically replace our installed image; the running download
    // and any existing installation keep their backing inode until the copy succeeds.
    let temporary = directory.join(format!(".install-{}.AppImage", std::process::id()));
    let result = (|| {
        fs::copy(source, &temporary)?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o755))?;
        fs::rename(&temporary, &image)
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    let icon = data.join("icons/hicolor/512x512/apps/dev.caelum.desktop.png");
    fs::create_dir_all(icon.parent().unwrap()).map_err(|error| error.to_string())?;
    fs::write(icon, ICON).map_err(|error| error.to_string())?;
    let applications = data.join("applications");
    fs::create_dir_all(&applications).map_err(|error| error.to_string())?;
    fs::write(
        applications.join(DESKTOP),
        ENTRY.replace("Exec=caelum-desktop %u", &format!("Exec={command} %u")),
    )
    .map_err(|error| error.to_string())?;
    Ok(image)
}

fn desktop_command(path: &Path) -> Result<String, String> {
    let path = path
        .to_str()
        .filter(|value| !value.chars().any(char::is_control))
        .ok_or("The installation path must be UTF-8 without control characters")?;
    // Exec quoting is decoded after desktop-entry string escapes. Percent is a
    // field-code escape, not shell syntax; doubled percent preserves literal paths.
    let mut quoted = String::from("\"");
    for character in path.chars() {
        if matches!(character, '\\' | '"' | '$' | '`') {
            quoted.push('\\');
        }
        if character == '%' {
            quoted.push('%');
        }
        quoted.push(character);
    }
    quoted.push('"');
    Ok(quoted.replace('\\', "\\\\"))
}

#[cfg(test)]
mod tests;
