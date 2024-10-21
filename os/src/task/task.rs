//! Types related to task management

use crate::{config::MAX_SYSCALL_NUM, syscall::TaskInfo};

use super::TaskContext;

/// The task control block (TCB) of a task.
#[derive(Copy, Clone)]
pub struct TaskControlBlock {
    /// The task status in it's lifecycle
    pub task_status: TaskStatus,
    /// The task context
    pub task_cx: TaskContext,
    /// The task info record
    pub task_info: TaskInfo,
}

impl TaskControlBlock {
    /// Create an empty TCB
    pub fn new() -> Self {
        Self {
            task_cx: TaskContext::zero_init(),
            task_status: TaskStatus::UnInit,
            task_info: TaskInfo {
                status: TaskStatus::Running,
                syscall_times: [0; MAX_SYSCALL_NUM],
                time: 0,
            },
        }
    }
}

/// The status of a task
#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    /// uninitialized
    UnInit,
    /// ready to run
    Ready,
    /// running
    Running,
    /// exited
    Exited,
}
