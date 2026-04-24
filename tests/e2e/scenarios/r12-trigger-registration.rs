// S11-001 — Scenario R-12: Trigger registration and lookup
//
// Verifies `TriggerRegistry::register` and `find_triggers` behave correctly.
// Unit-level coverage already exists in voltnuerongrid-store; this file links
// the scenario to the E2E scenario pack and provides a standalone integration stub.
//
// Run: cargo test -p voltnuerongrid-store r12_trigger_registration

use voltnuerongrid_store::{
    TriggerDefinition, TriggerEvent, TriggerGranularity, TriggerRegistry,
};

#[test]
fn scenario_passes() {
    r12_register_and_find();
    r12_no_false_positives();
    r12_duplicate_registration_rejected();
}

/// Step 1-4: register a trigger, find it by table+schema+event.
fn r12_register_and_find() {
    let mut reg = TriggerRegistry::new();

    let trigger = TriggerDefinition::new(
        "trg_users_after_insert",
        "users",
        "public",
        TriggerEvent::AfterInsert,
        TriggerGranularity::Row,
        "EXECUTE FUNCTION notify_insert()",
    );

    reg.register(trigger).expect("register should succeed for a new trigger name");

    let found = reg.find_triggers("users", "public", &TriggerEvent::AfterInsert);
    assert!(
        found.len() == 1,
        "expected exactly 1 trigger for AfterInsert on users.public, got {}",
        found.len()
    );
    assert_eq!(found[0].name, "trg_users_after_insert");
}

/// Step 5: wrong event returns empty slice.
fn r12_no_false_positives() {
    let mut reg = TriggerRegistry::new();

    reg.register(TriggerDefinition::new(
        "trg_after_insert_only",
        "orders",
        "public",
        TriggerEvent::AfterInsert,
        TriggerGranularity::Row,
        "EXECUTE FUNCTION log_order()",
    ))
    .unwrap();

    let not_found = reg.find_triggers("orders", "public", &TriggerEvent::AfterUpdate);
    assert!(
        not_found.is_empty(),
        "AfterUpdate should not match an AfterInsert trigger"
    );
}

/// Duplicate name must be rejected.
fn r12_duplicate_registration_rejected() {
    let mut reg = TriggerRegistry::new();

    let t1 = TriggerDefinition::new(
        "trg_dup",
        "t",
        "public",
        TriggerEvent::BeforeDelete,
        TriggerGranularity::Statement,
        "EXECUTE FUNCTION on_delete()",
    );
    let t2 = TriggerDefinition::new(
        "trg_dup",
        "t2",
        "public",
        TriggerEvent::AfterDelete,
        TriggerGranularity::Row,
        "EXECUTE FUNCTION on_delete2()",
    );

    reg.register(t1).expect("first registration should succeed");
    let err = reg.register(t2);
    assert!(
        err.is_err(),
        "duplicate trigger name should return Err, got Ok"
    );
}
