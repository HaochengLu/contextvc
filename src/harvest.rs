use crate::distill::distill_session;
use crate::object::{load_all_objects, KnowledgeObject};
use crate::paths::ContextPaths;
use anyhow::Result;
use std::collections::{HashMap, HashSet};

pub fn harvest(paths: &ContextPaths) -> Result<HarvestReport> {
    let distilled = distill_session(paths, None)?;
    let merged = semantic_merge(paths)?;
    Ok(HarvestReport {
        proposals_created: distilled,
        merge_updates: merged.updated,
        conflicted: merged.conflicted,
    })
}

#[derive(Debug, serde::Serialize)]
pub struct HarvestReport {
    pub proposals_created: usize,
    pub merge_updates: usize,
    pub conflicted: Vec<String>,
}

pub fn semantic_merge(paths: &ContextPaths) -> Result<MergeReport> {
    let _lock = crate::lockfile::OperationLock::acquire(paths)?;
    let mut objects = load_all_objects(&paths.objects)?;
    let conflicted = find_conflicts(&objects);
    let conflicted: HashSet<_> = conflicted.into_iter().collect();
    let mut updated = 0usize;

    for obj in &mut objects {
        if !conflicted.contains(&obj.frontmatter.id) || obj.frontmatter.status == "conflicted" {
            continue;
        }
        obj.frontmatter.status = "conflicted".into();
        obj.save()?;
        updated += 1;
    }

    let mut conflicted: Vec<_> = conflicted.into_iter().collect();
    conflicted.sort();

    Ok(MergeReport {
        updated,
        conflicted,
    })
}

#[derive(Debug, serde::Serialize)]
pub struct MergeReport {
    pub updated: usize,
    pub conflicted: Vec<String>,
}

fn find_conflicts(objects: &[KnowledgeObject]) -> Vec<String> {
    let mut by_scope: HashMap<String, Vec<&KnowledgeObject>> = HashMap::new();
    for obj in objects {
        if obj.frontmatter.status != "active" && obj.frontmatter.status != "conflicted" {
            continue;
        }
        let scopes = if obj.frontmatter.scope.is_empty() {
            vec!["**".to_string()]
        } else {
            obj.frontmatter.scope.clone()
        };
        for scope in scopes {
            by_scope.entry(scope).or_default().push(obj);
        }
    }

    let mut conflicted = HashSet::new();
    for (_scope, group) in by_scope {
        let constraints: Vec<_> = group
            .iter()
            .filter(|o| o.type_enum() == Some(crate::object::ObjectType::Constraint))
            .collect();
        for i in 0..constraints.len() {
            for j in (i + 1)..constraints.len() {
                if constraints_conflict(constraints[i], constraints[j]) {
                    conflicted.insert(constraints[i].frontmatter.id.clone());
                    conflicted.insert(constraints[j].frontmatter.id.clone());
                }
            }
        }

        let failures: Vec<_> = group
            .iter()
            .filter(|o| o.type_enum() == Some(crate::object::ObjectType::Failure))
            .collect();
        if failures.is_empty() {
            continue;
        }
        for decision in group
            .iter()
            .filter(|o| o.type_enum() == Some(crate::object::ObjectType::Decision))
        {
            if failures
                .iter()
                .any(|failure| decision_conflicts_with_failure(decision, failure))
            {
                conflicted.insert(decision.frontmatter.id.clone());
            }
        }
    }
    let mut out: Vec<_> = conflicted.into_iter().collect();
    out.sort();
    out
}

fn constraints_conflict(a: &KnowledgeObject, b: &KnowledgeObject) -> bool {
    if explicitly_linked(a, b) && polarity(a) != polarity(b) {
        return true;
    }
    for a_binding in &a.frontmatter.bindings {
        for b_binding in &b.frontmatter.bindings {
            if !matches!(
                a_binding.kind.as_str(),
                "command" | "file" | "source" | "symbol"
            ) || a_binding.kind != b_binding.kind
            {
                continue;
            }
            let same_key = match a_binding.kind.as_str() {
                "command" => a_binding.pattern == b_binding.pattern,
                "file" | "source" => a_binding.path == b_binding.path,
                "symbol" => a_binding.name == b_binding.name,
                _ => false,
            };
            if same_key && polarity(a) != polarity(b) {
                return true;
            }
        }
    }
    false
}

fn decision_conflicts_with_failure(decision: &KnowledgeObject, failure: &KnowledgeObject) -> bool {
    decision
        .frontmatter
        .evidence
        .iter()
        .any(|ev| ev == &failure.frontmatter.id)
        || decision.frontmatter.supersedes.as_deref() == Some(&failure.frontmatter.id)
        || decision.body.contains(&failure.frontmatter.id)
}

fn explicitly_linked(a: &KnowledgeObject, b: &KnowledgeObject) -> bool {
    a.frontmatter.supersedes.as_deref() == Some(&b.frontmatter.id)
        || b.frontmatter.supersedes.as_deref() == Some(&a.frontmatter.id)
        || a.frontmatter
            .evidence
            .iter()
            .any(|ev| ev == &b.frontmatter.id)
        || b.frontmatter
            .evidence
            .iter()
            .any(|ev| ev == &a.frontmatter.id)
        || a.body.contains(&b.frontmatter.id)
        || b.body.contains(&a.frontmatter.id)
}

fn polarity(obj: &KnowledgeObject) -> i8 {
    let text = format!("{} {}", obj.frontmatter.title, obj.body).to_ascii_lowercase();
    let negative = [
        "do not",
        "don't",
        "never",
        "avoid",
        "forbid",
        "forbidden",
        "ban",
        "block",
        "instead of",
    ]
    .iter()
    .any(|needle| text.contains(needle));
    let positive = [
        "must", "use ", "prefer", "require", "always", "should", "allow",
    ]
    .iter()
    .any(|needle| text.contains(needle));
    match (negative, positive) {
        (true, false) => -1,
        (false, true) => 1,
        (true, true) => -1,
        _ => 0,
    }
}
