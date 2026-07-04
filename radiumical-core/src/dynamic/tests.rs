use std::collections::HashMap;

use super::guard::{CompareOp, Guard, GuardContext};
use super::hook::{Hook, HookAction, HookTrigger};
use super::task::{DynamicTask, TaskState};
use super::DynamicOrchestrator;
use super::TickAction;

fn make_dyn() -> DynamicOrchestrator {
    DynamicOrchestrator::new(None)
}

#[test]
fn test_task_state_transitions() {
    let mut task = DynamicTask::new(1, "test".into());
    assert_eq!(task.state, TaskState::Pending);

    assert!(task.try_transition(TaskState::Ready));
    assert_eq!(task.state, TaskState::Ready);

    assert!(task.try_transition(TaskState::Running));
    assert_eq!(task.state, TaskState::Running);

    assert!(task.try_transition(TaskState::Done));
    assert_eq!(task.state, TaskState::Done);

    assert!(task.try_transition(TaskState::Ready));
}

#[test]
fn test_invalid_transition() {
    let mut task = DynamicTask::new(1, "test".into());
    assert!(!task.try_transition(TaskState::Done));
    assert_eq!(task.state, TaskState::Pending);
}

#[test]
fn test_guard_always_never() {
    let ctx = GuardContext {
        task_states: &HashMap::new(),
        emitted_events: &std::collections::HashSet::new(),
        metrics: &HashMap::new(),
        custom_guards: &HashMap::new(),
    };
    assert!(Guard::Always.evaluate(&ctx));
    assert!(!Guard::Never.evaluate(&ctx));
}

#[test]
fn test_guard_task_done() {
    let mut states = HashMap::new();
    states.insert(1, TaskState::Done);
    states.insert(2, TaskState::Running);
    let ctx = GuardContext {
        task_states: &states,
        emitted_events: &std::collections::HashSet::new(),
        metrics: &HashMap::new(),
        custom_guards: &HashMap::new(),
    };
    assert!(Guard::TaskDone(1).evaluate(&ctx));
    assert!(!Guard::TaskDone(2).evaluate(&ctx));
    assert!(!Guard::TaskDone(99).evaluate(&ctx));
}

#[test]
fn test_guard_and_or_not() {
    let mut states = HashMap::new();
    states.insert(1, TaskState::Done);
    states.insert(2, TaskState::Running);
    let ctx = GuardContext {
        task_states: &states,
        emitted_events: &std::collections::HashSet::new(),
        metrics: &HashMap::new(),
        custom_guards: &HashMap::new(),
    };

    let g = Guard::And(vec![Guard::TaskDone(1), Guard::TaskDone(2)]);
    assert!(!g.evaluate(&ctx));

    let g = Guard::Or(vec![Guard::TaskDone(1), Guard::TaskDone(2)]);
    assert!(g.evaluate(&ctx));

    let g = Guard::Not(Box::new(Guard::TaskDone(2)));
    assert!(g.evaluate(&ctx));
}

#[test]
fn test_guard_metric_compare() {
    let mut metrics = HashMap::new();
    metrics.insert("price".to_string(), 42.5);
    let ctx = GuardContext {
        task_states: &HashMap::new(),
        emitted_events: &std::collections::HashSet::new(),
        metrics: &metrics,
        custom_guards: &HashMap::new(),
    };

    assert!(Guard::MetricCompare {
        key: "price".into(),
        op: CompareOp::Gt,
        value: 40.0,
    }
    .evaluate(&ctx));

    assert!(!Guard::MetricCompare {
        key: "price".into(),
        op: CompareOp::Lt,
        value: 40.0,
    }
    .evaluate(&ctx));
}

#[test]
fn test_guard_event_emitted() {
    let mut events = std::collections::HashSet::new();
    events.insert("deploy.ready".to_string());
    let ctx = GuardContext {
        task_states: &HashMap::new(),
        emitted_events: &events,
        metrics: &HashMap::new(),
        custom_guards: &HashMap::new(),
    };

    assert!(Guard::EventEmitted("deploy.ready".into()).evaluate(&ctx));
    assert!(!Guard::EventEmitted("deploy.failed".into()).evaluate(&ctx));
}

#[test]
fn test_dynamic_orchestrator_tick() {
    let mut orch = make_dyn();

    let t1 = DynamicTask::new(1, "setup".into());
    let t2 = DynamicTask::new(2, "build".into()).with_deps(vec![1]);
    let t3 = DynamicTask::new(3, "test".into())
        .with_deps(vec![2])
        .with_guard(Guard::EventEmitted("build.success".into()));

    orch.add_task(t1);
    orch.add_task(t2);
    orch.add_task(t3);

    let actions = orch.tick();
    assert!(actions
        .iter()
        .any(|a| matches!(a, TickAction::TaskReady(1))));

    assert_eq!(orch.tasks[&1].state, TaskState::Ready);
    assert_eq!(orch.tasks[&2].state, TaskState::Pending);
    assert_eq!(orch.tasks[&3].state, TaskState::Pending);
}

#[test]
fn test_tagged_done_and_event() {
    let mut orch = make_dyn();
    orch.add_task(DynamicTask::new(1, "task".into()));

    orch.tagged_done(1, Some("output".into())).unwrap();

    assert_eq!(orch.tasks[&1].state, TaskState::Done);
    assert_eq!(orch.tasks[&1].output.as_deref(), Some("output"));
    assert!(orch.event_bus.has_emitted("task.done.1"));
}

#[test]
fn test_persistent_task() {
    let mut orch = make_dyn();
    let t = DynamicTask::new(1, "monitor".into()).persistent();
    orch.add_task(t);

    assert_eq!(orch.tasks[&1].state, TaskState::Persistent);

    let actions = orch.tick();
    assert!(actions
        .iter()
        .any(|a| matches!(a, TickAction::NeedsAgent { task_id: 1, .. })));
}

#[test]
fn test_retry_on_failure() {
    let mut orch = make_dyn();
    let t = DynamicTask::new(1, "flaky".into()).with_retries(3);
    orch.add_task(t);

    orch.transition(1, TaskState::Ready).unwrap();
    orch.transition(1, TaskState::Running).unwrap();
    orch.transition(1, TaskState::Failed).unwrap();

    let actions = orch.tick();
    assert!(actions
        .iter()
        .any(|a| matches!(a, TickAction::TaskRetry(1))));
    assert_eq!(orch.tasks[&1].state, TaskState::Ready);
    assert_eq!(orch.tasks[&1].retry_count, 1);
}

#[test]
fn test_hook_condition_trigger() {
    let mut orch = make_dyn();

    orch.metrics.insert("price".to_string(), 100.0);

    orch.hooks.push(Hook {
        id: "price_trigger".into(),
        trigger: HookTrigger::When(Guard::MetricCompare {
            key: "price".into(),
            op: CompareOp::Gt,
            value: 50.0,
        }),
        action: HookAction::StartTask(2),
        guard: None,
        max_fires: Some(1),
        fire_count: 0,
    });

    orch.add_task(DynamicTask::new(1, "watch".into()));
    orch.add_task(DynamicTask::new(2, "trade".into()));

    let actions = orch.tick();
    assert!(actions
        .iter()
        .any(|a| matches!(a, TickAction::FireHook(h) if h.id == "price_trigger")));
}

#[test]
fn test_format_status() {
    let mut orch = make_dyn();
    orch.add_task(DynamicTask::new(1, "setup".into()));
    orch.add_task(
        DynamicTask::new(2, "deploy".into())
            .with_agent("coder")
            .with_guard(Guard::TaskDone(1)),
    );
    orch.tasks.get_mut(&1).unwrap().state = TaskState::Done;

    let status = orch.format_status();
    assert!(status.contains("✓"));
    assert!(status.contains("○"));
    assert!(status.contains("@coder"));
    assert!(status.contains("[guarded]"));
}

#[test]
fn test_import_plan_roundtrip() {
    use crate::orchestrator::TaskStatus;

    let mut orch = make_dyn();
    orch.add_task(DynamicTask::new(1, "A".into()));
    orch.add_task(
        DynamicTask::new(2, "B".into())
            .with_deps(vec![1])
            .with_agent("coder"),
    );
    orch.tasks.get_mut(&1).unwrap().state = TaskState::Done;

    let plan = orch.export_plan("test");
    assert_eq!(plan.title, "test");
    assert_eq!(plan.tasks.len(), 2);
    assert_eq!(plan.tasks[0].status, TaskStatus::Done);
    assert_eq!(plan.tasks[1].agent.as_deref(), Some("coder"));

    let mut orch2 = make_dyn();
    orch2.import_plan(&plan);
    assert_eq!(orch2.tasks.len(), 2);
    assert_eq!(orch2.tasks[&1].state, TaskState::Done);
    assert_eq!(orch2.tasks[&2].agent.as_deref(), Some("coder"));
}

#[test]
fn test_orchestrator_to_dynamic() {
    let mut simple = crate::orchestrator::Orchestrator::new(None);
    simple.create(
        "Test",
        vec![("Step 1".into(), vec![]), ("Step 2".into(), vec![1])],
    );
    simple.start(1).unwrap();
    simple.done(1).unwrap();

    let dyn_orch = simple.to_dynamic();
    assert_eq!(dyn_orch.tasks.len(), 2);
    assert_eq!(dyn_orch.tasks[&1].state, TaskState::Done);
    assert_eq!(dyn_orch.tasks[&2].state, TaskState::Running);
}
