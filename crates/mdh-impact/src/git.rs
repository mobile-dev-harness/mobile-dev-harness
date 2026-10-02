//! The change set: what differs between a base revision and the working tree, through the `git` CLI.
//! All paths are relative to the project directory.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mdh_core::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub status: Status,
    pub path: String,
    /// The path in the base revision, when it differs (renames).
    pub old_path: Option<String>,
}

pub struct Repo {
    dir: PathBuf,
}

impl Repo {
    pub fn open(dir: &Path) -> Result<Repo> {
        let repo = Repo {
            dir: dir.to_owned(),
        };
        match repo.git(&["rev-parse", "--is-inside-work-tree"]) {
            Ok(out) if out.trim() == "true" => Ok(repo),
            Ok(_) | Err(Error::CommandFailed { .. }) => Err(Error::NotARepository {
                dir: dir.display().to_string(),
            }),
            Err(e) => Err(e),
        }
    }

    /// The commit `base` names, abbreviated.
    pub fn resolve(&self, base: &str) -> Result<String> {
        self.git(&[
            "rev-parse",
            "--short",
            "--verify",
            "--quiet",
            &format!("{base}^{{commit}}"),
        ])
        .map(|s| s.trim().to_owned())
        .map_err(|_| Error::UnknownRevision {
            rev: base.to_owned(),
        })
    }

    /// Files that differ between `base` and the working tree (staged or not), plus untracked ones.
    pub fn changes(&self, base: &str) -> Result<Vec<FileChange>> {
        let out = self.git(&[
            "diff",
            "--relative",
            "--name-status",
            "-z",
            "-M",
            base,
            "--",
            ".",
        ])?;
        let mut fields = out.split('\0').filter(|s| !s.is_empty());
        let mut changes = Vec::new();
        while let Some(status) = fields.next() {
            let Some(path) = fields.next() else { break };
            let change = match status.as_bytes()[0] {
                b'A' => FileChange {
                    status: Status::Added,
                    path: path.into(),
                    old_path: None,
                },
                b'D' => FileChange {
                    status: Status::Deleted,
                    path: path.into(),
                    old_path: None,
                },
                b'R' | b'C' => {
                    let Some(new) = fields.next() else { break };
                    FileChange {
                        status: if status.starts_with('R') {
                            Status::Renamed
                        } else {
                            Status::Added
                        },
                        path: new.into(),
                        old_path: status.starts_with('R').then(|| path.into()),
                    }
                }
                _ => FileChange {
                    status: Status::Modified,
                    path: path.into(),
                    old_path: None,
                },
            };
            changes.push(change);
        }
        for path in self
            .git(&[
                "ls-files",
                "--others",
                "--exclude-standard",
                "-z",
                "--",
                ".",
            ])?
            .split('\0')
        {
            if !path.is_empty() {
                changes.push(FileChange {
                    status: Status::Added,
                    path: path.into(),
                    old_path: None,
                });
            }
        }
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(changes)
    }

    /// Every file of the project that git doesn't ignore, tracked or not.
    pub fn files(&self) -> Result<Vec<String>> {
        let out = self.git(&[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".",
        ])?;
        let mut files: Vec<String> = out
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        files.dedup();
        Ok(files)
    }

    /// Contents of `paths` at `base`, read in one `git cat-file --batch`; missing paths are left out.
    pub fn read_at(&self, base: &str, paths: &[&str]) -> Result<HashMap<String, Vec<u8>>> {
        let mut out = HashMap::new();
        if paths.is_empty() {
            return Ok(out);
        }
        let mut child = Command::new("git")
            .args(["cat-file", "--batch"])
            .current_dir(&self.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(git_missing)?;
        let mut stdin = child.stdin.take().expect("stdin is piped");
        let requests: String = paths.iter().map(|p| format!("{base}:./{p}\n")).collect();
        let writer = std::thread::spawn(move || stdin.write_all(requests.as_bytes()));
        let mut reader = BufReader::new(child.stdout.take().expect("stdout is piped"));
        for path in paths {
            let mut header = String::new();
            if reader.read_line(&mut header)? == 0 {
                break;
            }
            // `<oid> blob <size>`, or `<rev:path> missing`.
            let parts: Vec<&str> = header.split_whitespace().collect();
            let [_, "blob", size] = parts.as_slice() else {
                continue;
            };
            let size: usize = size.parse().map_err(|_| Error::Parse {
                tool: "git cat-file".into(),
                detail: header.trim().into(),
            })?;
            let mut content = vec![0; size + 1];
            reader.read_exact(&mut content)?;
            content.pop();
            out.insert((*path).to_owned(), content);
        }
        let _ = writer.join();
        let _ = child.wait();
        Ok(out)
    }

    fn git(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.dir)
            .stdin(Stdio::null())
            .output()
            .map_err(git_missing)?;
        if !output.status.success() {
            return Err(Error::CommandFailed {
                command: format!("git {}", args.join(" ")),
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

fn git_missing(e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::NotFound {
        Error::ToolNotFound {
            name: "git".into(),
            hint: "install git and make sure it is on PATH".into(),
        }
    } else {
        Error::Io(e)
    }
}
