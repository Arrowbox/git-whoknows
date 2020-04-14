extern crate regex;

use anyhow::Result;
use git2::{BlameHunk, BlameOptions, Repository, Oid, Commit};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::process::Command;
use structopt::clap::AppSettings;
use structopt::StructOpt;
use regex::Regex;

#[derive(StructOpt)]
#[allow(non_snake_case)]
#[structopt(global_settings = &[AppSettings::ColoredHelp])]
struct Args {
    #[structopt(name = "path", parse(from_os_str))]
    arg_path: PathBuf,
    #[structopt(short = "M")]
    /// find line moves within and across files
    flag_M: bool,
    #[structopt(short = "C")]
    /// find line copies within and across files
    flag_C: bool,
    #[structopt(short = "F")]
    /// follow only the first parent commits
    flag_F: bool,
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

    fn add_hunk (&mut self, hunk: &impl Hunk) {
        *self.commits.entry(hunk.sha1()).or_insert(0) += hunk.lines();
    }

    fn lines(&self) -> usize {
        self.commits.values().sum::<usize>()
    }
}

trait Hunk {
    fn sha1(&self) -> String;
    fn author(&self) -> String;
    fn email(&self) -> String;
    fn lines(&self) -> usize;
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

struct RawHunk<'rh> {
    commit: Commit<'rh>,
    _lines: usize,
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

fn run_external_blame<'rh>(repo: &'rh Repository, path: &PathBuf) -> Result<Vec<RawHunk<'rh>>> {
    let mut hunks: Vec<RawHunk> = Vec::new();

    let output = Command::new("git")
        .arg("-C")
        .arg(format!("{}", repo.path()
                                .parent()
                                .unwrap()
                                .display()
                                .to_string()))
        .arg("blame")
        .arg("--line-porcelain")
        .arg("--")
        .arg(format!("{}", path.display().to_string()))
        .output()?;

    if !output.status.success() {
        println!("Error with command");
    }

    let pattern = Regex::new(r"(?x)
                                ^([0-9a-zA-Z]{40})\s+ # 40 character SHA-1
                                [0-9]+\s+ # Original line number
                                [0-9]+\s+ # Final line number
                                ([0-9]+) # Line count")?;

    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| pattern.captures(line))
        .map(|cap| {
                 RawHunk {
                    commit: repo.find_object(
                        Oid::from_str(&cap[1].to_string()).unwrap(), None)
                        .unwrap()
                        .into_commit()
                        .unwrap(),
                    _lines: cap[2].to_string().parse::<usize>().unwrap(),
                 }
             })
        .for_each(|hunk| hunks.push(hunk));

    Ok(hunks)
}

fn main() -> Result<()> {
    let args = Args::from_args();

    let repo = Repository::discover(&args.arg_path)?;

    // Construct the path relative to the Git repository.
    let repo_base_path = repo.path().parent().unwrap();
    let arg_path = args.arg_path.canonicalize()?;
    let path = if repo_base_path == arg_path {
        repo_base_path
    } else {
        arg_path.strip_prefix(repo_base_path)?
    };

    // Prepare our blame options
    let mut opts = BlameOptions::new();
    opts.track_copies_same_commit_moves(args.flag_M)
        .track_copies_same_commit_copies(args.flag_C)
        .first_parent(args.flag_F);

    let mut tracker = TrackedFile::new(&path.display().to_string());

    let blame = run_external_blame(&repo, &args.arg_path)?;
    //let blame = repo.blame_file(path, Some(&mut opts))?;

    for hunk in blame.iter() {
        tracker.add_hunk(&hunk);
    }

    println!("File: {}", tracker.path);
    let mut owners: Vec<&Owner> = tracker.owners.values().collect();
    owners.sort_by(|a, b| b.lines().cmp(&a.lines()));

    for owner in owners {
        println!("  {}", owner);
    }

    Ok(())
}
