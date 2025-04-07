//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the operating system.
//!
//! Be careful when you see `__switch` ASM function in `switch.S`. Control flow around this function
//! might not be what you expect.

mod context;
mod switch;
#[allow(clippy::module_inception)]
mod task;

use crate::loader::{get_app_data, get_num_app};
use crate::mm::{MapPermission, VirtAddr};
use crate::sync::UPSafeCell;
use crate::trap::TrapContext;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use lazy_static::*;
use switch::__switch;
use crate::config::MAX_TASK_NUM;
pub use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;

/// The task manager, where all the tasks are managed.
///
/// Functions implemented on `TaskManager` deals with all task state transitions
/// and task context switching. For convenience, you can find wrappers around it
/// in the module level.
///
/// Most of `TaskManager` are hidden behind the field `inner`, to defer
/// borrowing checks to runtime. You can see examples on how to use `inner` in
/// existing functions on `TaskManager`.
pub struct TaskManager {
    /// total number of tasks
    num_app: usize,
    /// use inner value to get mutable access
    inner: UPSafeCell<TaskManagerInner>,
}

/// The task manager inner in 'UPSafeCell'
struct TaskManagerInner {
    /// task list
    tasks: Vec<TaskControlBlock>,
    /// id of current `Running` task
    current_task: usize,
    syscall_count: [[i32;MAX_TASK_NUM];MAX_TASK_NUM], // 维护每个syscall的调用次数
}

lazy_static! {
    /// a `TaskManager` global instance through lazy_static!
    pub static ref TASK_MANAGER: TaskManager = {
        println!("init TASK_MANAGER");
        let num_app = get_num_app();
        println!("num_app = {}", num_app);
        let mut tasks: Vec<TaskControlBlock> = Vec::new();
        for i in 0..num_app {
            tasks.push(TaskControlBlock::new(get_app_data(i), i));
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                    syscall_count: [[0;MAX_TASK_NUM];MAX_TASK_NUM],
                })
            },
        }
    };
}

impl TaskManager {
    /// Run the first task in task list.
    ///
    /// Generally, the first task in task list is an idle task (we call it zero process later).
    /// But in ch4, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        let mut inner = self.inner.exclusive_access();
        let next_task = &mut inner.tasks[0];
        next_task.task_status = TaskStatus::Running;
        let next_task_cx_ptr = &next_task.task_cx as *const TaskContext;
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut _, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].task_status = TaskStatus::Ready;
    }

    /// Change the status of current `Running` task into `Exited`.
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].task_status = TaskStatus::Exited;
    }

    /// Find next task to run and return task id.
    ///
    /// In this case, we only return the first `Ready` task in task list.
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
    }

    /// Get the current 'Running' task's token.
    fn get_current_token(&self) -> usize {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_user_token()
    }

    /// Get the current 'Running' task's trap contexts.
    fn get_current_trap_cx(&self) -> &'static mut TrapContext {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_trap_cx()
    }
    fn get_task_id(&self) -> usize {
        let inner = self.inner.exclusive_access();
        inner.current_task
    }

    fn get_syscall_count(&self,task_id : usize,id:usize) -> usize {
        let inner = self.inner.exclusive_access();
        inner.syscall_count[task_id][id] as usize
    }
    
    fn inc_syscall_count(&self,task_id : usize,id:usize) {
        let mut inner = self.inner.exclusive_access();
        inner.syscall_count[task_id][id] += 1;
    }

    /// Change the current 'Running' task's program break
    pub fn change_current_program_brk(&self, size: i32) -> Option<usize> {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].change_program_brk(size)
    }


    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        if let Some(next) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            inner.tasks[next].task_status = TaskStatus::Running;
            inner.current_task = next;
            let current_task_cx_ptr = &mut inner.tasks[current].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next].task_cx as *const TaskContext;
            drop(inner);
            // before this, we should drop local variables that must be dropped manually
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr);
            }
            // go back to user mode
        } else {
            panic!("All applications completed!");
        }
    }

    /// Map a new memory area to the current task.
    fn mmap(&self, start : VirtAddr, end : VirtAddr, permission : MapPermission) -> Result<(), String>{
        let mut inner = self.inner.exclusive_access();
        let currcent_task = inner.current_task;
        let memory_set = &mut inner.tasks[currcent_task].memory_set;
        let mut next = start.floor();
        let end1 = end.ceil();
        while next < end1{
           if let Some(pte) = memory_set.translate(next){
                if pte.is_valid(){
                    return Err("mmap error: is already mapped".to_string());
                }
            }
            next.0+=1;
           }
           memory_set.insert_framed_area(start, end, permission | MapPermission::U);
        Ok(())
    }

    /// unmap a memory area from the current task.
    fn munmap(&self,start:VirtAddr,end:VirtAddr)->Result<(),String>{
        let mut inner = self.inner.exclusive_access();
        let currcent_task = inner.current_task;
        let memory_set = &mut inner.tasks[currcent_task].memory_set;
        let mut next = start.floor();
        let end1 = end.ceil();
        while next < end1{
            if let Some(pte) = memory_set.translate(next){
                if !pte.is_valid(){
                    return Err("munmap error: is not mapped".to_string());
                }
            }
            next.0+=1;
        }
      match memory_set.remove_framed_area(start, end) {
          Some(_)=>{
            Ok(())
          },None=>{
              return Err("munmap error: is not mapped".to_string());
          }
      }
    }

    
}

/// Run the first task in task list.
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

/// Switch current `Running` task to the task we have found,
/// or there is no `Ready` task and we can exit with all applications completed
fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// Change the status of current `Running` task into `Ready`.
fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// Change the status of current `Running` task into `Exited`.
fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}

/// Get the current 'Running' task's token.
pub fn current_user_token() -> usize {
    TASK_MANAGER.get_current_token()
}

/// Get the current 'Running' task's trap contexts.
pub fn current_trap_cx() -> &'static mut TrapContext {
    TASK_MANAGER.get_current_trap_cx()
}

/// Change the current 'Running' task's program break
pub fn change_program_brk(size: i32) -> Option<usize> {
    TASK_MANAGER.change_current_program_brk(size)
}

/// 统计当前任务调用syscall的次数
pub fn inc_syscall_count(id:usize){
    TASK_MANAGER.inc_syscall_count(get_task_id(),id) 
}

/// 获得当前任务调用syscall的次数
pub fn get_syscall_count(id:usize)->usize{
    TASK_MANAGER.get_syscall_count(get_task_id(),id) 
}

/// 获得当前任务的id
pub fn get_task_id() -> usize {
    TASK_MANAGER.get_task_id()
}

/// map memory
pub fn mmap(start:usize,len:usize,prot:usize) -> Result<(),String>{
    let mut permission = MapPermission::empty();
    if (prot & 0x1) != 0{
        permission.insert(MapPermission::R);
    }
    if (prot & 0x2) != 0{
        permission.insert(MapPermission::W);
    }
    if (prot & 0x4) != 0{
        permission.insert(MapPermission::X);
    }
    return TASK_MANAGER.mmap(VirtAddr(start),VirtAddr(len+start),permission);
}

/// unmap memory
pub fn munmap(start:usize,len:usize)->Result<(),String>{
    return TASK_MANAGER.munmap(VirtAddr(start),VirtAddr(len+start));
}