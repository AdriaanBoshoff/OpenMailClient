//! An in-memory store: the sample mailbox, loose `.msg`/`.eml` files, and the working copy other
//! stores are written from.

use crate::{FolderId, FolderInfo, FolderKind, Item, ItemId, Store, StoreError, Summary};

#[derive(Clone, Debug)]
struct MemFolder {
    info: FolderInfo,
    items: Vec<(u64, Item)>,
}

#[derive(Clone, Debug)]
pub struct MemStore {
    name: String,
    folders: Vec<MemFolder>,
    next_id: u64,
}

impl MemStore {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), folders: Vec::new(), next_id: 1 }
    }

    /// A store with Outlook's default mail folders.
    pub fn with_default_folders(name: &str) -> Self {
        let mut s = Self::new(name);
        for n in ["Inbox", "Drafts", "Sent Items", "Deleted Items", "Archive", "Junk Email", "Outbox"] {
            s.push_folder(None, n, "IPF.Note");
        }
        s
    }

    fn fresh_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    pub fn push_folder(&mut self, parent: Option<FolderId>, name: &str, container_class: &str) -> FolderId {
        let id = FolderId(self.fresh_id());
        let info = FolderInfo {
            id,
            parent,
            name: name.to_string(),
            kind: FolderKind::guess(name, container_class),
            container_class: container_class.to_string(),
            total: 0,
            unread: 0,
            has_children: false,
            hidden: false,
        };
        self.folders.push(MemFolder { info, items: Vec::new() });
        if let Some(p) = parent
            && let Some(pf) = self.folders.iter_mut().find(|f| f.info.id == p)
        {
            pf.info.has_children = true;
        }
        id
    }

    pub fn folder_by_kind(&self, kind: FolderKind) -> Option<FolderId> {
        self.folders.iter().find(|f| f.info.kind == kind).map(|f| f.info.id)
    }

    pub fn folder_by_name(&self, name: &str) -> Option<FolderId> {
        self.folders.iter().find(|f| f.info.name.eq_ignore_ascii_case(name)).map(|f| f.info.id)
    }

    pub fn push_item(&mut self, folder: FolderId, item: Item) -> Option<ItemId> {
        let id = self.fresh_id();
        let f = self.folders.iter_mut().find(|f| f.info.id == folder)?;
        f.items.push((id, item));
        recount(f);
        Some(ItemId(id))
    }

    fn folder_mut(&mut self, folder: FolderId) -> Result<&mut MemFolder, StoreError> {
        self.folders.iter_mut().find(|f| f.info.id == folder).ok_or(StoreError::NoFolder)
    }
}

fn recount(f: &mut MemFolder) {
    f.info.total = u32::try_from(f.items.len()).unwrap_or(u32::MAX);
    f.info.unread = u32::try_from(f.items.iter().filter(|(_, i)| !i.is_read()).count()).unwrap_or(u32::MAX);
}

impl Store for MemStore {
    fn display_name(&self) -> String {
        self.name.clone()
    }

    fn writable(&self) -> bool {
        true
    }

    fn apart(&mut self) -> Result<Option<crate::Apart>, StoreError> {
        let copy = self.clone();
        Ok(Some(Box::new(move |_| Ok(Box::new(copy) as Box<dyn Store>))))
    }

    fn folders(&mut self) -> Result<Vec<FolderInfo>, StoreError> {
        Ok(self.folders.iter().map(|f| f.info.clone()).collect())
    }

    fn list(&mut self, folder: FolderId) -> Result<Vec<Summary>, StoreError> {
        let f = self.folder_mut(folder)?;
        Ok(f.items.iter().map(|(id, it)| it.summary(*id)).collect())
    }

    fn load(&mut self, folder: FolderId, id: ItemId) -> Result<Item, StoreError> {
        let f = self.folder_mut(folder)?;
        f.items.iter().find(|(i, _)| *i == id.0).map(|(_, it)| it.clone()).ok_or(StoreError::NoItem)
    }

    fn update(&mut self, folder: FolderId, id: ItemId, item: Item) -> Result<(), StoreError> {
        let f = self.folder_mut(folder)?;
        let slot = f.items.iter_mut().find(|(i, _)| *i == id.0).ok_or(StoreError::NoItem)?;
        slot.1 = item;
        recount(f);
        Ok(())
    }

    fn add(&mut self, folder: FolderId, item: Item) -> Result<ItemId, StoreError> {
        self.push_item(folder, item).ok_or(StoreError::NoFolder)
    }

    fn remove(&mut self, folder: FolderId, id: ItemId) -> Result<Item, StoreError> {
        let f = self.folder_mut(folder)?;
        let pos = f.items.iter().position(|(i, _)| *i == id.0).ok_or(StoreError::NoItem)?;
        let (_, item) = f.items.remove(pos);
        recount(f);
        Ok(item)
    }

    fn create_folder(&mut self, parent: FolderId, name: &str) -> Result<FolderId, StoreError> {
        let class = self.folder_mut(parent)?.info.container_class.clone();
        Ok(self.push_folder(Some(parent), name, &class))
    }

    fn create_top_folder(&mut self, name: &str) -> Result<FolderId, StoreError> {
        Ok(self.push_folder(None, name, "IPF.Note"))
    }

    fn create_folder_of(&mut self, parent: Option<FolderId>, name: &str, class: &str) -> Result<FolderId, StoreError> {
        if let Some(p) = parent {
            self.folder_mut(p)?;
        }
        Ok(self.push_folder(parent, name, class))
    }

    fn rename_folder(&mut self, folder: FolderId, name: &str) -> Result<(), StoreError> {
        self.folder_mut(folder)?.info.name = name.to_string();
        Ok(())
    }

    fn delete_folder(&mut self, folder: FolderId) -> Result<(), StoreError> {
        self.folder_mut(folder)?;
        // Children go with the folder: collect the subtree, then drop it.
        let mut doomed = vec![folder];
        let mut i = 0;
        while let Some(&cur) = doomed.get(i) {
            doomed.extend(self.folders.iter().filter(|f| f.info.parent == Some(cur)).map(|f| f.info.id));
            i += 1;
        }
        self.folders.retain(|f| !doomed.contains(&f.info.id));
        self.mark_parents();
        Ok(())
    }

    fn move_folder(&mut self, folder: FolderId, parent: FolderId) -> Result<(), StoreError> {
        self.folder_mut(folder)?;
        self.folder_mut(parent)?;
        // The folder and all under it, in the order they are listed.
        let mut moved = vec![folder];
        let mut i = 0;
        while let Some(&cur) = moved.get(i) {
            let under: Vec<FolderId> =
                self.folders.iter().filter(|f| f.info.parent == Some(cur) && !moved.contains(&f.info.id)).map(|f| f.info.id).collect();
            moved.extend(under);
            i += 1;
        }
        if moved.contains(&parent) {
            return Err(StoreError::Format("a folder cannot be moved into itself or a folder inside it".into()));
        }
        self.folder_mut(folder)?.info.parent = Some(parent);
        // Parents stay listed before their children: the moved folders go after the last folder
        // that is under their new parent.
        let (mut taken, mut rest): (Vec<MemFolder>, Vec<MemFolder>) =
            std::mem::take(&mut self.folders).into_iter().partition(|f| moved.contains(&f.info.id));
        taken.sort_by_key(|f| moved.iter().position(|id| *id == f.info.id));
        let mut under = vec![parent];
        let mut after = 0;
        for (at, f) in rest.iter().enumerate() {
            if f.info.id == parent || f.info.parent.is_some_and(|p| under.contains(&p)) {
                under.push(f.info.id);
                after = at + 1;
            }
        }
        let tail = rest.split_off(after.min(rest.len()));
        rest.extend(taken);
        rest.extend(tail);
        self.folders = rest;
        self.mark_parents();
        Ok(())
    }
}

impl MemStore {
    fn mark_parents(&mut self) {
        let parents: Vec<FolderId> = self.folders.iter().filter_map(|f| f.info.parent).collect();
        for f in &mut self.folders {
            f.info.has_children = parents.contains(&f.info.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_follow_items() {
        let mut s = MemStore::with_default_folders("Test");
        let inbox = s.folder_by_kind(FolderKind::Inbox).unwrap();
        let mut it = Item::new_mail();
        it.set_read(false);
        let id = s.add(inbox, it).unwrap();
        assert_eq!(s.folders().unwrap().iter().find(|f| f.id == inbox).unwrap().unread, 1);
        let mut it = s.load(inbox, id).unwrap();
        it.set_read(true);
        s.update(inbox, id, it).unwrap();
        assert_eq!(s.folders().unwrap().iter().find(|f| f.id == inbox).unwrap().unread, 0);
        s.remove(inbox, id).unwrap();
        assert!(matches!(s.load(inbox, id), Err(StoreError::NoItem)));
    }

    #[test]
    fn deleting_a_folder_takes_its_subtree() {
        let mut s = MemStore::with_default_folders("Test");
        let inbox = s.folder_by_kind(FolderKind::Inbox).unwrap();
        let a = s.create_folder(inbox, "A").unwrap();
        let b = s.create_folder(a, "B").unwrap();
        s.delete_folder(a).unwrap();
        let ids: Vec<FolderId> = s.folders().unwrap().iter().map(|f| f.id).collect();
        assert!(!ids.contains(&a) && !ids.contains(&b) && ids.contains(&inbox));
        assert!(!s.folders().unwrap().iter().find(|f| f.id == inbox).unwrap().has_children);
    }
}
