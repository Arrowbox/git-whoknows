use nom::character::complete::{digit1, hex_digit1, newline, space0, space1};

use anyhow::Result;
use chrono::offset::FixedOffset;
use chrono::{DateTime, NaiveDateTime, TimeZone};

use std::path::Path;
use std::process::Command;

pub fn generate_blame(path: &Path) -> Result<String> {
    let output = Command::new("git")
        .current_dir(path.parent().unwrap())
        .args(&["blame", "--porcelain", "--", &path.to_str().unwrap()])
        .output()
        .expect("Failure to run blame command.");
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn parse_blame(txt: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut txt = txt;
    while txt != "" {
        let (i, line) = parse_line(txt).expect("Failure to parse line");
        lines.push(line);
        txt = i;
    }
    lines
}

fn is_newline(c: char) -> bool {
    c == '\n'
}

#[derive(Debug, PartialEq)]
pub struct HeaderExtra<'a> {
    pub author: &'a str,
    pub author_mail: &'a str,
    pub author_time: DateTime<FixedOffset>,
    pub committer: &'a str,
    pub committer_mail: &'a str,
    pub committer_time: DateTime<FixedOffset>,
    pub summary: &'a str,
    pub boundary: Option<bool>,
    pub previous: Option<&'a str>,
    pub filename: &'a str,
}

named!(parse_header_extra <&str, HeaderExtra>,
       do_parse!(
           author: delimited!(tag!("author "), take_till!(is_newline), tag!("\n")) >>
           author_mail: delimited!(tag!("author-mail "), take_till!(is_newline), tag!("\n")) >>
           author_time: delimited!(tag!("author-time "), take_till!(is_newline), tag!("\n")) >>
           author_tz: delimited!(tag!("author-tz "), take_till!(is_newline), tag!("\n")) >>
           committer: delimited!(tag!("committer "), take_till!(is_newline), tag!("\n")) >>
           committer_mail: delimited!(tag!("committer-mail "), take_till!(is_newline), tag!("\n")) >>
           committer_time: delimited!(tag!("committer-time "), take_till!(is_newline), tag!("\n")) >>
           committer_tz: delimited!(tag!("committer-tz "), take_till!(is_newline), tag!("\n")) >>
           summary: delimited!(tag!("summary "), take_till!(is_newline), tag!("\n")) >>
           boundary: opt!(terminated!(tag!("boundary"), newline)) >>
           previous: opt!(delimited!(tag!("previous "), take_till!(is_newline), tag!("\n"))) >>
           filename: delimited!(tag!("filename "), take_till!(is_newline), tag!("\n")) >>
           (
               {
                   let author_time = i64::from_str_radix(author_time, 10).expect("Failure to convert author time to integer.");
                   let author_tz = i32::from_str_radix(author_tz, 10).expect("Failure to convert author tz to integer.");
                   let author_time = DateTime::<FixedOffset>::from_utc(NaiveDateTime::from_timestamp(author_time, 0), TimeZone::from_offset(&FixedOffset::east(author_tz)));
                   let committer_time = i64::from_str_radix(committer_time, 10).expect("Failure to convert committer time to integer.");
                   let committer_tz = i32::from_str_radix(committer_tz, 10).expect("Failure to convert committer tz to integer.");
                   let committer_time = DateTime::<FixedOffset>::from_utc(NaiveDateTime::from_timestamp(committer_time, 0), TimeZone::from_offset(&FixedOffset::east(committer_tz)));
                   let boundary = boundary.map(|b| b == "boundary");
                   HeaderExtra {
                       author,
                       author_mail,
                       author_time,
                       committer,
                       committer_mail,
                       committer_time,
                       summary,
                       boundary,
                       previous,
                       filename
                   }
               }
           ))
       );

#[derive(Debug, PartialEq)]
pub struct Header<'a> {
    pub hash: &'a str,
    pub line_num_orig: usize,
    pub line_num_final: usize,
    pub num_lines_in_group: Option<usize>,
    pub extra: Option<HeaderExtra<'a>>,
}

named!(parse_header <&str, Header>,
       do_parse!(
           hash: hex_digit1 >>
           space1 >>
           line_num_orig: digit1 >>
           space1 >>
           line_num_final: digit1 >>
           space0 >>
           num_lines_in_group: opt!(digit1) >>
           newline >>
           extra: opt!(parse_header_extra) >>
           (
               Header {
                   hash: hash,
                   line_num_orig: usize::from_str_radix(line_num_orig, 10).expect("failure to parse original line number"),
                   line_num_final: usize::from_str_radix(line_num_final, 10).expect("failure to parse final line number"),
                   num_lines_in_group: num_lines_in_group.map(|x| usize::from_str_radix(x, 10).expect("Failure to parse number of lines in group")),
                   extra: extra,
               }
           ))
       );

#[derive(Debug, PartialEq)]
pub struct Line<'a> {
    pub header: Header<'a>,
    pub line: &'a str,
}

named!(parse_line <&str, Line>,
       do_parse!(
           header: parse_header >>
           line: delimited!(tag!("\t"), take_till!(is_newline), newline) >>
           (
               Line {
                   header,
                   line
               }
            ))
       );

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_header_extra() {
        let input = r#"author Brandon Edens
author-mail <brandonedens@gmail.com>
author-time 1586576941
author-tz -0700
committer Brandon Edens
committer-mail <brandonedens@gmail.com>
committer-time 1586577179
committer-tz -0700
summary Switch to anyhow and modify main to return Result.
previous dbdf0caee4e14c03e5c3b8c7575219b3affe5657 src/main.rs
filename src/main.rs
"#;
        assert_eq!(
            parse_header_extra(input),
            Ok((
                "",
                HeaderExtra {
                    author: "Brandon Edens",
                    author_mail: "<brandonedens@gmail.com>",
                    author_time: DateTime::<FixedOffset>::from_utc(
                        NaiveDateTime::from_timestamp(1586576941, 0),
                        TimeZone::from_offset(&FixedOffset::east(-0700))
                    ),
                    committer: "Brandon Edens",
                    committer_mail: "<brandonedens@gmail.com>",
                    committer_time: DateTime::<FixedOffset>::from_utc(
                        NaiveDateTime::from_timestamp(1586577179, 0),
                        TimeZone::from_offset(&FixedOffset::east(-0700))
                    ),
                    summary: "Switch to anyhow and modify main to return Result.",
                    filename: "src/main.rs",
                    previous: Some("dbdf0caee4e14c03e5c3b8c7575219b3affe5657 src/main.rs"),
                }
            ))
        );
    }

    #[test]
    fn test_abridged_line() {
        let input = r#"dbdf0caee4e14c03e5c3b8c7575219b3affe5657 42 54
	.add_hunk(commit);
"#;
        assert_eq!(
            parse_line(input),
            Ok((
                "",
                Line {
                    header: Header {
                        hash: "dbdf0caee4e14c03e5c3b8c7575219b3affe5657",
                        line_num_orig: 42,
                        line_num_final: 54,
                        num_lines_in_group: None,
                        extra: None,
                    },
                    line: ".add_hunk(commit);",
                }
            ))
        );
    }
}
