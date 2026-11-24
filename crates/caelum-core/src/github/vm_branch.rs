use super::{gh_executable, github_command, Gh};
use crate::remote::Source;
use std::{path::Path, time::Duration};

pub(crate) fn create_vm_branch(source: &Source, tip: &str) -> Result<(), String> {
    let gh = gh_executable();
    let mut command = github_command(&gh, Path::new(&source.project));
    command.args([
        "api",
        "--hostname",
        "github.com",
        "--method",
        "POST",
        &format!("repos/{}/git/refs", source.repository),
        "-f",
        &format!("ref=refs/heads/{}", source.base),
        "-f",
        &format!("sha={tip}"),
    ]);
    gh.output(command, Duration::from_secs(30)).map(|_| ()).map_err(|e| {
        format!("Could not create origin/{} from origin/main. Check GitHub CLI access and permission to create branches: {e}", source.base)
    })
}
