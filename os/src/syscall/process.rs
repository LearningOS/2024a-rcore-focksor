//! Process management syscalls
use core::mem::size_of;

use crate::{
    config::MAX_SYSCALL_NUM, mm::{write_data_to_user_space, MapPermission, PageTable, StepByOne, VirtAddr}, task::{
        change_program_brk, current_task_insert_framed_area, current_user_token, exit_current_and_run_next, get_task_info, suspend_current_and_run_next, TaskStatus
    }, timer::get_time_us
};


#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// Task information
#[allow(dead_code)]
#[derive(Copy, Clone)]
pub struct TaskInfo {
    /// Task status in it's life cycle
    pub status: TaskStatus,
    /// The numbers of syscall called by task
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
    /// Total running time of task
    pub time: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    let ts = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    write_data_to_user_space(
        current_user_token(), _ts as *const u8, 
        &ts as *const TimeVal as *const u8, size_of::<TimeVal>());
    0
}

/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info NOT IMPLEMENTED YET!");
    let info = get_task_info();
    write_data_to_user_space(current_user_token(), _ti as *const u8, 
    &info as *const TaskInfo as *const u8, size_of::<TaskInfo>());
    0
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    let start_va: VirtAddr = _start.into();
    let end_va: VirtAddr = (_start + _len).into();

    if start_va.page_offset() != 0 || _port & !0x7 != 0 || _port & 0x7 == 0 {
        return -1;
    }

    let page_table = PageTable::from_token(current_user_token());
    // check all memory is valid.
    let mut start_vpn = start_va.floor();
    while start_vpn <= end_va.ceil() {
        match page_table.translate(start_vpn) {
            Some(pte) => {
                if pte.is_valid() {
                    return -1;
                }
            },
            None => {}
        }
        start_vpn.step();
    }

    let mut permission = MapPermission::empty();
    permission.set(MapPermission::R, (_port & (1 << 0)) != 0);
    permission.set(MapPermission::W, (_port & (1 << 1)) != 0);
    permission.set(MapPermission::X, (_port & (1 << 2)) != 0);
    permission.set(MapPermission::U, true);
    current_task_insert_framed_area(start_va, end_va, permission);

    0
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    -1
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
