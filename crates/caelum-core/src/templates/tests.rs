use super::*;
use std::{fs, os::unix::fs::symlink, process::Command};

fn template(body: &str) -> Template {
    Template::parse("explain".into(), body.into()).unwrap()
}

#[test]
fn argument_expansion_is_literal_single_pass_and_quote_aware() {
    let t = template("---\r\ndescription: \"Review: request\"\r\nargument_hint: '<pull request>'\r\n---\r\n$2 | $1 | $3 | $4 | $10 | $11 | $ARGUMENTS | $ARGUMENTSX | $0");
    let args = r#"#42 "two words" '$ARGUMENTS' café\ au 5 6 7 8 9 '$1'"#;
    assert_eq!(t.description, "Review: request");
    assert_eq!(t.argument_hint, "<pull request>");
    assert_eq!(
        t.render(args).unwrap(),
        format!("two words | #42 | $ARGUMENTS | café au | $1 |  | {args} | $ARGUMENTSX | $0")
    );
    assert!(t.render("\"unfinished").is_err());
    assert_eq!(
        template("$ARGUMENTS").render("what's new?").unwrap(),
        "what's new?"
    );
    assert!(template("$1").render("").is_err());
    assert!(template("$1$1")
        .render(&"a".repeat(FILE_LIMIT / 2 + 1))
        .is_err());
    assert!(Template::parse("explain".into(), "---\ndescription: unclosed".into()).is_none());
    for name in ["/models", "/ask", "//plain", "/one/two"] {
        assert!(invocation(name).is_none());
    }
}

#[test]
fn discovery_prefers_checkout_then_user_without_following_links() {
    let tmp = tempfile::tempdir().unwrap();
    let roots = ["project", "user"].map(|n| tmp.path().join(n));
    for root in &roots {
        let dir = root.join(".caelum/commands");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("both.md"),
            root.file_name().unwrap().as_encoded_bytes(),
        )
        .unwrap();
    }
    let dir = roots[0].join(".caelum/commands");
    fs::write(roots[1].join(".caelum/commands/user-only.md"), "user").unwrap();
    fs::create_dir(dir.join("directory.md")).unwrap();
    symlink(
        roots[1].join(".caelum/commands/both.md"),
        dir.join("link.md"),
    )
    .unwrap();
    for (name, body) in [
        ("models.md", b"reserved".as_slice()),
        ("bad.md", &[0xff]),
        ("ignore.txt", b"not markdown"),
    ] {
        fs::write(dir.join(name), body).unwrap();
    }
    fs::write(dir.join("large.md"), vec![b'a'; FILE_LIMIT + 1]).unwrap();
    fs::write(dir.join("exact.md"), vec![b'a'; FILE_LIMIT]).unwrap();
    let project = read_dir(&roots[0]).unwrap();
    let items = merge(project.clone(), read_dir(&roots[1]).unwrap());
    assert_eq!(
        items.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["both", "exact", "user-only"]
    );
    assert_eq!(items[0].render("").unwrap(), "project");
    let remote = Command::new("sh")
        .args(["-c", REMOTE_READ])
        .current_dir(&roots[0])
        .output()
        .unwrap();
    assert!(
        remote.status.success(),
        "{}",
        String::from_utf8_lossy(&remote.stderr)
    );
    let remote = remote_files(&remote.stdout).unwrap();
    assert_eq!(merge(remote, vec![]), merge(project, vec![]));

    let outside = tmp.path().join("linked");
    fs::create_dir(&outside).unwrap();
    symlink(roots[1].join(".caelum"), outside.join(".caelum")).unwrap();
    assert!(read_dir(&outside).unwrap().is_empty());
}

#[test]
fn combined_catalogue_has_a_deterministic_count_limit() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join(".caelum/commands");
    fs::create_dir_all(&dir).unwrap();
    for i in (0..COUNT_LIMIT + 3).rev() {
        fs::write(dir.join(format!("t{i:03}.md")), "hi").unwrap();
    }
    let project = read_dir(root.path()).unwrap();
    assert_eq!(project.len(), COUNT_LIMIT);
    let items = merge(project, vec![("another".into(), "user".into())]);
    assert_eq!(items.len(), COUNT_LIMIT);
    assert_eq!(items.last().unwrap().name, "t127");
}
