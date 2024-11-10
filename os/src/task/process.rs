//! Implementation of  [`ProcessControlBlock`]

use super::id::RecycleAllocator;
use super::manager::insert_into_pid2process;
use super::TaskControlBlock;
use super::{add_task, SignalFlags};
use super::{pid_alloc, PidHandle};
use crate::fs::{File, Stdin, Stdout};
use crate::mm::{translated_refmut, MemorySet, KERNEL_SPACE};
use crate::sync::{Condvar, Mutex, Semaphore, UPSafeCell};
use crate::trap::{trap_handler, TrapContext};
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefMut;

/// Process Control Block
pub struct ProcessControlBlock {
    /// immutable
    pub pid: PidHandle,
    /// mutable
    inner: UPSafeCell<ProcessControlBlockInner>,
}

/// Inner of Process Control Block
pub struct ProcessControlBlockInner {
    /// is zombie?
    pub is_zombie: bool,
    /// memory set(address space)
    pub memory_set: MemorySet,
    /// parent process
    pub parent: Option<Weak<ProcessControlBlock>>,
    /// children process
    pub children: Vec<Arc<ProcessControlBlock>>,
    /// exit code
    pub exit_code: i32,
    /// file descriptor table
    pub fd_table: Vec<Option<Arc<dyn File + Send + Sync>>>,
    /// signal flags
    pub signals: SignalFlags,
    /// tasks(also known as threads)
    pub tasks: Vec<Option<Arc<TaskControlBlock>>>,
    /// task resource allocator
    pub task_res_allocator: RecycleAllocator,
    /// mutex list
    pub mutex_list: Vec<Option<Arc<dyn Mutex>>>,
    /// semaphore list
    pub semaphore_list: Vec<Option<Arc<Semaphore>>>,
    /// condvar list
    pub condvar_list: Vec<Option<Arc<Condvar>>>,
    /// the lock banker of the process
    pub lock_banker: LockBanker,
}

impl ProcessControlBlockInner {
    #[allow(unused)]
    /// get the address of app's page table
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    /// allocate a new file descriptor
    pub fn alloc_fd(&mut self) -> usize {
        if let Some(fd) = (0..self.fd_table.len()).find(|fd| self.fd_table[*fd].is_none()) {
            fd
        } else {
            self.fd_table.push(None);
            self.fd_table.len() - 1
        }
    }
    /// allocate a new task id
    pub fn alloc_tid(&mut self) -> usize {
        self.task_res_allocator.alloc()
    }
    /// deallocate a task id
    pub fn dealloc_tid(&mut self, tid: usize) {
        self.task_res_allocator.dealloc(tid)
    }
    /// the count of tasks(threads) in this process
    pub fn thread_count(&self) -> usize {
        self.tasks.len()
    }
    /// get a task with tid in this process
    pub fn get_task(&self, tid: usize) -> Arc<TaskControlBlock> {
        self.tasks[tid].as_ref().unwrap().clone()
    }
}

impl ProcessControlBlock {
    /// inner_exclusive_access
    pub fn inner_exclusive_access(&self) -> RefMut<'_, ProcessControlBlockInner> {
        self.inner.exclusive_access()
    }
    /// new process from elf file
    pub fn new(elf_data: &[u8]) -> Arc<Self> {
        trace!("kernel: ProcessControlBlock::new");
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        // allocate a pid
        let pid_handle = pid_alloc();
        let process = Arc::new(Self {
            pid: pid_handle,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: vec![
                        // 0 -> stdin
                        Some(Arc::new(Stdin)),
                        // 1 -> stdout
                        Some(Arc::new(Stdout)),
                        // 2 -> stderr
                        Some(Arc::new(Stdout)),
                    ],
                    signals: SignalFlags::empty(),
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                    lock_banker: LockBanker::new(),
                })
            },
        });
        // create a main thread, we should allocate ustack and trap_cx here
        let task = Arc::new(TaskControlBlock::new(
            Arc::clone(&process),
            ustack_base,
            true,
        ));
        // prepare trap_cx of main thread
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        let ustack_top = task_inner.res.as_ref().unwrap().ustack_top();
        let kstack_top = task.kstack.get_top();
        drop(task_inner);
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            ustack_top,
            KERNEL_SPACE.exclusive_access().token(),
            kstack_top,
            trap_handler as usize,
        );
        // add main thread to the process
        let mut process_inner = process.inner_exclusive_access();
        process_inner.tasks.push(Some(Arc::clone(&task)));
        drop(process_inner);
        insert_into_pid2process(process.getpid(), Arc::clone(&process));
        // add main thread to scheduler
        add_task(task);
        process
    }

    /// Only support processes with a single thread.
    pub fn exec(self: &Arc<Self>, elf_data: &[u8], args: Vec<String>) {
        trace!("kernel: exec");
        assert_eq!(self.inner_exclusive_access().thread_count(), 1);
        // memory_set with elf program headers/trampoline/trap context/user stack
        trace!("kernel: exec .. MemorySet::from_elf");
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        let new_token = memory_set.token();
        // substitute memory_set
        trace!("kernel: exec .. substitute memory_set");
        self.inner_exclusive_access().memory_set = memory_set;
        // then we alloc user resource for main thread again
        // since memory_set has been changed
        trace!("kernel: exec .. alloc user resource for main thread again");
        let task = self.inner_exclusive_access().get_task(0);
        let mut task_inner = task.inner_exclusive_access();
        task_inner.res.as_mut().unwrap().ustack_base = ustack_base;
        task_inner.res.as_mut().unwrap().alloc_user_res();
        task_inner.trap_cx_ppn = task_inner.res.as_mut().unwrap().trap_cx_ppn();
        // push arguments on user stack
        trace!("kernel: exec .. push arguments on user stack");
        let mut user_sp = task_inner.res.as_mut().unwrap().ustack_top();
        user_sp -= (args.len() + 1) * core::mem::size_of::<usize>();
        let argv_base = user_sp;
        let mut argv: Vec<_> = (0..=args.len())
            .map(|arg| {
                translated_refmut(
                    new_token,
                    (argv_base + arg * core::mem::size_of::<usize>()) as *mut usize,
                )
            })
            .collect();
        *argv[args.len()] = 0;
        for i in 0..args.len() {
            user_sp -= args[i].len() + 1;
            *argv[i] = user_sp;
            let mut p = user_sp;
            for c in args[i].as_bytes() {
                *translated_refmut(new_token, p as *mut u8) = *c;
                p += 1;
            }
            *translated_refmut(new_token, p as *mut u8) = 0;
        }
        // make the user_sp aligned to 8B for k210 platform
        user_sp -= user_sp % core::mem::size_of::<usize>();
        // initialize trap_cx
        trace!("kernel: exec .. initialize trap_cx");
        let mut trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            task.kstack.get_top(),
            trap_handler as usize,
        );
        trap_cx.x[10] = args.len();
        trap_cx.x[11] = argv_base;
        *task_inner.get_trap_cx() = trap_cx;
    }

    /// Only support processes with a single thread.
    pub fn fork(self: &Arc<Self>) -> Arc<Self> {
        trace!("kernel: fork");
        let mut parent = self.inner_exclusive_access();
        assert_eq!(parent.thread_count(), 1);
        // clone parent's memory_set completely including trampoline/ustacks/trap_cxs
        let memory_set = MemorySet::from_existed_user(&parent.memory_set);
        // alloc a pid
        let pid = pid_alloc();
        // copy fd table
        let mut new_fd_table: Vec<Option<Arc<dyn File + Send + Sync>>> = Vec::new();
        for fd in parent.fd_table.iter() {
            if let Some(file) = fd {
                new_fd_table.push(Some(file.clone()));
            } else {
                new_fd_table.push(None);
            }
        }
        // create child process pcb
        let child = Arc::new(Self {
            pid,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: new_fd_table,
                    signals: SignalFlags::empty(),
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                    lock_banker: LockBanker::new(),
                })
            },
        });
        // add child
        parent.children.push(Arc::clone(&child));
        // create main thread of child process
        let task = Arc::new(TaskControlBlock::new(
            Arc::clone(&child),
            parent
                .get_task(0)
                .inner_exclusive_access()
                .res
                .as_ref()
                .unwrap()
                .ustack_base(),
            // here we do not allocate trap_cx or ustack again
            // but mention that we allocate a new kstack here
            false,
        ));
        // attach task to child process
        let mut child_inner = child.inner_exclusive_access();
        child_inner.tasks.push(Some(Arc::clone(&task)));
        drop(child_inner);
        // modify kstack_top in trap_cx of this thread
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        trap_cx.kernel_sp = task.kstack.get_top();
        drop(task_inner);
        insert_into_pid2process(child.getpid(), Arc::clone(&child));
        // add this thread to scheduler
        add_task(task);
        child
    }
    /// get pid
    pub fn getpid(&self) -> usize {
        self.pid.0
    }
}
/// lock type enum
pub enum LockType {
    /// lock type: mutex
    Mutex = 0,
    /// lock type: semaphore
    Semaphore = 1,
}
/// lock banker
pub struct LockBanker {
    /// enable detected
    pub enable_detect: bool,
    /// available resource
    pub available: Vec<isize>,
    /// allocation
    pub allocation: Vec<Vec<isize>>,
    /// need resource
    pub need: Vec<Vec<isize>>,
}
impl LockBanker {
    /// a new lock banker
    pub fn new() -> Self {
        let mut new = Self {
            enable_detect: false,
            available: Vec::new(),
            allocation: Vec::new(),
            need: Vec::new(),
        };
        new.available.resize(2, 0);
        new.allocation.push(vec![0, 0]);
        new.need.push(vec![0, 0]);
        new
    }
    /// add a lock resource
    pub fn add_available(&mut self, lock_type: LockType, count: usize, lock_id: usize) {
        let lock_type = lock_type as usize;
        debug!("add available type {} count {} id {}", lock_type, count, lock_id);
        let lock_type = lock_type + lock_id - 1;
        if lock_type >= self.available.len() {
            debug!("new type {}, current len {}", lock_type, self.available.len());
            self.available.resize(lock_type + 1, 0);
            debug!("available resize to {}", self.available.len());
            for i in self.allocation.iter_mut() {
                i.resize(lock_type + 1, 0);
            }
            for i in self.need.iter_mut() {
                i.resize(lock_type + 1, 0);
            }
        }
        self.available[lock_type] += count as isize;
        debug!("added a lock type {} count {}, available: {}", lock_type, count, self.available[lock_type]);
    }
    /// add a lock usage record
    pub fn record_lock(&mut self, thread_id: usize, lock_type: LockType, lock_id: usize) -> bool {
        let lock_type = lock_type as usize + lock_id - 1;
        debug!("record a lock, thread_id {}, type {}", thread_id, lock_type);
        if thread_id > self.allocation.len() - 1 {
            for _ in (self.allocation.len() - 1)..thread_id {
                self.allocation.push(vec![0; self.available.len()]);
                self.need.push(vec![0; self.available.len()]);
            }
            debug!("new thread id {}, new threads len is {}", thread_id, self.allocation.len());
        }
        self.need[thread_id][lock_type] += 1;
        debug!("deadlock detect enable: {}", self.enable_detect);
        let ret = match self.enable_detect {
            true => self.detect(lock_type),
            false => true,
        };
        debug!("deadlock detect pass: {}", ret);
        if ret {
            self.allocation[thread_id][lock_type] += 1;
            debug!("type {} available {} thread {} allocation {}",
                   lock_type, self.available[lock_type], 
                   thread_id, self.allocation[thread_id][lock_type]);
        }
        ret
    }
    /// remove a lock usage record
    pub fn record_unlock(&mut self, thread_id: usize, lock_type: LockType, lock_id: usize) {
        let lock_type = lock_type as usize + lock_id - 1;
        self.allocation[thread_id][lock_type] -= 1;
        debug!("record a unlock, thread_id {}, type {}, allocation {}", 
                thread_id, lock_type, self.allocation[thread_id][lock_type])
    }
    /// detect deadlock
    pub fn detect(&mut self, lock_type: usize) -> bool {
        debug!("detect deadlock type {} available {}", lock_type, self.available[lock_type]);
        let mut work = self.available.clone();
        let mut finish = vec![false; self.allocation.len()];

        loop {
            debug!("finish: {:?}", finish);
            let mut flag = true;
            for (thread_id, finished) in finish.iter_mut().enumerate() {
                let need = self.need[thread_id][lock_type];
                let available = work[lock_type];
                debug!("thread {} finish {}, need {}, lock type {} available {}, allocation {}", 
                    *finished, thread_id, need, lock_type, available, self.allocation[thread_id][lock_type]);
                if !*finished && need <= available {
                    flag = false;
                    work[lock_type] += self.allocation[thread_id][lock_type];
                    work[lock_type] -= need;
                    *finished = true;
                    debug!("thread {} finish true", thread_id);
                }
            }
            if flag {
                break;
            }
        }
        debug!("finish: {:?}", finish);
        finish.iter().all(|&x| x)
    }
    /// remove a thread
    pub fn remove_thread(&mut self, thread_id: usize) {
        if thread_id < self.allocation.len() {
            self.need[thread_id] = vec![0, 0];
        }
    }
}
