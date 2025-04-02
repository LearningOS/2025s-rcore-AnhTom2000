//! Process management syscalls

use crate::{
    task::{self,exit_current_and_run_next, suspend_current_and_run_next},
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("[kernel] Application exited with code {}", exit_code);
    
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next(); // 赞同当前应用并切换到下一个应用
    0
}

/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}

// TODO: implement the syscall
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    if trace_request == 0 as usize{
        let addr = id as *const u8;
        let data = unsafe { addr.read_volatile() };
        return data as isize;
    }else if trace_request == 1 as usize{
        let addr = id as *mut u8;
        unsafe { addr.write(data as u8) }
        return 0;
    }else if trace_request == 2 as usize{
        let syscall_count = task::get_syscall_count(id);
        return syscall_count as isize;
    }
    -1
}
