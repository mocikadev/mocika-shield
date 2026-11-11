//! 执行器内部按任务种类复核来源；兼容旧证明结构，不放宽加固的固定材料集合。
use super::*;
use crate::automation::{
    plan, sign_plan, snapshot,
    task_materials::{MaterialBundle, MaterialError},
};

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum Sources {
    Protect(Box<plan::Sources>),
    Sign(Box<sign_plan::Sources>),
}
impl Sources {
    pub fn matches(&self, request: &Request) -> Result<(), CheckError> {
        match (self, &request.sign) {
            (Self::Sign(sources), Some(sign)) if request.shared.is_none() => {
                sources.matches(sign, request.config.is_some())
            }
            (Self::Protect(sources), None) => {
                match (&sources.shared, &request.shared) {
                    (Some(identity), Some(shared)) => identity.matches(shared)?,
                    (None, None) => {}
                    _ => return Err(error("SOURCE_PROOF_INVALID")),
                }
                if sources.config.is_some() != request.config.is_some() {
                    return Err(error("SOURCE_PROOF_INVALID"));
                }
                protect_refs(sources)?;
                Ok(())
            }
            _ => Err(error("SOURCE_PROOF_INVALID")),
        }
    }
    pub fn bind_request(&self, request: &mut Request) {
        match self {
            Self::Protect(sources) => {
                if let (Some(shared), Some(identity)) = (&mut request.shared, &sources.shared) {
                    shared.certificate_id = None;
                    shared.use_default_certificate = false;
                    shared.runtime_profile = None;
                    shared.environment_policy = None;
                    shared.binding = Some(identity.binding.clone());
                }
            }
            Self::Sign(sources) => {
                if let Some(sign) = &mut request.sign {
                    sign.certificate_id = None;
                    sign.binding = Some(sources.binding.clone());
                }
            }
        }
    }
    pub fn shared_binding(&self) -> Result<Option<String>, CheckError> {
        match self {
            Self::Protect(sources) => sources
                .shared
                .as_ref()
                .map(|id| id.binding.encode())
                .transpose(),
            Self::Sign(_) => Ok(None),
        }
    }
    pub fn content(&self) -> Option<[u8; 32]> {
        match self {
            Self::Sign(s) => Some(s.content_sha256),
            Self::Protect(_) => None,
        }
    }
    pub fn tools(&self) -> (&snapshot::FileSnapshot, &snapshot::FileSnapshot) {
        match self {
            Self::Protect(s) => (&s.java, &s.keytool),
            Self::Sign(s) => (&s.java, &s.keytool),
        }
    }
    pub fn recheck(&self, deadline: Instant, cancel: &AtomicBool) -> Result<(), CheckError> {
        match self {
            Self::Protect(s) => {
                snapshot::recheck_checked(&protect_refs(s)?, &mut || checkpoint(deadline, cancel))?;
                if let Some(binding) = &s.java_adapter {
                    binding.recheck(&mut || checkpoint(deadline, cancel))?;
                }
                if let Some(shared) = &s.shared {
                    shared.recheck()?;
                }
                checkpoint(deadline, cancel)
            }
            Self::Sign(s) => {
                snapshot::recheck_checked(&s.files(), &mut || checkpoint(deadline, cancel))?;
                if let Some(binding) = &s.java_adapter {
                    binding.recheck(&mut || checkpoint(deadline, cancel))?;
                }
                s.recheck_record()?;
                checkpoint(deadline, cancel)
            }
        }
    }
    pub fn verify_materials(
        &self,
        root: &Path,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Result<(), CheckError> {
        let check = || checkpoint(deadline, cancel).map_err(|_| MaterialError::Interrupted);
        let verified = match self {
            Self::Protect(s) if s.java_adapter.is_some() => MaterialBundle::verify_sources_version(
                root,
                &s.materials(),
                s.material_version(),
                check,
            ),
            Self::Protect(s) if s.shared.is_some() => {
                MaterialBundle::verify_shared_sources(root, &s.materials(), check)
            }
            Self::Protect(s) => MaterialBundle::verify_sources(root, &s.materials(), check),
            Self::Sign(s) => MaterialBundle::verify_sources_version(
                root,
                &s.materials(),
                s.material_version(),
                check,
            ),
        };
        verified.map_err(|_| error("MATERIAL_INVALID"))?;
        checkpoint(deadline, cancel)
    }
    pub fn verify_content(
        &self,
        path: &Path,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Result<(), CheckError> {
        if let Self::Sign(s) = self {
            let actual = shield_core::inspect_signing_input(path, || {
                checkpoint(deadline, cancel).map_err(|e| anyhow::anyhow!(e.code))
            })
            .map_err(|_| error("ARTIFACT_INVALID"))?;
            if actual.content_sha256 != s.content_sha256
                || actual.certificate_binding != s.certificate_binding
            {
                return Err(error("ARTIFACT_INVALID"));
            }
        }
        checkpoint(deadline, cancel)
    }
}

fn protect_refs(sources: &plan::Sources) -> Result<Vec<&snapshot::FileSnapshot>, CheckError> {
    crate::automation::adapter::require(sources.java_adapter.as_ref())?;
    if sources.config.is_none() && sources.shared.is_none() {
        return Err(error("SOURCE_PROOF_INVALID"));
    }
    let values = [
        &sources.input,
        &sources.keystore,
        &sources.java,
        &sources.keytool,
        &sources.apktool,
        &sources.apksigner,
        &sources.resources,
    ];
    for (source, expected) in values.iter().zip([
        "input",
        "keystore",
        "java",
        "keytool",
        "apktool",
        "apksigner",
        "resources",
    ]) {
        if source.role != expected {
            return Err(error("SOURCE_PROOF_INVALID"));
        }
    }
    if sources.config.as_ref().is_some_and(|c| c.role != "config") {
        return Err(error("SOURCE_PROOF_INVALID"));
    }
    Ok(sources.files())
}
