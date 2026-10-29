//! Job queue, worker threads, per-host limit, retries, duplicate-link check.

use crate::sites::SITES;
use crate::uploader::{close_handle, make_pad, open_session, upload_once, CancelToken, HInternet, Options, Outcome};
use crate::util::human_size;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, Ordering::SeqCst};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

pub const PER_SITE_LIMIT: i32 = 2; // be polite: at most 2 parallel uploads per host
pub const MAX_RETRIES: u32 = 2;

/// log colour: 0 text, 1 ok, 2 err, 3 warn, 4 muted
pub enum Event {
    Log { site: Option<usize>, tag: u8, text: String },
    JobStart { file: usize },
    Result { site: usize, file: usize, copy: u32, link: String, error: String },
    Done { cancelled: bool },
}

pub type Notify = Box<dyn Fn(Event) + Send + Sync>;

#[derive(Clone)]
pub struct FileItem {
    pub path: Vec<u16>,
    pub name: String,
    pub size: u64,
}

#[derive(Clone, Copy)]
struct Job {
    site: usize,
    file: usize,
    copy: u32,
}

struct State {
    pending: VecDeque<Job>,
    site_active: Vec<i32>,
    links: HashMap<(usize, usize), HashSet<String>>, // (site,file) -> links seen
}

struct Shared {
    files: Vec<FileItem>,
    opt: Options,
    state: Mutex<State>,
    cv: Condvar,
    cancel: CancelToken,
    running: AtomicBool,
    sent: AtomicI64,
    finished: AtomicI32,
    active: AtomicI32,
    total_bytes: i64,
    total_jobs: i32,
    notify: Notify,
}

#[derive(Default)]
pub struct Engine {
    sh: Option<Arc<Shared>>,
    coord: Option<JoinHandle<()>>,
}

impl Engine {
    pub fn running(&self) -> bool {
        self.sh.as_ref().is_some_and(|s| s.running.load(SeqCst))
    }
    pub fn total_bytes(&self) -> i64 {
        self.sh.as_ref().map_or(0, |s| s.total_bytes)
    }
    pub fn sent_bytes(&self) -> i64 {
        self.sh.as_ref().map_or(0, |s| s.sent.load(SeqCst))
    }
    pub fn total_jobs(&self) -> i32 {
        self.sh.as_ref().map_or(0, |s| s.total_jobs)
    }
    pub fn finished_jobs(&self) -> i32 {
        self.sh.as_ref().map_or(0, |s| s.finished.load(SeqCst))
    }
    pub fn active_jobs(&self) -> i32 {
        self.sh.as_ref().map_or(0, |s| s.active.load(SeqCst))
    }

    pub fn start(&mut self, files: Vec<FileItem>, site_idx: &[usize], copies: u32, opt: Options, parallel: usize, notify: Notify) {
        self.join();
        let copies = copies.max(1);
        let mut pending = VecDeque::new();
        let mut total_bytes: i64 = 0;
        for &s in site_idx {
            let site = &SITES[s];
            for (f, item) in files.iter().enumerate() {
                if site.max_bytes > 0 && item.size > site.max_bytes {
                    notify(Event::Log {
                        site: Some(s),
                        tag: 3,
                        text: format!("{}: too big (max {}), skipped", item.name, human_size(site.max_bytes as f64)),
                    });
                    continue;
                }
                for c in 1..=copies {
                    pending.push_back(Job { site: s, file: f, copy: c });
                    total_bytes += item.size as i64;
                }
            }
        }
        let total_jobs = pending.len() as i32;
        let sh = Arc::new(Shared {
            files,
            opt,
            state: Mutex::new(State { pending, site_active: vec![0; SITES.len()], links: HashMap::new() }),
            cv: Condvar::new(),
            cancel: CancelToken::default(),
            running: AtomicBool::new(true),
            sent: AtomicI64::new(0),
            finished: AtomicI32::new(0),
            active: AtomicI32::new(0),
            total_bytes,
            total_jobs,
            notify,
        });
        self.sh = Some(sh.clone());
        let parallel = parallel.clamp(1, 16);
        self.coord = Some(std::thread::spawn(move || coordinator(sh, parallel)));
    }

    pub fn cancel(&self) {
        if let Some(sh) = &self.sh {
            if !sh.running.load(SeqCst) {
                return;
            }
            sh.cancel.cancel_all();
            let _g = sh.state.lock().unwrap();
            sh.cv.notify_all();
        }
    }

    pub fn join(&mut self) {
        if let Some(h) = self.coord.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.cancel();
        self.join();
    }
}

fn coordinator(sh: Arc<Shared>, parallel: usize) {
    let n = parallel.min((sh.total_jobs.max(1)) as usize);
    let pool: Vec<_> = (0..n)
        .map(|_| {
            let s = sh.clone();
            std::thread::spawn(move || worker(s))
        })
        .collect();
    for t in pool {
        let _ = t.join();
    }
    let cancelled = sh.cancel.is_set();
    sh.running.store(false, SeqCst);
    (sh.notify)(Event::Done { cancelled });
}

fn worker(sh: Arc<Shared>) {
    let session = open_session(); // one session per worker -> keep-alive reuse per host
    loop {
        let job = {
            let mut st = sh.state.lock().unwrap();
            loop {
                if sh.cancel.is_set() || st.pending.is_empty() {
                    drop(st);
                    close_handle(session);
                    return;
                }
                let State { pending, site_active, .. } = &mut *st;
                if let Some(pos) = pending.iter().position(|j| site_active[j.site] < PER_SITE_LIMIT) {
                    let j = pending.remove(pos).unwrap();
                    site_active[j.site] += 1;
                    break j;
                }
                st = sh.cv.wait_timeout(st, Duration::from_millis(500)).unwrap().0;
            }
        };
        sh.active.fetch_add(1, SeqCst);
        run_job(&sh, session, job);
        sh.active.fetch_sub(1, SeqCst);
        {
            let mut st = sh.state.lock().unwrap();
            st.site_active[job.site] -= 1;
        }
        sh.cv.notify_all();
    }
}

fn sleep_cancelable(sh: &Shared, ms: u64) {
    let st = sh.state.lock().unwrap();
    let _ = sh.cv.wait_timeout_while(st, Duration::from_millis(ms), |_| !sh.cancel.is_set());
}

fn run_job(sh: &Shared, session: HInternet, j: Job) {
    let s = &SITES[j.site];
    let item = &sh.files[j.file];

    (sh.notify)(Event::JobStart { file: j.file });

    let mut use_pad = s.known_dup && j.copy > 1;
    let mut added: i64 = 0;
    let mut attempt = 0;
    let mut o: Outcome;

    loop {
        let pad = if use_pad { make_pad() } else { String::new() };
        o = upload_once(session, s, &item.path, &item.name, item.size, &sh.opt, &pad, &sh.sent, &mut added, &sh.cancel);
        if sh.cancel.is_set() {
            break;
        }
        if !o.link.is_empty() {
            let dup = {
                let mut st = sh.state.lock().unwrap();
                let seen = st.links.entry((j.site, j.file)).or_default();
                let dup = !use_pad && seen.contains(&o.link);
                if !dup {
                    seen.insert(o.link.clone());
                }
                dup
            };
            if dup {
                (sh.notify)(Event::Log {
                    site: Some(j.site),
                    tag: 3,
                    text: format!("{} #{}: same link as another copy, uploading a unique copy\u{2026}", item.name, j.copy),
                });
                sh.sent.fetch_sub(added, SeqCst);
                added = 0;
                use_pad = true;
                continue;
            }
            break;
        }
        if o.retryable && attempt < MAX_RETRIES {
            attempt += 1;
            (sh.notify)(Event::Log {
                site: Some(j.site),
                tag: 3,
                text: format!("{} #{}: {} - retrying ({}/{})", item.name, j.copy, o.error, attempt, MAX_RETRIES),
            });
            sh.sent.fetch_sub(added, SeqCst);
            added = 0;
            sleep_cancelable(sh, if attempt == 1 { 2000 } else { 5000 });
            if sh.cancel.is_set() {
                break;
            }
            continue;
        }
        break;
    }

    sh.sent.fetch_add(item.size as i64 - added, SeqCst); // count the job as done, like the original app
    sh.finished.fetch_add(1, SeqCst);

    if sh.cancel.is_set() && o.link.is_empty() {
        return; // cancelled: no result row
    }
    (sh.notify)(Event::Result { site: j.site, file: j.file, copy: j.copy, link: o.link, error: o.error });
}
