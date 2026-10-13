use crate::error::CoreError;

/// Pads a numeric job id to at least three digits.
pub fn normalize_id(raw: &str) -> Result<String, CoreError> {
    if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_digit()) {
        return Err(CoreError::BadId(raw.to_string()));
    }
    let n: u32 = raw.parse().map_err(|_| CoreError::BadId(raw.to_string()))?;
    Ok(format!("{n:03}"))
}

/// Lowercases, joins alphanumeric runs with dashes and caps the result at 40 characters.
pub fn slugify(text: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in text.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('-');
            }
            gap = false;
            out.push(c);
        } else {
            gap = true;
        }
    }
    let cut: String = out.chars().take(40).collect();
    let cut = cut.trim_matches('-').to_string();
    if cut.is_empty() {
        "job".to_string()
    } else {
        cut
    }
}

pub fn job_dir_name(date: &str, id: &str, slug: &str) -> String {
    format!("{date}-{id}-{slug}")
}

fn leading_number(text: &str) -> Option<u32> {
    let digits: String = text.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// Reads the id out of a `YYYY-MM-DD-NNN-slug` directory name.
fn id_from_dir(name: &str) -> Option<u32> {
    let mut parts = name.splitn(4, '-');
    let (y, m, d, rest) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    let date_ok = y.len() == 4 && m.len() == 2 && d.len() == 2;
    if !date_ok || !(y.chars().chain(m.chars()).chain(d.chars())).all(|c| c.is_ascii_digit()) {
        return None;
    }
    leading_number(rest)
}

/// Reads the id out of a `factory/NNN-slug` branch name.
fn id_from_branch(name: &str) -> Option<u32> {
    leading_number(name.strip_prefix("factory/")?)
}

/// The next id is one more than the largest id seen on the board, in job directories or in branches.
pub fn next_job_id(board_ids: &[String], dir_names: &[String], branch_names: &[String]) -> String {
    let from_board = board_ids.iter().filter_map(|i| leading_number(i));
    let from_dirs = dir_names.iter().filter_map(|d| id_from_dir(d));
    let from_branches = branch_names.iter().filter_map(|b| id_from_branch(b));
    let max = from_board.chain(from_dirs).chain(from_branches).max().unwrap_or(0);
    format!("{:03}", max.saturating_add(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|i| i.to_string()).collect()
    }

    #[test]
    fn normalize_pads_and_rejects_non_digits() {
        assert_eq!(normalize_id("7").expect("ok"), "007");
        assert_eq!(normalize_id("1000").expect("ok"), "1000");
        assert!(normalize_id("x1").is_err());
        assert!(normalize_id("").is_err());
        assert!(normalize_id("99999999999999999999").is_err());
    }

    #[test]
    fn slugify_matches_the_python_reference() {
        let feature = "GitHub issue #20 (https://github.com/Snehal1112/rocket/issues/20)";
        assert_eq!(slugify(feature), "github-issue-20-https-github-com-snehal1");
        assert_eq!(slugify("  !!  "), "job");
        assert_eq!(slugify("Ünï cödé"), "n-c-d");
    }

    #[test]
    fn dir_name_has_date_id_slug() {
        assert_eq!(job_dir_name("2026-10-13", "001", "x"), "2026-10-13-001-x");
    }

    #[test]
    fn next_id_takes_the_max_of_all_three_sources() {
        assert_eq!(next_job_id(&[], &[], &[]), "001");
        let next = next_job_id(
            &s(&["001", "002"]),
            &s(&["2026-10-13-004-foo", "notes", "2026-10-13-oops"]),
            &s(&["factory/003-bar", "main", "factory/x-y"]),
        );
        assert_eq!(next, "005");
        assert_eq!(next_job_id(&s(&["999"]), &[], &[]), "1000");
    }
}
