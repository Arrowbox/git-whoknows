use anyhow::Result;
use git2::{BlameHunk, BlameOptions, Oid, Repository, Signature};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt)]
#[allow(non_snake_case)]
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
    #[allow(dead_code)]
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

    fn add_hunk(&mut self, commit: &BlameHunk) {
        let owner = Owner::new(&commit.final_signature());
        self.owners
            .entry(owner.email.clone())
            .or_insert(owner)
            .add_hunk(commit);
    }
}

struct Owner {
    #[allow(dead_code)]
    name: String,
    email: String,
    commits: HashMap<Oid, usize>,
}

impl Owner {
    fn new(sig: &Signature) -> Owner {
        let email = String::from_utf8_lossy(sig.email_bytes()).to_string();
        let name = String::from_utf8_lossy(sig.name_bytes()).to_string();
        Owner {
            name,
            email,
            commits: HashMap::new(),
        }
    }

    fn add_hunk(&mut self, commit: &BlameHunk) {
        *self.commits.entry(commit.final_commit_id()).or_insert(0) += commit.lines_in_hunk();
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

fn main() -> Result<()> {
    let args = Args::from_args();

    //let path = Path::new(&args.arg_path[..]);
    let repo = Repository::discover(&args.arg_path)?;

    let path = args
        .arg_path
        .strip_prefix(repo.path().parent().unwrap())
        .unwrap();

    // Prepare our blame options
    let mut opts = BlameOptions::new();
    opts.track_copies_same_commit_moves(args.flag_M)
        .track_copies_same_commit_copies(args.flag_C)
        .first_parent(args.flag_F);

    let mut tracker = TrackedFile::new(&path.display().to_string());

    let blame = repo.blame_file(path, Some(&mut opts))?;

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
