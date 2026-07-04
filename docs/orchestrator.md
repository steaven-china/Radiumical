# Orchestrator

Radiumical includes two orchestrators for multi-step task execution: a simple linear orchestrator and a dynamic event-driven orchestrator.

## Simple Orchestrator

The simple orchestrator manages ordered task lists with dependency tracking.

### Task States

```
Pending ──(deps satisfied)──▶ Active ──(completed)──▶ Done
  │                                                      │
  ├──(blocked)──▶ Blocked                                │
  └──(skipped)──▶ Skipped ◀──────────────────────────────┘
```

### Usage

The agent creates plans via the `orchestrate` tool:

```json
{
  "title": "Refactor auth module",
  "tasks": [
    {"id": 1, "title": "Analyze current auth code"},
    {"id": 2, "title": "Design new auth flow", "deps": [1]},
    {"id": 3, "title": "Implement new auth", "deps": [2]},
    {"id": 4, "title": "Update tests", "deps": [3]}
  ]
}
```

### Features

- **Dependency tracking** — tasks only become active when all dependencies are done
- **Auto-advance** — completing a task automatically starts the next ready task
- **Agent assignment** — tasks can be assigned to specific agent roles
- **Context injection** — the current plan is injected into the LLM system prompt
- **Persistence** — plans saved to `~/.radi/orchestrator/{session}.json`

### TUI

| Command | Description |
|---------|-------------|
| `/plan` | Switch to Plan mode |
| `/plan vis` | Toggle plan visualization panel |
| `/plan show` | Show current plan inline |

## Dynamic Orchestrator

The dynamic orchestrator is an event-driven execution engine with guards, hooks, and programmable workflows.

### Task States

```
Pending ──(deps + guard OK)──▶ Ready ──(agent assigned)──▶ Running ──▶ Done
  ▲                                                                │
  │              ┌── Suspended ◀──(suspend action)─────────────────┤
  │              │                                                 │
  │              └──(resume)──▶ Ready                              │
  │                                                              │
  └────────────────────── re-trigger / retry ◀────────────────────┘
                                                               
Failed ──(retry_count < max)──▶ Ready (retry)
```

### Guards

Guards are composable boolean conditions that control task readiness:

| Guard | Description |
|-------|-------------|
| `Always` | Always true |
| `Never` | Always false (blocks task) |
| `TaskDone(id)` | True when specified task is done |
| `TaskState(id, state)` | True when task is in specified state |
| `EventEmitted(key)` | True when event has been emitted |
| `MetricCompare{key, op, value}` | Compare a metric against a value |
| `And(a, b)` | Both guards true |
| `Or(a, b)` | Either guard true |
| `Not(a)` | Invert guard |
| `Custom(name)` | Custom guard implementation |

Compare operators: `Eq`, `Neq`, `Gt`, `Lt`, `Gte`, `Lte`

### Hooks

Hooks fire actions when triggers match:

| Trigger | Description |
|---------|-------------|
| `OnStart` | When task starts |
| `OnDone` | When task completes |
| `OnError` | When task fails |
| `When(Guard)` | When guard condition becomes true |
| `WhileRunning` | Continuously while task is running |
| `OnEvent(key)` | When named event is emitted |

| Action | Description |
|--------|-------------|
| `StartTask(id)` | Start another task |
| `EmitEvent(key, payload)` | Emit a named event |
| `SetMetric(key, value)` | Set a metric value |
| `MarkDone(id)` | Mark a task as done |
| `SuspendTask(id)` | Suspend a running task |
| `ResumeTask(id)` | Resume a suspended task |
| `SpawnAgent(role)` | Spawn a new agent |
| `Sequence([actions])` | Execute actions in order |

### Event Bus

The event bus provides inter-task communication:

```rust
Event {
    key: String,          // Event identifier
    source_task: usize,   // Task that emitted the event
    payload: String,      // Arbitrary payload
    timestamp: DateTime,  // When it was emitted
}
```

### Cluster Integration

The `AgentCluster` drives the dynamic orchestrator with persistent worker agents:

- **Worker pool** — up to 4 concurrent workers (configurable)
- **Role-aware assignment** — tasks matched to workers by role
- **Tick loop** — every 500ms: collect results, advance orchestrator, assign tasks, fire hooks
- **Completion detection** — signals `AllDone` when all tasks terminal and all workers idle

### Conversion

The two orchestrators are interoperable:

```rust
// Simple → Dynamic
let dynamic = simple_orchestrator.to_dynamic();

// Dynamic → Simple (export/restore)
let (tasks, next_id) = dynamic.export_plan();
dynamic.import_plan(tasks, next_id);
```
