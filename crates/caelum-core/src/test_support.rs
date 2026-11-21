use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

pub(crate) fn write_executable(path: &Path, contents: impl AsRef<[u8]>) {
    let mut writer = Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "write-fixture"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .expect("start executable fixture writer");
    writer
        .stdin
        .take()
        .expect("piped fixture input")
        .write_all(contents.as_ref())
        .expect("write executable fixture");
    assert!(
        writer.wait().expect("wait for fixture writer").success(),
        "could not create executable fixture {}",
        path.display()
    );
}
