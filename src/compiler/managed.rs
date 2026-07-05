use crate::MANAGED_BEGIN;
use crate::MANAGED_END;
use std::sync::LazyLock;

static BEGIN_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"<!-- ctx:begin[^>]*-->").unwrap());

pub fn wrap_managed(id: &str, hash: &str, body: &str) -> String {
    format!("{MANAGED_BEGIN} id={id}, hash={hash} -->\n{body}\n{MANAGED_END}\n")
}

pub fn split_managed(content: &str) -> (String, String) {
    let end_marker = "<!-- ctx:end -->";
    let mut managed = String::new();
    let mut human = String::new();
    let mut rest = content;
    while let Some(start) = BEGIN_RE.find(rest) {
        human.push_str(&rest[..start.start()]);
        rest = &rest[start.end()..];
        if let Some(end) = rest.find(end_marker) {
            managed.push_str(rest[..end].trim());
            managed.push('\n');
            rest = &rest[end + end_marker.len()..];
        } else {
            break;
        }
    }
    human.push_str(rest);
    (managed, human)
}

pub fn managed_digest(content: &str) -> String {
    let (managed, _) = split_managed(content);
    crate::lockfile::digest_content(managed.trim())
}

pub fn merge_into_file(existing: &str, managed_block: &str) -> String {
    if let Some(start) = BEGIN_RE.find(existing) {
        let before = &existing[..start.start()];
        let rest = &existing[start.end()..];
        if let Some(end) = rest.find(MANAGED_END) {
            let after = &rest[end + MANAGED_END.len()..];
            return format!("{before}{managed_block}{after}");
        }
    }
    format!("{existing}\n\n{managed_block}")
}

pub fn block_hash(body: &str) -> String {
    crate::lockfile::digest_content(body)[..8].to_string()
}
