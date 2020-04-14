extern crate rayon;
extern crate regex;

use anyhow::Result;
use git2::{BlameHunk, Commit, Oid, Repository};
use rayon::prelude::*;
use regex::Regex;
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::process::Command;
use structopt::clap::AppSettings;
use structopt::StructOpt;

#[derive(StructOpt)]
#[allow(non_snake_case)]
#[structopt(global_settings = &[AppSettings::ColoredHelp])]
struct Args {
    #[structopt(name = "files", parse(from_os_str))]
    file_list: Vec<PathBuf>,
}

struct TrackedFile {
    path: String,
    owners: HashMap<String, Owner>,
}

impl TrackedFile {
    fn new(path: &String) -> TrackedFile {
        TrackedFile {
            path: path.clone(),
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
        .arg(format!(
            "{}",
            path.parent().unwrap().display().to_string()
            //repo.path().parent().unwrap().display().to_string()
        ))
        .arg("blame")
        .arg("--line-porcelain")
        .arg("--")
        .arg(format!("{}", path.display().to_string()))
        .output()?;

    if !output.status.success() {
        println!("Error with git-blame for {}", path.display());
    }

    let pattern = Regex::new(
        r"(?x)
          ^([0-9a-zA-Z]{40})\s+ # 40 character SHA-1
          [0-9]+\s+ # Original line number
          [0-9]+\s+ # Final line number
          ([0-9]+) # Line count",
    )?;

    String::from_utf8_lossy(&output.stdout)
        //.unwrap()
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

fn analyze_file(file: &PathBuf) -> Result<TrackedFile> {
    let repo = Repository::discover(file)?;

    // Construct the path relative to the Git repository.
    let repo_base_path = repo.path().parent().unwrap();
    let arg_path = file.canonicalize()?;
    let path = if repo_base_path == arg_path {
        repo_base_path
    } else {
        arg_path.strip_prefix(repo_base_path)?
    };

    let mut tracker = TrackedFile::new(&path.display().to_string());

    let blame = run_external_blame(&repo, &file)?;

    for hunk in blame.iter() {
        tracker.add_hunk(&hunk);
    }

    Ok(tracker)
}

fn main() -> Result<()> {
    let args = Args::from_args();

    let tracked_files: Vec<TrackedFile> = args
        .file_list
        .par_iter()
        .map(|path| match analyze_file(path) {
            Ok(file) => file,
            Err(error) => {
                println!("Problem with {:?}", error);
                TrackedFile::new(&"Unknown".to_string())
            }
        })
        .collect();

    for file in tracked_files {
        println!("File: {}", file.path);
        let mut owners: Vec<&Owner> = file.owners.values().collect();
        owners.sort_by(|a, b| b.lines().cmp(&a.lines()));

        for owner in owners {
            println!("  {}", owner);
        }
    }

    Ok(())
}
