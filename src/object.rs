use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ObjectType {
    Constraint,
    Decision,
    Failure,
    Howto,
    Codemap,
    Preference,
}

impl ObjectType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Constraint => "constraints",
            Self::Decision => "decisions",
            Self::Failure => "failures",
            Self::Howto => "howtos",
            Self::Codemap => "codemap",
            Self::Preference => "preferences",
        }
    }

    pub fn prefix(&self) -> char {
        match self {
            Self::Constraint => 'c',
            Self::Decision => 'd',
            Self::Failure => 'f',
            Self::Howto => 'h',
            Self::Codemap => 'm',
            Self::Preference => 'p',
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "constraint" | "constraints" => Some(Self::Constraint),
            "decision" | "decisions" => Some(Self::Decision),
            "failure" | "failures" => Some(Self::Failure),
            "howto" | "howtos" => Some(Self::Howto),
            "codemap" => Some(Self::Codemap),
            "preference" | "preferences" => Some(Self::Preference),
            _ => None,
        }
    }

    pub fn projection_weight(&self) -> u32 {
        match self {
            Self::Constraint => 100,
            Self::Decision => 80,
            Self::Preference => 40,
            Self::Howto => 30,
            Self::Codemap => 20,
            Self::Failure => 0,
        }
    }

    pub fn all() -> [ObjectType; 6] {
        [
            Self::Constraint,
            Self::Decision,
            Self::Failure,
            Self::Howto,
            Self::Codemap,
            Self::Preference,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ObjectStatus {
    Proposed,
    Active,
    Conflicted,
    Stale,
    Deprecated,
}

impl ObjectStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Active => "active",
            Self::Conflicted => "conflicted",
            Self::Stale => "stale",
            Self::Deprecated => "deprecated",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Binding {
    pub kind: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub sha: Option<String>,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(default)]
    pub enforcement: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ObjectFrontmatter {
    pub id: String,
    #[serde(rename = "type")]
    pub object_type: String,
    pub title: String,
    #[serde(default)]
    pub scope: Vec<String>,
    pub status: String,
    #[serde(default = "default_trust")]
    pub trust: String,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub bindings: Vec<Binding>,
    pub created: String,
    #[serde(default)]
    pub verified: Option<String>,
    #[serde(default)]
    pub supersedes: Option<String>,
}

fn default_trust() -> String {
    "human".into()
}

fn default_confidence() -> f32 {
    1.0
}

#[derive(Debug, Clone)]
pub struct KnowledgeObject {
    pub path: PathBuf,
    pub frontmatter: ObjectFrontmatter,
    pub body: String,
}

impl KnowledgeObject {
    pub fn parse(content: &str, path: PathBuf) -> Result<Self> {
        let (front, body) = split_frontmatter(content)?;
        let frontmatter: ObjectFrontmatter = serde_yaml::from_str(front)
            .with_context(|| format!("invalid frontmatter in {}", path.display()))?;
        Ok(Self {
            path,
            frontmatter,
            body: body.trim().to_string(),
        })
    }

    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::parse(&content, path.to_path_buf())
    }

    pub fn to_markdown(&self) -> String {
        let yaml = serde_yaml::to_string(&self.frontmatter).unwrap_or_default();
        format!("---\n{yaml}---\n\n{}", self.body)
    }

    pub fn save(&self) -> Result<()> {
        let mut to_save = self.clone();
        to_save.body = crate::redact::redact_text(&to_save.body);
        to_save.frontmatter.title = crate::redact::redact_text(&to_save.frontmatter.title);
        let raw = to_save.to_markdown();
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, raw)?;
        Ok(())
    }

    pub fn type_enum(&self) -> Option<ObjectType> {
        ObjectType::from_str(&self.frontmatter.object_type)
    }

    pub fn status_enum(&self) -> ObjectStatus {
        match self.frontmatter.status.as_str() {
            "proposed" => ObjectStatus::Proposed,
            "conflicted" => ObjectStatus::Conflicted,
            "stale" => ObjectStatus::Stale,
            "deprecated" => ObjectStatus::Deprecated,
            _ => ObjectStatus::Active,
        }
    }

    pub fn is_projectable(&self) -> bool {
        matches!(self.status_enum(), ObjectStatus::Active)
            && self
                .type_enum()
                .map(|t| t.projection_weight() > 0)
                .unwrap_or(false)
    }
}

pub fn split_frontmatter(content: &str) -> Result<(&str, &str)> {
    let content = content.strip_prefix("---").context("missing frontmatter")?;
    let end = content.find("\n---").context("unclosed frontmatter")?;
    let front = &content[..end];
    let body = &content[end + 4..];
    Ok((front, body))
}

pub fn hash_id(object_type: ObjectType, seed: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    let digest = hex::encode(hasher.finalize());
    format!("{}-{}", object_type.prefix(), &digest[..8])
}

pub fn slugify(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn new_object(
    object_type: ObjectType,
    title: &str,
    body: &str,
    scope: Vec<String>,
    status: &str,
) -> KnowledgeObject {
    let id = hash_id(object_type, &(title.to_owned() + body));
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let filename = format!("{}-{}.md", slugify(title), &id[2..]);
    let path = PathBuf::from(filename);
    KnowledgeObject {
        path,
        frontmatter: ObjectFrontmatter {
            id,
            object_type: object_type.as_str().trim_end_matches('s').to_string(),
            title: title.to_string(),
            scope,
            status: status.to_string(),
            trust: "human".into(),
            confidence: 1.0,
            evidence: vec![],
            bindings: vec![],
            created: today.clone(),
            verified: Some(today),
            supersedes: None,
        },
        body: body.to_string(),
    }
}

pub fn load_all_objects(objects_root: &Path) -> Result<Vec<KnowledgeObject>> {
    let mut objects = Vec::new();
    if !objects_root.exists() {
        return Ok(objects);
    }
    for entry in WalkDir::new(objects_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
    {
        objects.push(KnowledgeObject::load(entry.path())?);
    }
    objects.sort_by(|a, b| a.frontmatter.id.cmp(&b.frontmatter.id));
    Ok(objects)
}

pub fn load_proposals(proposals_root: &Path) -> Result<Vec<KnowledgeObject>> {
    load_all_objects(proposals_root)
}

pub fn objects_digest(objects: &[KnowledgeObject]) -> String {
    let mut hasher = Sha256::new();
    for obj in objects {
        hasher.update(obj.frontmatter.id.as_bytes());
        hasher.update(obj.frontmatter.status.as_bytes());
        hasher.update(obj.to_markdown().as_bytes());
    }
    hex::encode(hasher.finalize())
}

pub fn object_frontmatter_schema_json() -> Result<String> {
    let mut schema = serde_json::to_value(schemars::schema_for!(ObjectFrontmatter))?;
    if let Some(map) = schema.as_object_mut() {
        map.remove("$schema");
    }
    if let Some(props) = schema.get_mut("properties").and_then(|v| v.as_object_mut()) {
        props.insert(
            "type".into(),
            serde_json::json!({
                "type": "string",
                "enum": ["constraint", "decision", "failure", "howto", "codemap", "preference"]
            }),
        );
        props.insert(
            "status".into(),
            serde_json::json!({
                "type": "string",
                "enum": ["proposed", "active", "conflicted", "stale", "deprecated"]
            }),
        );
        props.insert(
            "trust".into(),
            serde_json::json!({
                "type": "string",
                "default": "human",
                "enum": ["human", "agent_auto", "agent_verified"]
            }),
        );
        props.insert(
            "confidence".into(),
            serde_json::json!({
                "type": "number",
                "format": "float",
                "default": 1.0,
                "minimum": 0.0,
                "maximum": 1.0
            }),
        );
        props.insert(
            "id".into(),
            serde_json::json!({
                "type": "string",
                "pattern": "^[cdfhmp]-[a-f0-9]{8,}$"
            }),
        );
    }
    if let Some(binding_props) = schema
        .pointer_mut("/$defs/Binding/properties")
        .and_then(|v| v.as_object_mut())
    {
        binding_props.insert(
            "kind".into(),
            serde_json::json!({
                "type": "string",
                "enum": ["file", "source", "command", "dep", "symbol"]
            }),
        );
        binding_props.insert(
            "enforcement".into(),
            serde_json::json!({
                "type": ["string", "null"],
                "default": null,
                "enum": ["warn", "ask", "block", null]
            }),
        );
    }
    Ok(serde_json::to_string_pretty(&schema)?)
}

pub fn validate_object(obj: &KnowledgeObject) -> Vec<String> {
    let mut errors = Vec::new();
    let Some(object_type) = obj.type_enum() else {
        errors.push(format!(
            "{}: invalid type `{}`",
            obj.path.display(),
            obj.frontmatter.object_type
        ));
        return errors;
    };
    if !matches!(
        obj.frontmatter.status.as_str(),
        "proposed" | "active" | "conflicted" | "stale" | "deprecated"
    ) {
        errors.push(format!(
            "{}: invalid status `{}`",
            obj.path.display(),
            obj.frontmatter.status
        ));
    }
    let expected_prefix = format!("{}-", object_type.prefix());
    if !obj.frontmatter.id.starts_with(&expected_prefix) {
        errors.push(format!(
            "{}: id `{}` does not match type prefix `{}`",
            obj.path.display(),
            obj.frontmatter.id,
            expected_prefix
        ));
    }
    if obj.frontmatter.title.trim().is_empty() {
        errors.push(format!("{}: title is required", obj.path.display()));
    }
    if !(0.0..=1.0).contains(&obj.frontmatter.confidence) {
        errors.push(format!(
            "{}: confidence must be between 0.0 and 1.0",
            obj.path.display()
        ));
    }
    if !matches!(
        obj.frontmatter.trust.as_str(),
        "human" | "agent_auto" | "agent_verified"
    ) {
        errors.push(format!(
            "{}: invalid trust `{}`",
            obj.path.display(),
            obj.frontmatter.trust
        ));
    }
    for scope in &obj.frontmatter.scope {
        if scope.trim().is_empty() {
            errors.push(format!(
                "{}: scope entries cannot be empty",
                obj.path.display()
            ));
        }
    }
    for binding in &obj.frontmatter.bindings {
        if !matches!(
            binding.kind.as_str(),
            "file" | "source" | "command" | "dep" | "symbol"
        ) {
            errors.push(format!(
                "{}: invalid binding kind `{}`",
                obj.path.display(),
                binding.kind
            ));
        }
        if let Some(enforcement) = &binding.enforcement {
            if !matches!(enforcement.as_str(), "warn" | "ask" | "block") {
                errors.push(format!(
                    "{}: invalid binding enforcement `{}`",
                    obj.path.display(),
                    enforcement
                ));
            }
        }
    }
    errors
}

pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub fn parse_date(s: &str) -> Option<DateTime<Utc>> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
}
