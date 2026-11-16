use super::*;

#[test]
fn desktop_exec_preserves_spaces_unicode_percent_and_reserved_characters() {
    let command = desktop_command(Path::new("/home/café $user/a% b/\"Caelum\".AppImage")).unwrap();
    assert_eq!(
        command,
        "\"/home/café \\\\$user/a%% b/\\\\\"Caelum\\\\\".AppImage\""
    );
    assert!(desktop_command(Path::new("/home/user\n/evil")).is_err());
}

#[test]
fn installation_updates_only_owned_files_and_uses_the_stable_image_path() {
    let root = std::env::temp_dir().join(format!("caelum-install-test-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("download.AppImage");
    let data = root.join("data with spaces");
    fs::write(&source, b"first image").unwrap();
    let installed = install(&source, &data).unwrap();
    fs::write(data.join("caelum/unrelated"), b"keep").unwrap();
    fs::write(&source, b"second image").unwrap();
    install(&source, &data).unwrap();
    assert_eq!(fs::read(&installed).unwrap(), b"second image");
    assert_eq!(fs::read(data.join("caelum/unrelated")).unwrap(), b"keep");
    let entry = fs::read_to_string(data.join("applications").join(DESKTOP)).unwrap();
    assert!(entry.contains(&format!("Exec={} %u", desktop_command(&installed).unwrap())));
    assert_eq!(
        fs::metadata(installed).unwrap().permissions().mode() & 0o777,
        0o755
    );
    fs::remove_dir_all(root).unwrap();
}
