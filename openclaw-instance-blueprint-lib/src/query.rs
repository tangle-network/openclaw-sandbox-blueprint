//! Read-only query helpers for operator APIs.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::runtime_adapter::InstanceRuntimeAdapter;
use crate::state::InstanceRecord;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiAccessView {
    pub public_url: Option<String>,
    pub tunnel_status: String,
    pub auth_mode: String,
    pub owner_only: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeView {
    pub backend: String,
    pub image: Option<String>,
    pub container_name: Option<String>,
    pub container_id: Option<String>,
    pub container_status: Option<String>,
    pub ui_host_port: Option<u16>,
    pub ui_local_url: Option<String>,
    pub ui_auth_scheme: Option<String>,
    pub ui_auth_env_key: Option<String>,
    pub has_ui_bearer_token: bool,
    pub setup_url: Option<String>,
    pub setup_status: Option<String>,
    pub setup_command: Option<String>,
    pub setup_instructions: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceView {
    pub id: String,
    pub name: String,
    pub template_pack_id: String,
    pub claw_variant: String,
    pub execution_target: String,
    pub status: String,
    pub owner: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub ui_access: UiAccessView,
    pub runtime: RuntimeView,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TemplatePack {
    pub id: String,
    pub name: String,
    pub mode: String,
    pub description: String,
}

pub fn instance_view(record: &InstanceRecord) -> InstanceView {
    InstanceView {
        id: record.id.clone(),
        name: record.name.clone(),
        template_pack_id: record.template_pack_id.clone(),
        claw_variant: record.claw_variant.to_string(),
        execution_target: record.execution_target.to_string(),
        status: record.state.to_string(),
        owner: record.owner.clone(),
        created_at: record.created_at,
        updated_at: record.updated_at,
        ui_access: UiAccessView {
            public_url: record.ui_access.public_url.clone(),
            tunnel_status: record.ui_access.tunnel_status.to_string(),
            auth_mode: record.ui_access.auth_mode.to_string(),
            owner_only: record.ui_access.owner_only,
        },
        runtime: RuntimeView {
            backend: record.runtime.backend.clone(),
            image: record.runtime.image.clone(),
            container_name: record.runtime.container_name.clone(),
            container_id: record.runtime.container_id.clone(),
            container_status: record.runtime.container_status.clone(),
            ui_host_port: record.runtime.ui_host_port,
            ui_local_url: record.runtime.ui_local_url.clone(),
            ui_auth_scheme: record.runtime.ui_auth_scheme.clone(),
            ui_auth_env_key: record.runtime.ui_auth_env_key.clone(),
            has_ui_bearer_token: record.runtime.ui_bearer_token.is_some(),
            setup_url: record.runtime.setup_url.clone(),
            setup_status: record.runtime.setup_status.clone(),
            setup_command: record.runtime.setup_command.clone(),
            setup_instructions: record.runtime.setup_instructions.clone(),
            last_error: record.runtime.last_error.clone(),
        },
    }
}

pub fn list_instance_views(adapter: Arc<dyn InstanceRuntimeAdapter>) -> Result<Vec<InstanceView>> {
    let mut records = adapter.list_instances()?;
    let mut views = Vec::with_capacity(records.len());

    for record in records.drain(..) {
        let refreshed = adapter.refresh_instance(record.clone())?;
        if refreshed != record {
            let _ = adapter.save_instance(refreshed.clone())?;
            views.push(instance_view(&refreshed));
        } else {
            views.push(instance_view(&record));
        }
    }
    Ok(views)
}

pub fn get_instance_view(
    adapter: Arc<dyn InstanceRuntimeAdapter>,
    instance_id: &str,
) -> Result<Option<InstanceView>> {
    let Some(record) = adapter.get_instance(instance_id)? else {
        return Ok(None);
    };

    let refreshed = adapter.refresh_instance(record.clone())?;
    if refreshed != record {
        let _ = adapter.save_instance(refreshed.clone())?;
        return Ok(Some(instance_view(&refreshed)));
    }
    Ok(Some(instance_view(&record)))
}

pub fn load_template_packs() -> Result<Vec<TemplatePack>> {
    let root = std::env::var("OPENCLAW_TEMPLATES_DIR")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("config/templates"));

    let mut packs = Vec::new();
    if !root.exists() {
        return Ok(packs);
    }

    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let template_json = entry.path().join("template.json");
        if !template_json.exists() {
            continue;
        }
        let data = std::fs::read_to_string(template_json)?;
        let pack: TemplatePack = serde_json::from_str(&data)?;
        packs.push(pack);
    }

    packs.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(packs)
}
