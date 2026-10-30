//! The Search Management Queue ([MS-PST] 2.4.8.4.1): what a save changed, told to the program
//! that keeps a data file's search folders, so that it works out again which searches each
//! message changed belongs to. The criteria of a search are its own and are not to be read
//! or changed ([MS-PST] 2.4.8.6.4), so a search folder's members are not worked out here:
//! the queue is how the specification has them kept in step, and a change not queued leaves
//! a search folder out of step with what it searches (2.4.8.4.1).

/// The node of the queue ([MS-PST] 2.4.8.4.1).
pub(crate) const NID_SEARCH_MANAGEMENT_QUEUE: u32 = 0x1E1;

/// One SUD is two bytes of flags, two of type and sixteen of data ([MS-PST] 2.4.8.1.1).
pub(crate) const SUD_SIZE: usize = 20;

/// More than this many updates in the queue and none are added: a search folder is then
/// looked through again whole by the program that keeps it, sooner than a queue grown
/// without end is read.
const MAX_QUEUED: usize = 200_000;

/// One change, as a Search Update Descriptor ([MS-PST] 2.4.8.1.1, the types of 2.4.8.1.1 and
/// the data of 2.4.8.2). The flags are 0, as Outlook was seen to queue them in a file of its
/// own (2026-10-30): SUDF_DOMAIN_CHECKED is for the program that checks them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sud {
    /// SUDT_MSG_ADD: a message put into `parent` (2.4.8.2.1).
    MsgAdd { parent: u32, msg: u32 },
    /// SUDT_MSG_MOD: a message of `parent` changed (2.4.8.2.1).
    MsgMod { parent: u32, msg: u32 },
    /// SUDT_MSG_DEL: a message taken out of `parent` (2.4.8.2.1).
    MsgDel { parent: u32, msg: u32 },
    /// SUDT_MSG_MOV: a message moved from `old` to `new` (2.4.8.2.2).
    MsgMov { new: u32, msg: u32, old: u32 },
    /// SUDT_FLD_ADD: a folder made in `parent` (2.4.8.2.3).
    FldAdd { parent: u32, fld: u32 },
    /// SUDT_FLD_MOD: a folder changed (2.4.8.2.4).
    FldMod { fld: u32 },
    /// SUDT_FLD_DEL: a folder deleted (2.4.8.2.4).
    FldDel { fld: u32 },
    /// SUDT_FLD_MOV: a folder moved into `parent` (2.4.8.2.3).
    FldMov { parent: u32, fld: u32 },
}

impl Sud {
    /// The SUD as it is stored: wFlags, wSUDType, then its SUDData, zero after what it uses.
    pub(crate) fn bytes(self) -> [u8; SUD_SIZE] {
        let (ty, words): (u16, &[u32]) = match &self {
            Sud::MsgAdd { parent, msg } => (0x0001, &[*parent, *msg]),
            Sud::MsgMod { parent, msg } => (0x0002, &[*parent, *msg]),
            Sud::MsgDel { parent, msg } => (0x0003, &[*parent, *msg]),
            Sud::MsgMov { new, msg, old } => (0x0004, &[*new, *msg, *old]),
            Sud::FldAdd { parent, fld } => (0x0005, &[*parent, *fld, 0, 0]),
            Sud::FldMod { fld } => (0x0006, &[*fld, 0]),
            Sud::FldDel { fld } => (0x0007, &[*fld, 0]),
            Sud::FldMov { parent, fld } => (0x0008, &[*parent, *fld, 0, 0]),
        };
        let mut out = [0u8; SUD_SIZE];
        let bytes = ty.to_le_bytes().into_iter().chain(words.iter().flat_map(|w| w.to_le_bytes()));
        for (slot, b) in out.iter_mut().skip(2).zip(bytes) {
            *slot = b;
        }
        out
    }
}

/// The queue's data once `suds` are added behind what is in it, and the head that goes with
/// it: those already taken from the front (before `head`, [MS-PST] 2.4.8.3) are left out, so
/// the head is at 0 again. `None` when the queue is not one that may be read (its length is
/// not a whole number of SUDs, or its head is not at one of them, 2.4.8.3) or is full: it is
/// then left as it is.
pub(crate) fn queued(was: &[u8], head: u32, suds: &[Sud]) -> Option<Vec<u8>> {
    let head = usize::try_from(head).ok()?;
    if !was.len().is_multiple_of(SUD_SIZE) || !head.is_multiple_of(SUD_SIZE) || head > was.len() {
        return None;
    }
    let waiting = was.get(head..)?;
    let count = (waiting.len() / SUD_SIZE).checked_add(suds.len())?;
    if count > MAX_QUEUED {
        return None;
    }
    let mut out = Vec::with_capacity(count.saturating_mul(SUD_SIZE));
    out.extend_from_slice(waiting);
    for sud in suds {
        out.extend_from_slice(&sud.bytes());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sud_is_laid_out_as_the_specification_and_outlook_have_it() {
        // As Outlook queued a folder it made, in a file of its own: flags 0, type 5, the
        // parent, the folder, then zeros.
        let b = Sud::FldAdd { parent: 0x8022, fld: 0x81A2 }.bytes();
        assert_eq!(b[..12], [0, 0, 5, 0, 0x22, 0x80, 0, 0, 0xA2, 0x81, 0, 0]);
        assert!(b[12..].iter().all(|x| *x == 0));
        let b = Sud::MsgMov { new: 0x8062, msg: 0x200024, old: 0x8082 }.bytes();
        assert_eq!(b[2..4], [4, 0]);
        assert_eq!(b[4..16], [0x62, 0x80, 0, 0, 0x24, 0, 0x20, 0, 0x82, 0x80, 0, 0]);
        assert_eq!(Sud::MsgMod { parent: 1, msg: 2 }.bytes()[2], 2);
        assert_eq!(Sud::FldDel { fld: 9 }.bytes()[2..8], [7, 0, 9, 0, 0, 0]);
    }

    #[test]
    fn what_was_taken_from_the_front_is_dropped_and_the_rest_waits_before_the_new() {
        let a = Sud::MsgAdd { parent: 1, msg: 2 };
        let b = Sud::MsgDel { parent: 1, msg: 3 };
        let c = Sud::FldMod { fld: 4 };
        let was = [a.bytes(), b.bytes()].concat();
        assert_eq!(queued(&was, 0, &[c]), Some([a.bytes(), b.bytes(), c.bytes()].concat()));
        assert_eq!(queued(&was, 20, &[c]), Some([b.bytes(), c.bytes()].concat()));
        assert_eq!(queued(&was, 40, &[c]), Some(c.bytes().to_vec()), "an emptied queue starts again");
        assert_eq!(queued(&[], 0, &[]), Some(Vec::new()));
    }

    #[test]
    fn a_queue_that_may_not_be_read_is_left_as_it_is() {
        let a = Sud::MsgAdd { parent: 1, msg: 2 };
        let was = a.bytes();
        assert_eq!(queued(&was[..19], 0, &[a]), None, "not a whole number of SUDs");
        assert_eq!(queued(&was, 7, &[a]), None, "a head inside a SUD");
        assert_eq!(queued(&was, 40, &[a]), None, "a head past the end");
        assert_eq!(queued(&was, u32::MAX, &[a]), None);
        // Full: no more is added, and nothing is lost.
        let many = vec![a; MAX_QUEUED];
        assert_eq!(queued(&[], 0, &many).map(|q| q.len()), Some(MAX_QUEUED * SUD_SIZE));
        assert_eq!(queued(&was, 0, &many), None);
    }
}
