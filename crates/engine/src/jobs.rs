//! Work done on a thread of its own, such as talking to a mail server: the window is never
//! kept waiting for a network. A job's work gets no `Session`; what it found is handed back as
//! a step that `Session::poll` takes on the session's own thread, so the mailbox is only ever
//! changed there.

use crate::{CmdResult, EngineError, Session};
use openmailclient_model::Progress;
use serde_json::{Value, json};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::sync::{Arc, Mutex};

/// What a job found, taken on the session's thread: it says what came of it, in words for the
/// status bar and as a result for whoever asked.
pub type Ended = Box<dyn FnOnce(&mut Session) -> (String, Value) + Send>;

/// Results of jobs that ended, kept for whoever asks after them.
const MAX_FINISHED: usize = 20;
/// Parts a job may hand back before the session has taken them: past these it waits, so what
/// it holds stays bounded however much it is doing (a mailbox of 50 GB comes down part by part).
const MAX_WAITING_PARTS: usize = 4;

/// What a job hands back: a part while it runs, or what it found at the end.
enum Handed {
    Part(Ended),
    End(Ended),
}

/// A way for a job's work to hand back what it has found so far, each part taken on the
/// session's thread as it comes, before what it found at the end.
pub struct Parts {
    to: Option<SyncSender<Handed>>,
    /// Where the work is done here and now: kept, and taken when it ends.
    kept: Mutex<Vec<Ended>>,
}

impl Parts {
    /// Hand back a part. Waits while the session has parts it has not taken yet; false when
    /// the session is gone, so there is no one to take it.
    pub(crate) fn hand(&self, part: Ended) -> bool {
        match &self.to {
            Some(to) => to.send(Handed::Part(part)).is_ok(),
            None => match self.kept.lock() {
                Ok(mut kept) => {
                    kept.push(part);
                    true
                }
                Err(_) => false,
            },
        }
    }

    /// Whether parts are taken while the work runs: false where it is done here and now, so
    /// that what it hands back is held till it ends.
    pub(crate) fn streams(&self) -> bool {
        self.to.is_some()
    }
}

pub struct Job {
    /// Counted up from 1 over the run, so one can be named while others come and go.
    pub id: u64,
    /// What it is doing, in words.
    pub what: String,
    pub progress: Arc<Progress>,
    done: Receiver<Handed>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.progress.stop();
    }
}

/// A job's work ended in a panic: said, and nothing taken from it.
fn broke(what: String) -> Ended {
    Box::new(move |_| (format!("{what} failed inside; nothing was changed"), json!({ "ok": false })))
}

impl Session {
    /// Do `work` on a thread of its own and take what it found when it is done; with `wait`,
    /// or where there are no threads, do it here and now and give back what it found.
    pub(crate) fn begin(&mut self, what: String, wait: bool, work: impl FnOnce(&Progress) -> Ended + Send + 'static) -> CmdResult {
        self.begin_in_parts(what, wait, move |progress, _| work(progress))
    }

    /// As `begin`, the work handing back parts of what it finds as it goes (`Parts::hand`),
    /// each taken by `poll` as it comes.
    pub(crate) fn begin_in_parts(&mut self, what: String, wait: bool, work: impl FnOnce(&Progress, &Parts) -> Ended + Send + 'static) -> CmdResult {
        let progress = Arc::new(Progress::default());
        if wait || cfg!(target_arch = "wasm32") {
            let parts = Parts { to: None, kept: Mutex::new(Vec::new()) };
            let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&progress, &parts))).unwrap_or_else(|_| broke(what.clone()));
            // A data file being saved takes no change until the save ends: what the job found
            // waits for it, so none of it is turned away.
            self.end_saves(true);
            for part in parts.kept.into_inner().unwrap_or_default() {
                let (status, _) = part(self);
                self.status = status;
                self.end_saves(true);
            }
            let (status, value) = ended(self);
            self.status = status;
            return Ok(value);
        }
        let (tx, done) = sync_channel(MAX_WAITING_PARTS);
        let theirs = Arc::clone(&progress);
        let name = what.clone();
        std::thread::Builder::new()
            .name("job".to_string())
            .spawn(move || {
                let parts = Parts { to: Some(tx), kept: Mutex::new(Vec::new()) };
                let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&theirs, &parts))).unwrap_or_else(|_| broke(name));
                if let Some(tx) = parts.to {
                    let _ = tx.send(Handed::End(ended));
                }
            })
            .map_err(|_| EngineError::Format("no thread could be had for this; ask again with \"wait\": true".into()))?;
        self.status = format!("{what}…");
        self.job_seq = self.job_seq.saturating_add(1);
        self.jobs.push(Job { id: self.job_seq, what: what.clone(), progress, done });
        Ok(json!({ "started": true, "job": what }))
    }

    /// Ask every job to stop, as when the window is closed: what each has found is still taken
    /// when it hands it back, and what it had not come to waits for the next time.
    pub fn stop_jobs(&mut self) {
        for job in &self.jobs {
            job.progress.stop();
        }
    }

    /// Stop every job and take what each hands back within `within`; a job that has not
    /// ended by then is left to end alone, nothing of it taken. Leaving is never kept waiting
    /// for a server: what a receive had not kept comes again next time.
    pub fn leave_jobs(&mut self, within: std::time::Duration) {
        if self.jobs.is_empty() {
            return;
        }
        self.stop_jobs();
        let until = std::time::Instant::now() + within;
        loop {
            self.end_saves(true);
            self.end_jobs(false);
            if self.jobs.is_empty() || std::time::Instant::now() >= until {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        self.jobs.clear();
    }

    /// Keep what came of a piece of work that ended, the newest last.
    pub(crate) fn ended(&mut self, what: Value) {
        self.finished.push(what);
        if self.finished.len() > MAX_FINISHED {
            self.finished.remove(0);
        }
    }

    /// Take what the jobs that are done found; with `wait`, wait for every one first. True
    /// when one ended.
    pub(crate) fn end_jobs(&mut self, wait: bool) -> bool {
        // What a job found is taken once no data file is being saved: a file being saved takes
        // no change, and mail that came or went would be turned away from it.
        if !wait && !self.saves.is_empty() {
            return false;
        }
        let mut ended = false;
        let mut at = 0;
        while let Some(job) = self.jobs.get(at) {
            let handed = if wait {
                job.done.recv().unwrap_or_else(|_| Handed::End(broke(job.what.clone())))
            } else {
                match job.done.try_recv() {
                    Ok(handed) => handed,
                    Err(TryRecvError::Empty) => {
                        at += 1;
                        continue;
                    }
                    Err(TryRecvError::Disconnected) => Handed::End(broke(job.what.clone())),
                }
            };
            let step = match handed {
                // A part is taken and the job goes on. Without waiting, one part of each job is
                // taken a time round, so the window draws between parts however many wait; a
                // part that started a save waits for it before the next is taken.
                Handed::Part(part) => {
                    let (status, _) = part(self);
                    self.status = status;
                    ended = true;
                    if wait {
                        self.end_saves(true);
                    } else {
                        if !self.saves.is_empty() {
                            return true;
                        }
                        at += 1;
                    }
                    continue;
                }
                Handed::End(step) => step,
            };
            let job = self.jobs.remove(at);
            ended = true;
            let (status, value) = step(self);
            let ok = value.get("ok").and_then(Value::as_bool) != Some(false) && !job.progress.stopped();
            self.status = status.clone();
            self.ended(json!({ "job": job.what, "result": value, "said": status, "ok": ok, "stopped": job.progress.stopped() }));
        }
        ended
    }
}

/// Work that runs, as Show Progress lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Running {
    /// "job-N", "save-N" (the data file's place) or "export-N" (its place in the list).
    pub id: String,
    pub what: String,
    /// From 0 to 1; 0 while it is not known how much there is.
    pub done: f32,
    /// Asked to stop, and stopping at the next step it comes to.
    pub stopping: bool,
}

impl Session {
    /// Everything that runs on a thread of its own, the oldest first.
    pub fn running(&self) -> Vec<Running> {
        let saves = self.saves.iter().map(|s| Running {
            id: format!("save-{}", s.store),
            what: format!("Saving {}", s.name),
            done: s.progress.fraction().clamp(0.0, 1.0),
            stopping: s.progress.stopped(),
        });
        let exports = self.exports.iter().enumerate().map(|(i, e)| Running {
            id: format!("export-{i}"),
            what: format!("Exporting to {}", e.to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            done: e.progress.fraction().clamp(0.0, 1.0),
            stopping: e.progress.stopped(),
        });
        let jobs = self.jobs.iter().map(|j| Running {
            id: format!("job-{}", j.id),
            what: j.what.clone(),
            done: j.progress.fraction().clamp(0.0, 1.0),
            stopping: j.progress.stopped(),
        });
        saves.chain(exports).chain(jobs).collect()
    }
}
