use cargo_mvl::check::{check_single, check_source, ToolOutcome, TOOL_ORDER};

#[test]
fn tool_order_is_limit_total_refine_effect_ifc() {
    assert_eq!(TOOL_ORDER, ["limit", "total", "refine", "effect", "ifc"]);
}

#[test]
fn check_source_runs_all_five_tools_in_order() {
    let results = check_source("fn f() {}");
    let names: Vec<&str> = results.iter().map(|r| r.tool).collect();
    assert_eq!(names, TOOL_ORDER);
}

#[test]
fn limit_runs_for_real_and_finds_a_violation() {
    let results = check_source("fn f() { unsafe {} }");
    let limit = results.iter().find(|r| r.tool == "limit").unwrap();
    match &limit.outcome {
        ToolOutcome::Ran(diagnostics) => assert_eq!(diagnostics.len(), 1),
        other => panic!("expected Ran(_), got {other:?}"),
    }
}

#[test]
fn total_runs_for_real_and_finds_a_violation() {
    let results = check_source("#[mvl::total] fn f(x: Option<i32>) -> i32 { x.unwrap() }");
    let total = results.iter().find(|r| r.tool == "total").unwrap();
    match &total.outcome {
        ToolOutcome::Ran(diagnostics) => assert_eq!(diagnostics.len(), 1),
        other => panic!("expected Ran(_), got {other:?}"),
    }
}

#[test]
fn clean_source_has_no_diagnostics_from_implemented_tools() {
    let results = check_source("fn f() -> i32 { 1 }");
    for tool in ["limit", "total"] {
        let result = results.iter().find(|r| r.tool == tool).unwrap();
        match &result.outcome {
            ToolOutcome::Ran(diagnostics) => {
                assert!(diagnostics.is_empty(), "{tool}: expected no diagnostics")
            }
            other => panic!("expected Ran(_) for {tool}, got {other:?}"),
        }
    }
}

#[test]
fn refine_effect_ifc_are_reported_not_yet_implemented() {
    let results = check_source("fn f() {}");
    for tool in ["refine", "effect", "ifc"] {
        let result = results.iter().find(|r| r.tool == tool).unwrap();
        assert!(
            matches!(result.outcome, ToolOutcome::NotYetImplemented { .. }),
            "{tool}: expected NotYetImplemented, got {:?}",
            result.outcome
        );
    }
}

#[test]
fn check_single_runs_only_the_named_tool() {
    let result = check_single("limit", "fn f() { unsafe {} }").unwrap();
    assert_eq!(result.tool, "limit");
    assert!(matches!(result.outcome, ToolOutcome::Ran(_)));
}

#[test]
fn check_single_returns_none_for_an_unknown_tool() {
    assert!(check_single("nonexistent", "fn f() {}").is_none());
}

#[test]
fn check_single_reports_not_yet_implemented_for_refine() {
    let result = check_single("refine", "fn f() {}").unwrap();
    match result.outcome {
        ToolOutcome::NotYetImplemented { tracking_issue } => assert_eq!(tracking_issue, "#8"),
        other => panic!("expected NotYetImplemented, got {other:?}"),
    }
}

#[test]
fn malformed_source_yields_a_parse_error_for_implemented_tools() {
    let results = check_source("fn f( {{{");
    for tool in ["limit", "total"] {
        let result = results.iter().find(|r| r.tool == tool).unwrap();
        assert!(
            matches!(result.outcome, ToolOutcome::Error(_)),
            "{tool}: expected Error(_), got {:?}",
            result.outcome
        );
    }
}
