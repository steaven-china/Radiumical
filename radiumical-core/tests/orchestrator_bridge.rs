//! Integration tests for the orchestrator → dynamic bridge.

use radiumical_core::dynamic::{DynamicOrchestrator, DynamicTask, Guard, TaskState};
use radiumical_core::orchestrator::{Orchestrator, TaskStatus};

#[test]
fn roundtrip_simple_plan_through_dynamic() {
    let mut orch = Orchestrator::new(None);
    orch.create(
        "Deploy",
        vec![
            ("Build".into(), vec![]),
            ("Test".into(), vec![1]),
            ("Deploy".into(), vec![2]),
        ],
    );
    orch.start(1).unwrap();
    orch.done(1).unwrap();
    orch.start(2).unwrap();

    // Upgrade to dynamic
    let mut dyn_orch = orch.to_dynamic();
    assert_eq!(dyn_orch.tasks.len(), 3);
    assert_eq!(dyn_orch.tasks[&1].state, TaskState::Done);
    assert_eq!(dyn_orch.tasks[&2].state, TaskState::Running);
    assert_eq!(dyn_orch.tasks[&3].state, TaskState::Pending);

    // Add a guard on task 3
    dyn_orch.tasks.get_mut(&3).unwrap().guard = Some(Guard::EventEmitted("tests.pass".into()));

    // Export back to simple plan
    let plan = dyn_orch.export_plan("Deploy");
    assert_eq!(plan.tasks.len(), 3);
    assert_eq!(plan.tasks[0].status, TaskStatus::Done);
    assert_eq!(plan.tasks[1].status, TaskStatus::Active);
    assert_eq!(plan.tasks[2].status, TaskStatus::Pending);
}

#[test]
fn dynamic_tick_advances_tasks_with_met_deps() {
    let mut orch = DynamicOrchestrator::new(None);
    orch.add_task(DynamicTask::new(1, "Step 1".into()));
    orch.add_task(DynamicTask::new(2, "Step 2".into()).with_deps(vec![1]));
    orch.add_task(DynamicTask::new(3, "Step 3".into()).with_deps(vec![2]));

    // Tick 1: task 1 should become Ready
    let actions = orch.tick();
    assert!(actions
        .iter()
        .any(|a| matches!(a, radiumical_core::dynamic::TickAction::TaskReady(1))));
    assert!(!actions
        .iter()
        .any(|a| matches!(a, radiumical_core::dynamic::TickAction::TaskReady(2))));

    // Complete task 1
    orch.tagged_done(1, Some("done".into())).unwrap();

    // Tick 2: task 2 should become Ready
    let actions = orch.tick();
    assert!(actions
        .iter()
        .any(|a| matches!(a, radiumical_core::dynamic::TickAction::TaskReady(2))));
}

#[test]
fn dynamic_guard_blocks_until_event() {
    let mut orch = DynamicOrchestrator::new(None);
    orch.add_task(DynamicTask::new(1, "Build".into()));
    orch.add_task(
        DynamicTask::new(2, "Deploy".into())
            .with_deps(vec![1])
            .with_guard(Guard::EventEmitted("approved".into())),
    );

    orch.tagged_done(1, None).unwrap();

    // Tick: task 2 deps met but guard blocks
    let actions = orch.tick();
    assert!(!actions
        .iter()
        .any(|a| matches!(a, radiumical_core::dynamic::TickAction::TaskReady(2))));

    // Emit the event
    orch.event_bus.emit(radiumical_core::dynamic::Event {
        key: "approved".into(),
        source_task: None,
        payload: None,
        timestamp: 0,
    });

    // Now the guard should pass
    let actions = orch.tick();
    assert!(actions
        .iter()
        .any(|a| matches!(a, radiumical_core::dynamic::TickAction::TaskReady(2))));
}

#[test]
fn dynamic_retry_on_failure() {
    let mut orch = DynamicOrchestrator::new(None);
    orch.add_task(DynamicTask::new(1, "Flaky".into()).with_retries(2));

    // Move to Running then Failed
    orch.transition(1, TaskState::Ready).unwrap();
    orch.transition(1, TaskState::Running).unwrap();
    orch.transition(1, TaskState::Failed).unwrap();

    // Tick should auto-retry
    let actions = orch.tick();
    assert!(actions
        .iter()
        .any(|a| matches!(a, radiumical_core::dynamic::TickAction::TaskRetry(1))));
    assert_eq!(orch.tasks[&1].state, TaskState::Ready);
    assert_eq!(orch.tasks[&1].retry_count, 1);

    // Fail again — retry again
    orch.transition(1, TaskState::Running).unwrap();
    orch.transition(1, TaskState::Failed).unwrap();
    let actions = orch.tick();
    assert!(actions
        .iter()
        .any(|a| matches!(a, radiumical_core::dynamic::TickAction::TaskRetry(1))));
    assert_eq!(orch.tasks[&1].retry_count, 2);

    // Third failure — max retries exceeded, stays Failed
    orch.transition(1, TaskState::Running).unwrap();
    orch.transition(1, TaskState::Failed).unwrap();
    let _actions = orch.tick();
    assert_eq!(orch.tasks[&1].state, TaskState::Failed);
}
