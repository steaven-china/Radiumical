use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::task::TaskState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Guard {
    Always,
    Never,
    TaskDone(u32),
    TaskState(u32, TaskState),
    EventEmitted(String),
    MetricCompare {
        key: String,
        op: CompareOp,
        value: f64,
    },
    And(Vec<Guard>),
    Or(Vec<Guard>),
    Not(Box<Guard>),
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CompareOp {
    Eq,
    Neq,
    Gt,
    Lt,
    Gte,
    Lte,
}

impl Guard {
    pub fn evaluate(&self, ctx: &GuardContext) -> bool {
        match self {
            Guard::Always => true,
            Guard::Never => false,
            Guard::TaskDone(id) => ctx
                .task_states
                .get(id)
                .map(|s| *s == TaskState::Done)
                .unwrap_or(false),
            Guard::TaskState(id, expected) => ctx
                .task_states
                .get(id)
                .map(|s| s == expected)
                .unwrap_or(false),
            Guard::EventEmitted(key) => ctx.emitted_events.contains(key),
            Guard::MetricCompare { key, op, value } => {
                let actual = ctx.metrics.get(key).copied().unwrap_or(0.0);
                match op {
                    CompareOp::Eq => (actual - value).abs() < f64::EPSILON,
                    CompareOp::Neq => (actual - value).abs() >= f64::EPSILON,
                    CompareOp::Gt => actual > *value,
                    CompareOp::Lt => actual < *value,
                    CompareOp::Gte => actual >= *value,
                    CompareOp::Lte => actual <= *value,
                }
            }
            Guard::And(guards) => guards.iter().all(|g| g.evaluate(ctx)),
            Guard::Or(guards) => guards.iter().any(|g| g.evaluate(ctx)),
            Guard::Not(inner) => !inner.evaluate(ctx),
            Guard::Custom(expr) => {
                ctx.custom_guards
                    .get(expr)
                    .map(|f| f())
                    .unwrap_or(false)
            }
        }
    }
}

pub struct GuardContext<'a> {
    pub task_states: &'a HashMap<u32, TaskState>,
    pub emitted_events: &'a std::collections::HashSet<String>,
    pub metrics: &'a HashMap<String, f64>,
    pub custom_guards: &'a HashMap<String, Box<dyn Fn() -> bool + Send + Sync>>,
}
