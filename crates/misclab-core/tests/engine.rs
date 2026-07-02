//! Integration tests for the node-graph engine.

use std::collections::HashMap;

use misclab_core::cancel::CancellationToken;
use misclab_core::graph::executor::GraphExecutor;
use misclab_core::graph::model::{Edge, NodeInstance, PortRef, SerializedGraph};
use misclab_core::graph::port::{PortType, PortValue};
use misclab_core::nodes::default_registry;
use misclab_core::progress::NullSink;
use serde_json::json;

fn node(id: &str, descriptor_id: &str, params: serde_json::Value) -> NodeInstance {
    NodeInstance {
        id: id.into(),
        descriptor_id: descriptor_id.into(),
        params,
        position: (0.0, 0.0),
    }
}

fn edge(from_node: &str, from_port: &str, to_node: &str, to_port: &str) -> Edge {
    Edge {
        from: PortRef {
            node: from_node.into(),
            port: from_port.into(),
        },
        to: PortRef {
            node: to_node.into(),
            port: to_port.into(),
        },
    }
}

#[test]
fn runs_text_input_into_output() {
    let reg = default_registry();
    let graph = SerializedGraph {
        nodes: vec![
            node("a", "text_input", json!({ "text": "flag{hello}" })),
            node("b", "text_output", json!({})),
        ],
        edges: vec![edge("a", "text", "b", "text")],
    };

    let exec = GraphExecutor::new(&reg, &graph).expect("graph builds");
    let outputs = exec
        .run(&NullSink, &CancellationToken::new())
        .expect("graph runs");

    match outputs.get("b").and_then(|m| m.get("value")) {
        Some(PortValue::Text(s)) => assert_eq!(s, "flag{hello}"),
        other => panic!("unexpected output: {other:?}"),
    }
}

#[test]
fn rejects_cycles() {
    let reg = default_registry();
    let graph = SerializedGraph {
        nodes: vec![
            node("a", "text_output", json!({})),
            node("b", "text_output", json!({})),
        ],
        edges: vec![
            edge("a", "value", "b", "text"),
            edge("b", "value", "a", "text"),
        ],
    };
    assert!(GraphExecutor::new(&reg, &graph).is_err());
}

#[test]
fn rejects_edge_to_unknown_node() {
    let reg = default_registry();
    let graph = SerializedGraph {
        nodes: vec![node("a", "text_input", json!({ "text": "x" }))],
        edges: vec![edge("a", "text", "ghost", "text")],
    };
    assert!(GraphExecutor::new(&reg, &graph).is_err());
}

#[test]
fn standalone_node_runs() {
    let reg = default_registry();
    let inputs = HashMap::new();
    let out = GraphExecutor::run_node(
        &reg,
        "text_input",
        &inputs,
        &json!({ "text": "hi" }),
        &NullSink,
        &CancellationToken::new(),
    )
    .expect("standalone runs");

    match out.get("text") {
        Some(PortValue::Text(s)) => assert_eq!(s, "hi"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn port_type_validation() {
    assert!(PortType::Text.accepts(PortType::Text));
    assert!(PortType::Any.accepts(PortType::Bytes));
    assert!(PortType::Bytes.accepts(PortType::Any));
    // Text inputs accept scalar/list sources (coerced to string at the boundary).
    assert!(PortType::Text.accepts(PortType::Number));
    assert!(PortType::Text.accepts(PortType::Bool));
    assert!(PortType::Text.accepts(PortType::StringList));
    // Unrelated / reverse directions still don't connect.
    assert!(!PortType::Number.accepts(PortType::Text));
    assert!(!PortType::Bytes.accepts(PortType::Number));
}

#[test]
fn number_coerced_into_text_input_standalone() {
    let reg = default_registry();
    let mut inputs = HashMap::new();
    inputs.insert("text".to_string(), PortValue::Number(42.0));
    let out = GraphExecutor::run_node(
        &reg,
        "text_output",
        &inputs,
        &json!({}),
        &NullSink,
        &CancellationToken::new(),
    )
    .unwrap();
    // text_output echoes its input on "value"; coercion turned it into Text("42").
    match out.get("value") {
        Some(PortValue::Text(s)) => assert_eq!(s, "42"),
        other => panic!("expected Text(\"42\"), got {other:?}"),
    }
}

#[test]
fn selector_drives_select_param() {
    // Graph: "flag" → hash.data ; selector("SHA1") → hash.algorithm (a select param).
    // The hash node's own default is SHA256, so a correct result proves the
    // selector overrode the parameter.
    let reg = default_registry();
    let graph = SerializedGraph {
        nodes: vec![
            node("in", "text_input", json!({ "text": "flag" })),
            node("sel", "selector", json!({ "value": "SHA1" })),
            node("h", "hash", json!({ "algorithm": "SHA256" })),
        ],
        edges: vec![
            edge("in", "text", "h", "data"),
            edge("sel", "value", "h", "algorithm"),
        ],
    };
    let out = GraphExecutor::new(&reg, &graph)
        .unwrap()
        .run(&NullSink, &CancellationToken::new())
        .unwrap();
    let got = match out.get("h").and_then(|m| m.get("text")) {
        Some(PortValue::Text(s)) => s.clone(),
        o => panic!("no hash output: {o:?}"),
    };

    let hash_of = |algo: &str| {
        let mut i = HashMap::new();
        i.insert("data".to_string(), PortValue::Text("flag".into()));
        match GraphExecutor::run_node(
            &reg,
            "hash",
            &i,
            &json!({ "algorithm": algo }),
            &NullSink,
            &CancellationToken::new(),
        )
        .unwrap()
        .get("text")
        {
            Some(PortValue::Text(s)) => s.clone(),
            _ => panic!("no digest"),
        }
    };
    assert_eq!(got, hash_of("SHA1"), "selector should drive algorithm=SHA1");
    assert_ne!(got, hash_of("SHA256"), "must not fall back to the default");
}

#[test]
fn number_output_drives_text_input_in_graph() {
    // text_input → length(Number) → text_output(Text): the number is coerced.
    let reg = default_registry();
    let graph = SerializedGraph {
        nodes: vec![
            node("a", "text_input", json!({ "text": "hello" })),
            node("b", "length", json!({})),
            node("c", "text_output", json!({})),
        ],
        edges: vec![
            edge("a", "text", "b", "text"),
            edge("b", "length", "c", "text"),
        ],
    };
    let exec = GraphExecutor::new(&reg, &graph).unwrap();
    let out = exec.run(&NullSink, &CancellationToken::new()).unwrap();
    match out.get("c").and_then(|m| m.get("value")) {
        Some(PortValue::Text(s)) => assert_eq!(s, "5"),
        other => panic!("expected Text(\"5\"), got {other:?}"),
    }
}

#[test]
fn descriptors_are_exported() {
    let reg = default_registry();
    let ds = reg.descriptors();
    assert!(ds.iter().any(|d| d.id == "text_input"));
    assert!(ds.iter().any(|d| d.id == "text_output"));
}
