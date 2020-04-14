# git-whoknows - Find "who knows" about a file

# Synopsis:
`git whoknows [<options>] <path>`

# Description

Describes who is likely familiar with a file

# Todo

* [] Capture most recent hunk date
* [] Capture oldest hunk date
* [] Handle submodules and their git-dirs
* [] Allow for tabular output
* [] Add weighting how the list is sorted
* [] Allow for specifying lines to increase weight for hunk
* [] Allow for custom weighting scheme
* [] Allow for more git-blame options

# Options

* `-L <lines>` - Specifically for a set of lines, can be specified multiple times
* `--no-table/table` - Format output as an ascii table or comma-delimited
* `--weight=<commits>,<lines>,<latest>,<earliest>` - Custom weightings for different metrics

# Examples

## Information about a file

```
> git whoknows src/main.rs
name, email, score, commits, lines, latest, earliest
Jayson Messenger, <jmessenger@gmail.com>, 12, 4, 10, 2020-04-10, 2019-02-01
John Smith, <jsmith@gmail.com>, 10, 2, 12, 2019-01-01, 2019-01-01
```
