use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug)]
pub struct UniqueMap<V>(pub BTreeMap<String, V>);

impl<V> Default for UniqueMap<V> {
    fn default() -> Self {
        Self(BTreeMap::new())
    }
}

impl<'de, V> Deserialize<'de> for UniqueMap<V>
where
    V: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct UniqueMapVisitor<V>(PhantomData<V>);

        impl<'de, V> Visitor<'de> for UniqueMapVisitor<V>
        where
            V: Deserialize<'de>,
        {
            type Value = UniqueMap<V>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a mapping with unique string keys")
            }

            fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut entries = BTreeMap::new();
                while let Some((key, value)) = access.next_entry::<String, V>()? {
                    if entries.contains_key(&key) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate declaration ID {key:?}"
                        )));
                    }
                    entries.insert(key, value);
                }
                Ok(UniqueMap(entries))
            }
        }

        deserializer.deserialize_map(UniqueMapVisitor(PhantomData))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    #[serde(default)]
    pub artifacts: UniqueMap<Artifact>,
    #[serde(default)]
    pub sources: UniqueMap<Source>,
    #[serde(default)]
    pub lineage: Vec<LineageEdge>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Unavailable,
    Unknown,
    NotApplicable,
}

impl Availability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
            Self::NotApplicable => "not_applicable",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub status: Availability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrieved_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rights: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Snapshot>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub path: String,
    pub sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Relationship {
    DerivedFrom,
}

impl Relationship {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DerivedFrom => "derived_from",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineageEdge {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub relation: Relationship,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Verified,
    Missing,
    HashMismatch,
    SizeMismatch,
    Unavailable,
    Unknown,
    NotApplicable,
    Unverified,
}

impl VerificationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Missing => "missing",
            Self::HashMismatch => "hash_mismatch",
            Self::SizeMismatch => "size_mismatch",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
            Self::NotApplicable => "not_applicable",
            Self::Unverified => "unverified",
        }
    }
}

impl fmt::Display for VerificationStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.as_str().to_ascii_uppercase())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub code: String,
    pub severity: String,
    pub entity_id: Option<String>,
    pub path: Option<String>,
    pub message: String,
}

impl Finding {
    pub fn new(
        code: &str,
        severity: &str,
        entity_id: Option<&str>,
        path: Option<&str>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.to_owned(),
            severity: severity.to_owned(),
            entity_id: entity_id.map(str::to_owned),
            path: path.map(str::to_owned),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct VerificationCheck {
    pub id: String,
    pub kind: String,
    pub status: VerificationStatus,
    pub availability: Option<Availability>,
    pub path: Option<String>,
    pub expected_sha256: Option<String>,
    pub actual_sha256: Option<String>,
    pub expected_size_bytes: Option<u64>,
    pub actual_size_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ValidationReport {
    pub command: &'static str,
    pub status: &'static str,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Serialize)]
pub struct VerificationReport {
    pub command: &'static str,
    pub status: &'static str,
    pub checks: Vec<VerificationCheck>,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LineageNode {
    pub id: String,
    pub kind: String,
    pub status: VerificationStatus,
    pub availability: Option<Availability>,
    pub path: Option<String>,
    pub locator: Option<String>,
    pub version: Option<String>,
    pub revision: Option<String>,
    pub retrieved_at: Option<String>,
    pub rights: Option<String>,
    pub cycle: bool,
    pub upstream: Vec<LineageNode>,
}

#[derive(Debug, Serialize)]
pub struct LineageReport {
    pub command: &'static str,
    pub artifact: String,
    pub status: &'static str,
    pub artifact_status: VerificationStatus,
    pub artifact_path: Option<String>,
    pub artifact_sha256: Option<String>,
    pub artifact_version: Option<String>,
    pub artifact_revision: Option<String>,
    pub upstream: Vec<LineageNode>,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Serialize)]
pub struct AuditReport {
    pub command: &'static str,
    pub status: &'static str,
    pub checks: Vec<VerificationCheck>,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Serialize)]
pub struct ErrorReport<'a> {
    pub command: &'a str,
    pub status: &'static str,
    pub error: ErrorDetail<'a>,
}

#[derive(Debug, Serialize)]
pub struct ErrorDetail<'a> {
    pub code: &'a str,
    pub message: &'a str,
}
