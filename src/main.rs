#[macro_use]
extern crate nom;

mod blame;

use anyhow::Result;
use git2::{BlameHunk, Commit, Oid, Repository};
use rayon::prelude::*;
use regex::Regex;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use structopt::clap::AppSettings;
use structopt::StructOpt;

struct TrackedFile {
    path: String,
    owners: HashMap<String, Owner>,
}

impl TrackedFile {
    fn new(path: String) -> TrackedFile {
        TrackedFile {
            path,
            owners: HashMap::new(),
        }
    }

    fn add_hunk(&mut self, hunk: &impl Hunk) {
        let owner = Owner::new(hunk);
        self.owners
            .entry(hunk.email())
            .or_insert(owner)
            .add_hunk(hunk);
    }
}

struct Owner {
    name: String,
    email: String,
    commits: HashMap<String, usize>,
}

impl Owner {
    fn new(hunk: &impl Hunk) -> Owner {
        Owner {
            name: hunk.author(),
            email: hunk.email(),
            commits: HashMap::new(),
        }
    }

    fn add_hunk(&mut self, hunk: &impl Hunk) {
        *self.commits.entry(hunk.sha1()).or_insert(0) += hunk.lines();
    }

    fn lines(&self) -> usize {
        self.commits.values().sum::<usize>()
    }
}

impl fmt::Display for Owner {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} <{}>: Lines: {} Count: {}",
            self.name,
            self.email,
            self.lines(),
            self.commits.len()
        )
    }
}

/// Definition of a hunk with no dependencies.
struct BasicHunk {
    hash: String,
    author: String,
    mail: String,
    num_lines: usize,
}

impl Hunk for BasicHunk {
    fn sha1(&self) -> String {
        self.hash.clone()
    }
    fn author(&self) -> String {
        self.author.clone()
    }
    fn email(&self) -> String {
        self.mail.clone()
    }
    fn lines(&self) -> usize {
        self.num_lines
    }
}

struct RawHunk<'rh> {
    commit: Commit<'rh>,
    _lines: usize,
}

trait Hunk {
    fn sha1(&self) -> String;
    fn author(&self) -> String;
    fn email(&self) -> String;
    fn lines(&self) -> usize;
}

impl Hunk for &RawHunk<'_> {
    fn sha1(&self) -> String {
        self.commit.id().to_string()
    }
    fn author(&self) -> String {
        String::from_utf8_lossy(self.commit.author().name_bytes()).to_string()
    }
    fn email(&self) -> String {
        String::from_utf8_lossy(self.commit.author().email_bytes()).to_string()
    }
    fn lines(&self) -> usize {
        self._lines
    }
}

impl Hunk for BlameHunk<'_> {
    fn sha1(&self) -> String {
        self.final_commit_id().to_string()
    }
    fn author(&self) -> String {
        String::from_utf8_lossy(self.final_signature().name_bytes()).to_string()
    }
    fn email(&self) -> String {
        String::from_utf8_lossy(self.final_signature().email_bytes()).to_string()
    }
    fn lines(&self) -> usize {
        self.lines_in_hunk()
    }
}

fn run_external_blame<'rh>(repo: &'rh Repository, path: &PathBuf) -> Result<Vec<RawHunk<'rh>>> {
    let mut hunks: Vec<RawHunk> = Vec::new();

    let output = Command::new("git")
        .arg("-C")
        .arg(format!("{}", path.parent().unwrap().display().to_string()))
        .arg("blame")
        .arg("--line-porcelain")
        .arg("--")
        .arg(format!("{}", path.file_name().unwrap().to_str().unwrap()))
        .output()?;

    if !output.status.success() {
        println!("Error with git-blame for {}", path.display());
        return Err(anyhow::Error::msg("Error running git blame"));
    }

    let pattern = Regex::new(
        r"(?x)
          ^([0-9a-zA-Z]{40})\s+ # 40 character SHA-1
          [0-9]+\s+ # Original line number
          [0-9]+\s+ # Final line number
          ([0-9]+) # Line count",
    )?;

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| pattern.captures(line))
        .map(|cap| RawHunk {
            commit: repo
                .find_object(Oid::from_str(&cap[1].to_string()).unwrap(), None)
                .unwrap()
                .into_commit()
                .unwrap(),
            _lines: cap[2].to_string().parse::<usize>().unwrap(),
        })
        .for_each(|hunk| hunks.push(hunk));

    Ok(hunks)
}

fn analyze_file_nom(path: &Path) -> Result<TrackedFile> {
    let txt = blame::generate_blame(&path.canonicalize().unwrap())?;
    let lines = blame::parse_blame(&txt);

    let commits: HashMap<&str, (&str, &str)> = lines
        .iter()
        .filter_map(|line| {
            if let Some(extra) = &line.header.extra {
                Some((line.header.hash, (extra.author, extra.author_mail)))
            } else {
                None
            }
        })
        .collect();

    let mut tracked_file = TrackedFile::new(path.display().to_string());

    lines
        .iter()
        .filter_map(|line| {
            if let Some(num_lines_in_group) = line.header.num_lines_in_group {
                let commit = commits
                    .get(line.header.hash)
                    .expect("Commit information must be known for hunk.");
                Some(BasicHunk {
                    hash: line.header.hash.to_string(),
                    author: commit.0.to_string(),
                    mail: commit.1.trim_start_matches("<").trim_end_matches(">").to_string(),
                    num_lines: num_lines_in_group,
                })
            } else {
                None
            }
        })
        .for_each(|hunk| {
            tracked_file.add_hunk(&hunk);
        });

    Ok(tracked_file)
}

fn analyze_file(file: &PathBuf) -> Result<TrackedFile> {
    let repo = Repository::discover(file)?;

    // Construct the path relative to the Git repository.
    let repo_base_path: PathBuf = repo.path().iter().take_while(|x| *x != ".git").collect();
    let arg_path = file.canonicalize()?;
    let path = if arg_path.starts_with(&repo_base_path) {
        arg_path.strip_prefix(&repo_base_path)?.to_path_buf()
    } else {
        arg_path
    };

    let mut tracker = TrackedFile::new(path.display().to_string());

    let blame = run_external_blame(&repo, &file)?;

    for hunk in blame.iter() {
        tracker.add_hunk(&hunk);
    }

    Ok(tracker)
}

#[derive(StructOpt)]
#[allow(non_snake_case)]
#[structopt(global_settings = &[AppSettings::ColoredHelp])]
struct Args {
    #[structopt(name = "filter-email", long)]
    email: Option<Vec<String>>,

    #[structopt(name = "filter-name", long)]
    name: Option<Vec<String>>,

    #[structopt(name = "summary", long)]
    /// Print out summary of owners
    summary: bool,

    /// Use the regex parser
    #[structopt(long)]
    regex: bool,

    #[structopt(name = "files", parse(from_os_str))]
    file_list: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::from_args();

    let tracked_files: Vec<TrackedFile> = args
        .file_list
        .par_iter()
        .filter_map(|path| {
            if args.regex {
                analyze_file(path).ok()
            } else {
                analyze_file_nom(path).ok()
            }
        })
        .collect();

    for file in tracked_files {
        let mut owners: Vec<&Owner> = file
            .owners
            .values()
            .filter(|s| match &args.email {
                Some(email) => email.iter().any(|e| s.email.contains(e)),
                None => true,
            })
            .filter(|s| match &args.name {
                Some(name) => name.iter().any(|n| s.email.contains(n)),
                None => true,
            })
            .collect();

        if !owners.is_empty() {
            println!("File: {}", file.path);
            owners.sort_by_key(|a| a.lines());
            owners.reverse();
            owners.iter().for_each(|x| println!(" {}", x));
        }
    }

    Ok(())
}
