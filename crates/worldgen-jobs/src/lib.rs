//! A small priority job pool (native thread backend).
//!
//! Jobs are pure closures that receive a [`CancelToken`] and return `Option<T>` (`None` when they
//! observed cancellation). Results carry the key and [`GenVersion`] they were submitted with so
//! the owner can discard stale results. Worker threads never touch engine state.
//!
//! A wasm Web-Worker backend would implement the same `submit`/`try_recv` surface (see
//! target-architecture §10); it is not implemented here.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;
use worldgen_core::{CancelToken, GenVersion};

type Work<T> = Box<dyn FnOnce(&CancelToken) -> Option<T> + Send>;

struct Queued<K, T> {
    prio: f32,
    seq: u64,
    key: K,
    version: GenVersion,
    cancel: CancelToken,
    work: Work<T>,
}

impl<K, T> PartialEq for Queued<K, T> {
    fn eq(&self, o: &Self) -> bool {
        self.seq == o.seq
    }
}
impl<K, T> Eq for Queued<K, T> {}
impl<K, T> PartialOrd for Queued<K, T> {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl<K, T> Ord for Queued<K, T> {
    // BinaryHeap is a max-heap: the *smallest* priority value must compare greatest.
    fn cmp(&self, o: &Self) -> Ordering {
        o.prio.total_cmp(&self.prio).then(o.seq.cmp(&self.seq))
    }
}

struct Shared<K, T> {
    queue: Mutex<(BinaryHeap<Queued<K, T>>, bool)>,
    cv: Condvar,
}

/// Completed (or cancelled) job.
pub struct JobResult<K, T> {
    pub key: K,
    pub version: GenVersion,
    /// `None` if the job was cancelled.
    pub value: Option<T>,
    pub run_ms: f32,
}

pub struct JobPool<K: Send + 'static, T: Send + 'static> {
    shared: Arc<Shared<K, T>>,
    rx: Receiver<JobResult<K, T>>,
    workers: Vec<JoinHandle<()>>,
    seq: u64,
    submitted: u64,
    received: u64,
}

impl<K: Send + 'static, T: Send + 'static> JobPool<K, T> {
    pub fn new(threads: usize) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new((BinaryHeap::new(), false)),
            cv: Condvar::new(),
        });
        let (tx, rx): (Sender<JobResult<K, T>>, _) = channel();
        let workers = (0..threads.max(1))
            .map(|i| {
                let (shared, tx) = (shared.clone(), tx.clone());
                std::thread::Builder::new()
                    .name(format!("worldgen-job-{i}"))
                    .spawn(move || worker(shared, tx))
                    .expect("spawn worker")
            })
            .collect();
        Self {
            shared,
            rx,
            workers,
            seq: 0,
            submitted: 0,
            received: 0,
        }
    }

    /// Submit a job. Lower `priority` runs first. Returns its cancel token.
    pub fn submit<F>(&mut self, key: K, version: GenVersion, priority: f32, work: F) -> CancelToken
    where
        F: FnOnce(&CancelToken) -> Option<T> + Send + 'static,
    {
        let cancel = CancelToken::new();
        self.seq += 1;
        self.submitted += 1;
        let q = Queued {
            prio: priority,
            seq: self.seq,
            key,
            version,
            cancel: cancel.clone(),
            work: Box::new(work),
        };
        self.shared.queue.lock().unwrap().0.push(q);
        self.shared.cv.notify_one();
        cancel
    }

    pub fn try_recv(&mut self) -> Option<JobResult<K, T>> {
        let r = self.rx.try_recv().ok()?;
        self.received += 1;
        Some(r)
    }

    /// Jobs submitted but not yet received back (queued + running + awaiting collection).
    pub fn outstanding(&self) -> usize {
        (self.submitted - self.received) as usize
    }

    pub fn thread_count(&self) -> usize {
        self.workers.len()
    }
}

impl<K: Send + 'static, T: Send + 'static> Drop for JobPool<K, T> {
    fn drop(&mut self) {
        {
            let mut q = self.shared.queue.lock().unwrap();
            q.1 = true;
            for j in q.0.iter() {
                j.cancel.cancel();
            }
        }
        self.shared.cv.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

fn worker<K: Send + 'static, T: Send + 'static>(
    shared: Arc<Shared<K, T>>,
    tx: Sender<JobResult<K, T>>,
) {
    loop {
        let job = {
            let mut g = shared.queue.lock().unwrap();
            loop {
                if let Some(j) = g.0.pop() {
                    break Some(j);
                }
                if g.1 {
                    break None;
                }
                g = shared.cv.wait(g).unwrap();
            }
        };
        let Some(job) = job else { return };
        let t = Instant::now();
        let value = if job.cancel.is_cancelled() {
            None
        } else {
            (job.work)(&job.cancel)
        };
        let _ = tx.send(JobResult {
            key: job.key,
            version: job.version,
            value,
            run_ms: t.elapsed().as_secs_f32() * 1000.0,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AO};
    use std::time::Duration;

    fn drain<K: Send + 'static, T: Send + 'static>(
        p: &mut JobPool<K, T>,
        n: usize,
    ) -> Vec<JobResult<K, T>> {
        let mut out = vec![];
        let t = Instant::now();
        while out.len() < n && t.elapsed() < Duration::from_secs(10) {
            if let Some(r) = p.try_recv() {
                out.push(r);
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        out
    }

    #[test]
    fn delivers_results_with_versions() {
        let mut p: JobPool<u32, u32> = JobPool::new(3);
        for i in 0..20 {
            p.submit(i, GenVersion(7), i as f32, move |_| Some(i * 2));
        }
        let r = drain(&mut p, 20);
        assert_eq!(r.len(), 20);
        assert!(
            r.iter()
                .all(|x| x.version == GenVersion(7) && x.value == Some(x.key * 2))
        );
        assert_eq!(p.outstanding(), 0);
    }

    #[test]
    fn single_thread_runs_in_priority_order() {
        let mut p: JobPool<u32, u32> = JobPool::new(1);
        // block the worker so all jobs are queued before any runs
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let g2 = gate.clone();
        p.submit(999, GenVersion(0), -1.0, move |_| {
            let (m, cv) = &*g2;
            let mut open = m.lock().unwrap();
            while !*open {
                open = cv.wait(open).unwrap();
            }
            Some(0)
        });
        for (k, pr) in [(1, 5.0), (2, 1.0), (3, 3.0), (4, 2.0)] {
            p.submit(k, GenVersion(0), pr, move |_| Some(k));
        }
        std::thread::sleep(Duration::from_millis(50));
        {
            let (m, cv) = &*gate;
            *m.lock().unwrap() = true;
            cv.notify_all();
        }
        let r = drain(&mut p, 5);
        let order: Vec<u32> = r.iter().map(|x| x.key).collect();
        assert_eq!(order, vec![999, 2, 4, 3, 1]);
    }

    #[test]
    fn cancelled_queued_jobs_do_not_run() {
        let mut p: JobPool<u32, u32> = JobPool::new(1);
        let ran = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let g2 = gate.clone();
        p.submit(0, GenVersion(0), -1.0, move |_| {
            let (m, cv) = &*g2;
            let mut open = m.lock().unwrap();
            while !*open {
                open = cv.wait(open).unwrap();
            }
            Some(0)
        });
        let mut tokens = vec![];
        for k in 1..=5 {
            let ran = ran.clone();
            tokens.push(p.submit(k, GenVersion(0), k as f32, move |_| {
                ran.fetch_add(1, AO::SeqCst);
                Some(k)
            }));
        }
        for t in &tokens {
            t.cancel();
        }
        {
            let (m, cv) = &*gate;
            *m.lock().unwrap() = true;
            cv.notify_all();
        }
        let r = drain(&mut p, 6);
        assert_eq!(r.len(), 6);
        assert_eq!(ran.load(AO::SeqCst), 0);
        assert!(r.iter().filter(|x| x.key > 0).all(|x| x.value.is_none()));
    }

    #[test]
    fn running_job_observes_cancel() {
        let mut p: JobPool<u32, u32> = JobPool::new(1);
        let t = p.submit(1, GenVersion(0), 0.0, |c| {
            for _ in 0..2000 {
                if c.is_cancelled() {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Some(1)
        });
        std::thread::sleep(Duration::from_millis(20));
        t.cancel();
        let r = drain(&mut p, 1);
        assert!(r[0].value.is_none());
    }
}
