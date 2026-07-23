#![allow(clippy::module_name_repetitions)]

//! Fixed-capacity cooperative scheduler used before process context switching.

pub const MAX_TASKS: usize = 8;

/// Kernel task roles available during the M3/M4 runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskKind {
    Idle,
    Shell,
    SystemMonitor,
}

/// Lifecycle state of one early kernel task.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskState {
    Empty,
    Ready,
    Running,
    Blocked,
}

/// Read-only scheduler statistics exposed to diagnostics and the shell.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SchedulerStats {
    pub task_count: usize,
    pub context_switches: u64,
    pub dispatches: u64,
    pub current_task_id: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TaskSlot {
    kind: TaskKind,
    state: TaskState,
    runs: u64,
}

impl TaskSlot {
    const fn empty() -> Self {
        Self {
            kind: TaskKind::Idle,
            state: TaskState::Empty,
            runs: 0,
        }
    }
}

/// Round-robin scheduler for fixed kernel tasks.
///
/// This stage schedules task *work functions*. Register/stack context switching
/// is intentionally deferred until user-mode process support.
pub struct Scheduler {
    tasks: [TaskSlot; MAX_TASKS],
    task_count: usize,
    current_index: usize,
    context_switches: u64,
    dispatches: u64,
}

impl Scheduler {
    /// Creates an empty scheduler.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tasks: [TaskSlot::empty(); MAX_TASKS],
            task_count: 0,
            current_index: 0,
            context_switches: 0,
            dispatches: 0,
        }
    }

    /// Registers a ready kernel task and returns its numeric identifier.
    #[must_use]
    pub fn add_task(&mut self, kind: TaskKind) -> Option<usize> {
        if self.task_count == MAX_TASKS {
            return None;
        }

        let id = self.task_count;
        self.tasks[id] = TaskSlot {
            kind,
            state: TaskState::Ready,
            runs: 0,
        };
        self.task_count += 1;
        Some(id)
    }

    /// Selects the next ready task using round-robin ordering.
    #[must_use]
    pub fn dispatch_next(&mut self, _tick: u64) -> Option<TaskKind> {
        if self.task_count == 0 {
            return None;
        }

        if self.tasks[self.current_index].state == TaskState::Running {
            self.tasks[self.current_index].state = TaskState::Ready;
        }

        for offset in 1..=self.task_count {
            let candidate = (self.current_index + offset) % self.task_count;
            if self.tasks[candidate].state == TaskState::Ready {
                if candidate != self.current_index || self.dispatches == 0 {
                    self.context_switches = self.context_switches.saturating_add(1);
                }
                self.current_index = candidate;
                let task = &mut self.tasks[candidate];
                task.state = TaskState::Running;
                task.runs = task.runs.saturating_add(1);
                self.dispatches = self.dispatches.saturating_add(1);
                return Some(task.kind);
            }
        }

        None
    }

    /// Changes one task's readiness state.
    #[must_use]
    pub fn set_blocked(&mut self, task_id: usize, blocked: bool) -> bool {
        let Some(task) = self.tasks.get_mut(task_id) else {
            return false;
        };
        if task.state == TaskState::Empty {
            return false;
        }
        task.state = if blocked {
            TaskState::Blocked
        } else {
            TaskState::Ready
        };
        true
    }

    /// Returns aggregate scheduler diagnostics.
    #[must_use]
    pub const fn stats(&self) -> SchedulerStats {
        SchedulerStats {
            task_count: self.task_count,
            context_switches: self.context_switches,
            dispatches: self.dispatches,
            current_task_id: self.current_index,
        }
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{Scheduler, TaskKind};

    #[test]
    fn scheduler_round_robins_ready_tasks() {
        let mut scheduler = Scheduler::new();
        scheduler.add_task(TaskKind::Idle).unwrap();
        scheduler.add_task(TaskKind::Shell).unwrap();
        scheduler.add_task(TaskKind::SystemMonitor).unwrap();

        assert_eq!(scheduler.dispatch_next(1), Some(TaskKind::Shell));
        assert_eq!(scheduler.dispatch_next(2), Some(TaskKind::SystemMonitor));
        assert_eq!(scheduler.dispatch_next(3), Some(TaskKind::Idle));
        assert_eq!(scheduler.stats().task_count, 3);
        assert_eq!(scheduler.stats().dispatches, 3);
        assert!(scheduler.stats().context_switches >= 3);
    }

    #[test]
    fn blocked_tasks_are_skipped() {
        let mut scheduler = Scheduler::new();
        scheduler.add_task(TaskKind::Idle).unwrap();
        let shell = scheduler.add_task(TaskKind::Shell).unwrap();
        assert!(scheduler.set_blocked(shell, true));
        assert_eq!(scheduler.dispatch_next(1), Some(TaskKind::Idle));
    }
}
