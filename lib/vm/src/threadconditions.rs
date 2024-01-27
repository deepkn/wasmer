use fnv::FnvBuildHasher;
use wasmer_types::lib::std::sync::Arc;
use wasmer_types::lib::std::time::Duration;
use wasmer_types::lib::std::vec::Vec;
use wasmer_types::lib::std::fmt::{Display, Formatter, Result};
use wasmer_types::lib::std::collections::HashMap;

#[cfg(not(target_os = "theseus"))]
use wasmer_types::lib::std::thread::{current, park, park_timeout, Thread};

#[cfg(target_os = "theseus")]
use theseus_task::{schedule, get_my_current_task, scheduler::add_task, scheduler::remove_task, TaskRef, WeakTaskRef};

#[cfg(target_os = "theseus")]
use theseus_mutex::Mutex;

#[cfg(feature = "std")]
use thiserror::Error;

#[cfg(feature = "std")]
use dashmap::DashMap;

#[cfg(not(feature = "std"))]
use thiserror_core2::Error;

#[cfg(not(feature = "std"))]
use leapfrog::LeapMap;

/// Error that can occur during wait/notify calls.
#[derive(Debug, Error)]
// Non-exhaustive to allow for future variants without breaking changes!
#[non_exhaustive]
pub enum WaiterError {
    /// Wait/Notify is not implemented for this memory
    Unimplemented,
    /// To many waiter for an address
    TooManyWaiters,
    /// Unexpected error
    Unexpected,
}

impl Display for WaiterError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "WaiterError")
    }
}

/// A location in memory for a Waiter
#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug)]
pub struct NotifyLocation {
    /// The address of the Waiter location
    pub address: u32,
}

cfg_if::cfg_if! {
    if #[cfg(not(target_os = "theseus"))] {
        #[derive(Debug)]
        struct NotifyWaiter {
            pub thread: Thread,
            pub notified: bool,
        }

        #[derive(Debug, Default)]
        struct NotifyMap {
            pub map: DashMap<NotifyLocation, Vec<NotifyWaiter>, FnvBuildHasher>,
        }
    } else if #[cfg(target_os = "theseus")] {
        #[derive(Debug)]
        struct NotifyWaiter {
            pub task: WeakTaskRef,
            pub notified: bool,
        }

        #[derive(Debug, Default)]
        struct NotifyMap {
            pub map: HashMap<NotifyLocation, Vec<NotifyWaiter>, FnvBuildHasher>,
        }
    }
}

cfg_if::cfg_if! {
    if #[cfg(not(target_os = "theseus"))] {
        /// HashMap of Waiters for the Thread/Notify opcodes
        #[derive(Debug)]
        pub struct ThreadConditions {
            inner: Arc<NotifyMap>, // The Hasmap with the Notify for the Notify/wait opcodes
        }

        impl ThreadConditions {
            /// Create a new ThreadConditions
            pub fn new() -> Self {
                Self {
                    inner: Arc::new(NotifyMap::default()),
                }
            }

            // To implement Wait / Notify, a HasMap, behind a mutex, will be used
            // to track the address of waiter. The key of the hashmap is based on the memory
            // and waiter threads are "park"'d (with or without timeout)
            // Notify will wake the waiters by simply "unpark" the thread
            // as the Thread info is stored on the HashMap
            // once unparked, the waiter thread will remove it's mark on the HashMap
            // timeout / awake is tracked with a boolean in the HashMap
            // because `park_timeout` doesn't gives any information on why it returns

            /// Add current thread to the waiter hash
            pub fn do_wait(
                &mut self,
                dst: NotifyLocation,
                timeout: Option<Duration>,
            ) -> Result<u32, WaiterError> {
                // fetch the notifier
                if self.inner.map.len() >= 1 << 32 {
                    return Err(WaiterError::TooManyWaiters);
                }
                self.inner
                    .map
                    .entry(dst)
                    .or_insert_with(Vec::new)
                    .push(NotifyWaiter {
                        thread: current(),
                        notified: false,
                    });
                if let Some(timeout) = timeout {
                    park_timeout(timeout);
                } else {
                    park();
                }
                let mut bindding = self.inner.map.get_mut(&dst).unwrap();
                let v = bindding.value_mut();
                let id = current().id();
                let mut ret = 0;
                v.retain(|cond| {
                    if cond.thread.id() == id {
                        ret = if cond.notified { 0 } else { 2 };
                        false
                    } else {
                        true
                    }
                });
                let empty = v.is_empty();
                drop(bindding);
                if empty {
                    self.inner.map.remove(&dst);
                }
                Ok(ret)
            }

            /// Notify waiters from the wait list
            pub fn do_notify(&mut self, dst: NotifyLocation, count: u32) -> u32 {
                let mut count_token = 0u32;
                if let Some(mut v) = self.inner.map.get_mut(&dst) {
                    for waiter in v.value_mut() {
                        if count_token < count && !waiter.notified {
                            waiter.notified = true; // mark as was waiked up
                            waiter.thread.unpark(); // wakeup!
                            count_token += 1;
                        }
                    }
                }
                count_token
            }
        }

        impl Clone for ThreadConditions {
            fn clone(&self) -> Self {
                Self {
                    inner: self.inner.clone(),
                }
            }
        }
    } else if #[cfg(target_os = "theseus")] {
        /// HashMap of Waiters for the Thread/Notify opcodes
        #[derive(Debug)]
        pub struct ThreadConditions {
            inner: Arc<Mutex<NotifyMap>>, // The Hasmap with the Notify for the Notify/wait opcodes
        }

        impl ThreadConditions {
            /// Create a new ThreadConditions
            pub fn new() -> Self {
                Self {
                    inner: Arc::new(Mutex::new(NotifyMap::default())),
                }
            }

            // To implement Wait / Notify, a HasMap, behind a mutex, will be used
            // to track the address of waiter. The key of the hashmap is based on the memory
            // and waiter threads are "park"'d (with or without timeout)
            // Notify will wake the waiters by simply "unpark" the thread
            // as the Thread info is stored on the HashMap
            // once unparked, the waiter thread will remove it's mark on the HashMap
            // timeout / awake is tracked with a boolean in the HashMap
            // because `park_timeout` doesn't gives any information on why it returns

            /// Add current thread to the waiter hash
            pub fn do_wait(
                &mut self,
                dst: NotifyLocation,
                timeout: Option<Duration>,
            ) -> ::core::result::Result<u32, WaiterError> {
                // fetch the notifier
                let mut conds = self.inner.lock();
                if conds.map.len() >= 1 << 32 {
                    return Err(WaiterError::TooManyWaiters);
                }

                let taskref = get_my_current_task().ok_or(WaiterError::Unexpected)?;
                conds.map
                    .entry(dst)
                    .or_insert_with(Vec::new)
                    .push(NotifyWaiter {
                        task: taskref.downgrade(),
                        notified: false,
                    });
                if let Some(timeout) = timeout {
                    remove_task(&taskref);
                } else {
                    remove_task(&taskref);
                }
                let mut v = conds.map.get_mut(&dst).unwrap();
                let mut ret = 0;
                v.retain(|cond| {
                    if cond.task.upgrade().unwrap() == taskref {
                        ret = if cond.notified { 0 } else { 2 };
                        false
                    } else {
                        true
                    }
                });
                let empty = v.is_empty();
                if empty {
                    conds.map.remove(&dst);
                }
                Ok(ret)
            }

            /// Notify waiters from the wait list
            pub fn do_notify(&mut self, dst: NotifyLocation, count: u32) -> u32 {
                let mut count_token = 0u32;
                if let Some(mut v) = self.inner.lock().map.get_mut(&dst) {
                    for waiter in v {
                        if count_token < count && !waiter.notified {
                            waiter.notified = true; // mark as was waked up
                            add_task(waiter.task.upgrade().unwrap()); // wakeup!
                            count_token += 1;
                        }
                    }
                }
                count_token
            }
        }

        impl Clone for ThreadConditions {
            fn clone(&self) -> Self {
                Self {
                    inner: self.inner.clone(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threadconditions_notify_nowaiters() {
        let mut conditions = ThreadConditions::new();
        let dst = NotifyLocation { address: 0 };
        let ret = conditions.do_notify(dst, 1);
        assert_eq!(ret, 0);
    }

    #[test]
    fn threadconditions_notify_1waiter() {
        use std::thread;

        let mut conditions = ThreadConditions::new();
        let mut threadcond = conditions.clone();

        thread::spawn(move || {
            let dst = NotifyLocation { address: 0 };
            let ret = threadcond.do_wait(dst, None).unwrap();
            assert_eq!(ret, 0);
        });
        thread::sleep(Duration::from_millis(10));
        let dst = NotifyLocation { address: 0 };
        let ret = conditions.do_notify(dst, 1);
        assert_eq!(ret, 1);
    }

    #[test]
    fn threadconditions_notify_waiter_timeout() {
        use std::thread;

        let mut conditions = ThreadConditions::new();
        let mut threadcond = conditions.clone();

        thread::spawn(move || {
            let dst = NotifyLocation { address: 0 };
            let ret = threadcond
                .do_wait(dst, Some(Duration::from_millis(1)))
                .unwrap();
            assert_eq!(ret, 2);
        });
        thread::sleep(Duration::from_millis(50));
        let dst = NotifyLocation { address: 0 };
        let ret = conditions.do_notify(dst, 1);
        assert_eq!(ret, 0);
    }

    #[test]
    fn threadconditions_notify_waiter_mismatch() {
        use std::thread;

        let mut conditions = ThreadConditions::new();
        let mut threadcond = conditions.clone();

        thread::spawn(move || {
            let dst = NotifyLocation { address: 8 };
            let ret = threadcond
                .do_wait(dst, Some(Duration::from_millis(10)))
                .unwrap();
            assert_eq!(ret, 2);
        });
        thread::sleep(Duration::from_millis(1));
        let dst = NotifyLocation { address: 0 };
        let ret = conditions.do_notify(dst, 1);
        assert_eq!(ret, 0);
        thread::sleep(Duration::from_millis(100));
    }

    #[test]
    fn threadconditions_notify_2waiters() {
        use std::thread;

        let mut conditions = ThreadConditions::new();
        let mut threadcond = conditions.clone();
        let mut threadcond2 = conditions.clone();

        thread::spawn(move || {
            let dst = NotifyLocation { address: 0 };
            let ret = threadcond.do_wait(dst, None).unwrap();
            assert_eq!(ret, 0);
        });
        thread::spawn(move || {
            let dst = NotifyLocation { address: 0 };
            let ret = threadcond2.do_wait(dst, None).unwrap();
            assert_eq!(ret, 0);
        });
        thread::sleep(Duration::from_millis(20));
        let dst = NotifyLocation { address: 0 };
        let ret = conditions.do_notify(dst, 5);
        assert_eq!(ret, 2);
    }
}
