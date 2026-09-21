//! Canonical platform scope addresses shared by authorization and administration.
use super::super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryVisibility {
    Private,
    Shared,
}

// Field order is part of the existing v1 namespace encoding. Preserve it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryResourceScope {
    pub project_id: String,
    pub mission_id: Option<String>,
    pub agent_id: String,
    pub visibility: MemoryVisibility,
}

pub(super) fn valid_id(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(Error::ValidationError(
            "Identity components require 1–256 UTF-8 bytes without control characters".into(),
        ));
    }
    Ok(())
}
impl MemoryResourceScope {
    pub fn validate(&self) -> Result<()> {
        valid_id(&self.project_id)?;
        valid_id(&self.agent_id)?;
        if let Some(mission) = &self.mission_id {
            valid_id(mission)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyMemoryAddress {
    pub company_id: String,
    pub scope: MemoryResourceScope,
    pub private_subject_id: Option<String>,
}
impl CompanyMemoryAddress {
    pub fn new(
        company_id: &str,
        scope: &MemoryResourceScope,
        private_subject_id: Option<&str>,
    ) -> Result<Self> {
        let address = Self {
            company_id: company_id.into(),
            scope: scope.clone(),
            private_subject_id: private_subject_id.map(str::to_owned),
        };
        address.validate()?;
        Ok(address)
    }
    pub fn validate(&self) -> Result<()> {
        valid_id(&self.company_id)?;
        self.scope.validate()?;
        match (&self.scope.visibility, &self.private_subject_id) {
            (MemoryVisibility::Private, Some(subject)) => valid_id(subject),
            (MemoryVisibility::Shared, None) => Ok(()),
            _ => Err(Error::ValidationError(
                "Private scopes require a subject; shared scopes cannot select one".into(),
            )),
        }
    }
    pub fn namespace(&self) -> Result<String> {
        self.validate()?;
        let value =
            serde_json::to_string(&(&self.company_id, &self.scope, &self.private_subject_id))
                .map_err(|e| Error::Serialization(e.to_string()))?;
        Ok(format!("qdb:scope:v1:{value}"))
    }
    pub fn from_namespace(namespace: &str) -> Result<Option<Self>> {
        let Some(encoded) = namespace.strip_prefix("qdb:scope:v1:") else {
            return Ok(None);
        };
        let (company, scope, subject): (String, MemoryResourceScope, Option<String>) =
            serde_json::from_str(encoded).map_err(|_| inconsistent())?;
        let address =
            Self::new(&company, &scope, subject.as_deref()).map_err(|_| inconsistent())?;
        // Never normalize a stored address into a different physical namespace.
        if address.namespace()? != namespace {
            return Err(inconsistent());
        }
        Ok(Some(address))
    }
    pub fn workspace_id(&self) -> Result<String> {
        Ok(digest(self.namespace()?.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
}
