//! Taking back what was done to a message: a change to it, a move, a deletion.

use openmailclient_model::{FolderId, Item, ItemId, StoreError};

use crate::{EngineError, Session};

/// Steps that are kept. An older one is forgotten when a newer one comes.
pub const MAX_STEPS: usize = 100;

/// Something that was done to a message, with what is needed to take it back.
#[derive(Clone, Debug)]
pub enum Step {
    /// The message was changed where it stands; `before` is what it was.
    Edited { what: &'static str, store: usize, folder: FolderId, id: u64, before: Box<Item> },
    /// The message `was` in `from` is now `id` in `to`.
    Moved { what: &'static str, store: usize, from: FolderId, was: u64, to: FolderId, id: u64 },
    /// The message `was` in `folder` was removed for good; `item` is all of it.
    Removed { what: &'static str, store: usize, folder: FolderId, was: u64, item: Box<Item> },
    /// A message that was being written was discarded; it is the newest of those set aside.
    Discarded,
    /// Items were put in `folder` from a file at once: `added` are new, and each of `updated`
    /// was brought up to date from what it was.
    Imported { what: &'static str, store: usize, folder: FolderId, added: Vec<u64>, updated: Vec<(u64, Box<Item>)> },
    /// Rules were run on messages of a store at once (Run Rules Now): each as it was, where it
    /// is now and the copies made of it.
    Ruled { what: &'static str, store: usize, done: Vec<Ruled> },
}

/// What a rule did to one message, with what is needed to take it back.
#[derive(Clone, Debug)]
pub struct Ruled {
    pub from: FolderId,
    pub before: Box<Item>,
    /// Where it is now.
    pub at: FolderId,
    pub id: u64,
    pub copies: Vec<(FolderId, u64)>,
}

impl Step {
    /// The command that did it, as its label reads.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Edited { what, .. }
            | Self::Moved { what, .. }
            | Self::Removed { what, .. }
            | Self::Imported { what, .. }
            | Self::Ruled { what, .. } => what,
            Self::Discarded => "Discard",
        }
    }
}

impl Session {
    /// Keep `step` as the newest thing that can be taken back.
    pub fn did(&mut self, step: Step) {
        if self.undo.len() >= MAX_STEPS {
            self.undo.remove(0);
        }
        self.undo.push(step);
    }

    /// The message that was `was` in `folder` is `now`: older steps follow it.
    fn renumber(&mut self, in_store: usize, in_folder: FolderId, was: u64, now: u64) {
        for step in &mut self.undo {
            match step {
                Step::Edited { store, folder, id, .. } | Step::Moved { store, to: folder, id, .. }
                    if *store == in_store && *folder == in_folder && *id == was =>
                {
                    *id = now;
                }
                // What was made or brought up to date is known by its id too, so a delete
                // taken back before it does not leave it behind.
                Step::Ruled { store, done, .. } if *store == in_store => {
                    for r in done.iter_mut().filter(|r| r.at == in_folder && r.id == was) {
                        r.id = now;
                    }
                }
                Step::Imported { store, folder, added, updated, .. } if *store == in_store && *folder == in_folder => {
                    for id in added.iter_mut().filter(|id| **id == was) {
                        *id = now;
                    }
                    for (id, _) in updated.iter_mut().filter(|(id, _)| *id == was) {
                        *id = now;
                    }
                }
                _ => {}
            }
        }
    }

    /// Take back the newest step, and show the message it was done to. A step that can no
    /// longer be taken back (its message or its folder is gone) is dropped, and says so.
    pub fn undo_last(&mut self) -> Result<&'static str, EngineError> {
        let step = self.undo.pop().ok_or_else(|| EngineError::Disabled("Undo".into()))?;
        let what = step.what();
        let gone = |e: StoreError| EngineError::Format(format!("{what} can no longer be undone: {e}"));
        let (at_store, at_folder, at_id) = match step {
            Step::Discarded => {
                let draft = self.set_aside.pop().ok_or(StoreError::NoItem).map_err(gone)?;
                self.compose = Some(draft);
                return Ok(what);
            }
            Step::Edited { store, folder, id, before, .. } => {
                let st = self.stores.get_mut(store).ok_or(StoreError::NoFolder).map_err(gone)?;
                st.load(folder, ItemId(id)).map_err(gone)?;
                st.update(folder, ItemId(id), *before).map_err(gone)?;
                (store, folder, id)
            }
            Step::Moved { store, from, was, to, id, .. } => {
                if self.folder_info(store, from).is_none() {
                    return Err(gone(StoreError::NoFolder));
                }
                let st = self.stores.get_mut(store).ok_or(StoreError::NoFolder).map_err(gone)?;
                let item = st.remove(to, ItemId(id)).map_err(gone)?;
                let back = match st.add(from, item.clone()) {
                    Ok(back) => back.0,
                    Err(e) => {
                        // It stays where it was rather than be nowhere.
                        st.add(to, item)?;
                        return Err(gone(e));
                    }
                };
                self.renumber(store, from, was, back);
                (store, from, back)
            }
            Step::Imported { store, folder, added, updated, .. } => {
                // All of it is taken back that still can be; what is gone is gone already.
                let st = self.stores.get_mut(store).ok_or(StoreError::NoFolder).map_err(gone)?;
                for id in added {
                    let _ = st.remove(folder, ItemId(id));
                }
                for (id, before) in updated {
                    let _ = st.update(folder, ItemId(id), *before);
                }
                self.refresh_folders(store);
                self.reload_calendar()?;
                self.reload_people()?;
                self.reload_tasks()?;
                return Ok(what);
            }
            Step::Ruled { store, done, .. } => {
                // Each message is put back as it was, where it was, and its copies go; one gone
                // since is left as it is.
                let mut left = 0usize;
                let mut back_at = Vec::new();
                for r in done.into_iter().rev() {
                    let Some(st) = self.stores.get_mut(store) else { break };
                    for (f, id) in &r.copies {
                        let _ = st.remove(*f, ItemId(*id));
                    }
                    let put = if r.at == r.from {
                        st.update(r.from, ItemId(r.id), *r.before).map(|()| r.id)
                    } else {
                        match st.remove(r.at, ItemId(r.id)) {
                            Ok(now) => match st.add(r.from, *r.before) {
                                Ok(back) => Ok(back.0),
                                Err(e) => {
                                    // It stays where the rule put it rather than be nowhere.
                                    let _ = st.add(r.at, now);
                                    Err(e)
                                }
                            },
                            Err(e) => Err(e),
                        }
                    };
                    match put {
                        Ok(back) => back_at.push((r.from, r.at, r.id, back)),
                        Err(_) => left += 1,
                    }
                }
                for (from, at, was, back) in back_at {
                    if at != from {
                        self.renumber(store, at, was, back);
                    }
                }
                self.refresh_folders(store);
                self.reload_rows()?;
                if self.selected.is_some_and(|id| !self.rows.iter().any(|r| r.id == id)) {
                    self.selected = None;
                    self.open = None;
                }
                if left > 0 {
                    return Err(EngineError::Format(format!("{what} was undone but for {left} message(s) changed or gone since")));
                }
                return Ok(what);
            }
            Step::Removed { store, folder, was, mut item, .. } => {
                crate::cmd::imap_sync::back(self, store, &mut item);
                let st = self.stores.get_mut(store).ok_or(StoreError::NoFolder).map_err(gone)?;
                let back = st.add(folder, *item).map_err(gone)?.0;
                self.renumber(store, folder, was, back);
                (store, folder, back)
            }
        };
        self.refresh_folders(at_store);
        if self.module == crate::Module::Tasks {
            // A task taken back is chosen where it is, among the others.
            self.tasks.selected = Some((at_store, at_folder, at_id));
            self.reload_tasks()?;
            if self.tasks.rows.iter().any(|r| r.key() == (at_store, at_folder, at_id)) {
                return Ok(what);
            }
        }
        if self.module == crate::Module::People {
            // A contact taken back is chosen where it is, among the others.
            self.reload_people()?;
            if crate::cmd::people::people_folder(self) == Some((at_store, at_folder)) {
                self.people.selected = Some(at_id);
                return Ok(what);
            }
        }
        if self.current != Some((at_store, at_folder)) {
            self.select_folder(at_store, at_folder)?;
        } else {
            self.reload_rows()?;
        }
        if self.module == crate::Module::Calendar {
            self.reload_calendar()?;
        }
        // Shown as it is: looking at it again does not mark it read.
        let st = self.stores.get_mut(at_store).ok_or(StoreError::NoFolder)?;
        let item = st.load(at_folder, ItemId(at_id))?;
        self.selected = Some(at_id);
        self.open = Some(item);
        Ok(what)
    }
}
