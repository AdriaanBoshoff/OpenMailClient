# File format parity

**Status:** revision 36 · 2026-10-19 · folders of any kind made in a data file, kept in the journal and saved as that kind (M5.10); meetings to and from Outlook: a request sent as its calendar alone read with its words, and meeting messages kept by their properties alone read and written (M5.7); a meeting's organizer, attendees, answers and sequence read and written, in data files as [MS-OXOCAL] keeps them and in calendar files and iMIP messages (M5.7); an appointment's time zone read and written with its summer time, in data files as [MS-OXOCAL] keeps it and in calendar files as VTIMEZONE (M5.6); `.ics` calendar files read into appointments and written from them, as [MS-OXCICAL] maps the one onto the other (M5.4); recurring appointments and their exceptions read and written as Outlook keeps them, and Outlook reads ours (M5.3); a 4K-page file's larger data blocks are written deflated, as Outlook stores them, and Outlook opens such files (M3c.7); an `.ost` is exported with its hidden items and folder properties, and Outlook opens the export (M3c.6); a file from before M3c.1 is written whole on its next save and then opens in Outlook (M3c.5); a message changed by a save has as its size what is now stored of it (M3c.4); once a data file has been saved, later changes are saved on their own (M3c.3); data files with 4K pages are written, take changes and are saved as that kind, and Outlook Classic opens one of ours (M3c.2); files we write open in Outlook Classic with every folder and message as written, whole, ANSI and saved into in place (M3c.1); a save writes what changed into the data file where it is, with an undo file beside it until the save is checked (M3b.29); ANSI data files are written and an open one takes changes and stays ANSI (M3b.21); the structure check covers ANSI and 4K-page files (M3b.20); an export of any mailbox runs on a thread of its own (M3b.19); values of another type than their column's are shown as the column holds them, the structure check hears Stop and compares fill levels (M3b.18); attachments with new contents keep what is stored on them, unreadable ones are carried (M3b.17); values keep their kind and a saved file goes on from the header's counters (M3b.16); an exported file names the other modules' folders by id (M3b.15); an export of a Unicode `.pst` runs on a thread of its own (M3b.14); the structure check looks inside nodes (M3b.13); an attachment keeps what is stored on it when a message's attachments change, and search-folder rows of changed messages are brought up to date (M3b.11); the journal is kept in a folder of our own when the data file's folder cannot be written (M3b.7); `file.export` copies a Unicode `.pst` as it is stored (M3b.2); unsaved changes are journalled beside the file (M3b.1)

What each format carries in each direction, and how it was checked. "Outlook" means Outlook Classic
(version 16.0) driven through its automation interface on the dev machine: it opens the file and reports
the item's properties. No window is driven and nothing in a mailbox is changed. Control files Outlook
writes for these checks stay local and are never committed.

## `.msg` and `.oft` ([MS-OXMSG])

| What | OpenMailClient → Outlook | Outlook → OpenMailClient | Outlook → OpenMailClient → Outlook |
|---|---|---|---|
| Message class, subject | read back the same | read | same |
| Sender name, address, address type | read back the same | not checked (control items have no sender) | not checked |
| To and Cc recipients, with type | read back the same | not checked | not checked |
| Received and sent times | read back the same | read | same |
| Read state, importance, follow-up flag | read back the same | read | same |
| Categories (named property `Keywords`) | read back the same, after the fix below | read | same |
| Plain text body | read back the same | read | same |
| HTML body | Outlook reports an HTML item with the same text | not checked (Outlook stored its HTML as RTF) | not checked |
| RTF body (compressed, [MS-OXRTFCP]) | an RTF-only message (stored form, not compressed by us) is shown as rich text with its formatting, once PidTagRtfInSync is set; without it Outlook shows an empty plain-text body | decoded for reading (text, preview, formatted body with its pictures and the places of its attachments; wrapped HTML or text taken back out); the bytes themselves are kept untouched | preserved: Outlook shows the HTML again |
| File attachment: name, size, by-value | read back the same | not checked | not checked |
| Inline picture (attachment with a content id, `cid:` in the HTML) | drawn in the body at the size asked for | not checked | not checked |
| Attached message | not checked in Outlook | not checked | not checked |
| Attached object of another program (attach method 6, a storage: [MS-OXMSG] 2.2.2.2) | written as the storage it was; not checked in Outlook | read whole, every stream and storage with classes and state (our own round trip is pinned by a test) | not checked |
| Properties OpenMailClient does not interpret | n/a | kept | preserved (8 named properties survive) |
| Template (`.oft`) opened as a new item | subject, body, recipients, attachment, categories arrive | read | not checked |

Pinned by `crates/msg/src/tests.rs` (round trip through our own reader, layout, hostile input, and the
named-property lookup checksum).

### Found by this check

- **Named property lookup checksum.** Outlook locates a named property through the lookup stream chosen
  by a checksum of its name ([MS-OXMSG] 2.2.3.2.1). That checksum is the one in [MS-OXRTFCP] 3.1.3.2,
  which starts from zero and is not inverted at the end. OpenMailClient used the common CRC-32, so Outlook
  opened the file but showed no categories. Fixed; `named_property_lookup_uses_the_zero_seeded_checksum`.

### Not yet checked

Messages with an attached message, inline pictures, non-Latin text in every field, very large
attachments, meeting requests, contacts and tasks saved as `.msg`, and opening by double-click in the
Outlook window (only the automation interface was used).

## `.pst` and `.ost` ([MS-PST])

Read: ANSI, Unicode and 4K-page files. Written: Unicode and ANSI files (M3b.21) and 4K-page files (M3c.2); in an ANSI file text is 8-bit in the code page it will be read in when those bytes read back the same, and Unicode text otherwise. See [gaps.md](gaps.md) items 2, 7 and 8.

Written so far (M3.1, M3.2), Unicode with 512-byte pages only:

| Part ([MS-PST]) | Written | Checked by |
|---|---|---|
| Header, both checksums (2.2.2.6) | yes | our reader, our structure check; the checksum against the specification's sample header |
| Allocation maps, page maps, density list (2.2.2.7.2 to 2.2.2.7.4) | yes | our structure check |
| Free map and free page map pages (2.2.2.7.5, 2.2.2.7.6) | yes; their places are worked out, the specification gives none | a 33 MB file of ours; no Outlook-written file |
| Blocks, plain, permuted and cyclic (2.2.2.8) | yes | read back in each encoding |
| Data trees, one and two levels (2.2.2.8.3.2) | yes | read back, 33 MB item |
| Subnode trees with an index block (2.2.2.8.3.3) | yes | read back, 800 subnodes |
| Block and node B-trees (2.2.2.7.7) | yes | read back, 1,000 nodes |
| Heaps over many blocks, with fill levels (2.3.1) | yes; which half of a byte holds the earlier block's level is our reading | read back, more than eight blocks; our structure check (page maps; not the fill levels) |
| Index in a heap, any depth (2.3.2) | yes | read back, 3,000 records; our structure check |
| Property list; large values in subnodes (2.3.3) | yes; a value typed as an object that names no subnode is left out | read back, every value type of the model; our structure check |
| Table; rows in the heap or in a subnode (2.3.4) | yes | read back, 6,000 rows; our structure check |
| The nodes every file must have: message store, name-to-id map, root, search folders, table templates (2.7.1) | yes | read back; structure check clean |
| Folders with their hierarchy, contents and associated-contents tables (2.4.4) | name, class, counts, and the source's own properties and hidden items where it keeps them (M3c.6); a carried property the hierarchy table has a column for is in the folder's row too | read back, 1,500 messages in a folder |
| Messages with recipients and attachments (2.4.5, 2.4.6) | yes; properties we do not understand go out as they were read | read back, sample mailbox and three local files (counts only) |
| A message attached to a message (2.4.6.2.3) | yes, to 32 deep; where it is kept is our reading | read back; Outlook lists it as an attached message with its name (M3c.1) |
| Named properties (2.4.7) | yes; the bucket of a name is our reading | read back, 600 names |
| Recurring appointments ([MS-OXOCAL] 2.2.1.44 pattern in PidLidAppointmentRecur; clip start and end, recurrence type, time zone and pattern in words) and their exceptions (hidden attached messages, 2.2.10.1) | yes: daily, weekly, monthly by date, by nth day and on the last day, yearly; ends never, after a count or on a date; occurrences deleted and changed (subject, location, Show As, reminder, time), only what differs written; a pattern we cannot read is kept as stored and the appointment shown once | read back after export and reopening; Outlook Classic reads three series of ours with their patterns, a changed occurrence with its subject, location and time, and a deleted one (M5.3); a series is kept in its own zone with its summer time, read back after export (M5.6) |
| Hidden (associated) items; folder properties beyond name, class and counts | kept by `file.save` and by `file.export` of a `.pst` (copied as stored); carried by `file.export` from an `.ost`, with entry ids naming its folders made to name them in the new file (M3c.6), a folder's named properties left out | a hidden message and a folder property our writer never makes survive a save, a copy and an export from an `.ost`; Outlook shows the 32 hidden items of an exported `.ost` |
| Which folder is the Calendar, Contacts, Tasks, Notes, Journal and Drafts (ids on the root and the Inbox, [MS-OXOSFLD] 2.2.3) | kept by `file.save` and by `file.export` of a Unicode `.pst`; written by `file.export` from other sources; read from the root | read back |
| Changing a Unicode `.pst` that is open (`file.save`) | yes: nodes not touched are copied block for block; a changed message keeps every stored property that was not changed, also those under ids no name stands for; when its attachments change, each attachment the file holds stays in its subnode with every stored property (matched by being the same, or by its contents), and only new ones are written from the model; a search folder's table loses the rows of deleted messages and shows changed ones as they are now | read back: messages changed, moved, deleted and added, folders made, renamed, moved and deleted; attachments reordered, renamed, added and removed with a property no name stands for on each; a search table with rows written as another program would; damaged files; three local copies (counts only) |
| Changing an ANSI `.pst` that is open (`file.save`) | yes, as a Unicode one, and it is saved as an ANSI file (M3b.21) | read back over three saves, each held against the structure check; no Outlook-written ANSI file tried |
| Changing a 4K-page `.pst` that is open (`file.save`) | yes, written whole as a 4K-page file (M3c.2); not into the file where it is | read back after a save, held against the structure check |
| Changing an `.ost` | no: read-only, it is a server's copy | never |

**Seen in Outlook (M3c.1, 2026-10-13).** With the owner's say, copies of files this writer made
were added to the Outlook profile as extra data files through its automation interface, read and
taken out again, and held against Outlook's Inbox Repair Tool (scan only). Before this every file
we wrote was refused. What Outlook asks for beyond [MS-PST] 2.7.1, as the repair tool named it:
the receive folder table (node 0x62B), the outgoing queue (0x64C), templates 0x6D7 and 0x6F8 (with
a four-byte row index even in an ANSI file), the search update queue (0xEC1), one search folder
(0x2223 with 0x2226, 0x2227, 0x2230, listed in 0x201), store property 0x67FF, more columns in the
folder table templates, recipient row ids from 1, PidTagAttachSize as the sum of the attachment's
stored values, PidTagMessageSize as the bytes of the message node and its subnodes (for a message changed by a
save too, in place or whole, M3c.4), and an
attached object's storage in a subnode of kind 0x09. The tests in `crates/pst/src/tests_outlook.rs`
hold the writer to these.

| File | Repair tool (scan) | Outlook |
|---|---|---|
| A whole Unicode `.pst` (`file.export`) | only that it makes node 0xC01 and deletes an SDO | opens; every folder with its count and unread count; default folders found; every message with subject, sender, recipients, read and flag state, categories, importance, size, received time, body, attachments |
| A whole ANSI `.pst` | the same | the same |
| A file saved into in place (read, flag, category, a deletion) | the same | the same, with the changes |
| An `.ost` Outlook wrote, copied node for node into a `.pst` | 232 findings: the store's and folders' entries of an offline store | refused (M3c.6) |
| The same `.ost` exported from the model, Unicode, ANSI and 4K-page (M3c.6) | the same as any file of ours | opens; 36 folders, both messages, every default folder, all 32 hidden items; read back clean after Outlook |
| A file the build before M3c.1 wrote, Unicode and ANSI | 314 and 306 findings: missing nodes and columns, row and size mismatches | refused (M3c.1) |
| The same files after one save here (M3c.5: written whole, ids kept) | the same two as any file of ours | opens; every folder and message, the change saved; read back clean after Outlook |
| A file Outlook opened and grew, read back here | — | every item read; our structure check finds nothing |

Not seen: node 0xC01 (Outlook makes it itself; its layout is in no specification); a file that
lacks what Outlook asks for and whose table names a hidden item that is not there is saved as
stored, and stays as Outlook refuses it; an attached object that is not a real storage shows
as no attachment; no Outlook-written 512-byte-page `.pst` has been saved into. Each has a row in
ROADMAP, Do later.

**4K pages (M3c.2).** A data file with wVer 36, the kind newer versions of Outlook make, is in
no specification. What is written was measured on the few small Outlook-written `.ost` files kept
locally (their layout only, never their contents) and on what Outlook's Inbox Repair Tool asked of
files of ours:

| Part | Written (measured, not specified) |
|---|---|
| Header | "SM", wVer 36, wVerClient 19 (the repair tool asks 19 of a `.pst`; the `.ost` files measured have 12), the two maps of the header zero, bCryptMethod 0 |
| Pages | 4096 bytes: 4072 of body and a trailer of 24 (ptype twice, wSig, crc, bid, then a 16-bit 1) |
| Allocation maps | one heads each section of 4072 × 8 × 512 bytes (16 MB), the first at 0x22000, one bit for 512 bytes; no page maps, free maps or free page maps |
| Density list | at 0x21000, empty, carrying the header's next page id |
| Blocks | up to 65,512 bytes of data on 512-byte slots, a trailer of 24 (cb, wSig, crc, bid, a 16-bit 2, the inflated size, four zero bytes); not encoded; a data block whose bytes and trailer pass one slot is stored deflated (zlib, RFC 1950/1951) when that makes it smaller, as Outlook stores them (M3c.7), internal blocks never |
| B-tree pages | 4056 bytes of entries, 16-bit counts at 4056 and 4058, entry size at 4060 and level at 4061; block-tree leaves carry the inflated size |
| Heaps | as [MS-PST] 2.3.1, with blocks of 64 KB; a HID past the first block has the block number at bit 19, our reading (no Outlook-written heap of more than one block was at hand) |
| Contents tables | as [MS-PST] 2.3.4, with two subnodes more, 0x521 and 0x541: heaps with the client signature 0xA5 ([MS-PST] 2.3.1.2 calls it reserved), written empty. The repair tool reads a contents table without them, the template 0x60E too, as broken |

| File | Repair tool (scan) | Outlook |
|---|---|---|
| A whole 4K-page `.pst` (`file.export` with `"kind": "4k"`) | only that it makes an HMP, as of our Unicode files | opens; the same folders, counts, unread counts and 17 messages, field for field, as a Unicode file of the same mailbox opened beside it |
| 4K-page files with deflated blocks, from the sample and from an `.ost` (M3c.7) | only that it makes an HMP | opens; every folder and message, and the 32 hidden items of the `.ost`; read back clean after Outlook |
| The same file after Outlook opened it, read back here | — | Outlook added Calendar, Contacts and Tasks, as to any store it first opens; every item read; our structure check finds nothing |

Not known, and written as said or not at all (each a row in ROADMAP, Do later): what 0x521 and
0x541 hold for a table with rows; where Outlook puts the block number of a HID; what heads a
section after the first in a file Outlook grew (so a 4K-page file is saved whole, never into the
file where it is).

`file.export` writes the new file beside the target, holds it
against the structure check and only then puts it in the target's place. `file.save` does the
same for an open file, reads the new file back and compares it with what was shown, refuses
when another program wrote the file since it was opened or has it open, and on the first save
keeps the file as it was beside it (`name.before-openmailclient.pst`). Later saves write what changed into the file where it is (M3b.29): new blocks and pages go into its free end and past it, blocks that are as they were are named once more and not written again, and the header goes in last. What such a save writes over is kept in an undo file of our own beside the data file (`name.pst.openmailclient-undo`) until the file has passed the structure check and read back as what was shown; a save that fails, or a run that ends in the middle of one, leaves a file that is put back byte for byte, and until it is put back it is read as it was. Space such saves leave unused is given back by a whole rewrite once it passes 4 MB or a quarter of the file; `file.saveInPlace` turns writing into the file off. Once a file has been saved, later changes are saved on their own after five minutes at a quiet moment, the same way (M3c.3, `file.autoSave`); a file's first save is never made on its own. No Outlook-written `.pst` has been saved into, and none saved this way has been opened in Outlook. Until a save, each change is journalled in a file of our own beside the data file (`name.pst.openmailclient-journal`: our format, not Outlook's, and never read by it) and played back when the file is opened again; when the folder cannot be written the journal is kept in a folder of our own beside the settings instead (M3b.7); the data file is written only by a save. `openmailclient-cli inspect file.pst --check` holds a file's structure against
the format, for every kind we read (M3b.20).

## `.ics` (RFC 5545, [MS-OXCICAL])

Read and written (M5.4, `crates/formats/src/ics.rs`); checked through our own reader and by hand
against RFC 5545's examples. Outlook has not yet been given one of ours.

| What | Read | Written |
|---|---|---|
| SUMMARY, LOCATION, DESCRIPTION, CATEGORIES, CLASS | yes | yes |
| DTSTART, DTEND, DURATION; UTC, floating and VTIMEZONE times | yes; floating times taken as where we are | yes, in the event's own zone, defined in the file |
| All-day dates | yes | yes, as dates |
| TRANSP, X-MICROSOFT-CDO-BUSYSTATUS | yes | yes |
| The first VALARM | the reminder | the reminder (DISPLAY) |
| RRULE: daily, weekly, monthly, yearly, BYDAY, BYMONTHDAY, BYSETPOS, COUNT, UNTIL | yes | yes |
| EXDATE and RECURRENCE-ID | the series' deleted and changed occurrences | yes |
| UID | the GlobalObjectId, so a file opened again updates what is there | yes |
| RDATE, an RRULE no Outlook pattern holds | the first time alone, with a note | |
| ATTENDEE, ORGANIZER, SEQUENCE | yes: a meeting, its attendees by role (ROLE, CUTYPE) with their answers (PARTSTAT), kept as [MS-OXOCAL] keeps them (M5.7) | yes ([MS-OXCICAL] 2.1.3.1.1.20) |
| METHOD (RFC 5546): PUBLISH, REQUEST, REPLY, CANCEL | yes: a request answered from its message, a reply and a cancellation taken in from the right sender | yes, in iMIP messages (RFC 6047) |
| A meeting sent as its calendar alone, as Outlook sends one | yes: the meeting, and DESCRIPTION as its body | |
| A meeting's message kept by its properties alone, as Outlook keeps one in a data file or `.msg` ([MS-OXOCAL] 2.2.6 to 2.2.8) | yes: requests, answers and cancellations | yes: every meeting message received or sent is given the GlobalObjectId, state flags, sequence and answer Outlook reads |
| VTODO, VJOURNAL | counted, not read (M5.9) | |
| A zone's daylight rules | yes: a VTIMEZONE's yearly rules, or a zone known by its TZID; other rules at the offset of the event's start | yes, a yearly RRULE for each change (M5.6); rules by year later (ROADMAP, Do later) |

## `.eml` (RFC 5322, MIME)

Read and written; checked only through our own reader so far.
