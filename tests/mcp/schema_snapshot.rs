//! Schema contract stability tests.
//!
//! These tests catch accidental renames, removals, or type changes in the
//! MCP tool and resource surfaces by asserting against hardcoded expected
//! values.

use multorum::mcp::dto::ToolInputType;

// ---------------------------------------------------------------------------
// Orchestrator tools
// ---------------------------------------------------------------------------

#[test]
fn orchestrator_tool_names_stable() {
    let mut names: Vec<&str> =
        multorum::mcp::tool::orchestrator::descriptors().iter().map(|d| d.name).collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "ack_worker_outbox_message",
            "create_worker",
            "delete_worker",
            "discard_worker",
            "forward_perspective",
            "get_status",
            "get_worker",
            "list_perspectives",
            "list_workers",
            "merge_worker",
            "read_worker_outbox",
            "resolve_worker",
            "revise_worker",
            "rulebook_init",
            "rulebook_install",
            "rulebook_uninstall",
            "rulebook_validate",
        ]
    );
}

#[test]
fn orchestrator_tool_input_schemas_stable() {
    let descriptors = multorum::mcp::tool::orchestrator::descriptors();
    let schemas: Vec<(&str, Vec<(&str, ToolInputType, bool)>)> = descriptors
        .iter()
        .map(|d| {
            let inputs: Vec<_> = d.inputs.iter().map(|i| (i.name, i.kind, i.required)).collect();
            (d.name, inputs)
        })
        .collect();

    // No-input tools.
    for name in [
        "rulebook_init",
        "rulebook_validate",
        "rulebook_install",
        "rulebook_uninstall",
        "list_perspectives",
        "list_workers",
        "get_status",
    ] {
        let (_, inputs) = schemas.iter().find(|(n, _)| *n == name).unwrap();
        assert!(inputs.is_empty(), "{name} should have no inputs");
    }

    // get_worker
    let (_, inputs) = schemas.iter().find(|(n, _)| *n == "get_worker").unwrap();
    assert_eq!(inputs, &[("worker_id", ToolInputType::String, true)]);

    // read_worker_outbox
    let (_, inputs) = schemas.iter().find(|(n, _)| *n == "read_worker_outbox").unwrap();
    assert_eq!(
        inputs,
        &[("worker_id", ToolInputType::String, true), ("after", ToolInputType::Integer, false),]
    );

    // ack_worker_outbox_message
    let (_, inputs) = schemas.iter().find(|(n, _)| *n == "ack_worker_outbox_message").unwrap();
    assert_eq!(
        inputs,
        &[("worker_id", ToolInputType::String, true), ("sequence", ToolInputType::Integer, true),]
    );

    // create_worker
    let (_, inputs) = schemas.iter().find(|(n, _)| *n == "create_worker").unwrap();
    assert_eq!(
        inputs,
        &[
            ("perspective", ToolInputType::String, true),
            ("worker_id", ToolInputType::String, false),
            ("overwriting_worktree", ToolInputType::Boolean, false),
            ("body", ToolInputType::String, false),
            ("artifacts", ToolInputType::StringList, false),
        ]
    );

    // resolve_worker and revise_worker share the same schema.
    for name in ["resolve_worker", "revise_worker"] {
        let (_, inputs) = schemas.iter().find(|(n, _)| *n == name).unwrap();
        assert_eq!(
            inputs,
            &[
                ("worker_id", ToolInputType::String, true),
                ("reply_to", ToolInputType::Integer, false),
                ("body", ToolInputType::String, false),
                ("artifacts", ToolInputType::StringList, false),
            ],
            "{name} schema mismatch"
        );
    }

    // discard_worker and delete_worker share the same schema.
    for name in ["discard_worker", "delete_worker"] {
        let (_, inputs) = schemas.iter().find(|(n, _)| *n == name).unwrap();
        assert_eq!(inputs, &[("worker_id", ToolInputType::String, true)], "{name} schema mismatch");
    }

    // merge_worker
    let (_, inputs) = schemas.iter().find(|(n, _)| *n == "merge_worker").unwrap();
    assert_eq!(
        inputs,
        &[
            ("worker_id", ToolInputType::String, true),
            ("skip_checks", ToolInputType::StringList, false),
            ("body", ToolInputType::String, false),
            ("artifacts", ToolInputType::StringList, false),
        ]
    );
}

// ---------------------------------------------------------------------------
// Orchestrator resources
// ---------------------------------------------------------------------------

#[test]
fn orchestrator_resource_uris_stable() {
    let mut uris: Vec<&str> =
        multorum::mcp::resource::orchestrator::descriptors().iter().map(|d| d.uri).collect();
    uris.sort();
    assert_eq!(
        uris,
        vec![
            "multorum://orchestrator/perspectives",
            "multorum://orchestrator/rulebook/active",
            "multorum://orchestrator/status",
            "multorum://orchestrator/workers",
        ]
    );
}

#[test]
fn orchestrator_resource_template_uris_stable() {
    let uris: Vec<&str> =
        multorum::mcp::resource::orchestrator::templates().iter().map(|d| d.uri_template).collect();
    assert_eq!(
        uris,
        vec![
            "multorum://orchestrator/workers/{worker}",
            "multorum://orchestrator/workers/{worker}/outbox",
        ]
    );
}

// ---------------------------------------------------------------------------
// Worker tools
// ---------------------------------------------------------------------------

#[test]
fn worker_tool_names_stable() {
    let mut names: Vec<&str> =
        multorum::mcp::tool::worker::descriptors().iter().map(|d| d.name).collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "ack_inbox_message",
            "get_contract",
            "get_status",
            "read_inbox",
            "send_commit",
            "send_report",
        ]
    );
}

#[test]
fn worker_tool_input_schemas_stable() {
    let descriptors = multorum::mcp::tool::worker::descriptors();
    let schemas: Vec<(&str, Vec<(&str, ToolInputType, bool)>)> = descriptors
        .iter()
        .map(|d| {
            let inputs: Vec<_> = d.inputs.iter().map(|i| (i.name, i.kind, i.required)).collect();
            (d.name, inputs)
        })
        .collect();

    // No-input tools.
    for name in ["get_contract", "get_status"] {
        let (_, inputs) = schemas.iter().find(|(n, _)| *n == name).unwrap();
        assert!(inputs.is_empty(), "{name} should have no inputs");
    }

    // read_inbox
    let (_, inputs) = schemas.iter().find(|(n, _)| *n == "read_inbox").unwrap();
    assert_eq!(inputs, &[("after", ToolInputType::Integer, false)]);

    // ack_inbox_message
    let (_, inputs) = schemas.iter().find(|(n, _)| *n == "ack_inbox_message").unwrap();
    assert_eq!(inputs, &[("sequence", ToolInputType::Integer, true)]);

    // send_report
    let (_, inputs) = schemas.iter().find(|(n, _)| *n == "send_report").unwrap();
    assert_eq!(
        inputs,
        &[
            ("head_commit", ToolInputType::String, false),
            ("reply_to", ToolInputType::Integer, false),
            ("body", ToolInputType::String, false),
            ("artifacts", ToolInputType::StringList, false),
        ]
    );

    // send_commit
    let (_, inputs) = schemas.iter().find(|(n, _)| *n == "send_commit").unwrap();
    assert_eq!(
        inputs,
        &[
            ("head_commit", ToolInputType::String, true),
            ("body", ToolInputType::String, false),
            ("artifacts", ToolInputType::StringList, false),
        ]
    );
}

// ---------------------------------------------------------------------------
// Worker resources
// ---------------------------------------------------------------------------

#[test]
fn worker_resource_uris_stable() {
    let mut uris: Vec<&str> =
        multorum::mcp::resource::worker::descriptors().iter().map(|d| d.uri).collect();
    uris.sort();
    assert_eq!(
        uris,
        vec!["multorum://worker/contract", "multorum://worker/inbox", "multorum://worker/status",]
    );
}

#[test]
fn worker_resource_templates_empty() {
    assert!(multorum::mcp::resource::worker::templates().is_empty());
}
