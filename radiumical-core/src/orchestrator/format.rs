use super::types::{Plan, TaskStatus};

pub(super) fn format_plan(plan: &Plan) -> String {
    let title = if plan.title.is_empty() {
        "".into()
    } else {
        format!("# {}\n\n", plan.title)
    };

    let stats = {
        let total = plan.tasks.len();
        let done = plan
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Done)
            .count();
        let active = plan
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Active)
            .count();
        let blocked = plan
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Blocked)
            .count();
        let mut parts = vec![format!("{done}/{total} done")];
        if active > 0 {
            parts.push(format!("{active} active"));
        }
        if blocked > 0 {
            parts.push(format!("{blocked} blocked"));
        }
        format!("progress: {}\n", parts.join(" · "))
    };

    let mut tasks: Vec<_> = plan.tasks.iter().collect();
    tasks.sort_by_key(|t| t.order);
    let lines: Vec<String> = tasks
        .into_iter()
        .map(|t| {
            let icon = t.status.icon();
            let label = t.status.label();
            let dep_str = if t.deps.is_empty() {
                "".into()
            } else {
                format!(
                    " ← deps: #{}",
                    t.deps
                        .iter()
                        .map(|d| d.to_string())
                        .collect::<Vec<_>>()
                        .join(", #")
                )
            };
            let agent_str = t
                .agent
                .as_deref()
                .map(|a| format!(" @{a}"))
                .unwrap_or_default();
            format!(
                "  {icon} #{} [{}] {}{}{}",
                t.id, label, t.title, agent_str, dep_str
            )
        })
        .collect();

    format!("{title}{stats}\n{}", lines.join("\n"))
}
