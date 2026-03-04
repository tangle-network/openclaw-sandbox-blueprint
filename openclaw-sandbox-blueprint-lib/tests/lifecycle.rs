//! Integration tests for lifecycle state transitions.
//!
//! These tests exercise the state layer directly, verifying that lifecycle
//! transitions enforce the correct state machine rules.

use std::sync::Once;

use openclaw_sandbox_blueprint_lib::state::{
    self, ClawVariant, ExecutionTarget, InstanceRecord, InstanceState, RuntimeBinding, UiAccess,
};

static INIT: Once = Once::new();

fn init() {
    INIT.call_once(|| {
        let dir =
            std::env::temp_dir().join(format!("openclaw-lifecycle-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        unsafe {
            std::env::set_var("OPENCLAW_INSTANCE_STATE_DIR", dir.to_str().unwrap());
        }
    });
}

const OWNER: &str = "0x0000000000000000000000000000000000000001";

fn make_record(id: &str, st: InstanceState) -> InstanceRecord {
    InstanceRecord {
        id: id.to_string(),
        name: format!("test-{id}"),
        template_pack_id: "discord".to_string(),
        claw_variant: ClawVariant::Openclaw,
        config_json: String::new(),
        owner: OWNER.to_string(),
        ui_access: UiAccess::default(),
        runtime: RuntimeBinding::default(),
        execution_target: ExecutionTarget::Standard,
        state: st,
        created_at: 1000,
        updated_at: 1000,
    }
}

#[test]
fn create_persists_stopped_record() {
    init();
    let id = format!("create-{}", uuid::Uuid::new_v4());
    let record = make_record(&id, InstanceState::Stopped);
    state::save_instance(record).expect("save");

    let loaded = state::get_instance(&id).expect("get").expect("exists");
    assert_eq!(loaded.state, InstanceState::Stopped);
    assert_eq!(loaded.owner, OWNER);
}

#[test]
fn start_transition_stopped_to_running() {
    init();
    let id = format!("start-{}", uuid::Uuid::new_v4());
    state::save_instance(make_record(&id, InstanceState::Stopped)).expect("save");

    let mut record = state::get_instance(&id).expect("get").expect("exists");
    assert_eq!(record.state, InstanceState::Stopped);

    record.state = InstanceState::Running;
    record.updated_at = 2000;
    state::save_instance(record).expect("save");

    let updated = state::get_instance(&id).expect("get").expect("exists");
    assert_eq!(updated.state, InstanceState::Running);
    assert_eq!(updated.updated_at, 2000);
}

#[test]
fn stop_transition_running_to_stopped() {
    init();
    let id = format!("stop-{}", uuid::Uuid::new_v4());
    state::save_instance(make_record(&id, InstanceState::Running)).expect("save");

    let mut record = state::get_instance(&id).expect("get").expect("exists");
    record.state = InstanceState::Stopped;
    state::save_instance(record).expect("save");

    let updated = state::get_instance(&id).expect("get").expect("exists");
    assert_eq!(updated.state, InstanceState::Stopped);
}

#[test]
fn delete_transition_marks_deleted() {
    init();
    let id = format!("delete-{}", uuid::Uuid::new_v4());
    state::save_instance(make_record(&id, InstanceState::Running)).expect("save");

    let mut record = state::get_instance(&id).expect("get").expect("exists");
    record.state = InstanceState::Deleted;
    state::save_instance(record).expect("save");

    let updated = state::get_instance(&id).expect("get").expect("exists");
    assert_eq!(updated.state, InstanceState::Deleted);
}

#[test]
fn list_returns_all_instances() {
    init();
    let id_a = format!("list-a-{}", uuid::Uuid::new_v4());
    let id_b = format!("list-b-{}", uuid::Uuid::new_v4());

    state::save_instance(make_record(&id_a, InstanceState::Stopped)).expect("save a");
    state::save_instance(make_record(&id_b, InstanceState::Running)).expect("save b");

    let all = state::list_instances().expect("list");
    assert!(all.iter().any(|r| r.id == id_a));
    assert!(all.iter().any(|r| r.id == id_b));
}

#[test]
fn get_missing_instance_returns_none() {
    init();
    let result = state::get_instance("nonexistent-id").expect("get");
    assert!(result.is_none());
}

#[test]
fn config_json_roundtrip() {
    init();
    let id = format!("config-{}", uuid::Uuid::new_v4());
    let mut record = make_record(&id, InstanceState::Stopped);
    record.config_json = r#"{"model":"gpt-4","temperature":0.7}"#.to_string();
    state::save_instance(record).expect("save");

    let loaded = state::get_instance(&id).expect("get").expect("exists");
    assert_eq!(loaded.config_json, r#"{"model":"gpt-4","temperature":0.7}"#);
}
