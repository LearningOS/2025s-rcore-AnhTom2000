//! Process management syscalls
use crate::mm::PageTable;
use crate::{mm::{translated_byte_buffer, VirtAddr}, 
task::{self,change_program_brk, current_user_token, exit_current_and_run_next, suspend_current_and_run_next}, 
timer::get_time_us};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

pub enum TraceMode {
    Read = 0,
    Write = 1,
    Select = 2
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
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us(); 
    let sec = us / 1_000_000; 
    let usec = us % 1_000_000;
    
    let time = TimeVal {
        sec,
        usec,
    };
      
    let time_bytes = unsafe {
        core::slice::from_raw_parts(
            &time as *const TimeVal as *const u8,
            core::mem::size_of::<TimeVal>(),
        )
    };
    let buffers =translated_byte_buffer(
        current_user_token(),
        ts as *const u8,
        core::mem::size_of::<TimeVal>(),
    );
    
    for buffer in buffers {
        let len = buffer.len();
        let time_bytes = &time_bytes[..len];
        buffer.copy_from_slice(time_bytes) ;
    }
    
    0
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    if trace_request == TraceMode::Read as usize || trace_request == TraceMode::Write as usize{
        let addr = VirtAddr::from(id); // 获取虚拟地址
        let pagetable = PageTable::from_token(current_user_token()); // 获取当前线程的页表
        let pte = pagetable.translate(addr.floor()); // 根据虚拟地址查对应页表项
        if pte.is_none() {
            return -1;
        }
        let pte = pte.unwrap();
        if !pte.is_valid() && !pte.is_user(){
            return -1;
        }
        if trace_request == TraceMode::Read as usize && pte.readable(){
            let res  = pte.ppn().get_bytes_array()[addr.page_offset()]; // 通过页表项查找物理地址
            return res as isize;
        }else if trace_request == TraceMode::Write as usize && pte.writable(){
            let phy_addr = pte.ppn().get_bytes_array()[addr.page_offset()] as *mut u8; // 通过页表项查找物理地址
            unsafe {
                phy_addr.write(data as u8); 
            }
            return 0;
        }else {
            return -1;
        }
    }else if trace_request == TraceMode::Select as usize{
        return task::get_syscall_count(id) as isize;
    }else {
       trace!("Unsupported trace request: {}", trace_request);
    }
    -1
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    if (prot & !0x7) !=0 || (prot & 0x7) == 0{
        return -1;
    }
    if let Err(error) = task::mmap(start, len, prot) {
        trace!("kernel: sys_mmap error: {}", error);
        return -1;
    }
    0
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    if start % 4096 != 0 || len % 4096 != 0 {
        return -1;
    }
    if let Err(error) = task::munmap(start, len) {
        trace!("kernel: sys_munmap error: {}", error);
        return -1;
    }
    0
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
