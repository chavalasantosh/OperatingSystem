#![allow(clippy::module_name_repetitions)]

//! Fixed-capacity protected-process model and timer-quantum scheduler.

use crate::paging::GuardedStack;

pub const MAX_PROCESSES: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessState {
    Empty,
    Ready,
    Running,
    Blocked,
    Exited,
    Faulted,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct CpuContext {
    pub instruction_pointer: u64,
    pub stack_pointer: u64,
    pub flags: u64,
    pub registers: [u64; 15],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AddressSpace {
    pub root_frame: u64,
    pub user_start: u64,
    pub user_end: u64,
    pub isolated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessControlBlock {
    pub pid: u32,
    pub state: ProcessState,
    pub address_space: AddressSpace,
    pub context: CpuContext,
    pub user_stack: GuardedStack,
    pub exit_code: i32,
    pub fault_address: u64,
    pub times_scheduled: u64,
}

impl ProcessControlBlock {
    const fn empty() -> Self {
        Self {
            pid: 0,
            state: ProcessState::Empty,
            address_space: AddressSpace {
                root_frame: 0,
                user_start: 0,
                user_end: 0,
                isolated: false,
            },
            context: CpuContext {
                instruction_pointer: 0,
                stack_pointer: 0,
                flags: 0,
                registers: [0; 15],
            },
            user_stack: GuardedStack {
                guard_page: crate::paging::VirtualPage::containing(0),
                stack_start: crate::paging::VirtualPage::containing(0),
                stack_pages: 0,
                stack_top: 0,
            },
            exit_code: 0,
            fault_address: 0,
            times_scheduled: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProcessStats {
    pub process_count: usize,
    pub runnable_count: usize,
    pub exited_count: usize,
    pub faulted_count: usize,
    pub context_switches: u64,
    pub preemptions: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessError {
    TableFull,
    UnknownPid,
    InvalidState,
}

pub struct ProcessTable {
    processes: [ProcessControlBlock; MAX_PROCESSES],
    process_count: usize,
    next_pid: u32,
    current_index: usize,
    context_switches: u64,
    preemptions: u64,
    quantum_ticks: u64,
    ticks_in_quantum: u64,
}

impl ProcessTable {
    #[must_use]
    pub const fn new(quantum_ticks: u64) -> Self {
        Self {
            processes: [ProcessControlBlock::empty(); MAX_PROCESSES],
            process_count: 0,
            next_pid: 1,
            current_index: MAX_PROCESSES - 1,
            context_switches: 0,
            preemptions: 0,
            quantum_ticks,
            ticks_in_quantum: 0,
        }
    }

    /// Creates a ready user process.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::TableFull`] when all process slots are occupied.
    pub fn spawn(
        &mut self,
        address_space: AddressSpace,
        user_stack: GuardedStack,
        entry: u64,
    ) -> Result<u32, ProcessError> {
        let Some(index) = self
            .processes
            .iter()
            .position(|process| process.state == ProcessState::Empty)
        else {
            return Err(ProcessError::TableFull);
        };
        let pid = self.next_pid;
        self.next_pid = self.next_pid.saturating_add(1);
        self.processes[index] = ProcessControlBlock {
            pid,
            state: ProcessState::Ready,
            address_space,
            context: CpuContext {
                instruction_pointer: entry,
                stack_pointer: user_stack.stack_top,
                flags: 0x202,
                registers: [0; 15],
            },
            user_stack,
            exit_code: 0,
            fault_address: 0,
            times_scheduled: 0,
        };
        self.process_count += 1;
        Ok(pid)
    }

    #[must_use]
    pub fn schedule_next(&mut self, preemptive: bool) -> Option<u32> {
        if self.process_count == 0 {
            return None;
        }
        if self.processes[self.current_index].state == ProcessState::Running {
            self.processes[self.current_index].state = ProcessState::Ready;
        }
        for offset in 1..=self.processes.len() {
            let candidate = (self.current_index + offset) % self.processes.len();
            if self.processes[candidate].state == ProcessState::Ready {
                self.current_index = candidate;
                let process = &mut self.processes[candidate];
                process.state = ProcessState::Running;
                process.times_scheduled = process.times_scheduled.saturating_add(1);
                self.context_switches = self.context_switches.saturating_add(1);
                self.preemptions = self
                    .preemptions
                    .saturating_add(if preemptive { 1 } else { 0 });
                self.ticks_in_quantum = 0;
                return Some(process.pid);
            }
        }
        None
    }

    #[must_use]
    pub fn on_timer_tick(&mut self) -> Option<u32> {
        self.ticks_in_quantum = self.ticks_in_quantum.saturating_add(1);
        if self.quantum_ticks != 0 && self.ticks_in_quantum >= self.quantum_ticks {
            return self.schedule_next(true);
        }
        None
    }

    /// Marks a process exited.
    ///
    /// # Errors
    ///
    /// Returns an error when the PID is unknown or already terminal.
    pub fn exit(&mut self, pid: u32, code: i32) -> Result<(), ProcessError> {
        let process = self.find_mut(pid)?;
        if matches!(process.state, ProcessState::Exited | ProcessState::Faulted) {
            return Err(ProcessError::InvalidState);
        }
        process.state = ProcessState::Exited;
        process.exit_code = code;
        Ok(())
    }

    /// Records a user fault without stopping the kernel.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::UnknownPid`] for an unknown PID.
    pub fn fault(&mut self, pid: u32, address: u64) -> Result<(), ProcessError> {
        let process = self.find_mut(pid)?;
        process.state = ProcessState::Faulted;
        process.fault_address = address;
        Ok(())
    }

    #[must_use]
    pub fn stats(&self) -> ProcessStats {
        ProcessStats {
            process_count: self.process_count,
            runnable_count: self
                .processes
                .iter()
                .filter(|process| {
                    matches!(process.state, ProcessState::Ready | ProcessState::Running)
                })
                .count(),
            exited_count: self
                .processes
                .iter()
                .filter(|process| process.state == ProcessState::Exited)
                .count(),
            faulted_count: self
                .processes
                .iter()
                .filter(|process| process.state == ProcessState::Faulted)
                .count(),
            context_switches: self.context_switches,
            preemptions: self.preemptions,
        }
    }

    fn find_mut(&mut self, pid: u32) -> Result<&mut ProcessControlBlock, ProcessError> {
        self.processes
            .iter_mut()
            .find(|process| process.pid == pid && process.state != ProcessState::Empty)
            .ok_or(ProcessError::UnknownPid)
    }
}

#[cfg(test)]
mod tests {
    use super::{AddressSpace, ProcessTable};
    use crate::paging::GuardedStack;

    #[test]
    fn timer_quantum_preempts_processes() {
        let mut table = ProcessTable::new(2);
        let space = AddressSpace {
            root_frame: 0x1000,
            user_start: 0x400000,
            user_end: 0x800000,
            isolated: true,
        };
        let stack = GuardedStack::new(0x800000, 2).unwrap();
        let first = table.spawn(space, stack, 0x401000).unwrap();
        let second = table.spawn(space, stack, 0x402000).unwrap();
        assert_eq!(table.schedule_next(false), Some(first));
        assert_eq!(table.on_timer_tick(), None);
        assert_eq!(table.on_timer_tick(), Some(second));
        assert_eq!(table.stats().preemptions, 1);
        table.exit(first, 0).unwrap();
        table.fault(second, 0xdead).unwrap();
        assert_eq!(table.stats().exited_count, 1);
        assert_eq!(table.stats().faulted_count, 1);
    }
}
