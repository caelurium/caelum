use super::*;
use std::fs;

#[test]
fn parses_defaults_worktree_setup_and_runner_presets() {
    let raw = r#"
# Project-wide Caelum configuration
[defaults]
provider = "opencode"
model = "claude-sonnet-4"
effort = "high"
agent = "build"

[worktree]
setup = [
  "pnpm install --frozen-lockfile", # install deps
  'direnv allow',
]

[[runner.presets]]
name = "Test"
command = "cargo test --workspace"

[[runner.presets]]
name = "Check"
command = "make check"

[future_section]
ignored = "value"
"#;

    let cfg = ProjectConfig::parse(raw).expect("valid config");
    assert_eq!(cfg.defaults.provider, Some(Provider::OpenCode));
    assert_eq!(cfg.defaults.model.as_deref(), Some("claude-sonnet-4"));
    assert_eq!(cfg.defaults.effort.as_deref(), Some("high"));
    assert_eq!(cfg.defaults.agent.as_deref(), Some("build"));
    assert_eq!(
        cfg.worktree.setup,
        vec![
            "pnpm install --frozen-lockfile".to_owned(),
            "direnv allow".to_owned(),
        ]
    );
    assert_eq!(
        cfg.runner_presets,
        vec![
            RunnerPreset {
                name: "Test".into(),
                command: "cargo test --workspace".into(),
            },
            RunnerPreset {
                name: "Check".into(),
                command: "make check".into(),
            },
        ]
    );
    assert_eq!(
        cfg.combined_setup_command().as_deref(),
        Some("pnpm install --frozen-lockfile && direnv allow")
    );
}

#[test]
fn fingerprint_changes_when_setup_commands_change() {
    let a = ProjectConfig::parse("[worktree]\nsetup = [\"pnpm install\"]\n").unwrap();
    let b =
        ProjectConfig::parse("[worktree]\nsetup = [\"pnpm install\", \"direnv allow\"]\n").unwrap();
    let empty = ProjectConfig::default();

    assert!(empty.setup_fingerprint().is_none());
    assert_ne!(a.setup_fingerprint(), b.setup_fingerprint());
    assert_eq!(a.setup_fingerprint(), a.setup_fingerprint());
}

#[test]
fn rejects_invalid_provider_and_incomplete_presets() {
    let err = ProjectConfig::parse("[defaults]\nprovider = \"unknown\"\n").unwrap_err();
    assert!(err.contains("unknown provider"));

    let err = ProjectConfig::parse("[[runner.presets]]\nname = \"OnlyName\"\n").unwrap_err();
    assert!(err.contains("requires both"));
}

#[test]
fn loads_from_disk_and_ignores_missing_or_symlinked_config() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(load(dir.path().to_str().unwrap()).unwrap(), None);

    let caelum = dir.path().join(".caelum");
    fs::create_dir(&caelum).unwrap();
    fs::write(
        caelum.join("config.toml"),
        "[defaults]\nprovider = \"codex\"\n[[runner.presets]]\nname = \"Build\"\ncommand = \"cargo build\"\n",
    )
    .unwrap();
    let loaded = load(dir.path().to_str().unwrap()).unwrap().unwrap();
    assert_eq!(loaded.defaults.provider, Some(Provider::Codex));
    assert_eq!(loaded.runner_presets.len(), 1);
    assert_eq!(loaded.runner_presets[0].name, "Build");

    #[cfg(unix)]
    {
        let external = dir.path().join("outside.toml");
        fs::write(&external, "[defaults]\nprovider = \"opencode\"\n").unwrap();
        fs::remove_file(caelum.join("config.toml")).unwrap();
        std::os::unix::fs::symlink(&external, caelum.join("config.toml")).unwrap();
        assert_eq!(load(dir.path().to_str().unwrap()).unwrap(), None);
    }
}
