use super::Access;
use crate::{files, git, remote::boat::checkout_quote as quote};
use caelum_protocol::{
    GitFileDiff, GitRepositoryStatus, ProjectSearchMatch, ProjectSearchMode, ProjectSearchResults,
};

const CONTENT_SEARCH: &str = r#"import json,os,subprocess,sys
root,query=sys.argv[1:]
env=os.environ.copy()
env["GIT_TERMINAL_PROMPT"]="0"
command=["git","-C",root,"--literal-pathspecs","grep","--untracked","--exclude-standard","-n","-z","-I","-i","-F","-e",query,"--","."]
process=subprocess.Popen(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env)
matches=[]
truncated=False
while True:
    record=process.stdout.readline(1024*1024+1)
    if not record:
        break
    oversized=len(record)>1024*1024 and not record.endswith(b"\n")
    while oversized and not record.endswith(b"\n"):
        record=process.stdout.readline(1024*1024+1)
        if not record:
            break
    if oversized:
        truncated=True
    fields=record.rstrip(b"\n").split(b"\0",2)
    if len(fields)!=3:
        continue
    path=fields[0].decode("utf-8","replace")
    number=fields[1].decode("ascii","ignore")
    line=fields[2].decode("utf-8","replace")
    folded=line.casefold()
    needle=query.casefold()
    found=folded.find(needle)
    if found<0:
        continue
    if len(matches)==200:
        truncated=True
        process.kill()
        break
    offset=0
    source_index=None
    for index,char in enumerate(line):
        if offset==found:
            source_index=index
            break
        offset+=len(char.casefold())
        if offset>found:
            break
        if offset==found:
            source_index=index+1
            break
    if source_index is None:
        column=None
        start=0
    else:
        column=len(line[:source_index].encode("utf-16-le"))//2+1
        start=max(0,source_index-60)
    matches.append({"path":path,"line":int(number) if number.isdigit() else None,"column":column,"preview":line[start:start+240]})
process.communicate()
print(json.dumps({"matches":matches,"truncated":truncated},ensure_ascii=True))
"#;

impl Access {
    async fn git(&self, args: &[&str], codes: &[i32]) -> Result<Vec<u8>, String> {
        let root = quote(&self.root);
        let args = args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
        let script=format!("test \"$(git -C {root} rev-parse --show-toplevel)\" = {root} && GIT_TERMINAL_PROMPT=0 GIT_OPTIONAL_LOCKS=0 LC_ALL=C.UTF-8 git -C {root} --literal-pathspecs {args}");
        self.run(&script, 8 * 1024 * 1024, codes).await
    }
    pub(crate) async fn branch_context(&self) -> Result<git::GitBranchContext, String> {
        let branch = self.git(&["branch", "--show-current"], &[0]).await?;
        let branch = String::from_utf8(branch).map_err(|_| "Invalid VM branch")?;
        Ok(git::GitBranchContext {
            root: self.root.clone().into(),
            branch: (!branch.trim().is_empty()).then(|| branch.trim().into()),
        })
    }
    pub(super) async fn status(&self) -> Result<GitRepositoryStatus, String> {
        let root = quote(&self.root);
        let args = git::PORCELAIN_STATUS_ARGS
            .iter()
            .map(|argument| quote(argument))
            .collect::<Vec<_>>()
            .join(" ");
        let script = format!(
            "test \"$(git -C {root} rev-parse --show-toplevel)\" = {root} && branch=$(git -C {root} branch --show-current) && printf '%s\\n' \"$branch\" && GIT_TERMINAL_PROMPT=0 GIT_OPTIONAL_LOCKS=0 LC_ALL=C.UTF-8 git -C {root} --literal-pathspecs {args}"
        );
        let data = self.run(&script, 8 * 1024 * 1024, &[0]).await?;
        let delimiter = data
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or("Invalid VM Git status response.")?;
        let (branch, status) = data.split_at(delimiter);
        let status = &status[1..];
        let branch = std::str::from_utf8(branch).map_err(|_| "Invalid VM branch.")?;
        let mut changes = git::parse_status(status).map_err(|e| e.to_string())?;
        self.stamps(&mut changes).await;
        Ok(GitRepositoryStatus {
            project_path: self.root.clone(),
            repository_root: Some(self.root.clone()),
            branch: (!branch.is_empty()).then(|| branch.into()),
            changes,
        })
    }
    pub(super) async fn diff(&self, path: &str) -> Result<GitFileDiff, String> {
        files::validate_relative_path(path).map_err(|e| e.to_string())?;
        let mut args = git::PORCELAIN_STATUS_ARGS.to_vec();
        args.extend_from_slice(&["--", path]);
        let changes =
            git::parse_status(&self.git(&args, &[0]).await?).map_err(|e| e.to_string())?;
        let change = changes
            .iter()
            .find(|f| f.path == path)
            .ok_or("Not a changed file on the VM.")?;
        let read = async |extra: &[&str], paths: &[&str], codes: &[i32]| {
            let mut args = vec!["diff", "--no-ext-diff", "--no-textconv", "--color=never"];
            args.extend_from_slice(extra);
            args.push("--");
            args.extend_from_slice(paths);
            self.git(&args, codes)
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        };
        Ok(GitFileDiff {
            path: path.into(),
            staged: read(&["--cached"], &[path], &[0]).await?,
            unstaged: read(&[], &[path], &[0]).await?,
            untracked: if change.index_status == "?" {
                read(&["--no-index"], &["/dev/null", path], &[0, 1]).await?
            } else {
                String::new()
            },
        })
    }
    pub(super) async fn search(
        &self,
        query: &str,
        mode: ProjectSearchMode,
    ) -> Result<ProjectSearchResults, String> {
        if query.chars().count() > 256 || query.contains('\0') {
            return Err("Search query is too long or invalid.".into());
        }
        let mut result = ProjectSearchResults {
            matches: vec![],
            truncated: false,
            skipped: 0,
        };
        match mode {
            ProjectSearchMode::Files => {
                let data = self
                    .git(
                        &[
                            "ls-files",
                            "-z",
                            "--cached",
                            "--others",
                            "--exclude-standard",
                            "--deduplicate",
                        ],
                        &[0],
                    )
                    .await?;
                let mut ranked: Vec<_> = data
                    .split(|b| *b == 0)
                    .filter(|p| !p.is_empty())
                    .filter_map(|p| std::str::from_utf8(p).ok())
                    .filter_map(|p| files::path_score(p, query.trim()).map(|s| (s, p)))
                    .collect();
                ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
                result.truncated = ranked.len() > 200;
                result.matches = ranked
                    .into_iter()
                    .take(200)
                    .map(|(_, p)| ProjectSearchMatch {
                        path: p.into(),
                        line: None,
                        column: None,
                        preview: None,
                    })
                    .collect();
            }
            ProjectSearchMode::Content if !query.trim().is_empty() => {
                let root = quote(&self.root);
                let script = format!(
                    "cd -P {root} && python3 -c {} {root} {}",
                    quote(CONTENT_SEARCH),
                    quote(query)
                );
                let data = self.run(&script, 1024 * 1024, &[0]).await?;
                let response: serde_json::Value =
                    serde_json::from_slice(&data).map_err(|e| e.to_string())?;
                result.truncated = response["truncated"].as_bool().unwrap_or(false);
                for value in response["matches"].as_array().into_iter().flatten() {
                    result.matches.push(ProjectSearchMatch {
                        path: value["path"].as_str().unwrap_or_default().to_owned(),
                        line: value["line"]
                            .as_u64()
                            .and_then(|line| usize::try_from(line).ok()),
                        column: value["column"]
                            .as_u64()
                            .and_then(|column| usize::try_from(column).ok()),
                        preview: value["preview"].as_str().map(str::to_owned),
                    });
                }
            }
            _ => {}
        }
        Ok(result)
    }
}
