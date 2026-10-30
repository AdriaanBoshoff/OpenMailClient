# OpenMailClient roadmap

**Status:** M2 (formatted reading) closed; M1's checks against Outlook still open; M3 (writing `.pst`) closed: a mailbox is written as a `.pst`, changes to an open one are kept when saved, settings are kept between runs; the do-later batch (M3b) closed; files we write open in Outlook (M3c.1); 4K-page data files written (M3c.2); changes saved on their own (M3c.3); a changed message's size (M3c.4); files from before M3c.1 opened (M3c.5); hidden items exported from an `.ost` (M3c.6); compressed 4K blocks (M3c.7); M3c closed; accounts (M4) in progress: IMAP, POP3 and SMTP clients (M4.1); TLS and the credential store (M4.2); accounts (M4.3); sending (M4.4); receiving (M4.5); Account Settings (M4.6); data files per account and messages laid out as Outlook lays them out (M4.7); pictures downloaded when asked for (M4.10); attachments opened on a double-click (M4.11); IMAP folders taken down as the server has them, the Sent copy put on the server, and IMAP kept in step both ways (M4.8); a server's own certificate trusted on the owner's word (M4.9); new mail told of and Send/Receive on a timer (M4.9); Show Progress (M4.9); Delay Delivery and Expires After (M4.9); only what changed fetched (M4.12); new mail told of as it comes (M4.12); shown as Outlook shows it (M1.8); IMAP folders followed both ways (M4.12); drafts on the server (M4.12); message places read apart from the window (M4.12); pictures decoded off the window's thread (M4.13); servers found from the address (M4.14) and its domain's DNS records (M4.15); M4 closed; mail shown as it comes down and closing while it does (M4.16); the Calendar (M5.1), Import from Outlook (M5.2) and recurring appointments (M5.3) and calendar files (M5.4); calendars kept in step with a CalDAV server (M5.5); time zones and daylight saving (M5.6); meetings, invitations sent, answered and called off (M5.7); People: contacts, contact groups and vCard files (M5.8); Tasks and the To-Do List (M5.9); reminders for appointments, tasks and flagged mail (M5.10); contacts kept in step with a CardDAV server, the Address Book and Check Names, Mail Merge and to-dos in calendar files (M5.11); tasks kept in step with a CalDAV server's task lists (M5.12); every setting in one Options window and the File tab in pages (M1.9); the To-Do Bar beside every module (M5.12); the first pre-release, 0.1.0-alpha.1: a Windows installer and Check for Updates from GitHub (R1); the second, 0.1.0-alpha.2, for Windows and Linux (R1); web access from a browser (M7) not started; third-party sign-in (M8) comes after the first release · revision 153 · 2026-10-30

## Headline numbers

| Measure | Now |
|---|---|
| Catalogued Outlook Classic features with a command | 90 of 134 (67%), see [docs/parity-checklist.md](docs/parity-checklist.md) |
| Tests | 771, all passing |
| File formats read | `.pst`/`.ost` (ANSI, Unicode, 4K pages), `.msg`, `.oft`, `.eml`, `.ics` |
| File formats written | `.msg`, `.oft`, `.eml`, `.ics`; a message prints to `.pdf` |
| Protocols | IMAP, POP3 and SMTP over TLS or STARTTLS; HTTPS for pictures on command, for published account settings and for updates |
| Releases | 0.1.0-alpha.2 (pre-release): a Windows x64 installer and a Linux x64 archive; Check for Updates from GitHub releases |

## Milestones

| | Milestone | State |
|---|---|---|
| M1 | **Data files first**: open `.pst`/`.ost`/`.msg`/`.eml` in an Outlook-Classic-style window; save messages Outlook can reopen | in progress |
| M2 | Formatted reading: HTML and RTF bodies laid out and drawn, inline pictures, printing | closed: HTML and RTF text, lists, tables, pictures and named fonts are drawn; a message or a folder's list prints to a PDF file, with a page setup and a preview. See "Do later (from M2)" |
| M3 | Writing `.pst`: journalled, crash-safe writes; rules and folder changes persist | closed: a mailbox is saved as a new `.pst`, changes to an open Unicode `.pst` are kept when saved, settings are kept between runs. Journalled changes are in M3b and rules in M5; see "Do later (from M2)". Opened in Outlook Classic (M3c.1) |
| M3b | The do-later batch from M2 and M3 (see "Do later (from M2)") | closed: unsaved changes are journalled (M3b.1), export copies a `.pst` as stored (M3b.2), panes and window kept (M3b.3), right-to-left text outside the body (M3b.4), print's own text shaped (M3b.5), right-to-left lists and quotations (M3b.6), the journal's loose ends (M3b.7), right-to-left text typed in a field (M3b.8), what was left of a right-to-left paragraph (M3b.9), copying out of a PDF tried in a reader (M3b.10), attachments and search-folder rows of a changed message (M3b.11), a save that does not hold the window (M3b.12), the structure check inside nodes (M3b.13), an export that does not hold the window (M3b.14), the other modules' folders named by id (M3b.15), value kinds, header counters and the keep-original choice (M3b.16), attachments with new contents and ones that cannot be read (M3b.17), cells of another type, the check hearing Stop and fill levels (M3b.18), an export of any mailbox on a thread of its own (M3b.19), the structure check for ANSI and 4K-page files (M3b.20), ANSI data files written and kept as ANSI (M3b.21), Table Style columns, headings and selected row, preview zoom (M3b.22), SVG, WebP and TIFF pictures and animation (M3b.23), several pictures at the sides and paper behind dark see-through ones (M3b.24), boxes inside one another and a colour behind the body (M3b.25), borders round text and rounded corners (M3b.26), RTF highlight colours and list overrides (M3b.27), metafiles in full (M3b.28), a save into the data file where it is (M3b.29) |
| M3c | Proof in Outlook and 4K-page writing | closed: files we write open in Outlook (M3c.1); 4K-page data files written (M3c.2); saving on its own (M3c.3); the size of a changed message (M3c.4); files from before M3c.1 (M3c.5); hidden items exported from an `.ost` (M3c.6); compressed 4K blocks (M3c.7) |
| M4 | Accounts: IMAP, POP3 and SMTP behind an account trait, with an in-process fake server for tests | closed: protocol clients and the fake server (M4.1); TCP, TLS and the credential store (M4.2); accounts in the engine (M4.3); sending (M4.4); receiving (M4.5); Account Settings in the window (M4.6); each account in a data file of its own, the live check passed (M4.7); every IMAP folder kept in step both ways (M4.8); certificates trusted on the owner's word, new mail told of, Show Progress, Delay Delivery (M4.9); pictures on command (M4.10); attachments opened (M4.11); CONDSTORE, QRESYNC, IDLE, folders and drafts on the server (M4.12); pictures off the window's thread (M4.13); servers found from the address (M4.14) and its DNS records (M4.15). OAuth sign-in is M8 |
| M5 | Calendar, People and Tasks modules | in progress: the Calendar's views and appointments (M5.1), Import from Outlook (M5.2), recurring appointments (M5.3), calendar files (M5.4), CalDAV sync (M5.5), time zones (M5.6), meetings (M5.7), People (M5.8), Tasks (M5.9), reminders (M5.10), CardDAV, the Address Book, Mail Merge and to-dos in calendar files (M5.11); see the M5 plan |
| M6 | MCP server, control channel, web build | not started |
| M7 | Web access: the mailbox, calendar, people and tasks reached from a browser on any device, as Outlook on the web reaches Exchange. OpenMailClient serves its own window, built for the web on M6's web build, over HTTPS from the machine it runs on, through no one else's server; the engine and the data files stay on that machine and the browser only draws, so desktop and browser are one session and a data file has one writer. Off until the owner turns it on; this machine and its network only unless opened wider; a browser paired once with a code shown in the desktop window, then signed in with a passkey (WebAuthn) or a password of its own kept only as a hash; sessions that end, devices listed and taken back. Message content stays data in the browser too: no scripts, a strict Content Security Policy, remote pictures only on command, fetched by the host. A layout for phones as well as desktops | not started |
| M8 | Third-party sign-in: Gmail, Outlook.com and Microsoft 365, Yahoo and other providers through OAuth 2.0 (SASL XOAUTH2 and OAUTHBEARER, RFC 7628), signed in through the system's browser with PKCE, tokens kept and renewed in the system's credential store, a provider's servers filled in from its address | after the first release, by the owner's choice: until then such accounts sign in with an app password |

## Current focus (M1)

1. Check the `.pst` reader against more real Outlook-written files, locally (results are never
   published). Done so far: a few small 4K-page `.ost` files.
2. Have Outlook Classic open OpenMailClient-written `.msg` files and compare, property by property. Done
   for plain mail with a file attachment ([docs/file-format-parity.md](docs/file-format-parity.md));
   attached messages and inline pictures remain.
3. Side-by-side layout comparison with Outlook Classic and the interaction parity doc.

## Current focus (M2)

1. Done: HTML bodies read into a formatted document (`crates/formats`) and drawn in the reading pane,
   with nothing run and nothing fetched.
2. Done: bodies stored as compressed RTF ([MS-OXRTFCP]) are decoded; RTF that wraps HTML or text
   ([MS-OXRTFEX]) is read as what it wraps, ordinary RTF is drawn formatted.
3. Done: pictures the message carries as attachments are decoded in-process and drawn in the body.
4. Done: an HTML and an RTF message of ours compared with Outlook's display of the same `.msg`;
   text size, table widths and placement, cell fills, indents and quotations follow it
   ([docs/ui-parity.md](docs/ui-parity.md), Message bodies).
5. Done: frames for pictures that are not loaded, row backgrounds, cell padding and spacing,
   vertical places in cells, percentage widths, and style sheets as far as Outlook follows them.
6. Done: pictures stand in the line of text or at its side with text beside them, text is drawn
   in the fonts the message names (installed ones only; nothing is bundled), and pictures written
   into the body (`data:`) are shown.
7. Done: italic faces of named fonts, what follows a picture at the side standing beside it,
   borders set in styles, cells that span rows.
8. Done: PNG and JPEG pictures written into RTF bodies, and RTF cells with their own lines,
   fills, places and merges down.
9. Done: a message prints to a PDF file in Memo Style (`file.print`), text, tables and borders;
   pictures are not printed yet.
10. Done: pictures in print - attached and written-in pictures are drawn on the page.
11. Done: attachments stand in their places in an RTF body; bitmaps, and metafiles that hold
    only a bitmap, are shown.
12. Done: an attached object of another program is kept whole through `.msg`, and is shown by
    the best picture it has.
13. Done: a pass over what is already built, to make each feature better than the one it
    follows (M2.13 to M2.21).
14. Done: Windows metafiles and enhanced metafiles that are drawings of lines, curves,
    filled shapes and text are drawn.
15. Done: an attachment that stands in the text is a card there, with its name and size.
16. Done: pictures written as bytes (`\bin`) are read.
17. Done: tables inside the cells of tables, and cells merged across, in RTF bodies.
18. Done: paragraph borders and the room paragraphs ask for above and below, in RTF bodies.
19. Done: list paragraphs of RTF bodies have their numbers and bullets as markers.
20. Done: tab stops in RTF bodies (left, centre, right, decimal).
21. Done: line spacing in RTF and HTML bodies.
22. Done: tab leaders, and the line between paragraphs in one border, in RTF bodies.
23. Done: list numbers from the list tables of RTF bodies.
24. Done: fills behind blocks, and room on each side of them, in HTML and RTF bodies.
25. Done: opening a link, after being asked.
26. Done: text beside pictures, and kerning, in print.
27. Done: page setup for print.
28. Done: a print preview with the page setup in it.
29. Done: Table Style: a folder's list printed as a table.
30. Done: bitmaps beside a metafile's drawing.
31. Done: M2 is closed; what it leaves is scheduled under "Do later (from M2)".
32. Done: right-to-left text is drawn and printed in the order it is read, and Arabic letters are joined (M2.41).
33. Done: print shapes text from each font's own tables, and a PDF says what its glyphs stand for (M2.42).
34. Done: the node database of a `.pst` is written, and a data file's structure can be checked (M3.1).
35. Done: property lists and tables of a `.pst` are written (M3.2).
36. Done: a mailbox is saved as a new `.pst`: folders, messages, attachments, the nodes every data file must have (M3.3).
37. Done: changes to an open Unicode `.pst` are kept: `file.save` writes the file anew beside itself, with everything untouched copied as it is stored (M3.4).
38. Done: settings are kept between runs: the page setup, the panes, the theme and ribbon, and the data files that were open (M3.5). M3 closed.
39. Done: unsaved changes to a data file are journalled as they are made and come back after a run that ended without saving (M3b.1).
40. Done: `file.export` of a Unicode `.pst` copies it as it is stored: hidden items, folder properties and unknown properties go along (M3b.2).
41. Done: the edges of the panes are dragged, and the window's size and place and the panes' sizes are kept between runs (M3b.3).
42. Done: right-to-left text reads the right way in the message list, the reading pane's header and folder names, and a turning mark cannot disguise a name (M3b.4).
43. Done: the head and foot of a printed page and the markers of lists are shaped and turned as the body is (M3b.5).
44. Done: a paragraph written from the right has its indent, its list marker and its quotation bars at the right, on screen and in print (M3b.6).
45. Done: the journal of unsaved changes is kept in a folder of our own when it cannot be kept beside the data file, is forced to disk once a second, and says exactly how many changes a damaged journal lost when that can be known (M3b.7).
46. Done: a line that is typed (the search box, a folder's new name, the address and subject lines, a page's head and foot) is seen in the order it is read while it is typed, with the cursor, the selection and a click where the text is seen (M3b.8).
47. Done: what was left of a paragraph written from the right: tab stops and their leaders are counted from the right, a border's own indent is at the right, text stands beside a picture on the right side of it, the columns of a table written from the right start at the right, and `<bdo>`, `<bdi>`, `dir` on a piece of a line and `unicode-bidi` say which way their text runs, on screen and in print (M3b.9).
48. Done: text copied out of our PDFs was tried in a reader (xpdf's `pdftotext` 4.06) and what it found was put right: text written from the right is given as it is seen, as readers expect it, and only glyphs that are not one letter each say what they stand for (M3b.10).
49. Done: when a message's list of attachments changes, each attachment the file already holds keeps what is stored on it; a search folder's table shows a changed message as it is now (M3b.11).
50. Done: a save asked for in the window is done on a thread of its own, with a bar that fills in the status bar and a way to stop it (M3b.12).
51. Done: the structure check looks inside the nodes: every heap, index, property list and table is held against the specification (M3b.13).
52. Done: an export of a data file runs on a thread of its own, with progress and a way to stop it (M3b.14).
53. Done: an exported file names the Calendar, Contacts, Tasks, Notes, Journal and Drafts folders by id on its root and its Inbox (M3b.15).
54. Done: the small things left in the writer: values keep their kind, a rewritten file goes on from the header's counters, and the file as it was need not be kept (M3b.16).
55. Done: a file attached again with other contents keeps what is stored on it, and an attachment that cannot be read stays where it is (M3b.17).
56. Done: a value of another type than its column's is shown as the column holds it; the structure check hears Stop and compares a heap's fill levels with its blocks (M3b.18).
57. Done: an export of any mailbox runs on a thread of its own: the sample, an `.ost`, an ANSI `.pst` and a 4K-page file as well (M3b.19).
58. Done: the structure check covers ANSI data files and files with 4K pages (M3b.20).
59. Done: ANSI data files are written, and a re-saved one keeps its kind (M3b.21).
60. Done: Table Style prints chosen columns, group headings and the selected message; the print preview has a zoom (M3b.22).
61. Done: SVG, WebP and TIFF pictures are shown, and pictures that move are played (M3b.23).
62. Done: Several pictures stand at the sides of one paragraph, and a dark see-through picture has paper behind it on the dark pane (M3b.24).
63. Done: Boxes stand inside one another, a table stands inside the block it was written in, and a body has its colour behind all of it (M3b.25).
64. Done: Text within a line is drawn in its border, and boxes and cells have rounded corners, in the window, on paper and in the PDF (M3b.26).
65. Done: An RTF highlight is one of the sixteen marker colours, and list numbers follow what their override changes (M3b.27).
66. Done: Metafiles draw parts of bitmaps, mirrored and turned ones, run-packed ones, every way of mixing, hatched and patterned brushes, and regions (M3b.28).
67. Done: A save writes what changed into the data file where it is, with what it writes over kept beside the file until the save is checked; M3b is closed (M3b.29).
68. Done: Outlook Classic opens the data files our writer makes, Unicode, ANSI and saved into in place, and sees every folder and message as we wrote them (M3c.1).
69. Done: Data files with 4K pages, the kind newer versions of Outlook make, are written, take changes and are saved as that kind; Outlook Classic opens one of ours and sees the same mailbox as in our Unicode file (M3c.2).
70. Done: Unsaved changes to a data file are saved on their own after five minutes at a quiet moment, checked as Save Changes is, and never for a file's first save (M3c.3).
71. Done: A message changed by a save, into the file where it is or whole, is given as its size what is now stored of it, in the message and in its folder's row (M3c.4).
72. Done: A data file that lacks what Outlook asks for, as those written here before M3c.1 do, is written whole by today's writer on its next save or copy, every folder and item keeping its id; Outlook opens such files, Unicode and ANSI, after one save (M3c.5).
73. Done: A mailbox written from what is read carries each folder's hidden items and its own properties, entry ids among them made to name the same folders in the new file: an `.ost` is exported with them, and a file another program wrote that lacks what Outlook asks for is written whole with them; Outlook opens an `.ost` exported as Unicode, ANSI and 4K-page files with every folder, message and hidden item (M3c.6).
74. Done: A data block of a 4K-page file that does not fit in one 512-byte slot is stored deflated when that makes it smaller, as Outlook stores them, and blocks shared by several messages are found again through what they inflate to; Outlook opens such files, written from the sample and from an `.ost`, with every folder, message and hidden item (M3c.7). M3c is closed.
75. Done: A new crate, `net`, speaks IMAP, POP3 and SMTP: it signs in only over TLS or STARTTLS, lists folders with their special uses, reads messages without marking them read, appends, flags, deletes and sends, and is tested against an in-process fake server and hostile scripted servers, with no socket (M4.1).
76. Done: Mail servers are reached over TCP and TLS 1.2 or 1.3, with the certificate checked against the roots the system trusts and the server's name before anything is said; passwords are kept in the system's credential store (Windows Credential Manager, the macOS keychain, the Secret Service), never in a file of ours (M4.2).
77. Done: Mail accounts are kept in the engine: added, listed, changed, removed and tested (signing in to both servers and sending nothing), on a thread of their own; kept in the settings file without their passwords, which are in the system's credential store (M4.3).
78. Done: Mail is sent: Send puts a message in the Outbox and the Outbox goes out through its account's SMTP server on a thread of its own; what went is kept in Sent Items, and what the server did not take stays in the Outbox with the reason (M4.4).
79. Done: Mail is received: Send/Receive (F9) takes new mail down from every account into the Inbox on a thread of its own, IMAP read without marking it read and POP3 taken off the server only once it is saved here; nothing is taken down twice (M4.5).
80. Done: The window has Account Settings (File > Account Settings…, and on the Send / Receive tab): accounts are added, changed, tested and removed there, the password typed into a field that shows dots and gives nothing away; the ribbon's Send/Receive All Folders and Send All work (M4.6).
81. Done: Each account keeps its mail in a data file of its own, as Outlook keeps one per account: received mail goes in its Inbox, what it sent in its Sent Items, and the file is saved on its own once mail is in it; Account Settings is laid out anew, and the live check against a real IMAP and SMTP server passed (M4.7).
82. Done: HTML messages laid out as Outlook lays them out: a column of tables centred in its cell, each side of a cell's padding, pale fills and a table's own fill drawn, rounded table corners, tables standing flush under one another, a pale border kept pale, and a picture not downloaded shown as a box of the size it asks for (M4.7, from the live check).
83. Done: Pictures on a server downloaded when asked for: from the info bar above the message or a right-click on a picture not shown (Download Pictures), over HTTPS only, never from this machine or the local network, kept in memory only and never by themselves (M4.10, `mail.downloadPictures`).
84. Done: Attachments opened or saved on a double-click, as Outlook asks: Open, Save As or Cancel, from a double-click, Enter or a right-click on an attachment; a program, a script or a file whose name hides its kind is only saved; an opened file is a read-only copy marked as from the internet, cleared away when the program ends (M4.11, `file.openAttachment`).
85. Done: IMAP folders taken down into the account's own data file as the server has them, and a copy of what is sent put in the server's Sent folder (M4.8, `accounts::Mark`, `sentOnServer`).
86. Done: IMAP kept in step both ways: read, flagged, moved and deleted messages, and messages put into the account from elsewhere, go to the server, and what was done there comes here (M4.8, `cmd/imap_sync.rs`, `Place`).
87. Done: A server's own certificate trusted on the owner's word: shown whole, trusted for that server only and only while it shows exactly that one (M4.9, `crates/net/src/cert.rs`, `account.trustCertificate`); buttons with room round their words, an outline and rounded corners.
88. Done: New mail told of at the corner of the screen, with a sound of our own and the taskbar button, and Send/Receive on a timer (M4.9, `crates/engine/src/arrival.rs`, `crates/ui-egui/src/alert.rs`, `apps/openmailclient/src/chime.rs`).
89. Done: Send/Receive's progress shown in a window of its own, with a way to stop one task or all (M4.9, `crates/engine/src/cmd/progress.rs`, `crates/ui-egui/src/progress.rs`).
90. Done: Delay Delivery and Expires After on a message being sent: a held message waits in the Outbox and goes at its minute, an expired one is struck through (M4.9, `compose.delayDelivery`, `compose.expires`, `Session::send_receive_due`).
91. Done: Only what changed is asked of an IMAP server that keeps mod-sequences: CONDSTORE's CHANGEDSINCE and QRESYNC's SELECT, what it says went made sure of before anything goes here (M4.12, `Imap::select_since`, `imap_sync::since`, `accounts::Step`).
92. Done: New mail told of as it comes: each IMAP account's Inbox watched with IDLE on a connection of its own and received from the moment the server says it changed (M4.12, `Imap::idle`, `crates/engine/src/watch.rs`, `mail.newMailAsItComes`).
93. Done: Shown as Outlook shows it, from a side-by-side look at an account's mail: cell padding in centimetres and points, links underlined only where their style says, recipients in Sent Items, previews read from the message with an end mark, Outlook's date headings, dates and times as the system's region writes them, and an `.ost` that starts at its mail folders with hidden folders left out (M1.8, `time::Region`, `Session::fill_previews`, `PstStore::named_near`).
94. Done: IMAP folders renamed, moved and deleted followed both ways: a folder renamed or moved on the server is renamed or moved here, known again by the server's id for it (OBJECTID, RFC 8474) or its UIDVALIDITY, without its mail taken down twice; one renamed, moved or put with the deleted mail here is renamed or moved there; one deleted here for good is deleted there; one the server no longer has is kept here with the deleted mail; a folder made here is made there (M4.12, `imap_sync::follow`, `Imap::rename`, `Imap::delete`, `Imap::status`, `Received::folders_gone`).
95. Done: Drafts on the server: a draft saved in an IMAP account goes into the account's own Drafts and onto the server's Drafts folder marked `\Draft`; saved again or sent, it takes its own place there and goes from Drafts on both sides; a draft written elsewhere comes here as a draft and opens to be written on with a double-click or the reading pane's bar; Save keeps the message open, as Outlook does (M4.12, `compose.editDraft`, `Draft::filed`, `ui.openSelected`).
96. Done: The account's data file read apart: the first receive of a run reads each message's place on the server on the receive's own thread, from the mailbox as it stands (`Store::apart`), not on the window's; what it learns is remembered when the receive ends (M4.12, `imap_sync::Gather`).
97. Done: Pictures decoded off the thread that draws the window: a message's pictures are decoded on a thread of their own; drawing waits only a moment for them, and a picture not ready yet stands as an empty frame until it is (M4.13, `pictures::Pictures`).
98. Done: An account's servers found from its address: from what is known of large providers, the settings its domain publishes for mail programs, or the usual server names greeted without signing in; the address itself is sent to no one (M4.14, `crates/net/src/find.rs`, `account.find`, Find settings in Account Settings).
99. Done: An account's servers found from the DNS too: the servers its domain's RFC 6186 SRV records name, implicit TLS first, and the provider its mail exchangers (MX) belong to, given that provider's own servers; read with a DNS client of our own that takes hostile answers apart safely (M4.15, `crates/net/src/dns.rs`); M4 closed.
100. Done: The Calendar: day, work week, week and month views of the data file's Calendar, a month to pick days from, appointments side by side when they overlap, all-day events in a band of their own, Show As drawn on each, a line at the time now; appointments made, opened, changed and deleted in a window of their own that names what clashes, every change undone (M5.1, `crates/model/src/calendar.rs`, `crates/engine/src/cmd/calendar.rs`, `crates/ui-egui/src/calendar.rs`).
101. Done: Dialogs in the theme's colours, and themes of your own: every dialog has a coloured head with its own mark and a close button, a foot that holds its buttons with the one that does the work in the accent, warnings in tinted boxes, and the window behind dimmed; nine built-in themes and any number of one's own, each with its own light and dark colours, changed in a Themes window that shows the change at once and checks that text can be read, or following the system's light or dark setting (M1.9, `crates/ui-egui/src/dialog.rs`, `crates/ui-egui/src/themes.rs`, `crates/ui-egui/src/theme.rs`).
102. Done: Import from Outlook: every IMAP and POP3 account of every Outlook profile, with its servers, ports, security and user name and never its password, and the data files the profile holds, shown before anything is done, then brought on a thread of their own: each account's data file is copied with Outlook kept from writing it meanwhile and checked, an `.ost` becomes a `.pst` of our own, and Outlook's own files are not changed (M5.2, `crates/engine/src/outlook.rs`, `crates/engine/src/cmd/import.rs`, `crates/ui-egui/src/import.rs`).
103. Done: Recurring appointments: a series made, read and saved as Outlook keeps it ([MS-OXOCAL] 2.2.1.44 pattern, exceptions as hidden attached messages, 2.2.10.1), shown on every day it falls, and one occurrence changed or deleted alone, from the window or by command (M5.3, `crates/model/src/recur.rs`, `crates/engine/src/cmd/calendar.rs`, `crates/ui-egui/src/calendar.rs`).
104. Done: Calendar files: an `.ics` file, or one attached to a message, shown event by event before anything is added, the events chosen added, one the calendar has already brought up to date by its UID instead of added twice, all of it undone at once; the calendar, a range of days or one appointment saved as an `.ics` file with its series, changed and deleted occurrences, and sent by mail as a file and a list (M5.4, `crates/formats/src/ics.rs`, `crates/engine/src/cmd/calendar_file.rs`, `crates/ui-egui/src/calendar_file.rs`).
105. Done: Calendars kept in step with a CalDAV server: each calendar of the server an account names is a Calendar folder of the account's own data file, what is made, changed and deleted on either side goes to the other on each Send/Receive, only what changed asked for by sync token, a change on both sides keeping both, over HTTPS with the account's own sign-in; My Calendars lists every calendar in its colour (M5.5, `crates/net/src/dav.rs`, `crates/engine/src/cmd/caldav.rs`).
106. Done: Time zones and daylight saving: dates shown in the system's zone or one chosen, with its summer time; an appointment kept in its own zone, so a series keeps its hour there across the change; zones written into data files as Outlook keeps them and defined in calendar files; a second zone's hours beside ours in the day and week views (M5.6, `crates/model/src/zone.rs`, `calendar.timeZone`, Calendar > Home > Time Zones).
107. Done: Meetings: an appointment with attendees, required, optional and rooms, sent to them as an invitation Outlook and other mail programs show as one (RFC 5546 iTIP by RFC 6047 iMIP, [MS-OXOCAL] meeting messages); an invitation that comes answered from a bar above it, Accept, Tentative or Decline, and only then put in the calendar; answers and cancellations taken in only from the one they have to come from; the organizer's tracking of who answered how; updates and cancellations sent; Reply with Meeting (M5.7, `crates/model/src/meeting.rs`, `crates/engine/src/cmd/meeting.rs`, `crates/ui-egui/src/meeting.rs`).
108. Done: People: the Contacts folder's contacts and contact groups in Outlook's five Current Views (People, Business Card, Card, Phone, List), found in every field as they are typed for, made, changed and deleted in windows of their own, every change undone; vCard files (2.1, 3.0 and 4.0) shown card by card before anything is added, someone already there brought up to date instead of added twice, and contacts saved as one; a contact sent on as a card, mail written to a contact or a group, and a message's sender added to Contacts (M5.8, `crates/model/src/contact.rs`, `crates/formats/src/vcf.rs`, `crates/engine/src/cmd/people.rs`, `crates/ui-egui/src/people.rs`).
109. Done: Mail shown as it comes down: a receive hands back what it has in parts, the newest first into an account's own data file, several messages to a command, memory bounded and the data file saved as it goes; the window closes while mail is received, at most three seconds later, and nothing is lost (M4.16, `Session::begin_in_parts`, `Imap::fetch_many`, `MailUi::close_now`).
110. Done: Tasks: a Tasks folder's tasks in Outlook's views (Simple List, Detailed, Prioritized, Active, Completed, Today, Next 7 Days, Overdue) and the To-Do List, where mail flagged for follow-up stands beside the tasks under headings by when each is due; tasks made, changed, ticked off and deleted, every change undone; a task added by typing its subject above the list; Follow Up on mail by Today, Tomorrow, This Week, Next Week or No Date, as Outlook flags it ([MS-OXOTASK], [MS-OXOFLAG]) (M5.9, `crates/model/src/task.rs`, `crates/engine/src/cmd/tasks.rs`, `crates/ui-egui/src/tasks.rs`).
111. Done: Reminders: the Reminders window comes up when an appointment, a task or a message flagged for follow-up comes due, from every data file open, with a sound of our own and the taskbar button; Dismiss, Dismiss All, Snooze and Open Item, written into the item as Outlook writes them ([MS-OXORMDR] 3.1.4.6), a series moving on to its next occurrence; a data file that cannot be written keeps them put away for the run (M5.10, `crates/model/src/reminder.rs`, `crates/engine/src/reminders.rs`, `cmd/reminders.rs`, `crates/ui-egui/src/reminders.rs`).
112. Done: Contacts kept in step with a CardDAV server, the Address Book and Check Names, names suggested as they are typed, Mail Merge, and to-dos in calendar files read into Tasks and Tasks saved as one (M5.11).
113. Done: Tasks kept in step with a CalDAV server's task lists, each in a Tasks folder, on each Send/Receive and by `tasks.sync` (M5.12, first part).
114. Done: the To-Do Bar: the month and the appointments to come, favourite people and the To-Do List at the right of any module, each module with its own parts (M5.12, `crates/engine/src/cmd/todo_bar.rs`, `crates/ui-egui/src/todo_bar.rs`, `view.todoBar`, `todo.bar`, `people.favorite`).
115. Next: the rest of M5.12.

## M5 plan

| Step | What |
|---|---|
| ~~M5.1~~ | done: the Calendar: day, work week, week and month views; appointments made, changed and deleted |
| ~~M5.2~~ | done: Import from Outlook: every IMAP and POP3 account of every profile with its servers and without its password, and the profile's data files, shown first and copied without changing Outlook's |
| ~~M5.3~~ | done: recurring appointments ([MS-OXOCAL] 2.2.1.44 pattern, exceptions), shown on every day they fall, one occurrence changed or deleted alone |
| ~~M5.4~~ | done: iCalendar: `.ics` files opened, imported and saved (RFC 5545, [MS-OXCICAL]) |
| ~~M5.5~~ | done: Calendar sync over CalDAV (RFC 4791), as with Nextcloud: calendars found from the account, changes asked for by sync token (RFC 6578) and ETags, both ways, HTTPS only, the password in the credential store; tested against an in-process fake CalDAV server |
| ~~M5.6~~ | done: time zones and daylight saving: an appointment kept in its own time zone, shown in the system's or one chosen; a second zone beside ours |
| ~~M5.7~~ | done: meeting requests: attendees, invitations sent and answered, updates and cancellations, tracking, Reply with Meeting |
| ~~M5.8~~ | done: People: contacts and contact groups listed in five views, found, made, changed and deleted; `.vcf` files opened and saved (RFC 6350, [MS-OXVCARD]); a contact sent on as a card; CardDAV sync is a do-later |
| ~~M5.9~~ | done: Tasks: tasks and flagged mail on the To-Do List, in Outlook's views; due dates, status, work and reminders kept as Outlook keeps them ([MS-OXOTASK], [MS-OXOFLAG]); a task added by typing it |
| ~~M5.10~~ | done: reminders for appointments, tasks and flagged mail in a window of their own, dismissed and snoozed as Outlook writes them ([MS-OXORMDR]) |
| ~~M5.11~~ | done: CardDAV (RFC 6352), the Address Book and Check Names, Mail Merge; to-dos in calendar files (RFC 5545 3.6.2); CalDAV task lists moved to M5.12 |
| M5.12 | M5's do-later batch: CalDAV task lists (done), the To-Do Bar (done), an invitation answered with a comment or without telling (done), what the contact and invitation windows still lack, themes in files, zone history, what more is brought from Outlook, and what reminders left |

## Do later (from M2)

M2 closed with these not done. They are not dropped: each has a slot, and is struck from this
table when it lands. The slots are a plan, and an item moves up when it starts to hurt.

| Do later | When |
|---|---|
| ~~Right-to-left text (Hebrew, Arabic) in the right order, and Arabic letters joined, on screen and in print~~ | done: M2.41 |
| ~~Shaping from the font's own tables in print (ligatures, marks placed, Indic and other complex scripts; Arabic joined by the font's tables and across a change of look; copying text out of a PDF in the order it is read)~~ | done: M2.42 |
| ~~Shaping for the text print sets itself: the name and page number at the head and foot of a page, list markers, the header's field names and values~~ | done: M3b.5 |
| ~~Copying text out of our PDFs tried in a PDF reader (xpdf's `pdftotext`)~~ | done: M3b.10 |
| Copying text out of our PDFs tried in the readers people use most (Acrobat, a browser's viewer): none of them gives its text to a script here | when one can be tried without driving a window |
| ~~Right-to-left text in the message list, the header of the reading pane and folder names~~ | done: M3b.4 |
| ~~Right-to-left text while it is typed in a field of one line: the search box, a folder's name being changed, the address and subject lines~~ | done: M3b.8 |
| Right-to-left text while it is typed in the body of a message being written (the toolkit's box draws it as stored; the body gets its own editor with the item windows) | M5 |
| A right-to-left line in the list, the header and the folder pane set from the right of its cell | M5, with views |
| ~~For a right-to-left paragraph: list markers, indents and quotation bars at the right~~ | done: M3b.6 |
| ~~For a right-to-left paragraph: tab stops and their leaders counted from the right; a frame's own indent at the right; text beside a picture at the side; columns of a right-to-left table from the right; `<bdo>` and `unicode-bidi`~~ | done: M3b.9 |
| Selecting and copying text in a paragraph with right-to-left writing; Arabic joined across a change of look on screen; other complex scripts checked on screen (egui shapes each piece from the font's tables; only Hebrew and Arabic were looked at) | with M5, when the item windows get their own text drawing |
| ~~Keeping the page setup between runs~~ | done: M3.5 |
| ~~Rules kept between runs (there are no rules yet; they are kept in the data file as Outlook keeps them, and in the settings for accounts without one)~~ | done: M5.13 |
| ~~The window's size and place and the widths of the panes kept between runs~~ | done: M3b.3 |
| ~~The columns of the list kept between runs (the list has no columns yet: they come with the single-line view)~~ | done: M5.21 |
| The window's place on a second screen kept (the size is; egui does not say where the other screens are) | M5 |
| Each folder's own view (sort, grouping, reading pane) kept, in the data file where Outlook keeps it | M5, with views |
| ~~A settings page in the app (the file is written and read; nothing shows or edits it but the commands that change each setting)~~ | done: M1.9, Options |
| ~~A `.pst` from our writer opened in Outlook Classic; with it, where a message attached to a message is kept~~ | done: M3c.1 |
| The bucket of a named property, written from the specification alone, seen by Outlook (no message of ours has a named property yet) | M5, with categories and custom fields |
| Node 0xC01, which Outlook makes when it first opens a file of ours (its layout is in no specification; Outlook makes it itself) | when its layout can be learned black-box from files Outlook writes |
| ~~The size of a message changed by an in-place save~~ (and by a whole one) | done: M3c.4 |
| ~~Files written by OpenMailClient before M3c.1 are refused by Outlook~~ (written whole on their next save or copy, ids kept) | done: M3c.5 |
| ~~A file another program wrote that lacks what Outlook asks for and holds hidden items is saved as stored~~ (written anew with its hidden items; one whose table names a hidden item that is not there is still copied) | done: M3c.6 |
| A save into an Outlook-written `.pst` with 512-byte pages, opened in Outlook afterwards (none at hand; Outlook could not be made to create one) | M1 checks against Outlook |
| An attached object that is not a real storage: Outlook lists no attachment for it | M5, with attachments of other programs |
| ~~Hidden (associated) items and a folder's own properties beyond name, class and counts kept when an open `.pst` is saved~~ | done: M3.4 |
| ~~Hidden (associated) items and a folder's own properties carried by `file.export` when the source is a Unicode `.pst`~~ | done: M3b.2 |
| ~~Hidden items and a folder's own properties carried by `file.export` from an ANSI `.pst`~~ | done: M3b.21 |
| ~~Hidden items and a folder's own properties carried by `file.export` from an `.ost` and a 4K-page file~~ (a 4K-page `.pst` is copied as stored; an `.ost` is written from the model with them) | done: M3c.6 |
| A folder's named properties, and search folders, carried by an export written from the model (an `.ost`) | M5, with search folders |
| ~~Changes journalled as they are made: a crash loses what was not saved~~ | done: M3b.1 |
| ~~The journal kept somewhere else when the data file's folder cannot be written; forced to disk on a timer as well as every 64 records; a count of records lost after a damaged one that is exact~~ | done: M3b.7 |
| ~~Unsaved changes saved on their own after a while~~ (never a file's first save) | done: M3c.3 |
| ~~Saving off the UI thread, with progress~~ | done: M3b.12 |
| ~~A save that does not rewrite the whole data file (needs writes into the data file that are journalled themselves)~~ | done: M3b.29 |
| ~~Unknown properties of an attachment kept when a message's attachments change~~ | done: M3b.11 |
| ~~Unknown properties of an attachment whose contents were changed too, and stored attachments that cannot be read~~ | done: M3b.17 |
| ~~The cells of a changed message in search-folder tables~~ | done: M3b.11 |
| ~~Working out again which search folders a changed message belongs to (its rows are brought up to date; a row is not added or taken away)~~ | done: M5.20 |
| ~~Changes to an ANSI `.pst` kept~~ (an `.ost` stays read-only for good: it is a server's copy) | done: M3b.21 |
| ~~Changes to a 4K-page `.pst` kept~~ (written whole) | done: M3c.2 |
| Saving into a 4K-page `.pst` where it is (it is written whole: what heads a section after the first is not known) | with a 4K-page file over 16 MB that Outlook wrote |
| ~~The node-id counters of a file's header carried into the rewritten file~~ | done: M3b.16 |
| ~~A choice not to keep the file as it was beside a data file on its first save~~ | done: M3b.16 (`file.keepOriginal`) |
| ~~The ids on the root and the Inbox that tell Outlook which folder is the Calendar, Contacts, Tasks, Notes, Journal and Drafts, for a file made by `file.export`~~ | done: M3b.15 |
| ~~Export off the UI thread, with progress and a way to stop~~ | done: M3b.14, for a Unicode `.pst` copied as stored |
| ~~Export off the UI thread for a mailbox written through the model (the sample, an `.ost`, an ANSI `.pst`, a 4K-page file)~~ | done: M3b.19 |
| ~~The structure check hearing Stop~~ | done: M3b.18 |
| ~~Table cells whose type differs from the column's kept in a saved `.pst`'s tables~~ | done: M3b.18, where nothing is lost by it; otherwise the cell is empty and the item keeps the value |
| ~~The structure check for ANSI and 4K-page (`.ost`) files~~ | done: M3b.20 |
| The structure check on a 4K-page file's heap fill levels, on the last eight bytes of its trailers and on a second section's map pages | with a 4K-page file that shows them, or a public description |
| ~~The structure check reaching into heaps, property lists and tables~~ | done: M3b.13 |
| ~~The structure check comparing a heap's fill levels with its free space~~ | done: M3b.18 |
| The structure check run on a 512-byte-page `.pst` written by Outlook | with the first such file we may read |
| ~~32-bit floats, currency and error values keeping their type when an item is written from the model~~ | done: M3b.16 |
| A heap's fill levels compared with an Outlook-written heap of several blocks | with the first such file we may read |
| The places of free map and free page map pages compared with an Outlook-written file over 32 MB | with the first such file we may read |
| ~~Writing ANSI data files (a re-saved file keeps its kind)~~ | done: M3b.21 |
| ~~Writing 4K-page data files~~ | done: M3c.2 |
| What the subnodes 0x521 and 0x541 of a 4K-page contents table (heaps with the client signature 0xA5) hold for a table with rows; they are written empty, and Outlook takes them so | when a 4K-page file shows what they hold black-box |
| Heaps of more than one block in a 4K-page file: where the block number stands in a HID is our reading, no Outlook-written file has such a heap | with the first Outlook-written 4K-page file that has one |
| ~~Compressed blocks in a 4K-page file (read; written uncompressed)~~ (data blocks of more than one slot written deflated) | done: M3c.7 |
| What Outlook makes of Unicode text in an ANSI file of ours, and of wVer 14 where a file had 15 | M1 checks against Outlook, with the owner's say |
| ~~The selected row only, chosen columns and group headings in Table Style~~ | done: M3b.22 |
| ~~Several messages selected at once (Table Style's selected rows then follow the selection)~~ | done: M5.19 |
| ~~Columns of the message list chosen in the window (Table Style then follows them)~~ | done: M5.21 |
| ~~A zoom in the print preview~~ | done: M3b.22 |
| ~~SVG, WebP and TIFF pictures; GIF animation~~ | done: M3b.23 |
| SVG drawings in the web build (one thread, a small stack); bitmaps inside a drawing, under the limits of other pictures; `.svgz` | M6, with the web app |
| Pictures decoded off the thread that draws the window | done: M4.13 |
| ~~A backing for see-through pictures on the dark pane; several pictures at the sides of one paragraph~~ | done: M3b.24 |
| On paper, a table or a bordered block beside a picture at the side (in the window it stands beside it) | M5, with the other print styles |
| ~~Boxes inside one another; a fill behind a table in a block and behind the whole body~~ | done: M3b.25 |
| ~~Borders on text within a line; rounded corners~~ | done: M3b.26 |
| ~~Outlook's highlight colours; list numbers an override changes (`\lfolevel`)~~ | done: M3b.27 |
| ~~Metafiles: a part of a bitmap, one mirrored, turned or packed in runs, copies that are not plain, patterned brushes, regions of many boxes~~ | done: M3b.28 |
| Pictures on a server, loaded on an explicit command (never by themselves): from the info bar and a right-click on a picture | done: M4.10 |
| Attachments opened on a double-click (Open, Save As, Cancel), a program or script only saved | done: M4.11 |
| An attachment shown in the reading pane on a click (Attachment Preview) without another program | M5, with the item windows |
| ~~An account's servers found from its address (the provider's published settings, the usual names tried)~~ | done: M4.14 |
| ~~A domain's provider found from its mail exchangers (MX), and RFC 6186 SRV records (a DNS client is needed)~~ | done: M4.15 |
| A server's own certificate trusted on the owner's word, and kept for that server only | done: M4.9 |
| Signing in with OAuth 2 (XOAUTH2, OAUTHBEARER) for providers that ask for it | M8, after the first release |
| A copy of what was sent put in the IMAP server's Sent folder, for servers that do not keep one themselves | done: M4.8 |
| The IMAP server's other folders taken down into the account's own data file, as the server has them | done: M4.8 |
| IMAP kept in step both ways: read, flagged, moved and deleted messages going back to the server | done: M4.8 |
| Only what changed fetched from an IMAP server (CONDSTORE and QRESYNC, RFC 7162) | done: M4.12 |
| New mail told of as it comes (IDLE, RFC 2177) | done: M4.12 |
| IMAP folders renamed, moved and deleted here or on the server, each side following the other | done: M4.12 |
| Drafts kept on the server as they are written | done: M4.12 |
| The place of each message on the server read once on a thread of its own, not on the window's the first time a run receives | done: M4.12 |
| New mail told of on the desktop and with a sound of our own, and Send/Receive on a timer | done: M4.9 |
| Send/Receive's progress shown in a window of its own (Show Progress) | done: M4.9 |
| Delay Delivery and Expires After on a message being sent, and a send held until a time | done: M4.9 |
| Sending pages to a printer | done: M1.10 |
| A printer's own settings (trays, two-sided, colour) from its driver's dialog; print styles other than Memo and Table | M5, with the calendar's print styles (after M5.10) |
| Comparing what is printed with what Outlook prints | when its print pane can be driven without touching the shared machine's printer; until then nothing is claimed |
| ~~Themes saved to a file of their own and read from one, to share them~~ | done: M5.12 |
| ~~Contacts kept in step with a CardDAV server (RFC 6352), as calendars are with CalDAV~~ | done: M5.11 |
| ~~The Address Book window, names checked against Contacts as they are typed in To, and Mail Merge~~ | done: M5.11 |
| ~~Mail Merge printed as letters and saved as one document, and fields chosen by a filter of the contacts (a merge writes messages today)~~ | done: M5.12 |
| ~~A contact's picture chosen in the contact window (`people.change` takes one already), and the other fields Outlook's Details page holds (office, profession, nickname, spouse)~~ | done: M5.12 |
| ~~Tasks in calendar files (VTODO, RFC 5545 3.6.2)~~ | done: M5.11 |
| ~~Tasks kept in step with a CalDAV server's task lists (calendars whose component set is VTODO, RFC 4791 5.2.3) in Tasks folders, as calendars are~~ | done: M5.12 |
| ~~Tasks dragged from the To-Do Bar to the Calendar, and a favourite's presence and recent mail~~ | done: M5.12 |
| A favourite's presence (free, busy, away) beside their name, from a chat or presence service an account offers (what they last sent is shown already) | M8, with the providers' own services |
| ~~Reminders standing above every other window in one of their own when the main window is hidden, and the sound chosen from a file of the owner's~~ | done: M5.12 |
| ~~A reminder of its own on one occurrence changed alone ([MS-OXOCAL] 2.2.1.44.2, an exception's ReminderSet and ReminderDelta) and the Reminders search folder Outlook keeps in a data file; reminders checked against Outlook's on a file of ours~~ | done: M5.12 |
| The Reminders search folder written into a data file with the other search folders of an export ([MS-OXORMDR] 2.2.2.1; Outlook makes it again itself), and reminders checked against Outlook's on a file of ours | with M1's checks against Outlook |
| ~~A task's recurrence made and changed in its window, and the next occurrence on completing~~ | done: M5.17 |
| ~~Assign Task and task requests ([MS-OXOTASK] 2.2.4)~~ | done: M5.18 |
| The Scheduling Assistant: attendees' free and busy times side by side, from a CalDAV server's free-busy (RFC 6638) and the data file | after M5.10, with CalDAV scheduling |
| ~~A comment written with an answer to an invitation, and an answer sent without one ("Do not send a response") chosen in the window~~ | done: M5.12 |
| ~~Zones whose rules changed over the years kept with each year's rules, and zones that follow another calendar beyond their current year~~ | done: M5.12 |
| Every zone's whole history, from the system's own zone database (the tz files on Linux and macOS, Windows' dynamic daylight saving in its registry), with Iran's summer time of 2008 to 2022 by the Persian calendar and rules before the 1980s; a year with two Ramadans written for Outlook as two changes | after M5, with the system's zone database |
| A meeting of ours and a time zone of ours checked in Outlook (opened from a data file, an invitation read) | with M1's checks against Outlook |
| ~~Signatures brought from Outlook; a POP3 account's own "leave a copy on the server" setting read; Outlook 2010's profile place~~ | done: M5.12 |
| ~~The AutoComplete list brought from Outlook ([MS-OXOCFG] 2.2.4)~~ | done: M5.14 |
| Rules brought from Outlook and kept in a data file as Outlook keeps them, so that each runs the other's: [MS-OXORULE] 2.2.9.3 leaves the stream Outlook keeps them in (PidTagRwRulesStream of the Inbox's hidden `IPM.RuleOrganizer` message) opaque, so this waits for a public description of it; descriptions others worked out are not used (clean room) | M5.12 |
| ~~Run Rules Now taken back with Undo; more of Outlook's conditions and actions (dates received, words in the sender's address alone, forward or reply, play a sound)~~ | done: M5.15 |
| ~~Rules on mail as it is sent (Outlook's "apply rule after I send the message": a copy kept elsewhere, a category, a delay)~~ | done: M5.16 |
| Signatures with formatting and pictures, made and put in as formatted text (Outlook's .htm and its _files folder brought whole) | when the compose body is formatted |
| Release signing: the installer signed (Authenticode) so SmartScreen knows it, and `SHA256SUMS.txt` signed with a key of the project's (Ed25519) that the updater checks, so a changed release is refused even from GitHub | R2, before 0.1.0 |
| A release workflow: the installer built, checked and published by GitHub Actions from a tag, not on a developer's machine | R2 |
| Packages for macOS (`.dmg`, an app bundle) and Linux (AppImage, `.deb`), and updates for them (the updater sends them to the release page until then) | R3, with M6's web build |
| The CLI put on `PATH` by the installer when asked, and smaller updates (only what changed) | R2 |

The ranked work list is [docs/gaps.md](docs/gaps.md).

## Progress log

- **2026-10-11** — M1.1: item model, MIME, `.msg` and `.pst` readers, engine with 48 commands, CLI.
  M1.2: the egui window (ribbon, folder pane, message list, reading pane, inline compose, File page),
  headless screenshots, the desktop app, and `cargo xtask ci`.
- **2026-10-11** — M1.3: `.ost` files are accepted (client magic "SO"); 4K-page reading confirmed
  against real offline stores, locally.
- **2026-10-11** — M1.4: Outlook Classic opens OpenMailClient-written `.msg` and `.oft` files and reads the
  same properties back; a file Outlook wrote survives a trip through OpenMailClient. Fixed the named-property
  lookup checksum, which had made Outlook drop categories. Added the `msg_dump` structure tool.
- **2026-10-11** - M1.5/M1.6: first side-by-side with Outlook Classic ([docs/ui-parity.md](docs/ui-parity.md)).
  Added the simplified single-line ribbon with the full-height module strip, and a dark colour set;
  `mail.select` takes a folder.
- **2026-10-11** - M2.1: HTML bodies are drawn formatted: text styles, colours, headings, lists,
  quotations and tables, in both colour sets. Scripts, style sheets, frames and forms are dropped;
  remote pictures and tracking pixels are counted and never fetched; hidden text stays hidden. New
  `formats` crate, and `ui.bodyFormat` to read as plain text instead.
- **2026-10-11** - M1.7: the classic ribbon was compared with Outlook's and rebuilt: both ribbon
  layouts now share one shell (title bar, full-height module strip, tab row) and the classic ribbon
  is a rounded panel 98 high. `mail.select` with only a folder goes to that folder.
- **2026-10-11** - M2.2: RTF bodies are read. PidTagRtfCompressed is decompressed (checked against
  the worked example in [MS-OXRTFCP]); a message kept only as RTF now has text, a list preview and a
  formatted body, and RTF that wraps HTML gives that HTML back (so it also survives conversion to
  `.eml`). The sample mailbox has an RTF-only receipt under Inbox/Receipts.
- **2026-10-11** - M2.3: pictures attached to the message (`cid:`) are drawn in the body. Only
  attachments the body points at are decoded, only as PNG, JPEG, GIF or BMP, with file size,
  dimensions and memory capped first; anything else stays a placeholder and nothing is fetched or
  handed to another program. The sample order mail carries a picture drawn in code.
- **2026-10-11** - M2.4: bodies follow Outlook's layout. Two sample messages were written as `.msg`,
  opened in Outlook Classic and compared with the reading pane. Unstyled HTML text is 12 pt; tables
  keep the width they ask for, stand left, centred or right, and fill their cells; a plain
  blockquote is set in and only quoted mail gets a bar; RTF cells take their widths from the row
  definition. An RTF-only message is now marked in sync, which Outlook needs to show it at all.
- **2026-10-11** - M2.5: style sheets and table details, checked against Outlook with three probe
  messages of our own. `<style>` rules by element name, first class and id are followed (and, as in
  Outlook, nothing else: no second class, no nested selectors, no `@media`). Tables have cell
  padding and spacing (1 and 2 unless set), collapsed borders, row backgrounds, percentage widths,
  and cells whose content stands in the middle unless told otherwise. A picture that is not loaded
  leaves an empty frame of the size it asks for. A link keeps the link colour whatever surrounds it.
- **2026-10-11** - M2.6: fonts and pictures in the text, checked against Outlook with a fourth probe
  message of our own. A new crate, `crates/fonts` (L1), turns the list of fonts a message names
  into installed faces: names are only ever compared with the names of installed fonts, open fonts
  of the same measures stand in for proprietary ones that are missing, an unknown font falls back
  to a serif as Outlook's does, and a font file is checked before the drawing code sees it. No font
  is bundled or fetched. Pictures now stand in the line of text on its baseline and wrap like
  words; `<img align=left|right>` puts one at the side with the text beside and then under it
  (a `float` style does not, as in Outlook). Pictures written into the body as `data:` addresses
  are decoded under the same caps as attached ones. RTF text takes its fonts from the font table.
- **2026-10-11** - M2.7: borders, cells over several rows and text beside pictures, checked against
  Outlook with a fifth probe message of our own. Italic and bold-italic text in a named font is
  drawn in the font's own italic face when it is installed (slanted only when it is not). The
  paragraphs, tables and lists after a picture at the side stand beside it until it ends, and
  `<br clear>` or a `clear` style starts below it. A `border` style is drawn round its paragraphs
  (once round all the paragraphs of one block), its table or its cell, per side, solid, dashed,
  dotted or double, with its padding; `bordercolor` colours a table's lines and a `border` wider
  than 1 draws a thick line round the table. `rowspan` is followed: cells under a cell that
  reaches down move aside (HTML 4.01 11.2.6.1) and rows grow to hold a tall one. Border widths,
  spans and padding are capped where they are read.
- **2026-10-11** - M2.8: RTF pictures and RTF tables, checked against Outlook with a sixth probe
  message of our own (RTF 1.9.1, "Pictures" and "Table Definitions"). A `\pict` that holds PNG or
  JPEG data is kept as its bytes and shown in the line of text, at the size its goal and scale ask
  for; metafiles and bitmaps leave a place of that size. A picture's hex digits and skipped
  destinations no longer count towards the cap on text, which used to cut a message short after a
  large picture. Cells draw only the lines the row names, per side, with their width, colour and
  kind (solid, double, dashed, dotted); lines between cells that touch are drawn once. Cell fills
  (`\clcbpat`), places (`\clvertal*`), side room (`\trgaph`) and cells merged down (`\clvmgf`,
  `\clvmrg`) are followed, and an RTF table stands right under the paragraph before it. A new
  example, `msg_with_rtf`, gives a `.msg` of ours an RTF body for such comparisons.
- **2026-10-11** - M2.9: printing. `file.print` (Ctrl+P, File > Print) lays the open message out in
  Memo Style - the owner's name over a line, the fields, the body, a number at the foot of each
  page - and writes a PDF file, on A4 or Letter. Two new crates: `layout` flows the document the
  reading pane draws down pages (paragraphs, lists, quotes, borders, tables with spans whose rows
  go on over a page end) and `pdf` writes the file (PDF 1.7, ISO 32000-1: compressed streams,
  Type 0 fonts with only the glyphs used, text that can be searched and copied). Fonts are the
  installed ones; a font whose licence bits forbid embedding (OpenType `OS/2` fsType) is passed
  over. Not yet: pictures (their place is named), kerning, shaping and right-to-left text, a
  printer other than a PDF file, and a comparison with what Outlook prints. Tests use a font
  built in code, so nothing is bundled.
- **2026-10-11** - M2.10: pictures in print. A picture a message carries (an attachment the body
  names by content id, or one written into the body) is drawn on the printed page at the size the
  body asks for, made smaller to fit the column and the page; what shows through it is kept (a
  soft mask in the PDF file). Each picture goes into the file once. Decoding moved from the
  window into the engine (`engine::pictures`), so the reading pane and print share one set of
  limits. A picture on a server is never fetched and stays named in words. Not yet: a picture at
  the side with text beside it (it is printed in the line), and a picture cut over a page end.
- **2026-10-11** - M2.11: attachments in RTF bodies, bitmaps and metafile bitmaps. Each place an
  RTF body keeps for an attachment (`\objattph`) is filled as [MS-OXRTFEX] 2.2.3.4 says: the
  attachments that are not hidden and have a rendering position, in the order of that position;
  when their number and the number of places differ, the places are given up. An attached object
  is shown as its own picture ([MS-OXCMSG] 2.2.2.17) when that can be read; any other attachment
  is named where it stands. A picture written into RTF as a bitmap (`\dibitmap`), or as a metafile
  (`\wmetafile`, `\emfblip`) that draws one bitmap and nothing else, is now shown and printed
  ([MS-WMF], [MS-EMF]); a metafile that is a drawing still leaves a place. The sample receipt has
  an attachment in its text. Not yet: metafiles that draw lines, shapes and text, the picture
  inside an attached object's own storage, an icon beside an attachment's name, `\bin` data.
- **2026-10-11** - M2.12: attached objects of other programs. An attachment with attach method 6
  ([MS-OXCMSG] 2.2.2.9) is a storage the owning program lays out ([MS-OXMSG] 2.2.2.2). Reading a
  `.msg` used to take that storage for an attached message and writing dropped it; now it is kept
  whole, as a compound file of its own (every stream and storage, with classes and state), and
  written back as the storage it was, its entry marked as an object (2.4.2.2). Nothing in an
  object is run or handed to another program. In an RTF body the object is shown by the best
  picture that can be read: the pictures it keeps of itself ([MS-OLEDS] 2.3.4), before the small
  one the message keeps (2.2.2.17). Beyond Outlook: when the object's own data is a picture file
  (2.3.6) that picture is shown, at its full quality, in place of the copy kept for showing; and an
  object that cannot be shown is named with what it calls itself (2.3.8) and not left as a blank
  frame. The sample receipt carries such an object, a permit card drawn in code.
- **2026-10-11** - M2.13: search, the first of the pass over what is already built. The search
  box matched one run of letters; it now understands words (all of them), quoted words, `-word`
  and `NOT`, `OR`, the fields `from:` `to:` `subject:` `category:`, and what a message is:
  `has:attachment`, `is:unread`, `is:flagged`, `is:important`, `before:` `after:` `on:` with a day,
  a month or a year, and `size:>2mb`; Outlook's own keywords (`hasattachments:yes`, `read:no`,
  `isflagged:yes`, `received:`) mean the same. Beyond Outlook: letters match without their accents
  ("zoe" finds "Zöe"), categories are searched with the rest, nothing needs an index, a keyword
  that is not understood is looked for as the text it is (so a query never fails and never quietly
  finds everything), and the command answers with the count. The owner asked (2026-10-11) that
  every feature, those already built too, be made better than the one it follows where it can be;
  the pass goes on with links, saving attachments and print.
- **2026-10-11** - M2.14: links that are not what they say, the second of the pass. Outlook shows
  a link's address when the pointer rests on it and leaves the comparing to the reader. OpenMailClient
  compares: a link whose text names one place and goes to another, one with a name before an `@`
  in its address, one that goes to a number, one written in letters that can pass for others, and
  one that is no web address at all (`javascript:`, `file:`) is underlined in the warning colour
  before anyone points at it, and says what is wrong in a sentence when they do ([RFC 3986] 3.2).
  A name and its own sub-sites count as one place, and a file's name is not taken for a site.
  Nothing is opened, fetched or resolved to tell. The sample's Junk Email folder has a message
  with each kind.
- **2026-10-11** - M2.15: saving attachments, the third of the pass. Outlook saves an attachment
  under the name the message gave it and asks before replacing a file. OpenMailClient has Save All
  Attachments (`file.saveAllAttachments`, a folder; from the File menu and beside the
  attachments) and goes further: every name is made safe first (one name and never a path, no
  device names, no hidden or direction-reversing characters, runs of spaces closed up, a length
  every file system takes, the ending kept), no file is ever replaced (a taken name gets a
  number), and an attached message is saved as `.msg`. An attachment a reader should think twice
  about is marked before it is pointed at and says why in a sentence: a name that reads as one
  kind of file and is another (`invoice.pdf.exe`), a program or script, a document that carries
  macros, a page that opens in a browser, a name with reversing characters, and content that is
  a program under another name. A long name is cut in its middle, so its ending always shows.
  Nothing is opened or run to tell.
- **2026-10-11** - M2.16: print, the fourth of the pass. On paper no pointer can rest on a link,
  so a printed message from Outlook does not say where its links go. OpenMailClient's does: a link
  that is not what its text says gets a number where it stands and a line after the body with
  where it goes and what is wrong with it; `"links": "all"` lists every link whose text is not
  its own address, and `"links": "none"` prints the body alone. Each page's foot says which page
  of how many it is, so a loose page tells whether the rest is with it, and the header lists
  each attachment under the name it would be saved as, with its size.
- **2026-10-11** - M2.17: arranging the list, the fifth of the pass. The list now stands in
  groups however it is arranged, as Outlook's does (by sender, recipient, subject, size,
  importance, flag and attachments, not only by date), and goes further: names and subjects are
  ordered without their case and accents (`Élodie` beside `Elodie`, not after `Zoe`), numbers in
  them by their value (`Invoice 9` before `Invoice 10`), each group's heading says how many
  messages it holds and how many of them are unread, and mail older than two weeks is grouped by
  its month and then its year, not under one "Older". Messages that are equal by the
  arrangement stand newest first whichever way it runs (they stood oldest first in an ascending
  one). The groups are part of `session.inspect`.
- **2026-10-11** - M2.18: answering and sending, the sixth of the pass. A message is looked
  over before it goes, where Outlook asks only about a missing subject and a forgotten
  attachment, and only once Send is pressed: OpenMailClient says, while the message is still being
  written, that it has no subject, that its new text speaks of an attachment and none is
  attached (the quoted original is not read), that something in To, Cc or Bcc is not an address
  mail can be sent to, that an address is named more than once, and that a reply to all would
  show the others the writer had the message as a hidden (Bcc) recipient. `compose.send` refuses
  with those words and keeps the message open unless it is given `"anyway": true`;
  `compose.check` and `session.inspect` tell them. Reply All names each address once however
  the original listed it, and an answer to a message one sent oneself goes to those it went to,
  not back to oneself.
- **2026-10-11** - M2.19: folders, the seventh of the pass. Delete Folder removed the folder
  and all it held for good; it now puts the folder, whole, in Deleted Items as Outlook does
  (under a numbered name when one of that name is there), and only a folder already there, or
  `"permanently": true`, is removed. Move Folder (`folder.move`) is new, and takes a deleted
  folder back. Beyond Outlook: deleting says what went with the folder (messages, unread,
  folders); Empty Folder takes the folders inside with the mail and can remove only what is
  older than a number of days (`"olderThanDays"`); Mark All as Read can take the folders inside
  too (`"subfolders": true`) and says how many it marked; Folder Properties tells the path, the
  size, the oldest and newest message and the totals with the folders inside in one call; a
  rename onto a neighbour's name is refused, and names differing only in case are one name in
  any alphabet, not only in ASCII.
- **2026-10-11** - M2.20: undo, the eighth of the pass. What is done to a message can be taken
  back with `edit.undo` (Ctrl+Z), as in Outlook: marking, flagging, categories, importance,
  moving, archiving, junk and deleting. Beyond Outlook: a hundred steps are kept, not one; a
  message deleted for good can be taken back for as long as the session lasts; the message is
  shown where it stands again, as unread as it was; the result says what was undone and how many
  steps remain, and `session.inspect` what the next one is; a step whose message has since gone
  says so and is dropped without touching anything else. Found on the way and fixed: a move
  whose second half failed left the message in neither folder; it now goes back where it was.
- **2026-10-11** - M2.21: writing and files, the ninth of the pass. Starting a new message or
  a reply while one was being written wrote over it, and Discard dropped it; what a writer has
  put into a message is now set aside instead (twenty are kept), Undo brings a discarded one
  back, and `compose.restore` any of them, which Outlook cannot do once its prompt is answered.
  Attaching refuses a folder and the same file a second time and says what the message now
  carries; the checks before sending also tell of attachments over about 20 MB together, an
  empty one, and a program or script that mail servers refuse. Save As takes a folder and saves
  the message there under its subject made safe, replacing nothing (`"format"` picks msg, eml
  or oft). Opening a data file that is open already goes to it instead of opening it twice.
- **2026-10-11** - M2.22: Windows metafiles that are drawings. A picture kept as a metafile that
  draws lines and filled shapes left only its place; it is now drawn ([MS-WMF] 2.3.3): lines,
  polylines, polygons and sets of them (filled every other part or all that is wound round),
  rectangles, rounded ones, ellipses, arcs, pies and chords, single points, with pens and solid
  brushes that are made, selected and deleted, the window that is set, moved and scaled, states
  saved and put back, and a clipping box. It works for a picture written into an RTF body, for an
  attached object's picture of itself and for the small picture a message keeps of an
  attachment, on screen and in print. Better than the one it follows: edges are smoothed, where
  Windows plays a metafile back with stepped ones, and the page is made at twice the size it is
  shown at, so it stays sharp on a dense screen and on paper. A metafile that asks for anything
  not drawn (text, a bitmap beside the drawing, a patterned or hatched brush, a region, a pen
  that inverts) is not drawn at all and keeps its place: half a picture is worse. A drawing is
  held to 800 pixels a side, 50,000 records, 500,000 points and a fixed amount of work, so a
  hostile one costs little. The sample receipt has a small map drawn this way. Not yet: text in
  metafiles, and enhanced metafiles (EMF) that are drawings.
- **2026-10-11** - M2.23: enhanced metafiles that are drawings ([MS-EMF] 2.3.5). The newer kind
  of metafile, which is what Office writes for a chart or a shape, is drawn on the same page of
  pixels as the older: lines, polylines, polygons and sets of them with long or short numbers,
  curves, rectangles, rounded ones, ellipses, arcs, pies and chords either way round, single
  points; paths that are begun, closed, filled and drawn along; pens (also the wide ones
  measured in the drawing's units), solid brushes and the ones every device has; the world's
  turn set and added to, the window onto the viewport, states saved and put back; clipping to a
  box, to a path that is a box and to a region of one box. The page is the picture's frame, so
  what a file draws outside it is cut off as it should be. As with the older kind, edges are
  smoothed and the page is made at twice the size shown; a metafile that asks for anything not
  drawn (text, bitmaps, patterned brushes and pens, regions of many boxes, a mapping mode in
  parts of an inch, right-to-left layout) keeps its place, and so does one whose drawing is all
  in its comments. Not yet: text in metafiles.
- **2026-10-12** - M2.24: text in metafiles ([MS-WMF] 2.3.3.5 and 2.3.3.20, 2.2.1.2; [MS-EMF]
  2.3.5.7 and 2.3.5.8, 2.2.5, 2.2.13). A chart or a diagram with labels on it is now drawn, not
  left as a placeholder. The letters are filled as shapes, from the outlines of an installed
  font (nothing is bundled and nothing is fetched; a name from a message is only ever looked up
  among installed fonts): the font the drawing names, or an open font of the same measures
  where that one is not installed; bold and italic faces where the family has them, the upright
  face leaning where it has no italic. Read: fonts by the height of the em or of the line,
  turned lines (a label up the side of a chart), underlining and striking out, the colour of
  the letters and the colour behind them, lines set by their left, middle or right and by
  their top, bottom or the line they stand on, lines that each start where the last ended,
  the box that is filled or cut to, text in bytes in the font's character set and text in
  UTF-16. A line that cannot be drawn right keeps the whole picture a placeholder, as before:
  no font answers, the font lacks a letter, letters that join or run right to left, numbers of
  glyphs in place of characters, a font of symbols, a world that is turned (enhanced
  metafiles). The installed fonts are now listed once for the whole program (reading, print
  and drawings shared three lists before). Beyond Outlook: the spacing the drawing gives for
  each letter is kept, so labels stay where they were meant even when another font stands in;
  letters are smoothed and made at twice the size shown, so they stay sharp on a dense screen
  and on paper; and what a drawing says becomes the picture's own text, so it is there for a
  screen reader, for the tooltip and for anything that reads the message as words. Not yet: a bitmap beside a drawing, patterned
  brushes.
- **2026-10-12** - M2.25: an attachment that stands in the text of an RTF body ([MS-OXRTFEX]
  2.2.3.4, [MS-OXCMSG] 2.2.2.16) is shown where it stands as a small card: our own drawing of
  a sheet of paper, the attachment's name and its size. Before, it was a line of words in
  brackets. The name is the one the file would be saved under, so nothing in it is hidden or
  reversed. A click on the card asks where to save the attachment, as a click in the list
  above the message does; nothing is opened or run from the text. An object of another
  program that has no picture to show is a card too, with what it calls itself, and cannot be
  clicked. Read as words (print, plain text, anything that reads the message for an agent)
  the place still says `[Attachment: name]`. Beyond Outlook: the card says how large the file
  is, and a file to think twice about (a program, a name with two endings or with hidden
  characters, a document that carries macros) is marked in the text itself, with the reason
  when the pointer rests on it - not only once it has been opened. A long name is cut in its
  middle, so the ending stays in sight.
- **2026-10-12** - M2.26: a picture written into an RTF body as bytes (`\binN`, RTF 1.9.1,
  "Pictures") is read and shown like one written as hex digits; before, it left only its
  place. A picture may be written partly one way and partly the other. The bytes are never
  read as text or as RTF, whatever they hold (braces, backslashes, line ends, a `\bin` of
  their own), inside a picture or anywhere else, and the readers that only want a body's
  words pass over them too. A picture with fewer bytes than it promised, with a byte split
  between hex digits and bytes, or with more than a body may keep, leaves its place and
  nothing is kept of it. Nothing here goes beyond Outlook: this is a case it reads and
  OpenMailClient did not.
- **2026-10-12** - M2.27: an RTF body's tables may stand inside the cells of other tables
  (RTF 1.9.1, "Nested Tables": `\itapN`, `\nestcell`, and a row defined after its cells in
  `{\*\nesttableprops ... \nestrow}`), and cells merged across (`\clmgf`, `\clmrg`) are one
  cell as wide as all of them, with the left line of the first and the right line of the last.
  Before, an inner table's cells were lost into the text of the outer cell and a merged cell
  was drawn as its parts. An inner row's settings never touch the outer row's, the paragraph
  end written for readers that know no nested tables (`\nonesttables`) is passed over, and a
  cell holds its paragraphs and tables in the order they were written. Tables stand at most 8
  deep and a cell spans at most 64 columns; an inner table nothing closes ends with the cell
  it is in. The sample receipt has a row merged across and a table in a cell. Nothing here
  goes beyond Outlook: these are cases it reads and OpenMailClient did not.
- **2026-10-12** - M2.28: a paragraph of an RTF body has its border (RTF 1.9.1, "Paragraph
  Borders": `\brdrt`, `\brdrr`, `\brdrb`, `\brdrl`, `\box`, with the line, width and colour
  words cells already had, and `\brsp` for the room inside) and the room it asks for above
  and below (`\sb`, `\sa`), on screen and in print. Paragraphs that follow one another with
  the same border stand inside one; a border stands at the paragraph's indent with the text
  just inside it, and where the body says nothing of the room the text still does not touch
  the lines. A cell's lines and its paragraphs' lines are told apart. Room above is at most
  96 pixels and room inside a border at most 64. Crash fix, found by the new hostile-input
  test: a border width near the largest number (`\brdrw2147483647`) overflowed while being
  rounded, in cells as well as paragraphs; it now saturates
  (`hostile_rtf_paragraph_borders_and_spacing_never_panic`). The sample receipt's note has a
  border at its left and room around it. Not read yet: the line between paragraphs
  (`\brdrbtw`), line spacing (`\sl`). Nothing here goes beyond Outlook.
- **2026-10-12** - M2.29: a list paragraph of an RTF body is a list item: the number or bullet
  the writer worked out for it (`\listtext`, and `\pntext` of older writers; RTF 1.9.1, "List
  Text" and "Bullets and Numbering") stands in the room its indent leaves, with the text beside
  it, on screen and in print. Before, it was text followed by a tab at the start of the line.
  A bullet written as a character of a symbol font is the bullet it stands for; a number is at
  most 12 characters; a list paragraph with nothing after its number is still one. Numbers are
  not worked out from the list tables: a writer that leaves no list text gets no number. On
  screen a list marker (of HTML bodies too) is now drawn in the letters of the text it stands
  before, where it used to be in the window's own at one size. The sample receipt has a short
  numbered list. Nothing here goes beyond Outlook.
- **2026-10-12** - M2.30: a tab in an RTF body goes to a tab stop, on screen and in print. A
  paragraph's own stops (`\tx`, with `\tqc`, `\tqr` and `\tqdec` before it; RTF 1.9.1,
  "Tabs") are kept from where its text starts, and the text after a tab starts at a left stop,
  is centred on a centre stop, ends at a right stop, or has its decimal point at a decimal stop.
  Where a paragraph has no stop ahead, a tab goes to the next half inch; before, every tab was
  the width of four spaces. A paragraph has at most 32 stops, none further than 4096 pixels
  along. Not yet: leaders (the dots up to a stop), bar tabs (they are no stop), and stops on
  the lines a long paragraph wraps onto. Preformatted text has a stop every eight characters
  in print. The sample receipt has two lines set
  with a decimal stop. Nothing here goes beyond Outlook.
- **2026-10-12** - M2.31: a paragraph's lines are as tall as it says, on screen and in print. In RTF
  (`\sl` and `\slmult`; RTF 1.9.1, "Spacing") that is a multiple of the lines' own height,
  a least height or an exact one; in HTML it is `line-height` (CSS 2.1, 10.8.1) as a number, a
  percentage or a length, and it holds for the paragraphs inside the element that says it.
  What a line gains or loses is below its text. A multiple is kept between a half and ten,
  a height to 1024 pixels, a printed line is never taller than its page (a line told to be
  taller was set off the sheet: found by the hostile test and fixed with it), and a line is
  never shorter than a picture standing in it. Not compared with Outlook's own spacing yet,
  so where the room goes within a line is ours. Nothing here goes beyond Outlook.
- **2026-10-12** - M2.32: the room a tab takes can be filled, and paragraphs in one border can have a
  line between them. A stop's leader (`\tldot`, `\tlmdot`, `\tlhyph`, `\tlul`, `\tlth`,
  `\tleq`; RTF 1.9.1, "Tabs") is drawn from the text before the tab to the text after it, on
  screen and in print, as marks drawn by us and not as characters of the font. `\brdrbtw`
  (RTF 1.9.1, "Paragraph Borders") is the line between one paragraph and the next inside one
  border, with its own width, kind and colour; before, it was read as no border at all. Not
  yet: on screen, in a paragraph whose lines are told to be taller, a leader stands at the
  foot of its line and not at the foot of its letters. The line between paragraphs is covered
  by tests and has not been looked at on screen. The sample receipt has dot leaders. Nothing
  here goes beyond Outlook.
- **2026-10-12** - M2.33: a list paragraph whose writer left its number out is counted. The lists of
  a body (`\listtable`) and which of them a paragraph is of (`\listoverridetable`, `\ls`,
  `\ilvl`) are read (RTF 1.9.1, "List Table" and "List Override Table"): each level's kind of
  number (figures, roman numerals, letters, a nought before one figure, a bullet), where it
  starts, and the words around it, with the numbers of the levels above where the words ask
  for them ("4.a"). A number the writer did write (`\listtext`) is still the one shown, and
  the paragraph is still counted. Before, such paragraphs were plain text with no number.
  Not yet: a start or a level changed in the override (`\lfolevel`), the older `\pn` lists
  without `\pntext`, kinds of number other than those named, and the indent a level asks for
  where the paragraph gives none. The second step of the sample receipt's list is counted
  this way. Nothing here goes beyond Outlook.
- **2026-10-12** - M2.34: a block's background is behind all of the block. A background on a
  paragraph, a heading, a `div`, a list item or a quotation (CSS 2.1 section 14.2) fills the
  box its paragraphs stand in, on screen and in print; before, it was drawn as a highlight
  behind the letters only. Padding is kept for each side (`padding` with one to four lengths,
  `padding-top` and the others; CSS 2.1 section 8.4), and padding alone makes room without a
  border. In RTF, `\cbpat` (RTF 1.9.1, "Paragraph Shading") is the same fill. Text in a
  filled box is read against the fill, so text that names no colour stays readable on a dark
  one, in the light pane and the dark one. Not yet: boxes inside one another (the inner one
  is drawn alone, with the outer one's fill where it has none), a table inside a filled block
  (it stands outside the fill), a fill on the body itself, background pictures, lengths in
  `em` and percentages. The sample checklist message has a filled note. Nothing here goes
  beyond Outlook.
- **2026-10-12** - M2.35: a link can be followed. A click on a link in the reading pane (text or a
  linked picture) opens a question, never the link: the words of the link, the place it
  really goes in large letters, any concern from the link check (a name that is not the one
  the text shows, a name hidden before an `@`, a bare number), and the whole address, with
  Open in browser, Copy address and Cancel (Cancel has the focus). Only web pages (`http`,
  `https`; RFC 3986 characters, with a host) and single plain mailboxes (`mailto`, RFC 6068)
  are followed; a file, a script, a program's own scheme or an address in other characters
  is refused, and the question says why. A mailbox starts a new message to it. Commands:
  `mail.links` lists a message's links with where each goes, `mail.openLink` checks one and
  returns its address (it wants `"confirm": true` for a link with a concern, and only takes
  a link the message has), `ui.linkDialog` asks. The engine opens nothing: only the Open
  button in the window hands the address to the system's browser (one argument, no shell),
  so a command from the control channel or an agent cannot start one. That last step, the
  browser starting, was not run on the dev machine and is untested end to end. What this
  does on purpose: a link is never opened by the click alone.
- **2026-10-12** - M2.36: print sets text beside a picture at the side, and kerns. A picture a body
  puts at one side (`align`, `float`) is printed there, with the lines beside it shorter by
  its width and the lines after its foot under it; what follows its paragraph stands beside
  it too, until a paragraph asks to start below it or a table, a line or a bordered block
  comes, as on screen. It goes to the next page with its text when its page has no room,
  and stands in the line where less than 90 points would be left beside it. Pairs of
  glyphs stand as close as their face says: from the `kern` feature of the face's `GPOS`
  table (OpenType, pair adjustment, lookup type 2, both formats, through extension
  lookups; the default language system of `latn`, else `DFLT`) or else from its `kern`
  table (format 0). Lines are measured and broken with the pairs counted, and the PDF
  shows them with the `TJ` operator (PDF 32000-1, 9.4.3). A pair is held to half an em and
  never moves a glyph back past its start; tables cut short or filled with anything move
  nothing. Not yet: more than one picture at the sides at once, the reading pane's own
  kerning (it sets text as the toolkit does), shaping and right-to-left text. The printed
  pages were checked by their numbers (tests, and a sample message printed in the
  installed fonts, where 19 of 33 pieces of text carry pairs), not by eye: nothing on this
  machine draws a PDF, and the print preview is next. Nothing here goes beyond Outlook.
- **2026-10-12** - M2.37: page setup. `file.pageSetup` keeps how pages are set up for printing and
  says how they are: the sheet (A4, Letter, Legal, A3, A5, B5, Executive, Tabloid), upright
  or on its long side, a margin for each edge in millimetres, and a header and a footer of
  three parts each (left, middle, right; one line, a part longer than its share is cut
  with a mark). In them `[Page]`, `[Pages]`, `[Page of Pages]`, `[Date]`, `[Time]`,
  `[User]` and `[Subject]` are filled in; these names are our own. What a name gives is not
  looked through for names again, so a subject that says `[Page]` prints as it is. With no
  header or footer the body has their room. `file.print` prints with the setup and takes
  the same parameters for one print only. A margin is held to a third of the sheet, and
  sizes that are no numbers fall back to the least. Not yet: a window for it (it comes
  with the print preview, next), the setup kept between runs, a font or size of one's own
  for header and footer. Not compared with Outlook's page setup; nothing here is claimed
  to go beyond it.
- **2026-10-12** - M2.38: print preview. A new crate, `render` (L3), draws a laid-out page as
  pixels: fills, rules (dashed and dotted as in the PDF), pictures and the outlines of the
  glyphs, from the same pages the PDF file is written from, so what is seen is what is
  printed. `file.printPreview` says how many pages a message takes and, with a path,
  writes one page as a PNG picture (24 to 600 pixels to the inch); it takes the page setup
  for that look only and keeps nothing. In the window, File > Print and Ctrl+P now open
  a print window (`ui.printPreview`): the page beside the paper, the direction, the four
  margins, and the header and footer parts, each of which runs `file.pageSetup` and is
  seen at once; Previous and Next, and Page Up, Page Down, Home and End, turn the pages;
  "Print to PDF" asks for the file as before. Sides of a drawn page are held to 4096
  pixels. `xtask layers` now names the one dependency allowed inside a layer (render on
  layout). The hostile-parameters test no longer leaves two files in the crate's folder.
  Not yet: the setup kept between runs, a zoom, several pages side by side. Outlook's
  print pane was not observed, so nothing is compared with it and nothing here is claimed
  to go beyond it; for agents, a page as a picture from the command line is our own.
- **2026-10-12** - The project is renamed from MailCraft to OpenMailClient: crates, binaries
  (`openmailclient`, `openmailclient-cli`), the settings folder and the documents. The repository
  is public. Nothing else changes.
- **2026-10-12** - M2.39: Table Style. A folder's list is printed as it is shown (searched,
  filtered and ordered): the owner's name over a line, the folder's name, a line saying
  how many items there are, how many unread and how they are chosen, then a row to a
  message with From (To in Sent Items, Drafts and Outbox), Subject, Received, Size and
  Notes (High, Attachment, Flagged, Done). Unread rows are bold; the row of column names
  stands again at the top of every page the table goes on to (`Table::repeat`, new in the
  layout). `file.print` and `file.printPreview` take `"style": "memo"` or `"table"`,
  `file.pageSetup` keeps it, and the print window chooses between the two; printing is
  now offered whenever a folder is shown, and Memo Style with no message open says so.
  `[Folder]` can be asked for in a header or footer. A list longer than 5,000 rows is cut
  and says so; a column is never narrower than a short word. The columns and the head of
  the page are our own: Outlook's Table Style was not observed (it would need its print
  dialog), so nothing is compared with it or claimed to go beyond it. Not yet: the
  columns of the view as the person arranged them, group headings, the selected rows
  only.
- **2026-10-12** - M2.40: bitmaps beside a metafile's drawing. A Windows or enhanced metafile that
  draws shapes or text and one or more bitmaps is now drawn whole, where it used to be left as
  its place: the bitmap's pixels are read ([MS-WMF] 2.2.2.9: 1, 4, 8, 16, 24 and 32 bits to
  the pixel, a table of colours or bit fields, bottom row first or top row first, the
  twelve-byte header too) and laid over the box the record names, inside what the drawing is
  cut to and under what is drawn after it (META_STRETCHDIB, META_DIBSTRETCHBLT, META_DIBBITBLT;
  EMR_STRETCHDIBITS, in a world that is scaled and moved). Limits: a bitmap side of 4,096
  pixels and 8 million pixels for all the bitmaps of one drawing, so a file of many large
  bitmaps is given up soon. Still not drawn, and so the metafile stays a place rather than
  half a picture: a part of a bitmap, one mirrored or turned, a copy that is not a plain one,
  colours that are numbers in a palette, pixels packed in runs (RLE), EMR_BITBLT and its
  kin, META_SETDIBTODEV beside a drawing. Checked by tests on files we build; no metafile of
  Outlook's was compared, and nothing here goes beyond Outlook.
- **2026-10-12** - M2 closed. Formatted reading is done as far as this milestone takes it: HTML and
  RTF bodies are laid out and drawn with their fonts, lists, tables, borders, fills and
  pictures (PNG, JPEG, GIF's first frame, bitmaps, metafiles that are drawings), attachments
  stand in their places, links open after being asked, and a message or a folder's list
  prints to a PDF file with a page setup and a preview. What M2 does not do is listed in the
  roadmap under "What M2 leaves" and ranked in docs/gaps.md (item 3); none of it is claimed
  as done. Nothing printed was compared with Outlook's print output. M1's checks against
  Outlook-written files stay open. Next is M3, writing `.pst`.
- **2026-10-12** - What M2 left is now scheduled, not just listed: "Do later (from M2)" gives
  every item a slot (M2.41 and M2.42 before M3, a batch M3b after it, M4, M5), and an item is
  struck from the table when it lands. Right-to-left text and shaping are taken first, before
  M3, because without them whole languages are unreadable.
- **2026-10-12** - M2.41: right-to-left text. A paragraph now knows which way it is written: HTML `dir`
  (`rtl`, `ltr`, `auto`) and `direction` in a style, with `text-align: start` and `end` as sides
  of that way; RTF `\rtlpar` and `\ltrpar`; plain text by its first letter that has a
  direction, paragraph by paragraph. A right-to-left paragraph starts at the right unless it is
  told a side. Each line is put in the order it is seen in by the Unicode rules (UAX #9, with
  the `unicode-bidi` crate for the levels and our own L1, L2 and L4: lines are broken first and
  turned one by one, brackets are mirrored), in the reading pane and in print, so Hebrew and
  Arabic read from the right with Latin words and numbers inside them the right way round. In
  print, Arabic letters take the shapes they have beside their neighbours (the presentation
  forms, with the lam-alef ligatures) where the font has them; on screen egui's shaper already
  joins them from the font's own tables, and is handed one piece of a word at a time because
  it takes everything it is handed at once to run one way. Limits: 200,000 characters to a
  paragraph in print, 60,000 characters and 8,000 pieces on screen, past which the text is
  drawn as it is stored. Made better on the way, for every table: text at the right or the
  middle of a cell narrower than the least a text wraps at was drawn past the cell's edge, on
  screen and in print, and now stays inside it. Moved to "Do later": selecting text in a
  paragraph that was turned, the rest of a right-to-left paragraph's furniture, and
  right-to-left text outside the body (the list, the header, folder names). Checked by tests
  on text we wrote and by looking at a Hebrew and Arabic message of ours on screen and on a
  printed page; Outlook's drawing of the same message was not compared, and nothing is claimed
  about it.
- **2026-10-12** - M2.42: shaping in print. A stretch of text in one face, one look and one direction is
  now handed whole to the face's own `GSUB` and `GPOS` tables (through `harfrust`, a Rust port
  of HarfBuzz, MIT), in print, in the print preview and in a PDF: letters drawn as one glyph
  are (fi, ffi), pairs stand as close as the font says, accents and vowel signs stand where the
  font puts them, Arabic is joined by the font's own shapes with its marks placed, and scripts
  that need their letters reordered or stacked (Devanagari, Thai) come out as the font draws
  them. A stretch is shaped knowing the letters beside it, so Arabic joins across a change of
  look (bold, colour) inside a word. A face with neither table, Arabic in a face that does not
  join it, and any answer that is out of order or names a glyph the face has not got keep the
  way of M2.41 (our own pairs and the presentation forms). Made better on the way: a PDF now
  says what its glyphs stand for (`/ActualText`) wherever that is not the glyphs' own
  characters one after another, and the pieces of a turned line are written in the order they
  are read, so text copied from or searched in a PDF of Hebrew, Arabic or ligatures is the text
  that was written; and a character the italic or bold face of a family lacks (Arial Italic has
  no Arabic) is taken from the family's plain face, leant or thickened, where it was a box
  before. Limits: a glyph is put no further from its place than twice its size and never
  outside the line's room; a piece says at most 4,096 UTF-16 units. Not shaped yet, in "Do
  later": the name and page number at the head and foot of a page, and list markers. Checked
  by tests on fonts we make ourselves (with `GSUB` and `GPOS` tables written by the tests, and
  300 damaged copies of them) and by looking at a printed page of our own text in Latin,
  Arabic, Hebrew, Devanagari and Thai with the fonts installed here; no PDF reader's copying
  was tried, and Outlook's printing of the same text was not compared.
- **2026-10-12** - M3.1: the lowest layer of a `.pst` is written. `crates/pst/src/write.rs` makes a whole
  Unicode data file (512-byte pages, the kind Outlook 2003 and later open) from blocks and
  nodes: the header with both checksums, an allocation map in front of every 253,952 bytes and
  the page maps older readers expect, the density list, blocks with their trailers, data trees
  of one and two levels for items up to 4 GB, subnode trees with an index block, and the two
  B-trees of pages, with every reference count kept. Blocks can be left plain or encoded either
  way Outlook does. A file is always written whole and front to back, never changed in place,
  which is what the temporary-file-and-swap of M3.4 needs. Nothing a user can reach writes a
  `.pst` yet: the layers above (M3.2, M3.3) are next. Made better on the way: blocks with the
  same bytes are stored once and counted (the same attachment in fifty messages is one set of
  blocks; the format allows it through its reference counts); and
  `openmailclient-cli inspect file.pst --check` now also holds the file's structure against
  the format - header checksums, allocation maps, every page and block checksum and signature,
  blocks that overlap or lie in space called free, and reference counts - where it only loaded
  every item before (opening a mailbox checks no checksums). The check reads only; it covers
  Unicode files with 512-byte pages and says so for ANSI and 4K-page files (slot: M3b).
  Checked by 8 tests: what is written is read back by our reader in all three encodings, a
  33 MB item across 133 sections, 800 subnodes, 1,000 nodes, refusals for what the format cannot
  hold, and 400 damaged copies that the check and the reader survive. Not checked: no file
  from this writer has been opened in Outlook (there is nothing above the node layer to open
  yet), and the places of the free map and free page map pages in files over 32 MB and 2 GB are
  worked out from the coverage the specification gives, not compared with a file Outlook wrote.
  The bit order of the allocation maps was seen in the structure of a local data file; our
  files fill them in whole bytes, so they do not depend on it.
- **2026-10-12** - M3.2: the middle layer of a `.pst` is written. `crates/pst/src/ltp_write.rs` builds a
  heap over as many blocks as it needs (with the block headers and fill levels of each kind),
  the index kept in a heap with as many levels as it needs, and on them a property list and a
  table: values of up to four bytes in the record (eight in a table row), up to 3,580 bytes in
  the heap, larger ones in a subnode of their own; a table's rows in the heap or, when they
  are many, in a subnode whose blocks each hold whole rows. Values go in and come out as the
  bytes the file stores, so a property this code does not understand is written as it was
  read. Still nothing a user can reach writes a `.pst`: folders and messages are M3.3. Checked
  by 5 tests, all through our own reader and the structure check: every kind of value the
  model has, 3,000 properties in one list (more than eight heap blocks, an index of two levels), 6,000 rows
  in one table, rows with cells missing, and refusals for a property or a row id given twice
  and for more than 255 columns. Not checked: nothing here has been opened in Outlook, and
  which half of a byte holds the earlier block's fill level is not said by the specification
  and was not seen in a file (no local data file has a heap of several blocks). Noted for
  M3.4: the model keeps a 32-bit float as a 64-bit one and a currency or error value as an
  integer, so those types change when an item is written from the model; items that were not
  changed will be copied as stored.
- **2026-10-12** - M3.3: a whole mailbox is saved as a `.pst`. `crates/pst/src/store_write.rs` writes the
  messaging layer over M3.1 and M3.2: the message store, the name-to-id map, the root folder,
  the search folders and tables every data file must have ([MS-PST] 2.7.1), then each folder
  with its hierarchy, contents and associated-contents tables, each message with its
  recipients and attachments, and messages attached to messages (to 32 deep). The command is
  `file.export` (File > Export to a Data File); `openmailclient-cli run --save out.pst` and
  `openmailclient-cli convert in.pst|in.ost out.pst` reach it too. Checked through our own
  reader only: the sample mailbox and a mailbox of 1,500 messages come back the same, a
  saved file saved again gives the same mailbox, and the structure check finds nothing; three
  local `.ost` copies went through with every item counted on the other side. **Not opened in
  Outlook** (that adds a data file to the owner's profile and waits for their say), so two
  readings of the specification stand unproved: where a message attached to a message is kept,
  and the bucket a named property falls in. Made better on the way: the new file is written
  beside the target, synced, held against the structure check and only then put in the
  target's place, so a failed export leaves what was there byte for byte and an unsound file
  never lands; a source that cannot give an item or a folder's list loses only that, and the
  result says how many (a damaged file can be salvaged into a sound one); the contents tables
  carry the sender's name and the flag, so a list shows them without opening each message; a
  data file's folders are now listed parent-then-children, as the folder pane shows them
  (`inspect` indented them wrongly before). Left for later, each with a slot: hidden
  (associated) items and folder properties beyond name, class and counts; the ids that tell
  Outlook which folder is the Calendar or Contacts; export off the UI thread with progress;
  table cells whose type differs from the column's.
- **2026-10-12** - M3.4: changes to an open `.pst` are kept. A Unicode `.pst` now opens editable: reading,
  flagging, moving, deleting, new messages and folder changes are held beside the open file
  (`crates/pst/src/edit.rs`), which stays byte for byte as it was until `file.save` (File >
  Save Changes, Ctrl+S). Saving writes a whole new file beside the old one. Every node the
  changes do not touch is copied as it is stored, block for block, so hidden (associated)
  items, views, search folders, folder properties and values the model has no place for go
  across untouched; a changed message keeps every stored property that was not changed, and
  its recipients and attachments as stored when they are the same; a moved message keeps its
  node and its id. The new file is held against the structure check, read back and compared
  with what was shown, and only then put in the old one's place in one step. A save is
  refused, with the file left alone and the changes still there, when another program wrote
  the file since it was opened or (on Windows) has it open. An `.ost`, an ANSI file and a
  4K-page file stay read-only. Closing a data file with unsaved changes is refused unless it
  says `{"save": true}` or `{"discard": true}`; leaving the app with unsaved changes says so
  once and needs a second asking. Nothing is ever written without being asked for. Checked
  through our own reader only (the sample mailbox, a file given content our writer never
  makes, damaged files, and three local copies converted and then changed: counts only);
  **not opened in Outlook**. Made better on the way: the first save of a data file keeps the
  file as it was beside it (`name.before-openmailclient.pst`, a second name for the same bytes,
  so no copying), and later saves never write over that; the File page says which data files
  have unsaved changes. Left for later, each with a slot: changes journalled as they are made
  (a crash loses what was not saved, never the file); a save that does not rewrite the whole
  file; saving off the UI thread; unknown properties of an attachment when a message's
  attachments change; stale cells for a changed message in search-folder tables; `file.export`
  still goes through the model.
- **2026-10-12** - M3.5: settings are kept between runs, and M3 is closed. `settings.save` and
  `settings.load` (`crates/engine/src/cmd/settings.rs`) write and read one small JSON file:
  the page setup and print style, where the reading pane is, whether the folder pane shows,
  the data files that were open, and what the window keeps for itself (light or dark, the
  ribbon's kind, bodies read as plain text). The app keeps it in the per-user configuration
  folder (`%APPDATA%\OpenMailClient\settings.json`; `~/.config/openmailclient/` on Linux,
  `~/Library/Application Support/OpenMailClient/` on macOS) and opens last time's data files
  again unless the run names a file or `--sample`; `--no-settings` neither reads nor writes
  it. The file is read as anything else from disk: capped at 1 MB, taken a part at a time, a
  part that makes no sense left out and counted, a file that is not ours changing nothing
  and never failing the start; only `.pst`/`.ost` paths are opened again, at most 32, and
  one that is gone is counted and said in the status line. It is written beside itself and
  put in place in one step. No passwords or tokens will ever go in it (CLAUDE.md: those live
  in the OS credential store). Made better on the way: settings are written when they
  change, not only on the way out, so a run that ends badly keeps them; each part stands by
  itself, so one bad value never costs the rest; the file is plain JSON a person can read
  and carry to another machine. M3 closes with what it was for done (a mailbox written as a
  `.pst`, changes to an open one kept, a check of the structure, settings kept); what its
  first wording also named has a slot: changes journalled as they are made (M3b) and rules
  kept between runs (M5, where rules are built). Nothing our writer made has been opened in
  Outlook yet (needs the owner's say).
- **2026-10-12** - M3b.1: unsaved changes are journalled. Each change to an open Unicode `.pst` is now
  also written, as it is made, to a journal beside the file (`name.pst.openmailclient-journal`,
  `crates/pst/src/journal.rs`); the data file itself is still written only by `file.save`.
  When the file is opened again after a run that ended without saving, `file.open` plays the
  journal back and the changes are there, unsaved as they were (`{"recover": false}` leaves
  them where they are); `file.save` puts them in the file and takes the journal away, and
  closing with `{"discard": true}` throws both away. The journal is a list of what was asked
  for, each record a length, a checksum and the request, and is tied to the file it was made
  for by the file's length and the sum of its header: a journal for a file that anyone has
  written since is never applied (it is left alone, and kept under `.old` when a new one
  starts). Playing back stops at the first record that is cut short, damaged or cannot be
  done; what came before stands, the journal as found is kept under `.old`, and the status
  line says how many were lost. A journal is read as hostile: lengths are believed only as
  far as the file goes, counts only as far as the record goes, messages attached to messages
  16 deep. Two running copies do not write one journal: the second is told, and its changes
  live in memory only (the File page and the word at exit say so). Made better on the way: a
  change to a message is journalled as what changed in it, and a move as the move, so marking
  a message with a 600 KB attachment as read and moving it costs under 1 KB; leaving the app
  with unsaved changes no longer loses them, and the word at exit says they come back. Each
  record reaches the system as it is made (a crash of the program loses nothing) and the disk
  every 64 records (a power cut may cost the last few; the cut record is dropped, never half
  applied). Not yet, each with a slot: the journal is beside the data file only (a folder
  that cannot be written means no journal, said on the File page); a save still rewrites the
  whole file.
- **2026-10-12** - M3b.2: export copies a data file as it is stored. `file.export` (and `convert` in the
  CLI) of a Unicode `.pst` now writes the new file node for node, the way `file.save` does,
  instead of going through the model: hidden (associated) items, a folder's own properties,
  the ids that say which folder is the Calendar, and properties this program does not know
  are in the copy (`"copied": true`). Unsaved changes are in the copy too, and the source
  stays as it is, still unsaved, with its journal. The copy keeps the id of the data file it
  was made from, as a copy of the file itself would. Made better on the way: a source with
  damage that cannot be copied so, or whose copy does not pass the structure check, is
  exported the old way, as far as it reads, and the status line and the result say that
  hidden items are then not in it (pinned by a test that damages a file in one place after
  another); the copy is also the file made small again (free space is not carried). The
  sample mailbox, an `.ost`, an ANSI and a 4K-page file still go through the model.
- **2026-10-12** - M3b.3: the panes are sized by dragging, and the window stands where it stood. The edge
  between the folder pane and the list, and between the list and the reading pane (beside it
  or under it), is taken hold of and dragged; the sizes go through a command (`ui.layout`),
  are held to what leaves the other panes room, and are kept in the settings with the
  window's size, its place and whether it was maximized. The next run opens the window there.
  A window is never put where no screen is: a settings file is believed only between the
  smallest window and the largest screen, the first frames see that the title bar can be
  reached on the screen as it is now and bring the window to the corner if not (tried on the
  real window: a place of 9000 across came back at 40, 40), and a place on a second screen
  is not kept (egui does not say where the other screens are; the size is). Made better on
  the way: a double click on an edge gives that pane the size it started with, and
  `ui.layout {"reset": true}` brings the panes and the window back to where a first run has
  them; the settings file is written at most once a second while a window is dragged. The
  list has no columns to keep yet (it shows each message on three lines); that waits for
  the views of M5.
- **2026-10-12** - M3b.4: right-to-left text outside the body. Every single line the window draws from a
  message or a data file (folder names, the sender, subject and preview in the list, the
  header of the reading pane, the title, the chips of attachments) goes through one place,
  and that place now puts a line with Hebrew or Arabic in it in the order it is read
  (UAX #9), its direction that of its first letter, the same way a paragraph of a body is
  turned. Made better on the way: a line too long for its room loses the end of the text as
  it is read, with the ellipsis there, and not whatever happens to stand at the right; and
  the marks that turn the text after them round (embeddings, overrides, isolates) are left
  out of such lines, so an attachment called `photo<mark>gpj.exe` is shown with the ending it
  has and not as `photoexe.jpg` (found by the test written for it: the marks were honoured
  at first). Looked at in a headless capture with our own Hebrew and Arabic lines. Not yet:
  the compose box and the search box (egui's own text editor draws them as stored), and a
  right-to-left line is still set from the left of its cell.
- **2026-10-12** - M3b.5: the text print sets itself is set as the body is. What stands at the head and
  the foot of a page and the marker before an item of a list were set glyph for character,
  from the left: they now go the way a paragraph's text goes, so letters the font draws as
  one glyph are, pairs stand as close as the font says, Arabic letters join, and a part with
  Hebrew or Arabic in it is drawn in the order it is read and copied out of the PDF in that
  order. (The field names and values of the header already were: they are paragraphs in a
  table.) Made better on the way: a part too long for its share of the line loses the end
  of what it says as that is read, whichever way it is drawn; a marker too long for the
  room before its item is cut the same way and no longer hangs off the sheet (found by the
  junk test: a twelve-letter marker stood fifteen points left of the page); and a body that
  filled what a print may hold still has its pages' heads and feet set. Looked at in a page
  rendered from our own text.
- **2026-10-12** - M3b.6: what a right-to-left paragraph moves to the right. A paragraph written from the
  right is now set in from the right by the room its list level and its quotation take; the
  marker of an item stands at the right of the item, outside its text; the bars of a
  quotation stand at the right. The same on screen and in print, pinned in print as the
  mirror of the same paragraph written from the left. Made better on the way: the marker is
  read the way its item is, so the stop after a number stands at its left (`.1`), and a
  marker at the right is held on the sheet as one at the left is. Looked at in the window
  and on a rendered page with our own Hebrew and Arabic lists. Not yet, and listed: tab
  leaders, a frame's own indent, text beside a picture, the columns of a right-to-left table,
  `<bdo>` and `unicode-bidi`.
- **2026-10-12** - M3b.7: the journal's loose ends. When the journal cannot be kept beside the data file
  (the folder is not to be written), it is kept in a folder of the program's own beside the
  settings (`journals/`, under the data file's name and a number made of the whole of where
  the file is), and found there when the file is opened again, also once the folder can be
  written again; what is cut off a damaged journal is kept beside the journal, wherever it
  is. A journal another running copy holds is never gone around that way. The window now
  has the journal forced to disk no more than a second after a change, where it was every
  64 changes only, so a power cut costs the last second and not up to 63 changes
  (`Session::settle`; the store has no clock of its own, which keeps it building for the
  web). The count of what a damaged journal lost is now exact where it can be: a record
  whose sum is wrong is counted with each that follows it, and a last record cut short is
  one; only when a length itself is wrong, so that records cannot be told apart any more,
  does the status line say "or more" (`lostMore` in what `file.open` returns). Pinned in
  `crates/pst/src/tests_journal.rs` and the engine's tests.
- **2026-10-12** - M3b.8: right-to-left text while it is typed. Every field of one line is now our own
  (`crates/ui-egui/src/line_edit.rs`): the search box, a folder's new name, the To, Cc, Bcc
  and Subject lines and the head and foot of a printed page. The toolkit's field drew text as
  it is stored, so Hebrew or Arabic mixed with other writing came out in the wrong order and
  the cursor stood at the wrong end of it. The line is now put in the order it is read as it
  is typed, Arabic letters joined; the cursor stands where the next letter will go on from;
  what is selected is marked where it is seen; a click lands on the letter under it; and the
  arrow keys go the way the eye does in a line read from the right. Made better on the way:
  a field come to with the keyboard has its text selected, to be typed over; what is typed
  or pasted is one line and brings no mark that turns text round, and one that is already in
  a name is shown as a dot and can be seen to be taken out; a paste is cut to what the field
  takes; typing is taken back a word at a time (Ctrl+Z, Ctrl+Y); and the lines above a message
  being written that say what is wrong with it, the names on its attachments and the words
  of the link question read the right way too (they were missed in M3b.4). Looked at in the
  window headlessly: `ui_shot` scripts can now click and type. Not yet, and listed: the body
  of a message being written is still the toolkit's box; it gets its own editor with the
  item windows (M5), where its text also stops being plain.
- **2026-10-12** - M3b.9: what was left of a paragraph written from the right. A tab now stands between what
  is on either side of it the way the paragraph is read (UAX #9, L1: a tab and the blanks
  before it take the paragraph's direction), so its stops are counted from the right and the
  dots or line that fill its room lie between the two texts, in print and on screen. A
  border set in from the side is set in from the side its paragraphs start at. The room a
  picture at the side takes is taken from the right side of the lines beside it. A table
  written from the right (`dir=rtl` on it or around it, `\rtlrow` in RTF) has its first
  column at the right, cells that cover several columns over the same ones, and stands at
  the right when the text around it is read from the right; its lines stay where they were.
  `<bdo>`, `<bdi>`, `dir` on a piece of a line and `unicode-bidi` (embed, isolate,
  bidi-override, isolate-override, plaintext) are carried as the marks Unicode has for them,
  opened again in every paragraph the element goes on in and closed where it ends, so the
  screen and the page read them the same way. Found and put right on the way: the leaders
  of tabs were never drawn on screen in a line with right-to-left text; the text of a cell
  in a table inside right-to-left text was put back at the left; the room beside a picture
  counted the paragraph's indent on the wrong side. Made better: the words of a link cannot
  be turned letter by letter (an override element or mark inside a link is dropped, and a
  link inside overridden text is set apart from it), so an address that is shown reads as
  it is written. Looked at headlessly, window and page. Not compared with Outlook.
- **2026-10-13** - M3b.10: copying text out of a printed PDF, tried in a reader. A message of our own with
  Latin ligatures, Hebrew, Arabic, brackets, numbers and accents was printed and read back
  with xpdf's `pdftotext` 4.06, the one reader on this machine that gives its text to a
  script. What it showed: a line of Hebrew or Arabic came out letter for letter backwards.
  The page said, around each piece of a line, how the piece is read (`/ActualText`); the
  reader took that and then turned it round as it turns every right-to-left run it meets.
  Now a piece written from the right is given the way every other PDF gives it: glyphs in
  the order they are seen, each saying the character whose shape it is (a bracket as it is
  drawn), and the reader does the turning. Only glyphs that are not one character each say
  what they stand for, each for itself and inside the text it belongs to: letters drawn as
  one glyph (fi, ffl, lam-alef), a letter drawn as several, a mark read with its letter.
  Made better on the way: a glyph the file already says is one character says so itself
  where it stands for another, so two characters that share a glyph are both copied right.
  Read back after the change: Hebrew and Arabic words, the ligatures, the accents and the
  brackets are copied as written, in the order read. Still wrong in that reader, and its
  own guess, not something the file can say: a full stop or a number in a line written from
  the right is put on the wrong side of the words, and a space beside a Hebrew word in a
  Latin line moves to its other side. Its `-raw` mode gives right-to-left text as it is
  seen, as it does for any PDF. No other reader was tried (none here gives its text to a
  script, and windows are not driven with keystrokes on this machine).
- **2026-10-13** - M3b.11: what is stored on an attachment is kept, and search folders show a changed
  message as it is now. Until now, adding, removing or moving one attachment of a stored
  message wrote every attachment again from the model, so anything stored on them that the
  model has no place for (a property under a name of another program's, a value of a kind
  not understood) was dropped from all of them. Now each attachment in the new list is
  matched with the one the file holds: first those that are the same but for their place,
  then those with the same contents and other properties. A matched attachment stays in
  the subnode it has, every stored property with it; only what the model says differently
  is written over, and its number in the list is set. Only an attachment that is truly new
  is written from the model, under an id past every one the file had. An attachment whose
  stored form cannot be opened is written from the model, as before. Made better on the
  way: a renamed attachment keeps what was stored on it too, and so does a message attached
  to a message, which stays in the file as it was stored and is not written again. Search
  folders: their tables were only cleared of deleted messages; the row of a changed message
  kept saying what the message said before. Now such a row is brought up to date the way
  the folder's own table is: what the message has now under the columns the table has,
  less what the message lost, and what only the table knows left alone. A table none of
  whose rows changed still goes across block for block. This is also the first test that a
  search folder's rows survive a save at all: our writer makes no search folder with rows,
  so the test writes one as another program would. Not done: the list of search folders a
  message belongs to is not worked out again (a message changed so that it no longer fits
  a search stays in that search's table until the program that made it looks again), and
  the third item of this task, a save that does not write the whole file, is M3b.12.
- **2026-10-13** - M3b.12: a save that does not hold the window. `file.save` takes `"wait": false`: the
  long part (writing the new file beside the old one, the structure check, reading it back
  and comparing) is done on a thread of its own, with its own handle on the data file and
  its own copy of the changes; the short part (the new file takes the old one's place, the
  journal is ended) is done by the caller when `Session::poll` finds the work finished, after
  looking once more that no other program changed the file meanwhile. Until then the data
  file is read as before and takes no changes: what is saved is what was shown when the
  save was asked for, and a change asked for meanwhile is refused with a reason, not lost.
  Selecting a message while a save runs shows it and does not mark it read. Closing the
  data file is refused until the save ends; leaving the program waits for it. The window
  saves this way (Ctrl+S and Save Changes on the File page); the command line and callers
  that say nothing wait, as before. The status bar shows the name of what is being saved,
  a bar that fills and a percentage, seen in a headless shot with a 240 MB mailbox written
  by our own exporter. Made better on the way: a save can be stopped (`file.stopSave`, and
  Stop in the status bar); the data file stays as it was, the changes stay unsaved and in
  the journal, and the status says it was stopped and not that it failed. The file written
  beside the data file is removed whether the save ends, is stopped, fails, or the store
  goes away first. A panic in the work becomes an error; no thread on the web build, where
  the save waits. Not done: every save still writes the whole file anew. Writing only what
  changed means writing into the file that holds the mail, which the rule that a failed
  write leaves the file byte for byte as it was forbids unless such writes are journalled
  themselves; that stays in Do later. Export still holds the window.
- **2026-10-13** - M3b.13: the structure check looks inside the nodes. Until now it stopped at blocks and
  nodes: checksums, maps, the two trees and the reference counts. A file could pass it and
  still hold a property list no other program can read. Now, in a file sound that far, every
  node that is to hold properties or a table (folders, messages, attachments, hidden items,
  the store, the name map, the tables of folders, attachments and recipients, and the same
  among the subnodes, to eight deep) is held against [MS-PST] 2.3: the heap's signature and
  kind, each block's page map (inside the block, after its header, allocations in order);
  an index's header, the sizes of its keys and entries, its levels, keys ascending; a
  property's value where its record says (an allocation that is there, or a subnode that is
  there), at the size its kind has, text of two bytes a letter with an even length, a list
  of values with its starts in order, an object naming a subnode that is there; a table's
  header at the size its columns need, the row laid out as four, two and one bytes and a
  bit for each column, columns in the order of their tags, each where a value of its size
  belongs and with a bit of its own, the row's id and version where they belong, the rows
  filling their allocation, the row index naming rows that are there, once each, with the
  id the row has, and each cell kept elsewhere being there. One line names the first thing
  wrong with a node. `Check::inside` has them; `openmailclient-cli inspect` lists them with
  the rest. What it found in our own writer on the first run: an item carrying a value
  typed as an object, with bytes that name no subnode, was written as it came; such a value
  is now left out (there is nothing it could be read from), and a value of a kind with a
  size of its own is written at that size whatever came. Made better on the way: a save is
  stopped by what it did wrong and not by what was wrong before. When the new file has
  something wrong inside a node, the file as it was is checked too, and only what is new
  stops the save: a file from another program with an oddity of its own is still changed
  and saved, its nodes going across as stored. 3 tests: 27 kinds of damage each named, some
  10,000 changed bytes and cut-short heaps without a panic, and a save refused for its own
  mistake and not for an old one. Not done: fill levels of a heap are not compared with the
  space that is free; the check covers Unicode files with 512-byte pages only, as before;
  no file written by Outlook of that kind is at hand to run it on (the profile here has
  `.ost` files with 4K pages), so what it says of such a file is not yet known.
- **2026-10-13** - M3b.14: an export that does not hold the window. `file.export` with `"wait": false`
  comes back at once with `{"started": true}` when the source is a Unicode `.pst` that can
  be copied as it is stored: the copy and the structure check run on a thread of their own,
  through a handle of their own on the data file, and write beside the target as before;
  `Session::poll` puts the new file in the target's place when the work is done, and
  `Session::finish_saves` waits for it before the window goes. The status bar says
  "Exporting to name.pst" with the bar, the percentage and Stop that a save has;
  `file.stopSave` stops exports as well as saves, and a stopped export leaves nothing where
  it was to go. File > Export in the window asks for it this way. Made better on the way:
  the mailbox is not locked while it is exported. A save holds the store until it is done,
  for the file under it is being replaced; an export takes the changes as they stand when
  it is asked for and reads the file itself, so mail is read, changed and deleted meanwhile
  and the new file holds the mailbox as it stood (a test deletes a message while the export
  runs and finds it in the exported file). Two exports to one file are refused; a data file
  is not closed while it is exported; a target opened here meanwhile is not written over; a
  session that goes while an export runs waits for it to stop and removes what it wrote.
  When the copy fails for a reason other than being stopped, the mailbox is written from
  what is read, as before, and that part holds the caller. 1 test. Not done: a mailbox that
  is in no data file, an `.ost`, an ANSI `.pst` and a 4K-page file are exported on the
  caller's thread still (they are written through the model, which is not to be shared
  between threads as it is); the bar was not looked at in a screenshot for an export, the
  drawing being the one a save uses with other words; the structure check at the end does
  not hear Stop, so a stop asked for in the last moments takes until the check is done.
- **2026-10-13** - M3b.15: the folders other programs find by id. A file made by `file.export` from
  a mailbox read through the model named only the Outbox, Sent Items and Deleted Items on
  its store. Now the first folder of each kind among Calendar, Contacts, Journal, Notes,
  Tasks and Drafts is named by an entry id on the root folder and on the Inbox
  ([MS-OXOSFLD] 2.2.3: 0x36D0 to 0x36D4 and 0x36D7), so a program that looks for the
  calendar by id and not by name finds it. Made better on the way: the reader takes the
  same ids from the root of any file it opens, so a Drafts folder is known as Drafts
  whatever it is called in the file's language (it was known by its English name alone;
  the other five were known by their class already). An id that names no folder in the
  file is passed over. 1 test. Not done: nothing names the Inbox in a data file (a server
  says which folder mail is delivered to), so an Inbox with another name is a mail folder
  like any other; written from the specification alone, as the rest of an exported file is
  until one is opened in Outlook.
- **2026-10-13** - M3b.16: three small things in the writer. (1) Values keep their kind. A 32-bit
  float was read as a 64-bit one, currency as a whole number, a time kept as days as a
  float, and an error code as a number, and each was written back as what it had been read
  as when an item was written from the model: another type, and for a float another size.
  The model now has a kind for each (`PropValue::Float32`, `Currency`, `FloatTime`,
  `Error`), so a `.pst`, a `.msg` and the journal of unsaved changes write them as they
  were. Made better on the way: an error code standing where a value could not be given is
  no longer taken for a number by what reads a property as one. (2) A file rewritten by a
  save goes on from the counters of the header it replaces ([MS-PST] 2.2.2.6): the next
  node id of each kind is never lower than it was, so the id of a deleted message is not
  given to a new one (a program holding the old id would have found another message
  under it), and the count of how often the header was written goes up by one where it was
  set back to one. Counters that make no sense are held to what a node id can be. (3)
  `file.keepOriginal` chooses whether the file as it was stays beside a data file on its
  first save; it does until told otherwise, and the choice is kept with the settings. 2
  tests, and more in three that were there. Not done: `file.keepOriginal` has no place on
  a settings page (there is none yet; M5); a file kept before is not removed when the
  choice is turned off.
- **2026-10-13** - M3b.17: attachments, the two cases left. (1) A file put in again with other
  contents. When a message's attachments changed, a stored attachment was known again by
  its contents alone, so a file that was worked on and attached again was written as a new
  one and lost every property this program has no place for. Now two more ways of knowing
  it follow the two there were: a file with other contents and the same properties, then a
  file with other contents under the same name and kind. Its new contents take the place
  of the old in the node it had (a plain file's bytes, or the object of another program in
  a subnode of its own, the old one let go), and what else is stored on it stays. A file
  with another name and other contents is a new file, as it should be; a message inside a
  message is known by what it holds alone. (2) Attachments that cannot be read. A message
  showed only those of its attachments this program could read, and saving a change to the
  list wrote only those: a damaged attachment, or the ones past the twenty thousand we
  read, were gone from the file though nobody had taken them away. They now stay as they
  are stored, after the others, with their rows; a message left with only such attachments
  still says it has attachments. Made better on the way: the property list of a changed
  node is written after the subnodes that stay with it are known, so a value that names
  one is checked against what is really there. 1 test (three files put in again, a file
  under another name, an attachment that cannot be read through two saves). Not done: an
  attachment that cannot be read is carried, not mended; its row keeps the place it is
  given after the readable ones and not the one it had among them.
- **2026-10-13** - M3b.18: three things left over in the data file's tables and its check. (1) A value
  of another type than its column's. A table's column has one type ([MS-PST] 2.3.4.2); a
  message that had the value as another type (a 16-bit number where the column is 32-bit,
  8-bit text where the column is Unicode) got no cell for it. Now the value goes in as the
  column's type when nothing is lost by that: whole numbers into a wider one or a narrower
  one with room, a 32-bit float as a 64-bit one and back when it comes back the same, text
  of 7-bit characters as the other kind of string. This holds for a changed message's row
  in its folder's table and in search folders' tables, for a message or folder that came
  from elsewhere, and for a mailbox written from the model. The item keeps the value as it
  was given in every case. Made better on the way: a value that cannot go in (a number too
  large, a kind that is no number) used to leave the cell saying what the message said
  before the change; the cell is now empty, so a list never shows what is no longer true.
  (2) The structure check hears Stop: `check_until` looks at the progress before each block
  and each node and gives `PstError::Stopped`; a save stopped while its new file is checked
  says it was stopped, and an export on a thread of its own stops in the check as it does in
  the copy. (3) Fill levels: the check reads the four bits a block that say how full each
  block of a heap is (the header's eight and each later map's 128, [MS-PST] 2.3.1.2 and
  2.3.1.4), and says when a block is said to be fuller than it is, to have more room than
  it has (unless it has given allocations up, which leave room its length does not show),
  or when a level is given to a block the heap does not have. A level may be one step off,
  for how another program counts the edge of a level is not known. Which half of a byte
  comes first was a guess; heaps of one block in Outlook-written files read locally have
  the level in the low half of the first byte, as we write it (counts only were looked at).
  3 tests. Not done: a heap of several blocks written by Outlook has not been seen, so the
  second block's half of the byte rests on the first; text beyond 7 bits is not converted
  between the kinds of string (it depends on a code page); the check is still not run on a
  512-byte-page `.pst` written by Outlook, there being none here.
- **2026-10-13** - M3b.19: an export that does not hold the window, for every kind of mailbox. A store
  gives itself apart (`Store::apart`): the mailbox as it stands when asked, to be had on
  another thread. A mailbox in memory hands over a copy of itself; a data file of any kind
  is opened there through a handle of its own, with the unsaved changes as they are, after
  a look that no other program changed the file meanwhile. `file.export` with
  `"wait": false` now starts for the sample mailbox, an `.ost`, an ANSI `.pst` and a
  4K-page file too, and the whole of the writing runs there: the copy of a Unicode `.pst`
  as it is stored, and the writing from what is read where that cannot be done.
  `write_store_until` says how far it is, an item a step, and stops when asked; the check
  after it hears the stop as well. This takes the place of `Store::copy_begin`, which could
  only copy. Made better on the way: when a copy as stored failed (a source with damage),
  the mailbox was written from what is read on the caller's thread, holding the window for
  as long as that took; that part runs on the export's thread now, so nothing of an export
  asked for this way holds the caller. A copy that was stopped is not tried the other way.
  1 test extended (the sample exported while a message is deleted, which the new file still
  has; an `.ost` exported and left byte for byte as it was; one stopped). Not done: an ANSI
  `.pst` and a 4K-page file were not exported this way in a test, there being no writer for
  them yet (they take the same path as the `.ost` in the test); a data file that was opened
  from bytes and is in no file is exported on the caller's thread.
- **2026-10-13** - M3b.20: the structure check for the other two kinds of data file. It knew Unicode files
  with 512-byte pages and said of the others that it could not look. It is now given the
  layout of the kind it is looking at (`Geom` in `crates/pst/src/check.rs`): the size of a
  page and of its trailer, where a map's bits are, how much one bit stands for, where the
  first map is, where a tree's page counts its entries, a block's trailer and its largest
  size, and whether an id is four bytes or eight. An ANSI file is held against [MS-PST]:
  the header with its one checksum and its root at 164, trailers of twelve bytes with the id
  before the checksum, maps with four bytes of padding before their bits, tree entries of
  twelve and sixteen bytes, blocks of up to 8180 bytes, subnode blocks whose entries begin
  after four bytes, and inside the nodes a table's row index with places of two bytes. A
  file with 4K pages (wVer 36) is in no specification; what is held against it was measured
  on the local `.ost` copies, structure only: pages of 4096 bytes with a trailer of 24 whose
  first sixteen are a Unicode trailer, entry counts of two bytes at 4056, the first
  allocation map at 0x22000 with a bit for 512 bytes and one section of 4072 * 8 * 512
  bytes, no page map or free maps, blocks of up to 64 KB less a trailer of 24, a checksum
  of the bytes as stored, and a block entry with the inflated size before the reference
  count. The three local files pass with nothing found, inside the nodes too (5 to 8 pages,
  99 to 313 blocks, 158 to 272 nodes; counts only, the files stay local). Made better on
  the way: a block stored deflated is inflated by the check and has to come to the size its
  entry gives, which the reader does not ask; and `inspect --check` of the CLI now says
  what it finds for every kind of data file we read. 3 tests: an ANSI file and a 4K-page
  file put together byte by byte in the test (not by our writer, and not from any real
  file), each sound and then with a block, the free count, a map and a heap's fill level
  wrong; both cut short and smashed 400 times without a panic. This is also the first
  4K-page fixture in the repository (gaps item 8), small as it is. Not done: a 4K-page
  file's heap fill levels (their steps for 64 KB blocks are not known) and the eight bytes
  after its trailers are not held against anything; a 4K-page file with a second section
  was not seen, so whether one has more map pages is not known; where an ANSI file has its
  free maps is worked out as for a Unicode file; no Outlook-written ANSI file was tried.
- **2026-10-13** - M3b.21: ANSI data files are written. The writer of the lower layer is given the kind
  of file it writes (`NdbWriter::of` in `crates/pst/src/write.rs`) and lays an ANSI file
  out as [MS-PST] has it: the header with wVer 14, its root at 164 and its one checksum;
  page trailers of twelve bytes; map pages with four bytes before their bits; tree entries
  of twelve and sixteen bytes; blocks of up to 8180 bytes with a trailer of twelve; ids of
  four bytes in the blocks that name blocks and subnodes; a table's row index with places
  of two bytes. A file past 2 GB is refused with a reason, not cut. An ANSI `.pst` that is
  open now takes changes and is saved as the kind it is (`file.save`): what is copied block
  for block has blocks of that kind's size, so it could be nothing else. `file.export`
  copies an ANSI `.pst` as it is stored, hidden items and unknown properties with it, and
  takes `"kind": "ansi"` or `"unicode"` to write the other kind from what is read. Made
  better on the way: text is never replaced by a question mark. It is written as 8-bit
  text in the code page the reader will use for that place (a message's own, its
  recipients and attachments by the message's, an embedded message by its own, folders
  and tables by Windows-1252) only when those bytes read back as the same text
  (`codepage::encode`), and is left as Unicode text when they would not; and a table now
  has one kind of text in a column in any file we write, where a value of the other kind
  was before written under a column of its own. 8 tests: a mailbox written as an ANSI file
  is the same mailbox as its Unicode twin, to the last property; text goes by the code
  page of its message (Cyrillic under 1251, a name 1251 has not, an embedded message
  under 1252); changes of every kind kept over three saves, each an ANSI file; an ANSI
  file on disk saved in place and copied; a file with more blocks than one block names
  and more subnodes than one block holds; cut short and smashed 150 times without a panic;
  the code pages encode only what comes back the same; the export by kind, waited for and
  not. Every file written in them passes the structure check of M3b.20. Not done: no
  ANSI file written by Outlook was tried, and none of ours was opened in Outlook; what a
  program that knows only 8-bit text makes of Unicode text in an ANSI file is not known;
  wVer is always 14 (a file that had 15 is saved with 14); a 4K-page file is not written
  (its layout is in no specification), so it stays read-only and its export goes through
  the model, as an `.ost` does.
- **2026-10-13** - M3b.22: Table Style prints what is asked of it, and the print window has a zoom. The
  columns of the table are chosen (`"columns"` of `file.pageSetup`, `file.print` and
  `file.printPreview`: who, from, to, subject, date, size, notes, categories, importance,
  flag, attachment, preview; `print::Field`), a heading stands over each group of the list
  as it is arranged, across every column (`"groups"`; `Row::heading` in
  `crates/layout/src/list.rs`), and `"rows": "selected"` prints the selected message's row
  alone. The print window has these beside the style, and shows the page at 25 to 400
  hundredths of its size on paper: steps, Ctrl and the wheel or a pinch over the page, and
  "Whole page"; a page larger than its room is moved about under it (`ui.printPreview` with
  `"zoom"`). Made better on the way: a heading says how many items its group has and how
  many are unread; the columns and the headings are kept with the settings, which rows is
  of the moment; the page says which message of how many it is when one is printed. Found
  and fixed: a page drawn larger than the graphics card's largest texture stopped the
  window, so a page is never drawn wider or taller than that. 2 tests: the columns, the
  headings in two arrangements, the selected row, twelve columns at once and what is
  refused; a heading across every column with its fill; the print window's test goes
  through the sizes, the refused ones and the changes to the table. Not done: one message
  is selected at a time, so "selected" is one row until there is a selection of several;
  the window's list has no columns of its own to follow, so the columns are the print's;
  a narrow column may break a word in two; Outlook's Table Style was not observed.
- **2026-10-13** - M3b.23: more kinds of picture, and pictures that move. WebP and TIFF files are read by
  the decoders the other kinds are (the `image` crate, under the same limits). An SVG drawing
  is drawn to pixels by `resvg` (Apache-2.0 OR MIT), in `crates/engine/src/svg.rs`: nothing
  in a drawing is run, since there is no script engine; no picture a drawing refers to is
  read, from a file, an address or its own text; its words are set in the fonts installed
  here, from the one list the program keeps (`Library::shared`). The reader is not ours, so a
  drawing is drawn on a thread of its own with room for the deepest nesting the reader takes,
  is given up after three seconds, and no other is begun while two that were given up still
  run. An animated GIF, PNG or WebP is played in the reading pane (`pictures::film`): every
  frame is the whole picture, shown as long as the file says and as many times; on paper and
  in a PDF it is its first frame. Made better on the way: a drawing has no pixels of its
  own, so it is kept at twice its size and is sharp on a dense screen and on paper; a
  photograph that says which way up it was taken (Exif orientation) is shown that way, in
  every kind that says so; a frame that asks for no time is given a tenth of a second
  and is not a blur; `view.animations` stops pictures moving and starts them, and is kept
  with the settings, for a reader who wants a still page. 7 tests: WebP and TIFF to the
  last pixel, cut short and bent at every byte, a TIFF that claims 60000 pixels a side;
  the photograph turned, and Exif that is not Exif; a drawing with its text, a box and no
  size, one a million wide, and pictures it refers to four ways, none read; hostile
  drawings (ten thousand million shapes by `use`, the same by XML entities, 6000 groups
  inside one another, a chain of 3000 uses, shapes that use themselves, numbers that are no
  sizes, a blur of 100000), cut short and bent; the frames and their times, a film cut at
  every byte, too many frames and too large a canvas shown still; the reel in the window
  through two playings to its last frame; the command and its setting. Not done: the web
  build does not draw SVG (it has one thread and a small stack; with the web app, M6); a
  bitmap inside a drawing is left out; a compressed drawing (`.svgz`) and one in UTF-16 are
  not read; an animated PNG or WebP was not tried, there being no writer for one here;
  a picture is decoded on the thread that draws the window, as before, so a drawing given up
  holds it for three seconds; what Outlook shows of any of these was not observed.
- **2026-10-13** - M3b.24: several pictures at the sides, and paper behind a dark picture on glass. A
  paragraph sets every picture that asks for a side at that side, one beside another, while
  room for text is left between them (eight of its own at most, sixteen standing at once);
  one for which no room is left stands in the text as before. A picture set while another
  still stands is set beside it. The text is as wide as the pictures leave it line by line:
  it widens where each one ends. This is so in the reading pane (`para` in
  `crates/ui-egui/src/body.rs`, which sets the text in a piece for each height at which a
  picture ends) and on the printed page (`Flow::asides` in `crates/layout/src/flow.rs`).
  In the dark reading pane a picture that is seen through in places and is dark where it is
  seen (a logo drawn for a white page) has light paper laid behind it, so that it can be
  read. Made better on the way: the paper is laid only where it helps. A light picture on
  glass was drawn for a dark page and is left as it is; a picture that covers its room needs
  none; and where the message chose what is behind (a filled cell or block) that choice is
  kept. A photograph with a few soft pixels at its corners is not taken for a drawing on
  glass. 3 tests: in the window, heights with one, two and three pictures at the sides, two
  at one side, pictures of different heights, one set while another stands, one with no room,
  inside a border, and sixty to a paragraph at widths from nothing to a million; how light a
  picture is, and the paper drawn in each of the cases above and in no other; on paper, where
  every line starts and ends beside two pictures of different heights, two at one side from
  the left and from the right, one set while another stands, one with no room, and forty to
  a paragraph in sixty paragraphs and in cells. Looked at in the window in both themes
  (`plan/shots/asides.jsonl`). Not done: on paper a table or a bordered block still starts
  under a picture at the side, where in the window it stands beside it; the paper is not
  laid behind a dark picture in a cell the message filled dark; what Outlook does with
  several pictures at the sides, or with see-through pictures in its dark mode, was not
  observed.
- **2026-10-13** - M3b.25: boxes inside one another, tables in blocks, a colour behind the body. A
  block with a border, a fill or room of its own that is written inside another is a box
  inside that box (`Frame::within` in `crates/formats/src/rich.rs`), eight deep at most
  (`MAX_BOXES`); before, the inner one took the outer one's place. A table written inside a
  filled or bordered block stands inside it (`Table::inside`), where before the block ended
  above the table and began again under it. A colour on `<body>` or `<html>` is the colour
  behind the whole message (`Doc::fill`, CSS 2.1 section 14.2): in the reading pane it fills
  the pane from edge to edge and text with no colour of its own is drawn to be read on it; on
  paper it lies behind the body on every page the body is on, under every other fill. Both
  drawers walk the boxes level by level with `rich::boxed_run`: `framed` in
  `crates/ui-egui/src/body.rs` and in `crates/layout/src/flow.rs`, where `Flow::lift` keeps
  each fill over the one it stands in. Made better on the way: a box keeps only its own fill,
  so a box with none inside a filled one no longer paints the outer colour a second time;
  white is kept as a colour on a coloured body as it is in a coloured block; what a layout
  table of one column held keeps the box the table stood in; and what a box holds never
  stands outside it, however much room boxes inside one another ask for (found by the
  hostile test on small paper: text was set past the right margin). 4 tests: in the reader,
  three levels of boxes and which paragraph stands in which, the colour nearest behind, the
  body colour from `<body>` and `<html>` and not from inside a table, five thousand blocks
  inside one another, and a chain of ten thousand made by hand; in the window, the heights of
  one box, two, and three paragraphs in two, a table in a filled block, three hundred deep at
  widths from nothing to a million, and the ink of text on a dark body in both themes; on
  paper, the order and the place of four fills inside one another, what text stands in
  which, the body colour on every page, and forty boxes deep with tables in cells on small
  paper. Looked at in the window in both themes (`plan/shots/boxes.jsonl`,
  `plan/shots/boxes-body.jsonl`). Not done: borders on text within a line and rounded
  corners (M3b.26); a word wider than a very narrow column still stands out of its cell; a
  body colour is not drawn behind the message header; what Outlook does with a body colour
  in its dark mode was not observed.
- **2026-10-13** - M3b.26: borders on text within a line; rounded corners. A border on a `<span>`,
  `<code>` or any other element within a line is kept on its text (`Style::edge` in
  `crates/formats/src/rich.rs`, CSS 2.1 section 8.5) where all four sides are the same line
  (`rich::one_edge`); a line on one side only is not drawn as a box. In the window each
  stretch of a line with the same border has its box (`body::outlines` in
  `crates/ui-egui/src/body.rs`), and on paper each piece has a line over and under it
  (`Look::edge` in `crates/layout/src/flow.rs`); a box is open at the side where its text
  goes on in the next line, and a border that names no colour has the colour of its text.
  `border-radius` rounds a block and a table cell (`Frame::radius`, `Cell::radius`, CSS
  Backgrounds and Borders 3 section 5.1): the first length is taken for all four corners,
  64 pixels at most (`MAX_RADIUS`) and never more than half the shorter side, so a very
  large radius makes a pill. On paper a rounded fill is a `Fill` with a `radius`, and a
  rounded border is a `Fill` with a `ring`: a band that follows the corners, in place of
  four straight lines. The rasteriser fills it between two outlines by the even-odd rule
  (`rounded` in `crates/render/src/lib.rs`) and the PDF writer writes four curves and `f*`
  (`Writer::fill_round` in `crates/pdf/src/lib.rs`, PDF 32000-1 section 8.5). Made better on
  the way: a fill with no border is rounded too; a cell that has only the table's own lines
  is rounded with them; and what cannot be rounded honestly keeps square corners with all
  of its lines, where sides differ, where the line is dashed, dotted or double, and where a
  box on paper goes on over a page. 5 tests: in the reader, which text has which border,
  the radius of blocks and cells, junk lengths and three thousand spans inside one another;
  in the window, how far a corner is rounded, the box round two letters where they are
  drawn, boxes open where text goes on over lines, heights unchanged by borders and
  corners, and any radius at widths from nothing to a million; on paper, the four lines
  round a word and where they stand, the colour of the text for a border with none, two
  sides only for a border over many lines, the fill and the band of a rounded box and their
  order, square corners for sides that differ and for boxes over pages, and any radius on
  three sizes of paper; in the rasteriser, the pixels in, on and outside the corners of a
  fill and a band at three scales, and a hundred junk pairs; in the PDF writer, the curves
  as written and junk numbers. Looked at in the window in both themes and on a printed
  page (`plan/shots/round.jsonl`, `plan/shots/round-print.jsonl`). Not done: text drawn in
  another order than it is stored (right-to-left) has no border round it in the window;
  each corner having its own radius, and radii that are a share of the box; padding of an
  element within a line; what Outlook, which draws HTML with Word, shows of either was not
  observed.
- **2026-10-13** - M3b.27: RTF highlight colours and list overrides. A highlight (`\highlightN`) is a
  place in the colour table (RTF 1.9.1, "Highlighting") and is shown as one of sixteen
  marker colours (`MARKERS` and `highlighter` in `crates/formats/src/rtf.rs`; ECMA-376 Part
  1, 17.18.40 names them): itself when it is one of them, a grey by how light it is when it
  has nearly no colour, else the pure colour of its hue, bright or dark. The one case seen
  in Outlook, a pale yellow shown as bright yellow in our own receipt message, is now the
  same; a colour behind text that is not a highlight (`\cb`, `\chcbpat`) is kept as
  written. A number that list paragraphs name may change levels of its list
  (`\lfolevel`; RTF 1.9.1, "List Override Table"): start one somewhere else
  (`\listoverridestartat`) or write it another way (`\listoverrideformat` with a level of
  its own). `Builder::override_word` reads them and `counted` follows them. Made better on
  the way: one list now counts on through every number that names it, where before each
  number counted from the start by itself, and a number that starts a level somewhere
  begins there again each time it takes over from another. 1 test: seven highlights from
  one colour table and what each is shown as, colours behind text left alone, places that
  are not in the table; thirteen paragraphs over four numbers of one list, with a start
  changed, a level written anew with and without a start of its own, and a level below
  counted under it; and fifteen hundred overrides of twelve levels each with numbers at the
  edge and words outside any level. Not done: what Outlook shows for highlights other than
  the pale yellow was not observed, so the choice by hue is ours; `\levelrestart`,
  `\levellegal` and levels that do not restart under the one above.
- **2026-10-13** - M3b.28: metafiles. A bitmap in a Windows or an enhanced metafile is laid by the way back
  from the page to its pixels (`Lay` and `placed` in `crates/formats/src/canvas.rs`), so a
  part of it, one mirrored (a width or a height below nothing) and one in a world that is
  turned are all the same work; `metafile::blit` does it for META_STRETCHDIB,
  META_DIBSTRETCHBLT, META_DIBBITBLT, META_PATBLT ([MS-WMF] 2.3.1, 2.3.3.12) and
  EMR_STRETCHDIBITS, EMR_BITBLT, EMR_STRETCHBLT ([MS-EMF] 2.3.1). Pixels packed in runs
  (BI_RLE8, BI_RLE4; [MS-WMF] 2.1.1.7) are unpacked (`unpacked` in
  `crates/formats/src/metafile.rs`). A copy is mixed with the brush and with what is there
  by the table in its raster operation (`mixed`; [MS-WMF] 2.1.1.31). Brushes are a colour,
  one of six hatchings (2.1.1.12) or a bitmap laid side by side (META_DIBCREATEPATTERNBRUSH,
  META_CREATEPATTERNBRUSH of one bit, EMR_CREATEDIBPATTERNBRUSHPT, EMR_CREATEMONOBRUSH).
  Regions of many boxes are filled, painted, framed, turned over and cut to (WMF 2.2.1.5 and
  2.3.3; EMF 2.2.24, records 71 to 75), and a box is kept out of (META_EXCLUDECLIPRECT,
  EMR_EXCLUDECLIPRECT): what is cut to is a share of each pixel (`Canvas::mask`,
  `Canvas::cut`). Made better on the way, beyond the row: all 256 ways of mixing are done,
  not the named few; a clip path of any shape is cut to, and a box in a turned world;
  regions join by either, one-not-both and without as well as both; a hatching's lines stay
  even at any size; and a metafile that holds one bitmap is taken as that bitmap only when
  it takes all of it, unmirrored (before, a part was shown as the whole). 6 tests and 5
  changed: runs of both kinds with steps, ends and runs past the side; five ways of mixing a
  bitmap and five with none, by every record; six hatchings, see-through and not, and three
  kinds of pattern; regions painted, filled, framed, turned over and cut to in both
  formats, joined four ways, with a state put back and a triangle cut to; bitmaps copied
  and stretched in an enhanced drawing; and every new record cut to twelve lengths of four
  kinds of junk, with boxes at the ends of their numbers. Not done: copies with a mask, see-
  through copies and ones onto a leaning box (EMR_MASKBLT, EMR_ALPHABLEND,
  EMR_TRANSPARENTBLT, EMR_PLGBLT), pens that mix or are patterned, colours that are numbers
  in a palette, a one-bit pattern in the text's colours, and a clip that is moved.
- **2026-10-13** - M3b.29: a save that does not rewrite the whole data file. `file.save` now writes what
  changed into the file where it is (`crates/pst/src/write_onto.rs`, `save_onto.rs`): new
  blocks and pages go into the free end of the file's last section and past its end, a block
  that comes out as it was is named once more and not written again, and nothing that holds
  mail is written over ([MS-PST] 2.2.2.7 for the maps, 2.2.2.6 for the header). The writes are journalled themselves: before the first of them, the
  bytes that will be written over (the header, the last section, a free map) go into an undo
  file of our own beside the data file (`name.pst.openmailclient-undo`, `undo.rs`) and are
  forced to disk; the header that names the new state is written only when all it names is
  on disk; the file is then held against the structure check (blocks from before the save
  are not read again, every block that names others is), read back and compared, and taking
  the undo file away is what ends the save. Anything that goes wrong before that puts the
  file back byte for byte, with its time of writing, and the save is done the old way, a
  whole new file swapped in. A run that ends in the middle leaves the undo file: the next
  open reads the data file as it was, through the undo file, writing nothing, and the next
  save puts it back first; the journal of unsaved changes still fits it. A file is written
  into only when it is plainly sound and nobody else can write it (on Windows it is opened
  so that the open fails otherwise); the first save that keeps the file as it was, a 4K-page
  file and an `.ost` are as before. Made better on the way, beyond the row: unused space
  earlier saves left is given back by itself (past 4 MB or a quarter of the file the save
  writes the file whole); a large attachment is not written again for a flag on its
  message; a save that is not waited for goes into the file too; `file.saveInPlace` turns
  it off and is kept with the settings; `file.save` says how many saves went into the file
  (`inPlace`). 11 tests: every kind of change over four saves, each held against the whole
  structure check and read back; a 1.5 MB attachment and a flag (the file grows by less
  than 600 KB and what was before the last section is byte for byte the same); an ANSI
  file; an error at each of five stages (the file is as it was, byte for byte, and the save
  is then done whole); a stop at each of six stages, a half-written header among them (the
  file reads as it was, a save puts it back byte for byte, and the changes come back from
  the journal and are saved); undo files cut at every 977th byte, with a byte changed, with
  sums that fit round junk, from another file and for a file changed since; the space given
  back; a save on a thread of its own, stopped and not; a file another program writes.
  Not done: no file written this way has been opened in Outlook (that waits for the owner's
  say, as before), and no Outlook-written `.pst` with 512-byte pages was at hand to save
  into: the only local files are 4K-page ones, which stay read-only. With this the do-later
  batch is done and M3b is closed.
- **2026-10-14** - M3c.1: files we write open in Outlook Classic. With the owner's say, copies of the
  files our writer makes were added to the Outlook profile as extra data files through its
  automation interface, walked and taken out again (the profile had the same three stores
  before and after), and held against Outlook's Inbox Repair Tool, scanning only. Before
  this arc Outlook opened none of them: every file we had written since M3 was refused.
  What the repair tool named and Outlook needed, beyond the nodes [MS-PST] 2.7.1 lists: the
  receive folder table (0x62B, one row: mail of any class goes to the Inbox), the outgoing
  queue (0x64C), two more tables new folders are made from (0x6D7, 0x6F8, whose row index
  is four bytes wide even in an ANSI file), the search folders' update queue (0xEC1) and one
  search folder with its parts (0x2223, 0x2226, 0x2227, 0x2230, listed in 0x201), a store
  property saying there is no password, and columns the folder tables were missing.
  Recipient rows are numbered from 1, not 0. PidTagAttachSize is now exactly the sizes of all
  the attachment's values as stored, and PidTagMessageSize exactly the bytes of the message
  node and all its subnodes, in the message and in its folder's row; the storage of an
  attached object (method 6) is kept in a subnode of a kind of its own (0x09). With those,
  the repair tool says nothing of a whole Unicode file, an ANSI one or one saved into in
  place (read, flagged, given a category, a message deleted) but that it makes node 0xC01
  and deletes an SDO, which Outlook makes itself; Outlook opened them all and showed every
  folder with the right count and unread count, found its default folders, and gave every
  one of the 17 messages the subject, sender, recipients, read and flag state, categories,
  importance, size, received time, body and attachments we wrote (an attached message as
  a message, an attached object as an object). Files Outlook had opened and grown were read
  back here: every item, and the structure check found nothing. 4 tests pin what was
  learned. Not done, each with a row in Do later: node 0xC01 is left to Outlook; a message
  changed by an in-place save keeps the size it had; files written by earlier versions need
  writing whole once (export) before Outlook takes them; no Outlook-written `.pst` with
  512-byte pages has been saved into (Outlook could not be made to create one); an object
  that is not a real storage is listed by Outlook as no attachment.
- **2026-10-14** - M3c.2: data files with 4K pages written. The layout is in no specification; it was
  measured on the few small Outlook-written `.ost` files kept locally (layout only) and is
  now written: pages of 4096 bytes with trailers of 24, blocks of up to 64 KB on 512-byte
  slots with a trailer that carries the inflated size, allocation maps heading sections of
  16 MB, an empty density list carrying the header's next page id, B-tree pages with
  16-bit counts, wVer 36 and wVerClient 19, and blocks not encoded. `file.export` takes
  `"kind": "4k"`, an open 4K-page `.pst` takes changes and `file.save` writes it whole as
  the same kind (into the file where it is is left for later: what heads a later section
  is not known). Outlook's Inbox Repair Tool, scanning only, first found 858 problems in
  such a file; three came out of measurement: the blocks must not be encoded, wVerClient
  must be 19, and every contents table, the template 0x60E too, has two subnodes more,
  0x521 and 0x541, heaps with the client signature 0xA5 that [MS-PST] calls reserved,
  written empty. With those it says only that it makes an HMP, as of our Unicode files.
  With the owner's say Outlook Classic opened a 4K file of ours and a Unicode one written
  from the same mailbox side by side, as extra data files taken out again (the profile had
  three stores before and after): the same folders, counts and unread counts, and every
  one of the 17 messages with the same subject, sender, recipients, flags, categories,
  size, body and attachments (only received times moved: the sample's dates follow the
  clock). Outlook added Calendar, Contacts and Tasks to it, as to any store it first
  opens; read back here, every item loads and our structure check finds nothing. 4 tests
  in `crates/pst/src/tests_4k.rs`. Not done, each with a row in Do later: what the 0xA5
  heaps hold for a table with rows; heaps of more than one block in a 4K file (their HID
  layout is our reading, no Outlook file has one); saving into a 4K file where it is;
  writing compressed 4K blocks.
- **2026-10-14** - M3c.3: unsaved changes saved on their own. `file.autoSave` (`{"minutes": 0 to 240}`,
  5 at first, 0 for only when asked, kept with the settings) has unsaved changes to a data
  file saved once they have waited that long, at a moment when nothing has been done for five
  seconds; in a mailbox in use all along, after three times as long, at the first pause of a
  second. The save is `file.save` with `"wait": false`: on a thread of its own, written into
  the file where it is with an undo file beside it, checked and read back, with progress and
  Stop; the status line says it was saved on its own. Better than a timer that writes
  whatever is there: a data file is never saved for the first time on its own, so opening a
  file and changing it does not write it unasked; it must have been saved here once, or have
  the file as it was kept beside it from a first save of an earlier run. A save that fails
  says so and is tried again after as long once more, not at every frame. The window's host
  gives the clock (`Session::autosave`); the engine has none, so the web app is not held up.
  1 test, and the settings test takes the new setting.
- **2026-10-14** - M3c.4: the size of a message changed by a save. A changed message kept the
  PidTagMessageSize it had: a flag was right, but a file added or taken away left the size
  wrong (Outlook accepts a size within a tenth; a large attachment is far outside that). Now
  the size is worked out as the message is written, as it is for a new one: the bytes of its
  property list and of every subnode, those written again and those kept where they are
  stored. Kept blocks are not read for it: a block's size is in the file's index, and a data
  tree's total in its first block (lcbTotal, [MS-PST] 2.2.2.8.3.2.1), so a flag on a message
  with a large attachment still reads a few blocks. The folder's row takes the same size.
  Unicode and ANSI, saved into in place and whole, twice over: 1 test, which fails on the
  code before.
- **2026-10-14** - M3c.5: files from before M3c.1 written whole. The data files this program wrote
  before M3c.1 lack nodes Outlook asks for, columns it wants in every table, and sizes as it
  counts them; Outlook refused them, and a save into the file or a copy as stored kept them
  as they were. Now a file that lacks those nodes, and all of whose contents are in what is
  read of it (no hidden items, no folder that is not listed but those every file has), is
  written whole by today's writer on its next save or copy, as the kind it is: better than
  asking for an export, it happens by itself, and every folder and item keeps the id it had
  (`write_store_keeping`), so what is open stays open and the save is held against the
  structure check and read back as before. Where that cannot be done (a hidden item, an id
  that cannot be kept, an item that cannot be read) the file is saved as it is stored, as
  before; the next save after a rewrite goes into the file where it is. Tried on files the
  build before M3c.1 wrote: Outlook's repair tool found 314 and 306 things wrong with them,
  and after one save nothing more than in any file of ours; Outlook opened both, Unicode
  and ANSI, with every folder and message, and they read back clean after it. 2 tests.
- **2026-10-14** - M3c.6: hidden items and folder properties carried by what is written from the model.
  An `.ost` was exported through the model and lost each folder's hidden items (views, rules,
  forms) and its own properties; copying it node for node was tried and Outlook refused the file
  it made (its repair tool found the store's and the folders' entries of an offline store, 232
  findings). Now the `Store` trait gives a folder's hidden items and its own properties
  (`hidden`, `folder_props`, `record_key`), and the writer from the model carries them: a hidden
  item goes out as a message of the associated kind with its row in the folder's table of
  associated items, a folder's own properties go into its property list and, where the table has
  columns for them, into its row in its parent's hierarchy table (the repair tool asks that), and
  an entry id of the source naming one of its folders is made to name that folder in the new
  file, by the new file's id. Better than a copy: the export is a `.pst` Outlook takes, of the
  kind asked for. The same carries hidden items when a file that lacks what Outlook asks for is
  written anew (M3c.5), which now keeps the store's own id too; only a row naming a hidden item
  that is not there still has such a file copied as stored. Tried on a copy of an `.ost` Outlook
  wrote: exported as Unicode, ANSI and 4K-page files, each opened in Outlook with all 36 folders,
  both messages, every default folder and all 32 hidden items, the repair tool finding no more
  than in any file of ours, and the file read back clean after Outlook. 4 tests.
- **2026-10-14** - M3c.7: compressed blocks in 4K-page files written; M3c closed.
  A 4K-page file was written with every block stored as it is, though Outlook stores its larger
  data blocks deflated. Measured on a local `.ost` Outlook wrote (its layout only): of 263 blocks,
  68 data blocks were deflated and no internal one; none of 476 bytes or less was, the smallest
  that was came to 492, and larger ones that would not shrink were stored as they are. Now a data
  block whose bytes and trailer pass one 512-byte slot is deflated (a zlib stream, RFC 1950/1951)
  when that makes it smaller; the trailer and the block tree say what it inflates to, and a block
  already written is reused for the same bytes, deflated or not. Better than stored as it is: a
  file of long bodies and text attachments takes a fraction of the room. Exported from the sample
  and from a copy of an `.ost`, the files' structure check and Outlook's repair tool find nothing
  more than in any file of ours, Outlook opens them with all folders, messages and the 32 hidden
  items of the `.ost`, and they read back clean after Outlook. 1 test. This closes M3c: what is
  left of it is in Do later, in M4, M5 and with M1's checks.
- **2026-10-14** - M4.1: IMAP, POP3 and SMTP clients with an in-process fake server.
  A new crate at L3, `crates/net`, holds clients for the three protocols over any stream a
  `Connector` gives, so the tests run on an in-memory pipe against a fake server of our own and
  touch no network. SMTP (RFC 5321, 6409) says EHLO, makes the connection secure with STARTTLS
  (RFC 3207), signs in with AUTH PLAIN or LOGIN (RFC 4954), keeps to the server's SIZE, asks for
  8BITMIME and SMTPUTF8 when the message needs them, dot-stuffs, and reports each recipient
  refused. POP3 (RFC 1939, 2449, 5034) lists by UIDL, retrieves and deletes. IMAP (RFC 9051,
  3501) lists folders with their special uses (RFC 6154) and modified UTF-7 names, selects,
  lists messages from a UID on, fetches with BODY.PEEK so nothing is marked read, sets flags and
  appends, using APPENDUID (RFC 4315) and LITERAL+ (RFC 7888) when offered. Better than many
  clients: a password is never sent over a connection that is not secure, whatever the server
  offers, PREAUTH over a connection meant to be upgraded is refused, bytes a server sent before
  STARTTLS took effect are refused (the injection of RFC 3207 6), addresses that would end a
  command early are refused, a password never prints, and every count and length a server gives
  is capped before anything is allocated. 16 tests, among them hostile servers that send
  broken, endless and noisy replies.
- **2026-10-14** - M4.2: TCP and TLS, and passwords in the system's credential store.
  `TcpConnector` opens a connection to each address of a server's name in turn and puts TLS 1.2
  or 1.3 over it (rustls with the ring provider), at once or after STARTTLS. The certificate is
  checked against the roots this system trusts and against the name the account gives, and the
  handshake is made before the first command, so a certificate that is not accepted is said as
  that rather than as a broken reply, and nothing is sent to such a server. A connector can also
  trust only certain certificates, for a server whose certificate is its own. Passwords are kept
  by account in the system's credential store through `Vault` (`SystemVault`; `MemoryVault` for
  tests and where there is none): Windows Credential Manager, the macOS keychain, the Secret
  Service on Linux and the BSDs. Neither is on the web build. Tested against a TLS server on
  this machine's loopback with certificates made for the test: TLS from the first byte, STARTTLS,
  a certificate signed by nobody trusted and one for another name (both refused before anything
  is said), and servers that are not there. 5 tests.
- **2026-10-14** - M4.3: accounts in the engine.
  An account (`crates/engine/src/accounts.rs`) is an address, a name to send under, a kind (IMAP
  or POP3), an incoming and an outgoing server with their ports and security, a sign-in name,
  and whether mail taken down by POP3 stays on the server. `account.add`, `account.list` (File >
  Account Settings), `account.edit`, `account.remove`, `account.setPassword` and `account.test`
  are commands; what is not given follows from what is (the address signs in, the incoming host
  sends over TLS, a port follows its security), and what makes no sense is refused. Accounts
  are kept in the settings file; their passwords are kept in the system's credential store under
  the account's id, given to no command's result, status or settings, and go when the account
  goes. Better than Outlook's test, which sends a message: `account.test` signs in to both
  servers and sends nothing, says which server failed and why, and runs on a thread of its own
  (`jobs`, which sending and receiving will use) with Stop. The window and the CLI reach servers
  over TLS and keep passwords in the system's store (`Session::use_this_system`); tests use the
  fake server. 6 tests.
- **2026-10-14** - M4.4: sending.
  `compose.send` files the message in the Outbox and, when there is an account and a way to
  reach servers, sends the Outbox; `mail.sendAll` (Send / Receive > Send All) sends it on its
  own. Each message goes from the account its sender names, or the first; the messages of one
  account go over one connection, on a thread of its own from the window (`"wait": false`),
  with progress and Stop. On the wire a message has a Message-ID of its own and no Bcc line, so
  hidden recipients stay hidden, while the envelope names every recipient; Sent Items keeps it
  as it went, every recipient with it. A message is put in Sent Items before it leaves the
  Outbox, so one that went is never lost; one the server did not take stays in the Outbox with
  the reason, a recipient the server refused is named, and a wrong password sends nothing and is
  never said. A send asked for while one runs follows it, and nothing goes twice; a data file is
  not closed from under a send. Better than Outlook: an answer goes from the account the message
  it answers reached, and carries In-Reply-To and References so conversations stay together in
  every client; the From of a message being written is chosen with `compose.setField`
  (`"from"`). 5 tests.
- **2026-10-14** - M4.5: receiving.
  `account.receive` takes new mail down from one account or every account, and
  `mail.sendReceive` (Send / Receive > Send/Receive All Folders, F9) sends the Outbox and then
  receives; from the window both run on threads of their own. New mail goes into the Inbox of
  the data file new mail is kept in. IMAP: the server's Inbox is read from the last message
  taken down on (a new UIDVALIDITY takes everything again), each message with BODY.PEEK so the
  server does not mark it read, and one the server has as read comes as read. POP3: each
  message's unique id is remembered once it is in the Inbox; with "leave on server" off, a
  message is taken off the server on a later receive, only once the data file it went into is
  a file with every change saved, so a message is never removed from a server before it is
  kept. What was taken down is kept with the account in the settings file, and moved on only
  past messages that are in the Inbox. At most 300 messages or 256 MB come in one receive and
  the rest wait, said in the status bar; a message that cannot be read as mail is kept whole as
  an attachment. 6 tests against the fake server.
- **2026-10-14** - M4.6: Account Settings in the window.
  File > Account Settings… (and a button on the Send / Receive tab) opens a window that lists
  the accounts and shows one: your name, the address, IMAP or POP3, the incoming and outgoing
  servers each with their security and port (left empty, the usual port for the security
  chosen; the outgoing server left empty is the incoming one), the user name, the password, and
  whether the outgoing server is signed in to and POP3 mail left on the server. Save adds or
  changes the account through `account.add` and `account.edit`, Test Account Settings… saves
  and then signs in to both servers on a thread of its own and shows what each said, and
  Remove asks once more first. Everything the window does is an `account.*` command, so the
  command line and an agent do the same. The password field (`LineEdit::password`) draws a dot
  for each character, copies and cuts nothing out, keeps no undo, selects all on a double click,
  tells the input method it is a password and tells screen readers nothing of what it holds;
  the window hands what was typed to the system's credential store and writes over its own copy
  at once. The ribbon's Send/Receive All Folders and Send All now run, on threads of their own.
  2 tests.
- **2026-10-14** - M4.7: each account in a data file of its own; Account Settings laid out anew.
  The live check against a real IMAP and SMTP server passed: both servers signed in to from
  Account Settings and the Inbox taken down. It showed that mail received went into whichever
  data file was open, mixed with what was there. Now, as Outlook keeps one per account, each
  account has a data file of its own (`account::account_store`): in the app it is made beside
  the settings (`Accounts\<address>.pst`) when the account is added, named in the settings
  (`dataFile`) and opened again on the next run. Mail received goes into its Inbox, a copy of
  what the account sent into its Sent Items, and the file is saved on a thread of its own once
  mail is in it, so POP3 mail can leave the server on the next receive. Removing an account
  keeps its file. A host that keeps no account folder (the command line, tests) works as before.
  Account Settings is laid out anew: the accounts as cards with their kind and address, the
  form in parts with headings whose fields line up, a tick or a cross for what each server
  said, where the account's mail is kept, and Save as the main button. 2 tests.
- **2026-10-14** - M4.7: messages laid out as Outlook lays them out.
  A newsletter received in the live check was drawn unlike Outlook draws it. Compared side by
  side on a synthetic copy of its layout: a table in a centred cell now stands in the middle
  (`fmt.align` before the table's own); a cell's padding is kept for each side (`Cell::padding`
  is `[u16; 4]`, from `padding` and `padding-top` and the rest), and a table whose cells are
  padded is drawn and not taken apart as layout; pale fills of cells, rows and tables are kept
  (on the dark pane a pale fill is still the pane); a table has its own fill and rounded
  corners (`Table::fill` and `Table::radius`), on screen and on paper; a table right under
  another touches it, as CSS gives tables no margin; a pale border is drawn in its own hue,
  made only as much darker as it needs to be seen, where before it fell back to the colour of
  text and showed black; and a picture not downloaded that gives only its width is a box of
  that width, as Outlook draws one, rather than words in brackets. 3 tests.
- **2026-10-15** - M4.10: pictures on a server downloaded when asked for.
  A message's pictures on a web server are still never fetched by themselves. The bar above a
  message that has some now says so and downloads them when clicked, and a right-click on a
  picture not shown offers Download Pictures, as Outlook does (`mail.downloadPictures`, on a
  thread of its own). Better than Outlook: only HTTPS is used, so nobody on the way sees or
  changes what comes; no server on this machine or the local network is reached, by its address
  or by a name that leads there (checked again where the connection is made, `http::local_ip`);
  the request carries no cookie, referrer or language; only a picture the message names can be
  asked for; each picture is capped at 8 MB and all of them at 64 MB, kept in memory only and
  never in the data file; and what is not a picture is not kept. A small HTTP/1.1 client
  (`crates/net/src/http.rs`) reads lengths, chunks and redirects (five at most, only to HTTPS),
  with the head, body and chunks capped, tested against the fake server's web pages. 9 tests.
- **2026-10-15** - M4.11: attachments opened or saved on a double-click.
  A double-click on an attachment, or Enter on the one picked, asks whether to open it, save it
  or do neither, as Outlook does; a right-click offers Open, Save As and Save All Attachments.
  Better than Outlook: the dialog says first what the reader should know of the file (a name
  that reads as a .pdf and is a page, a document that carries macros); a program, a script or a
  file whose name hides its kind is never opened from here, only saved; what is opened is a
  copy in a folder of the program's own, read-only so a change to it is not taken for a change
  to the attachment, and marked as come from the internet so the program it opens in is careful
  with it (Office opens it in Protected View); the copies are cleared away when the program
  ends, and those a run left are cleared a day on. An attached message opens here, as a
  message. Cancel is the answer a key press gives. 2 tests.
- **2026-10-15** - M4.8: IMAP folders taken down as the server has them; the Sent copy put on the server.
  An IMAP account with a data file of its own has every folder of the server there, not only
  the Inbox: Sent, Drafts, Trash, Junk and Archive go to Sent Items, Drafts, Deleted Items, Junk
  Email and Archive, as the server marks them (RFC 6154) or names them (Sent Messages, Spam,
  INBOX.Trash and the like), and any other folder is made with the same name and place. Each
  folder is followed by its own id mark, so nothing comes twice and a folder numbered anew is
  taken down again; marks of folders the server no longer has are forgotten. What is sent is
  put in the server's Sent folder, read, with its Bcc as the sender's copy keeps it, and a Sent
  folder is made when the server has none ("sentOnServer", a box in Account Settings).
  Better than Outlook: the copies put on the server are known by their Message-ID and not taken
  down beside the copy Sent Items keeps; folders that are views of mail in other folders (All
  Mail, Flagged, Important) are passed over, so nothing is there twice; a server folder named
  as the Outbox is never filled, so nothing on a server is sent by being taken down; one budget
  is shared by every folder, so a first receive of a large mailbox comes in parts, the Inbox
  first. A send or receive that ended while a data file was being saved no longer has its mail
  turned away from that file: what a job found waits for the save. Folders are made at the top
  of a `.pst` beside the Inbox (`Store::create_top_folder`), journalled as any change. 4 tests.
- **2026-10-15** - M4.8: IMAP kept in step both ways.
  Each message of an IMAP account's own data file knows its place on the server: the folder,
  its numbering (UIDVALIDITY), its id and the read and flagged state both sides last agreed
  on, kept as a property of this program's own on the message (`imap_sync::Place`). At each
  Send/Receive what was done here goes first: read and flagged (STORE), a move to another
  folder (MOVE, RFC 6851, or COPY, marked deleted and expunged, only that message where the
  server has UIDPLUS), a folder made here that mail was moved into (CREATE, under the parent's
  name), a message deleted for good (from Deleted Items or by emptying a folder, then deleted
  and expunged there) and a message put into the account from another data file (APPEND, its
  new id known from APPENDUID). Then what was done there comes here: read and flagged changes,
  and a message the server no longer has goes from here. A folder the server has is made here
  even while it is empty. A message deleted for good and taken back with Undo before the next
  Send/Receive stays on the server; taken back after, it goes up again.
  Better than Outlook: a message goes from here only when the server's whole list of a folder,
  as many as it says the folder holds, lacks it, so a short answer never empties a folder; where
  both sides changed the same flag, the change made here wins and the other side's other flag
  still comes; nothing is taken down twice, not the messages this receive put there nor those
  moved; a folder numbered anew finds its messages again by their Message-ID instead of taking
  them down beside themselves; the first time, mail taken down before is found on the server
  the same way and none of it is put there again; a change the server refuses is passed over and
  tried at the next Send/Receive. The copy Sent Items keeps of what is sent is found again on the
  server's Sent folder and given its place. The status line counts the changes kept in step, and
  `account.receive` returns them ("inStep").
  Fixed on the way: a folder written while empty had a contents table without the sender's and
  the flag's columns, and a save into the file in place kept that table, so a flag set on a
  message was gone from the list after a save, though the message kept it (`crates/pst/src/edit.rs`);
  such a table is given the columns, filled from each message, when its folder next changes.
  8 tests.
- **2026-10-15** - M4.9 (first part): a server's own certificate trusted on the owner's word.
  A server whose certificate this system does not trust is refused as before, and the
  certificate it showed is kept for this run: Account Settings shows it whole after a test or a
  Send/Receive, with whom it was made for, who signed it (or that the server made it itself),
  the server names in it, when it holds and its SHA-256 fingerprint to compare with what the
  server's keeper gives (read from the certificate as RFC 5280 lays it out, `cert::about`, with
  hostile-input tests). One click trusts it for that server of that account only
  (`account.trustCertificate`, kept in the settings as the fingerprint), and the account is
  tested again at once; "Stop trusting" puts the server back under the system's trust.
  Better than Outlook: Outlook trusts such a certificate for the session or puts it in the
  system's store, where every program trusts it for every server; here it is pinned to one
  server of one account and kept across runs, and nothing else trusts it. Only a certificate a
  server actually showed in this run can be trusted, so no fingerprint is taken on a command's
  word. A trusted server that shows another certificate is refused with a warning that someone
  may be between you and the server. A revoked certificate cannot be trusted. The server must
  still prove it holds the certificate's key; a trusted certificate is dropped when the server's
  name changes.
  Buttons in the window's own dialogs (Account Settings, the print window, the compose bar)
  have room round their words, an outline, rounded corners, an accent line under the pointer
  and a fill when pressed, so they read as buttons and are easy to hit.
  6 tests.
- **2026-10-15** - M4.9: new mail told of; Send/Receive on a timer.
  Each receive notes the unread messages it put in an Inbox, and the window tells of them as
  the owner chose (`mail.newMailAlerts`): an alert at the bottom right of the screen with the
  sender, subject, start of the text and the account, standing above other windows without
  taking the keyboard; a short chime made in code, two soft bell notes of our own, so no sound
  file is shipped; and the taskbar button asking for attention while the window is not in
  front. A click on the alert opens the message in its folder (`mail.openArrived`), × puts it
  away (`mail.dismissArrived`), and the pointer over it holds it. The status bar keeps a count
  of the new messages not yet opened, and a click there opens the newest.
  Send/Receive comes on its own every so often (`mail.sendReceiveEvery`, 30 minutes at first,
  from the ribbon's Send/Receive Groups menu with the three alerts), the first a few seconds
  after the window opens, never while one runs or a data file is being saved.
  Better than Outlook: mail already on a server when its account is first received from, or a
  whole Inbox the server numbered anew, is not told of as hundreds of new messages; mail read
  elsewhere before it came is not told of; several that come together are one alert that says
  how many more, and one sound; the timer counts from the end of the last Send/Receive, so
  pressing F9 puts the next one off instead of running two close together; the alert shows a
  line that shortens while it stands. What the alert and the commands show of a message is a
  stranger's text: it is marked as such and never acted on.
  6 tests.
- **2026-10-15** - M4.9: Show Progress.
  Everything that runs on a thread of its own (sending, receiving, testing an account, saving,
  exporting) is listed in the Send/Receive Progress window (Send / Receive, Download, Show
  Progress, or a click on the status bar's working words), each with its name and a bar of how
  far it is, a moving piece while that is not yet known, and Cancel Task; Cancel All stops them
  all. Stopping comes at the next step: what was done by then is kept, a data file being saved
  stays as it was with its changes unsaved, and a file being exported to is not written. The
  window says when Send/Receive comes on its own next, and lists what came of the last pieces
  of work, the newest first: done, failed, or stopped. The same is there for agents and the
  CLI as `mail.showProgress` and `mail.cancelTask`.
  Better than Outlook: saving and exporting are listed and stopped there too, not only
  Send/Receive; what ended stays listed with what came of it instead of vanishing, and a stopped
  one says it stopped and that what was done is kept; the window stands beside the mailbox,
  which stays in use while it is open.
  2 tests.
- **2026-10-15** - M4.9: Delay Delivery and Expires After.
  The compose window's Delivery menu sets when a message may go (In 1 hour, Tomorrow at
  8:00 AM, Monday at 8:00 AM, or a time typed in) and when it expires (Today at 5:00 PM, In 1
  day, In 1 week, or a time typed in); what is set stands under the toolbar with a way to undo
  it. Send leaves a held message in the Outbox, where the list says when it goes, and it goes
  on its own at that minute. The times are kept on the message as Outlook keeps them
  (PidTagDeferredDeliveryTime, PidTagExpiryTime), in the data file and in the list's table, so
  a held message waits through a restart. When it expires is told to recipients in an
  "Expires:" header, and read from mail that comes with one or with "Expiry-Date:"; an expired
  message is struck through in the list. When a held message goes is told to no one.
  The window's clock now moves on while it stays open: a window open past midnight says
  "Today" of the new day.
  Better than Outlook: a held message goes at its minute, not at the next Send/Receive after
  it, and with Send/Receive on a timer off; one held past its time while the window was closed
  goes when it opens; one that expired while it waited is not sent but stays in the Outbox
  with the reason; a time that has gone by, a day the month does not have, or an expiry before
  the message would go is refused when it is set, not found out later.
  3 tests.
- **2026-10-15** - M4.12: only what changed fetched from an IMAP server (CONDSTORE, QRESYNC).
  Each folder kept in step remembers where it was left: its UIDVALIDITY, its highest
  mod-sequence, how many messages it held and the id the next would have. On a server with
  CONDSTORE (RFC 7162) a Send/Receive selects each folder and, when nothing changed, asks
  nothing more; when flags changed it asks only for the messages changed since
  (UID FETCH ... (CHANGEDSINCE n)). With QRESYNC turned on (ENABLE QRESYNC), the SELECT itself
  says which messages went and which changed, in one round trip. A server without either, or
  one that says NOMODSEQ, is read in full as before. What is remembered is kept in the
  settings, the mod-sequence as text since it may be past what a JSON number holds.
  Better than the RFC asks: a message the server says went is asked for once more before
  anything here is removed for it, so a server that names a message by mistake loses nothing;
  without QRESYNC, nothing is taken as gone unless the count and the next id add up, and when
  they do not the whole list is read before CHANGEDSINCE is asked for, not after; a folder is
  remembered only after a pass that read all of it and changed nothing it could not, so a
  stopped pass is never taken for one that ended; hostile sets of ids (`0`, `1:*` repeated a
  hundred thousand times) and mod-sequences past 2^63 are refused, not trusted.
  6 tests.
- **2026-10-15** - M4.12: new mail told of as it comes (IDLE, RFC 2177).
  Each IMAP account's Inbox is watched on a connection of its own, on a thread of its own:
  the server is sent IDLE and, when it says something in the Inbox changed (EXISTS, EXPUNGE,
  FETCH, VANISHED), the account is received from at once, as a Send/Receive of that account
  alone would, and new mail is told of at the corner of the screen. IDLE is begun again every
  25 minutes, before a server may end it at 29. A server that does not offer IDLE is
  received from on the timer as before. Turned on and off with Send / Receive > New Mail as It
  Arrives (`mail.newMailAsItComes`), kept in the settings; on by default.
  Better than Outlook: the Inbox is opened with EXAMINE, so watching it changes nothing on the
  server, not even which messages are new; the watch only listens and the receive is begun on
  the session's own thread, so nothing is changed from two places at once; how each watch
  stands (watched, not offered, lost and tried again when) is shown in Show Progress; a
  connection that is lost is made again after 10 seconds when it had been watching a while,
  and one that could not be made after a minute, twice as long each time up to half an hour,
  and a refused password or a certificate not trusted is not tried again for half an hour, so
  a server is never hammered; a change heard while a receive runs is received once that
  receive ends, so a change made elsewhere in the meantime is not missed; a server that
  chatters without end or answers out of turn ends the watch, not the window; the window is
  woken when a watch hears something rather than looking all the time.
  5 tests.
- **2026-10-17** - M1.8: shown as Outlook shows it.
  An account's mail was put beside Outlook's showing of it, locally, and what differed was fixed:
  a table cell's padding written in centimetres, points or any other absolute unit is read (it
  was dropped, so a signature's cells sat tight); a link is underlined only when its style says
  so, as a signature's are not; Sent Items, Drafts and the Outbox show whom a message went to;
  a data file's rows, whose table holds no preview, get theirs from the message, a few at a time
  as they come on screen, and a body the preview holds whole ends with an end mark; the date
  headings are Outlook's (the days of this week by name, Last Week, Two Weeks Ago, Three Weeks
  Ago, Earlier This Month, Last Month, Older, and forward for a held message); dates and times
  are written as the system's region settings say, and weeks begin where they say; an `.ost`
  whose store does not say where its mail begins starts at the mailbox's IPM_SUBTREE, not at
  the file's root with Outlook's own folders, and folders marked hidden are left out of the
  folder pane but kept and saved.
  Better than Outlook: a region's pictures are read with control characters, length and missing
  date letters guarded; previews read from a message are kept while the folder is open and
  capped, so a long folder scrolled end to end stays smooth; padding that is not a finite length
  is no padding, and any padding is capped.
  6 tests.
- **2026-10-17** - M4.12: IMAP folders renamed, moved and deleted followed both ways.
  Each folder of an IMAP server is now paired with the folder here it is kept in, and what the
  two were called and where they stood when last in step is remembered. A folder renamed or
  moved on the server is found again by the server's own id for it (MAILBOXID, RFC 8474) or, where
  the server gives none, by its UIDVALIDITY asked with STATUS, and the folder here is renamed or
  moved to match, its messages keeping their places by the new name; nothing is taken down
  again. A folder renamed or moved here, or put with the deleted mail, is renamed there with
  RENAME (into the server's Trash for the deleted mail); one deleted here for good is deleted
  there with DELETE; a folder made here is made there. A folder the server no longer has is
  kept here, with what it holds, in Deleted Items.
  Better than Outlook: a server folder is deleted only when the owner deleted it here, never
  because it is merely missing here, and only while its UIDVALIDITY is still the one known, so a
  folder made there anew by that name stays; where two folders cannot be told apart nothing is
  done rather than the wrong one followed; a folder gone from the server keeps its mail here
  instead of losing it; the ids a server gives are checked as RFC 8474 says before they are kept.
  8 tests.
- **2026-10-17** - M4.12: Drafts on the server.
  A draft is saved into the Drafts folder of the account it goes from, in that account's own data
  file, and at the next Send/Receive it is put in the server's Drafts folder marked `\Draft`
  (RFC 9051 2.3.2), the folder made there when the server has none. Save keeps the message open,
  as in Outlook, and each later save takes the place of the one before, here and on the server;
  sending it takes it out of Drafts on both sides. A draft written in another program comes here
  as a draft, not as a sent message: a double-click on it in the list, or the bar above it in the
  reading pane, opens it to be written on (`compose.editDraft`, `ui.openSelected`), with its
  recipients, attachments, answer headers, delivery time and expiry kept.
  Better than Outlook: the draft a save replaces is taken out only when it is still that draft in
  that data file, never another message that came to stand in its place; nothing unsent but a
  draft ever goes to the server; a draft closed with nothing written since its save is not kept a
  second time as discarded.
  3 tests.
- **2026-10-17** - M4.12: the places of messages read apart from the window.
  To keep an IMAP account in step, a receive needs to know where each message of the account's
  data file is on the server, and that is kept on the message itself. It was read for every
  message on the window's thread the first time a run received, which held the window for a while
  on a large file. Now only what is remembered is taken there; the rest, and each message to be
  put on the server, is read on the receive's own thread through a handle of its own on the data
  file, with the unsaved changes as they were when the receive began (`Store::apart`,
  `imap_sync::Gather`). What it learns is remembered when the receive ends, so the next one reads
  nothing again. A store in no file is read where it is, as before.
  Better than Outlook: when the file cannot be read apart (another program changed it since it was
  opened, say), the account is not kept in step this time and says so, rather than being taken for
  a file without those messages; nothing goes from the server for it.
  2 tests.
- **2026-10-17** - M4.13: pictures decoded off the thread that draws the window.
  A message's pictures (its own attachments, pictures written into the body, and those downloaded
  from a server on command) were decoded on the thread that draws the window, so a large or slow
  one held it. They are now decoded on a thread of their own and made into textures there. Drawing
  waits a moment (20 ms) for them, so a small picture is there in the first frame as before, and
  a large one stands as an empty frame until it is ready; the window is asked to draw again as
  each comes. A thread still decoding for a message no longer read stops when no one is
  listening, and none of its pictures is shown on another message. The limits on how many
  pictures and how much memory a message may take are kept as before, counted again as each
  picture arrives. Where there are no threads (the web build) they are decoded where they are.
  Headless window pictures wait for every picture (`MailUi::wait_for_pictures`).
  Better than Outlook: a picture never holds the window, however large or damaged.
  1 test.
- **2026-10-17** - M4.14: an account's servers found from its address.
  `account.find` ("Find Account Settings", and Find settings in the window's Account Settings)
  finds the servers for an address in three ways, in order: what is known here of large
  providers (Gmail, Outlook.com, Yahoo, iCloud, AOL, Fastmail, GMX, Zoho, Yandex), with a note
  where the provider asks for an app password or its own sign-in; the settings the domain
  publishes for mail programs (`autoconfig.<domain>/mail/config-v1.1.xml`, then the domain's
  `.well-known` place), over HTTPS only; and the usual names (`imap.`, `mail.`, `smtp.` and the
  domain itself, on the usual ports), each greeted over TLS or made secure with STARTTLS and
  signed in to by none. What is found fills the form; nothing is kept until it is saved. The
  result says where the settings came from.
  The settings file is hostile: capped at 64 KB and 4096 nodes, read with no DTD, nested no
  deeper than 32 (a deeper file overflowed the reader's stack in the hostile test; it is now
  measured before it is read, with comments, CDATA and quoted values passed over whole so
  nothing can hide its depth), and a name, port or user name in it that is not one is passed
  over; a server offering no secure connection is never chosen.
  IMAP and POP3 opening split into reaching the server and signing in (`Imap::greets`,
  `Pop3::greets`); the fake server can be told which servers answer (`State::answers_only`).
  Better than Outlook: the address itself is sent to no one, not even the domain's own web
  server (only its domain is named, in the names asked for); no outside directory is asked;
  and the owner is told where the settings came from.
  10 tests.
- **2026-10-17** - M4.15: an account's servers found from its domain's DNS records; M4 closed.
  Finding an account's servers (`account.find`, Find settings) now asks the DNS between the
  settings a domain publishes and the usual names: first the services its SRV records name for
  mail programs (RFC 6186: `_imaps`, `_imap`, `_pop3s`, `_pop3`, `_submissions`, `_submission`,
  the implicit-TLS ones of RFC 8314 first; the lowest priority, then the highest weight; a
  target of "." taken as "not offered"), then the provider its mail exchangers belong to
  (Google Workspace, Microsoft 365, Yahoo, Fastmail, Zoho, iCloud, Yandex), whose servers are
  the ones kept here and never names read from the answer.
  A DNS client of our own (`crates/net/src/dns.rs`, RFC 1035): questions with an id no one can
  guess, asked over UDP of at most three of the system's name servers (from the registry on
  Windows, `/etc/resolv.conf` elsewhere), again over TCP when an answer is cut short; only an
  answer from that server, to that question, is taken. Answers are hostile: every length and
  count is checked against the bytes there are, compression pointers may only point back and
  are followed at most 16 times, names are at most 255 bytes of letters, digits, hyphens and
  underscores, and at most 32 records are kept. Lookups go through the connector, so the fake
  server answers them in tests.
  Better than Outlook: DNS answers are not signed, so what SRV records name comes with a note
  to check it, and is only filled into the form; MX only ever leads to a provider's known
  servers, so a changed answer cannot send the password to a server of its choosing.
  OAuth sign-in, the one M4 row left, is M8's. 12 tests (a 20 000-try bounded fuzz of answers
  among them).
- **2026-10-17** - M8 put after the first release, by the owner's choice.
  Third-party sign-in through OAuth 2.0 waits until after the first release; until then Gmail,
  Microsoft 365, Yahoo and the like are signed in to with an app password, as Find settings
  says. Next: M5, the Calendar, People and Tasks modules.
- **2026-10-17** - M5.1: the Calendar: day, work week, week and month views; appointments made, changed and deleted.
  The Calendar module shows the Calendar folder of the open data file (or the first store that
  has one) in Day, Work Week, Week and Month views (Ctrl+Alt+1 to 4, and on the ribbon), with a
  month at the side to pick a day from, Today, and back and forward by the view's own step
  (Alt+arrows). Appointments are read from the [MS-OXOCAL] properties (start and end, all day,
  Show As, reminder, recurring and meeting flags) and written the same way, so Outlook reads ours.
  The time grid has 24 hours scrolled to the morning, working hours shaded, appointments that
  overlap side by side, all-day events in a band above, a line at the time now; the month view
  shows each day's items and "+N more", which opens that day. Show As is drawn on each item:
  solid for Busy, striped for Tentative, outlined for Free, dotted for Working Elsewhere, purple
  for Out of Office. A double-click on an empty half hour (or a day of the month) makes one there.
  Commands: `calendar.view`, `calendar.today`, `calendar.goTo`, `calendar.next`,
  `calendar.previous`, `calendar.items` (its text as `untrusted_content`), `calendar.select`,
  `calendar.new`, `calendar.change`, `calendar.delete` (to Deleted Items), each undone by
  `edit.undo`; `ui.appointment` opens the window. Times are written and shown in local time;
  an all-day event runs midnight to midnight, and the last day given is its last day.
  Better than Outlook: the appointment window names the appointments it clashes with as you
  pick its time, says how long it lasts, and will not save one that ends before it starts.
  Hostile input: text is cut at 2000 characters and control characters left out, at most
  20 000 items are read, times outside 1601 to 9999 and reminders over a year are refused.
  Scheduled in M5: Import from Outlook (M5.2), recurrence (M5.3), iCalendar files (M5.4),
  CalDAV sync with Nextcloud and other servers (M5.5), then People and Tasks. 12 tests.
- **2026-10-17** - M1.9: dialogs in the theme's colours; themes of your own for light and dark.
  The dialogs were egui's own: a grey title bar on a white body, while the window around them
  had colour. Every dialog (Account Settings, Themes, the appointment window, Print, Send/Receive
  Progress, Open this link, Opening an attachment, the folder name prompt) now has a head in the
  theme's dialog colour with its own mark drawn in code, its name and a close button (the same as
  Cancel; Escape still is), a body on the surface colour with headings in the accent, and a foot
  on the pane colour that holds its buttons: the one that does the work filled with the accent,
  one that throws something away in the warning colour. Warnings and clashes stand in tinted
  boxes; a link's address and an attachment's name in cards of their own. While a modal dialog
  is open the window behind it is dimmed and takes no clicks. egui's own widgets (fields,
  selections, menus, links) take the theme's colours too.
  Themes: Blue, Teal, Forest, Plum, Rose, Amber, Graphite and High contrast are built in, each
  with a light set and a dark set; View > Themes opens a window that chooses one, shows both sets
  of each as small windows, and changes any of 25 colours of either set (accent, window, panes,
  text, title bar, status bar, dialog head, flag, warning…) by picker or by #RRGGBB, with a
  preview that shows each colour where it goes. A change to a built-in theme makes a copy that is
  yours; your themes are named, renamed, copied and deleted (at most 40), and kept in the
  settings file with the rest of the window. The window can follow the system's light or dark
  setting. Commands: `ui.themes`, `ui.useTheme`, `ui.themeColor`, `ui.newTheme`,
  `ui.renameTheme`, `ui.deleteTheme`, and `ui.theme` takes "system"; `ui.accountSettings` takes
  `"close": true`.
  Better than Outlook, which offers four fixed Office themes: any colour of either set can be
  changed; one accent brings the colours that go with it along; every pair of text and what is
  behind it is checked for contrast (WCAG 2) as it changes, and what is hard to read is named.
  Hostile input: a settings file's themes are read key by key, with only known colour keys and
  strict #RRGGBB taken, names cleaned and capped at 60 characters, built-in and repeated names
  left out, and at most 40 kept. Every dialog is drawn in every theme, light and dark, in a large
  window and a tiny one. Do later: themes saved to and read from a file of their own. 4 tests.
- **2026-10-20** - M5.8: People: contacts, contact groups and vCard files.
  The People module was a placeholder; it now shows the Contacts folder of the data files open
  in Outlook's five Current Views: People (a list of names beside the chosen one's card),
  Business Card, Card, Phone and List. My Contacts lists every Contacts folder, and a click shows
  that one. Contacts are read and written as Outlook keeps them ([MS-OXOCNTC]: names and file-as,
  three e-mail addresses with their one-off entry IDs, telephone numbers, business, home and other
  addresses with the one mail goes to, birthday, anniversary, notes, categories and the picture),
  and contact groups with their members ([MS-OXOCNTC] 2.2.2: members' one-off entry IDs), so
  Outlook reads what is made here. A contact is made, changed and deleted from its window
  (Ctrl+Shift+C), a group from its own (Ctrl+Shift+L), every change undone with Ctrl+Z.
  Search People (Ctrl+E) looks in every field as it is typed. vCard files, 2.1 as Outlook writes
  them, 3.0 and 4.0 (RFC 2426, RFC 6350, [MS-OXVCARD]), are shown card by card before anything is
  added; a card for someone already there, known by its UID or else by an e-mail address, brings
  them up to date keeping what the card does not say, instead of adding them twice; contacts are
  saved as a vCard file, sent on as cards (Forward Contact) and written to (E-mail); a message's
  sender is added to Contacts. Better than Outlook: the search finds notes and numbers too, a card
  opened again does not make a second contact, someone is always chosen after a delete (the one
  that took its place), and a contact's picture is decoded on a thread of its own. Hostile input:
  a vCard file is read within bounds (32 MB, 20 000 cards, 2 000 properties a card, 8 MB a line,
  nesting 8 deep), a picture named by a link is not fetched, and every command's parameters are
  checked (three e-mail addresses at most, a picture of at most 4 MB of a kind known, dates within
  1601 to 9999). Do later: CardDAV, the Address Book and Mail Merge (M5.11). 27 tests.
- **2026-10-19** - M5.10 (first part): a Calendar folder made in a data file that has none.
  An account's data file made before account files were given a calendar, people and tasks
  could not take a meeting: Accept was not there, as there was no calendar to put it in. Such
  a file is now given the Calendar, Contacts and Tasks it lacks when the account opens it,
  each of its own kind (its container class, [MS-OXOSFLD] 2.2.6), as Outlook makes them for an
  account; a folder of that kind already there, by any name, is kept as it is. A data file can
  now be given a folder of any kind, `Store::create_folder_of`, kept in the journal of unsaved
  changes like any other and saved as that kind. Found with a request sent from Outlook Classic
  to one of the owner's own accounts. `openmailclient-cli run --profile` runs commands on the
  window's own settings, accounts and data files and keeps them again at the end, so a thing
  seen in the window can be done and looked at from the command line, its times in the
  machine's own zone as the window's (`openmailclient_engine::local_zone`, shared by both).
  Looked at with Outlook Classic on IMAP accounts: Outlook's request accepted here reached it as
  an acceptance (`IPM.Schedule.Meeting.Resp.Pos`), and an invitation sent from here came to it
  as a meeting request at its hour, not answered and pencilled into its calendar. Hostile input: a class
  that is empty, longer than 255 characters or has control characters in it is refused, and so
  is a parent that is not there. 2 tests.
- **2026-10-19** - M5.7: meetings to and from Outlook.
  A meeting request sent from Outlook Classic to one of our accounts, and one of ours opened
  in Outlook, were looked at side by side. Outlook sends a request as its calendar alone, a
  single `text/calendar` part with no text beside it, its lines folded with tabs, its zone
  named as Windows names it, its UID the meeting's GlobalObjectId in hexadecimal and what it
  says in DESCRIPTION. Such a request was already known as one, but showed an empty body: what
  the calendar says is now the message's body (RFC 5545 3.8.1.5). Outlook keeps a meeting's
  messages in its data files and `.msg` files without any calendar, by their properties alone
  ([MS-OXOCAL] 2.2.6 to 2.2.8): a request, an answer or a cancellation kept so is now read from
  its GlobalObjectId, class, sender, times and attendees, so one in an imported data file is
  answered as one that came by mail. The other way: every meeting message we receive or send
  is now given the properties Outlook reads (the GlobalObjectId, state flags, sequence, the
  answer it stands at, when and where), so a data file of ours shows it to Outlook as the
  meeting it is; our replies name the meeting by Outlook's own id, so Outlook takes them in.
  `openmailclient-cli inspect file.msg --props` lists every property of a message, for looking
  at files like these on one's own machine. Better than Outlook: a request's words are kept
  as text, never its HTML, and an answer read from properties is still taken only from the
  attendee it names. Hostile input: at most 64 KB of a calendar is read for its words and
  20 000 characters kept; a quoted colon in a parameter is not taken as the value's start; a
  meeting message with no GlobalObjectId is no meeting. Do later: a data file of ours with
  meeting messages opened in Outlook by the M3c.1 checks. 4 tests.
- **2026-10-19** - M5.7: meetings: invitations sent, answered and called off.
  An appointment can be a meeting: it has attendees, those who must come, those who may and
  rooms or things, typed in the appointment window or given to `calendar.newMeeting` and
  `calendar.change`. Sending it (Send, `calendar.sendInvitations`) puts an invitation for each
  in the Outbox: a message Outlook and other mail programs show as an invitation, the class
  Outlook gives it ([MS-OXOCAL] 2.2.6.1) with an iCalendar REQUEST in it (RFC 5546 3.2.2,
  carried as RFC 6047 says), sent by the account's SMTP server with the rest of the Outbox. A
  meeting changed and sent again goes out as an update with its SEQUENCE one higher; one
  called off (Cancel Meeting, `calendar.cancelMeeting`) as a CANCEL, and is marked as such.
  An invitation that comes has a bar above it that says when and where the meeting is, whether
  one has answered, and what else is planned then, with Accept, Tentative, Decline and Show in
  Calendar; answering (`calendar.respond`) puts it in the calendar (or not, when declined),
  sends the organizer a REPLY unless asked not to, and moves the invitation to Deleted Items
  as Outlook does. An answer that comes is taken into the meeting it names, by its UID, and
  shown in the organizer's tracking (`calendar.attendees`, Tracking in the window); a
  cancellation that comes marks the meeting "Canceled: " and Free. Reply with Meeting
  (`mail.replyWithMeeting`) makes a meeting of a message, the sender required and the others
  optional. The meeting is kept in the data file as [MS-OXOCAL] keeps one: the state flags
  (2.2.1.10), the attendees as recipients with the organizer marked and each one's answer
  (2.2.4.10), one's own answer (2.2.1.11) and the sequence (2.2.1.1); in calendar files and
  over CalDAV with ORGANIZER, ATTENDEE and SEQUENCE ([MS-OXCICAL] 2.1.3.1.1.20).
  Better than Outlook: an invitation is not put in the calendar until it is answered (Outlook
  puts it there at once, as tentative, which lets anyone who can send mail write into one's
  calendar); an answer is taken in only from the attendee it names and a cancellation only
  from the meeting's organizer, anything else is left as a message to read; the bar says what
  clashes before one answers; a meeting can be saved without sending it, to send later; the
  calendar the message carries is the meeting, so it is not shown as a file to open.
  Hostile input: an iMIP message's calendar is read by the `.ics` reader with its bounds; at
  most 500 attendees are kept from a file or a command; names and addresses are cleaned of
  anything that could end a line or a value before they are written; a REPLY or CANCEL for a
  UID we do not have, or from the wrong person, changes nothing; every new command is in
  `hostile_params_never_panic`. Do later: the Scheduling Assistant; a comment typed with an
  answer in the window; zone rules by year and the checks in Outlook (see Do later). 22 tests.
- **2026-10-19** - M5.6: time zones and daylight saving.
  Times were kept and shown at one offset from UTC, the one this computer had when the window
  opened, so a weekly 09:00 meeting moved to 10:00 the week summer time began. Now a place's
  zone is kept with its rules: its offset in standard time, and the day and hour summer time
  begins and ends each year (`crates/model/src/zone.rs`, about 70 zones known by their tz
  database names and Outlook's names for them). The window takes the system's zone, found from
  the offsets the system keeps through the year and named as the system names it; dates
  everywhere (the message list, search, printing, the Calendar) are shown in it, or in another
  chosen in the Time Zones window (Calendar > Home > Time Zones) and kept in the settings.
  An appointment is kept in the zone it was made in, shown in the appointment window with a
  choice of any other: a series keeps its hour there whatever the season, and each occurrence
  falls where that zone says. A time that summer time skips moves on to the hour after; one
  that happens twice is the first.
  In a data file the zone is written as Outlook keeps it ([MS-OXOCAL] 2.2.1.41 TZDEFINITION
  in the start, end and recurrence zone properties, 2.2.1.39 TZREG and the zone's description
  for a series), read back from any of them. A calendar file defines each zone its events are
  given in (RFC 5545 3.6.5 VTIMEZONE with a yearly rule for each change) and an event is
  written in its own zone; one read takes the rules a VTIMEZONE gives, or a zone known by name
  where the file names one without defining it, as Outlook's files do.
  Commands: `calendar.timeZones` (each zone known, with its offset now), `calendar.timeZone`
  ({"zone": a name, or null for the system's}), `calendar.secondTimeZone`; `calendar.new` and
  `calendar.change` take "zone"; `calendar.items` says each appointment's; `ui.timeZones`
  opens the window; settings keep "timeZone" and "secondTimeZone".
  Better than Outlook: a second zone's hours stand beside ours in the day and week views as in
  Outlook, and the Time Zones window says what time it is now in each; the appointment window
  says when an appointment typed in another zone is where you are as you type it; a zone is
  asked for by either its tz name or Outlook's; the system's zone is named by the place the
  system names, not the first with the same rules.
  Hostile input: a TZDEFINITION is read only as far as its header and rule count say and at
  most 1 024 rules, a rule's month, week, weekday and hour are checked and offsets kept within
  18 hours; a VTIMEZONE whose rules no yearly pattern holds is read at the offset of each
  event's start, as before; zone names from files are cleaned and capped, and an unknown name
  given to a command is an error that changes nothing.
  Do later: zones whose rules changed over the years kept with each year's rules (an event long
  past shown at that year's offset) and zones whose rules follow another calendar (Jerusalem,
  Tehran's old rules, Cairo) beyond their current year (M5.7); the zone checked in Outlook with
  M5.7's meetings. 10 tests.
- **2026-10-18** - M5.5: calendars kept in step with a CalDAV server.
  An account's settings take a CalDAV server's address (Account Settings > Servers > Calendars),
  signed in to with the account's own name and password from the credential store, over HTTPS
  only. Its calendars are found from that address: the principal it names, or the one
  `/.well-known/caldav` leads to (RFC 6764), and the calendar home it keeps them in (RFC 4791 6.2.1).
  Each calendar becomes a Calendar folder of the account's own data file: the first is its
  Calendar, the others are made beside it under it, named and renamed as the server names them.
  On each Send/Receive, and on `calendar.sync`, on a thread of its own: what changed on the server
  is asked for by the calendar's sync token (RFC 6578), or, where it keeps none, by each event's
  ETag, and read many at a time (calendar-multiget, RFC 4791 7.9); what was made, changed or
  deleted here is put or deleted there only while the server still has what was last read of it
  (`If-Match`, `If-None-Match`). Each appointment kept in step carries its place: its path, its
  ETag and a hash of it as it was then, so a change on each side is known without asking.
  My Calendars lists every calendar of the data files open, in the server's colour where it gives
  one, with a check on the one shown; a click shows another. A certificate of the calendar
  server's own is trusted as for mail servers, for that host only.
  Commands: `calendar.sync` ("id", "wait"), `calendar.calendars`, `calendar.open` ({"store",
  "folder"}); `account.add` and `account.edit` take "caldav" (an `https:` address, or "" for
  none); `account.trustCertificate` takes "server": "calendar".
  Better than Outlook, which keeps no CalDAV calendar at all without an add-in: a change made on
  both sides keeps both (the server's in place, ours beside it as a copy) instead of losing one;
  the sync token kept is the one from before our own changes went up, so an answer lost on the
  way never loses a change; a read-only calendar takes no changes from here, and what is deleted
  in it here comes back; an appointment changed while a sync was under way is left for the next.
  Hostile input: a server's answer is read only up to 16 MB, 100 head lines, 4 million XML nodes
  40 deep, 50 000 responses and 100 calendars; hrefs are capped and one that leads to another
  server is not followed; at most 5 redirects; a sync cut short is taken in at most 20 rounds;
  names, colours, ETags and tokens are capped; at most 20 calendars, 2 000 events taken down and
  500 put in one sync, the rest with the next. Events read are untrusted content, as `.ics` files.
  Do later: attendees kept when an event is put (M5.7); an event's own time zone (M5.6); calendars
  shown side by side and overlaid; the Calendar folder made in an older account data file that has
  none (M5.10). 14 tests.
- **2026-10-18** - M5.4: iCalendar files opened, imported and saved.
  File > Open Calendar (and Home > Open Calendar in the Calendar) reads an `.ics` file (RFC 5545)
  and shows, before anything is added, each event: its subject, when it is (an all-day event by
  its days), where, how it repeats in words, and what of it cannot be kept. The events chosen are
  added to the Calendar as appointments, as [MS-OXCICAL] maps them: SUMMARY, LOCATION and
  DESCRIPTION, DTSTART and DTEND or DURATION in the file's own time zones (VTIMEZONE) or UTC or
  floating time, all-day dates, TRANSP and Outlook's own busy status, CATEGORIES, CLASS, the
  first VALARM as the reminder, and RRULE (daily, weekly, monthly and yearly, by day, by date, by
  the nth weekday and by set position, with COUNT or UNTIL) with EXDATE and changed occurrences
  (RECURRENCE-ID) written as the series' exceptions. An `.ics` attachment opens the same window
  from the attachment dialog's "Add to Calendar…", without saving it first.
  File > Save Calendar writes the whole calendar, a range of days or one appointment as an `.ics`
  file, each series with its pattern, deleted occurrences and changed ones, a time zone of its
  own for appointments kept in this computer's offset; Home > E-mail Calendar starts a message
  with the days shown listed in its body and attached as a calendar file.
  Commands: `calendar.import` ({"path"} or {"attachment"}, "preview", "only"),
  `calendar.saveAs` ({"path"}, "id" or "from" and "to", "name"), `calendar.email`
  ("from", "to", "name"); `ui.openCalendar`, `ui.openCalendarDialog`, `ui.saveCalendarDialog`.
  Better than Outlook, which either adds a file's events unseen or opens it as a second calendar
  beside yours: each event is shown first and chosen or left; the UID becomes the appointment's
  GlobalObjectId ([MS-OXCICAL] 2.1.3.1.1.20.26), so opening a file again, or a newer copy of it,
  brings what it has already up to date instead of adding everything twice, and says so for each;
  all of an import is taken back by one Undo; what could not be kept (attendees, a pattern that
  cannot be repeated, a time zone the file does not define) is said for each event; an invitation
  file is said to be one, and nobody is answered by opening it.
  Hostile input: a file is read only up to 32 MB, lines are cut at 256 KB, nesting at 8, 50 000
  components, 20 000 events, 2 000 properties of one and 1 000 values of a list; texts are capped;
  a date outside 1601 to 9999, an end before its start or a span longer than the Calendar shows is
  refused for that event alone; alarms that would play a sound or send mail are read only as a
  reminder, and nothing in a file is fetched or run. What is reported of a file is marked as
  untrusted content.
  Do later: an event's own time zone kept with it (M5.6), attendees and invitations answered
  (M5.7), tasks and journal entries in a file (M5.9). 7 tests.
- **2026-10-18** - M5.3: recurring appointments.
  An appointment repeats daily, every weekday, weekly on chosen days, monthly on a date, on the
  nth (or last) day, weekday, weekend day or named day of the month, or on its last day, and
  yearly the same ways; every so many days, weeks or months; for ever, so many times, or until a
  date. The pattern is read and written as Outlook keeps it ([MS-OXOCAL] 2.2.1.44: the
  RecurrencePattern blob in PidLidAppointmentRecur, with the clip start and end, the recurrence
  type, the time zone it was made in and the pattern in words), and a series is shown on every
  day it falls in the day, week and month views, each occurrence with a loop at its corner.
  One occurrence is changed or deleted alone: its time (kept between its neighbours, as Outlook
  keeps it), subject, location, Show As and reminder; a changed one is written as Outlook writes
  it, an exception in the pattern and a hidden attached message ([MS-OXOCAL] 2.2.10.1) holding
  only what differs, and is drawn with the loop struck through. Opening an occurrence opens it
  alone, and the window switches to the whole series and back with nothing typed lost; Delete
  there deletes that occurrence alone. Changing a series' time plans its occurrences again.
  A pattern this program cannot read is kept as it is and the appointment is shown once.
  Commands: `calendar.new` and `calendar.change` take "repeat" ("every": day, weekday, week,
  month or year, "interval", "on", "nth", "day", "lastDay", "count", "until", or null for none),
  `calendar.recurrence` sets it, and `calendar.select`, `calendar.change`, `calendar.delete` and
  `ui.appointment` take "occurrence" (the day it was first planned on); `calendar.items` lists
  each occurrence with its "occurrence", "changed" and "pattern".
  Better than Outlook, which asks "Open this occurrence or the series?" before it opens: the
  window opens on the occurrence and switches to the series with one click, keeping what was
  typed in each; the pattern is said in words as it is chosen, with the first days it falls on,
  and a pattern that can never fall, or an end before the start, is said at once instead of on
  Save; the tooltip of every occurrence says the whole pattern.
  Hostile input: a pattern blob is read with every count and offset checked, at most 999
  occurrences and exceptions each, days outside 1601 to 9999 refused; a blob that cannot be read
  is kept as stored; an interval is 1 to 999 days or 1 to 99 weeks or months; an occurrence
  cannot be moved past its neighbours; listing is capped as before. Outlook Classic opened a file of ours with three series and read each pattern as written (weekly on two days 8 times, every weekday with no end, the last Thursday of each month until a date), the changed occurrence with its own subject, place and time, and the deleted one as deleted.
  Do later: time zones and daylight saving for a series (M5.6), a series in `.ics` (M5.4).
  7 tests.
- **2026-10-17** - M5.2: Import from Outlook: accounts and their data files.
  File > Import from Outlook reads the Outlook profiles' settings (Office 2016 and later, and
  2013) and shows, before anything is done, each IMAP and POP3 account with its address, its
  incoming and outgoing servers, ports and security, how big its data file is and whether Outlook
  holds it open; the other data files of the profile (archives, files of old mail) with theirs;
  and what cannot be brought and why (an Exchange or Microsoft 365 account waits for M8). Each is
  chosen or left, and Import brings them on a thread of its own, with progress and Stop:
  - an account comes with its servers and user name, never its password: Outlook keeps passwords
    locked to the system, so the window says to give each one in Account Settings, and Done goes
    there;
  - its data file is copied with other writers kept out, so a copy is never torn, checked by
    opening it, and kept as the account's own data file; an `.ost` is written as a `.pst` of our
    own, so it can take changes. Its calendar, contacts and tasks come with its mail;
  - a data file of its own is copied beside the settings (Imported) and opened, kept for the next
    run as any data file;
  - an account here already brings only its data file, beside it, under a name of its own.
  Outlook's files are only read: nothing is written beside them, not even a journal, and one
  Outlook holds open for writing is said ("close Outlook") and not copied. The IMAP mail brought
  is known again by its Message-ID on the first Send/Receive, so it is not taken down twice.
  Commands: `file.importOutlook` (`{}` shows; `{"import": [ids], "withData", "wait"}` imports),
  `ui.importOutlook`. `ui_shot` takes a profile written by hand (`{"outlook": …}`).
  Better than Outlook, whose Import/Export brings one data file into one folder and no accounts:
  every account of every profile with its servers and its mail, together, shown first, while the
  window stays in use.
  Hostile input: profiles, sections, values and their lengths are capped (16, 512, 256, 64 KB);
  text is read from words or UTF-16 bytes to its first NUL, cut at 1024 characters, control
  characters left out; ports of 0 or over 65535 are dropped, paths that are not absolute or not
  `.pst`/`.ost` are not taken, ids are checked, and the accounts are checked as Account Settings
  checks them. Do later: signatures, rules and the AutoComplete list from Outlook; a POP3
  account's own "leave a copy on the server" setting (kept on, the safe way, until then); and
  Outlook 2010's profile place. 7 tests.
- **2026-10-17** — M1.10: printing on a printer, and the message body alone. `file.printers` lists
  the system's printers and chooses one (kept in the settings); `file.print` with `"printer"`
  prints there with copies (1 to 99) and a page range, on a thread of its own seen in Show
  Progress. Windows: the printers from the registry, the pages drawn at 300 dpi and given to the
  system's print spooler through Windows PowerShell and .NET (no `unsafe`, no shell, names
  passed apart from the script); elsewhere CUPS's `lp` takes the PDF file. Only a printer the
  system lists is printed on. Page setup gains `details` (the owner's name and the message's
  fields, or a list's folder name) and `headerFooter`, and `bodyOnly` turns both off; what the
  header and footer say is kept while they are not printed. The Print window has Printer,
  Copies, Pages and Include, Print and Save as PDF…; `ui.printPreview` takes `copies`, `pages`
  and `print`. Improved on Outlook: one click for the body alone, with the preview showing it.
  More screenshots in the README. Tests use a printer that keeps what it is sent; nothing was
  put on paper (the Windows path was tried with the spooler's preview, which prints nothing).
- **2026-10-21** - M4.16: mail shown as it comes down; the window closes while mail is received.
  A receive hands what it has taken down back in parts while it runs: every 200 messages,
  16 MB or second, whichever comes first, so a new account's Inbox fills as it downloads
  instead of all at the end. Into an account's own data file the newest come first, as Outlook
  does, and what was not come to is known next time by each message's place on the server, so
  a receive stopped part way loses nothing and takes nothing twice. Several messages are asked
  for in one command (`Imap::fetch_many`, up to 25 or 8 MB), not one by one. Memory stays
  bounded however big the mailbox: at most four parts wait to be taken (the receive waits for
  the window, not the other way round), the window takes one part of each job a frame, and the
  account's data file is saved in place after every 64 MB taken in, so a 50 GB mailbox never
  sits in memory. Closing the window while mail is received no longer freezes it: the receive is
  told to stop, what it has is kept, and the window closes as soon as it has handed that back,
  three seconds at the most (`MailUi::close_now`, `Session::leave_jobs`); what was left comes
  with the next Send/Receive. The status bar counts what has come so far, and the end counts
  every part. Better than Outlook: closing never waits on a server. 7 tests.
- **2026-10-21** - Milestones renumbered: web access is M7, third-party sign-in M8.
  By the owner's choice a new M7 is added: the mailbox, calendar, people and tasks reached from a
  browser on any device, served over HTTPS by OpenMailClient itself from the machine it runs on,
  with the engine and data files staying there. Third-party sign-in through OAuth 2.0, which was
  M7, is now M8 and still comes after the first release; every mention of it is renumbered.
- **2026-10-24** - M5.9: Tasks and the To-Do List.
  The Tasks module was a placeholder; it now shows the Tasks folders of the data files open in
  Outlook's views: Simple List, Detailed, Prioritized, Active, Completed, Today, Next 7 Days and
  Overdue, and the To-Do List, where the mail flagged for follow-up in every folder stands beside
  the tasks. Tasks are read and written as Outlook keeps them ([MS-OXOTASK]: start and due dates,
  status, % complete, date completed, total and actual work, owner, mileage, billing, companies,
  recurrence read; [MS-OXOFLAG] for flagged mail: the follow-up's start, due and complete dates and
  its to-do title), so Outlook reads what is made here. A task is made, changed and deleted from
  its window (Ctrl+Shift+K), ticked off from its box in the list, and every change is undone with
  Ctrl+Z; Follow Up on a message offers Today, Tomorrow, This Week, Next Week and No Date as
  Outlook does, and a flagged message opened from the To-Do List changes only its follow-up.
  Better than Outlook: a task is added by typing its subject on the line above the list, with
  "today", "tomorrow" or a date at its end for its due day; the To-Do List stands under headings
  by when each is due, late ones first and in red; how far along each is shows as a bar; every
  view is in the side pane however narrow the ribbon; the search looks in notes, owners and
  companies as it is typed; work is typed in minutes, hours, days or weeks; and the window says
  how long there is left, or how late it is. Undo now also follows a task made after a delete was
  taken back. Hostile input: subjects, notes and lists are capped (4 000 characters, 200 000 for
  notes, 100 companies), work within a hundred years, dates within 1601 to 9999, and every
  command is run with junk parameters. Do later: VTODO in calendar files and CalDAV task lists
  (M5.11), the To-Do Bar and reminders (M5.10), Assign Task. 17 tests.
- **2026-10-25** — R1: the first pre-release, 0.1.0-alpha.1. A Windows installer made with Inno
  Setup (`installer/openmailclient.iss`, `cargo xtask package`): per-user unless everyone is
  chosen, no administrator, the Start menu and a desktop icon each asked for, the program's own
  icon drawn in code (`crates/ui-egui/src/app_icon.rs`) and carried in the program file with its
  version; uninstalling leaves mail and settings. Check for Updates (`help.checkForUpdates`,
  `crates/engine/src/update.rs`): GitHub's releases read warily (4 MiB at most, versions by
  semver precedence, assets only from this repository's release downloads), pre-releases offered
  when asked and always to a pre-release; the installer brought down on a thread of its own with
  progress and Stop (`http::download`, streamed, never whole in memory), checked by size and by
  the SHA-256 the release's `SHA256SUMS.txt` names, kept only when both agree, checked again
  when asked to install and once more as it is run, and run when the program closes after every
  change is saved, starting it again after. Once a day on its own, only telling. Improved on
  the usual: nothing is fetched or run without being asked, and a release's notes are shown as
  text. Do later: signed installers and sums (R2), a release workflow (R2), macOS and Linux
  packages (R3). 10 tests.
- **2026-10-27** — Issues from a check of 736 real `.msg` files against Outlook Classic. A
  subject ending in a space lost it on `.msg` to `.eml`: the writer split a field at its spaces
  and the reader trimmed what an encoded word held. A field that folding would change (white
  space at its ends, runs of it, tabs, line breaks) is now written as encoded words, the reader
  keeps it, and unfolding takes away the line break alone (RFC 5322 2.2.3). `inspect` gives
  `body_length` beside `body_chars`: the body as Outlook counts it, UTF-16 units with CR LF line
  ends, checked equal to Outlook's `Body.Length` on a file of ours. 1 test.
- **2026-10-27** - M5.10: reminders.
  When an appointment, a task or a message flagged for follow-up comes due, from any data file
  open, the Reminders window comes up beside the mailbox with a sound of our own and the taskbar
  button, as `reminders.options` says (`show`, `sound`, kept with the settings). It lists each with
  what it is of, its subject and how long until it is due, late ones in red, and offers Dismiss
  All, Open Item, Dismiss and Snooze, as Outlook's does; View > Reminders Window opens it any time.
  What is done is written into the item as [MS-OXORMDR] 3.1.4.6 says, so Outlook reads the same:
  a snooze sets the signal time, a dismiss clears the reminder of a single item and moves a series
  on to its next occurrence. The folders are looked through once a minute, soon after a command,
  and exactly when the next one comes due; an item is read again only when its row changed or a
  command ran, so a look over a large data file reads little. Better than Outlook: in a data file
  that cannot be written a reminder put away stays away for the run instead of coming back each
  minute; an appointment is offered "minutes before start" only while that is still to come; what
  is chosen shows when and where in full; with none left the window says when the next comes; a
  snooze is held between now and a week. Commands: `view.reminders`, `reminders.list`,
  `reminders.dismiss`, `reminders.dismissAll`, `reminders.snooze` (minutes, minutes before start,
  or all), `reminders.open`, `reminders.options`; subjects and locations come back under
  `untrusted_content`. Hostile input: 20 000 items a folder looked at, 500 shown, subjects and
  locations cut to 255 characters, snoozes of 1 to 10 080 minutes, junk keys refused. Do later
  (M5.12): a window of its own above others, a chosen sound, an exception's own reminder,
  Outlook's Reminders search folder. 14 tests.
- **2026-10-27** - M5.11: CardDAV, the Address Book, Check Names, Mail Merge and to-dos.
  An account may name a CardDAV server (`carddav`, RFC 6352) beside its CalDAV one: each address
  book there is kept in step with a Contacts folder of the account's data file on each
  Send/Receive and by `people.sync`, by sync token or ETag as calendars are, a change on both
  sides keeping both. The Address Book (`people.addressBook`) lists every contact and group of
  every data file open, found as it is typed for, and puts the ones chosen in To, Cc or Bcc;
  names are suggested under an address line while it is typed (`people.suggest`), and Check Names
  (`compose.checkNames`, also before a send) turns a name into its contact's address or says
  which are not known or could be more than one. Mail Merge (`people.mailMerge`, People > Mail
  Merge) writes one message to each contact shown or chosen, groups opened to their members and
  no address twice, with fields written «First Name» or {{First Name}}, kept in Drafts to be
  looked over or put in the Outbox; the window shows the first messages as they will be while
  the text is written and puts a field in where the cursor is. A calendar file's to-dos (VTODO,
  RFC 5545 3.6.2) are shown and put in Tasks, known again by their UID, and `tasks.saveAs` saves
  the Tasks folder as one, with statuses RFC 5545 lacks kept beside the nearest it has. Better
  than Outlook, which hands a merge to Word and leaves to-dos out of calendar files: a merge is
  looked at before anything is made, a wrong field is said at once, and contacts without an
  address are listed, not dropped in silence. Hostile input: 20 000 items a folder, 50 000 book
  entries, 1 000 fields and 5 000 messages a merge, subjects cut to 1 000 characters and bodies
  to 1 MB at a character's edge, to-dos as many as events, days from 1601 to 9999; names, previews
  and to-do text come back under `untrusted_content`. Do later (M5.12): CalDAV task lists, a merge
  printed as letters. 21 tests.
- **2026-10-27** - M5.12 (first part): CalDAV task lists.
  The calendars of an account's CalDAV server that say they hold to-dos (their component set
  names VTODO, RFC 4791 5.2.3) are kept in step with Tasks folders of the account's data file,
  the first its Tasks and the others under it, on each Send/Receive and by `tasks.sync`
  (Update Task Lists): tasks made, changed and deleted here go there as to-dos, and what
  changes there comes here, by sync token or ETag, a change on both sides keeping both, as
  calendars are. A calendar that holds events and to-dos both is kept by the calendar sync and
  the task sync each taking only its own kind and leaving the other's alone, each with a
  token of its own (`DavSetup::tasks` beside `calendars`). A calendar that says nothing of what
  it holds is taken for one of events, so nothing is kept twice. Better than Outlook, which
  keeps no CalDAV server's tasks at all. 4 tests.
- **2026-10-28** - R1: the second pre-release, 0.1.0-alpha.2, for Windows and Linux.
  `cargo xtask package` on Linux makes `OpenMailClient-<version>-linux-x64.tar.gz`: both
  programs, the icon drawn in code, a desktop entry and `install.sh`, which installs for the one
  who runs it under `~/.local` with no administrator and takes it away with `--uninstall`,
  leaving mail and settings. The programs are linked by `cargo zigbuild` against glibc 2.31, so
  one archive starts on Ubuntu 20.04, Debian 11 and every distribution since, not only on the
  one it was built on; everything else (GTK, X11, Wayland, sound) is loaded as it runs. Check
  for Updates on Linux tells of a release and opens its page.
- **2026-10-28** - M5.12: the To-Do Bar, and the set changed in the Themes window.
  View > To-Do Bar shows any of three parts at the right of the module shown, and each module
  keeps its own, as Outlook does: Calendar, a month to choose a day from (a click shows that day
  in the Calendar) over the appointments of today and the six days after that have not ended,
  under day headings, the one under way marked; People, favourite people by their contacts'
  names, a click starting a message to one, added by name or address and taken off with the ×;
  Tasks, the To-Do List under Overdue, Today, Tomorrow, This Week, Next Week, Later and No Date,
  a box to tick each off and a line to add one, due today unless "tomorrow" or a date ends it.
  An appointment or a task double-clicked opens in its window from any module. What it shows is
  read again a minute on, or a second after anything is done, never on every frame
  (`Session::todo_bar_due`); the parts and the favourites are kept with the settings
  (`toDoBar`, `favorites`), and `todo.bar` gives the whole of it to the CLI and agents with the
  message-derived text under `untrusted_content`. Better than Outlook: weeks on the To-Do List
  and its headings begin on the region's first day, not always Monday (the Tasks module now
  does too); the add line takes a due day as it is typed. Hostile input: at most 60
  appointments, 300 tasks and 100 favourites, addresses of at most 320 characters with nothing
  that could break an address line, and a settings file's junk costs only itself.
  Also fixed: in the Themes window, choosing the dark set changed it without showing it, and
  Show > Dark left the light set being changed; the set being changed is now always the one
  shown. Do later: tasks dragged to the Calendar, a favourite's recent mail. 8 tests.
- **2026-10-28** - M5.12: an invitation answered with a comment, or without telling.
  Accept, Tentative and Decline above an invitation each open a menu on a click, as Outlook's
  do: Edit the Response before Sending, Send the Response Now, or Do Not Send a Response. The
  first opens a window with a few words to the organizer, sent in the answer's text and its
  COMMENT (`ui.answerInvitation`, then `calendar.respond`). Better than Outlook: the answer can
  still be changed in that window, and whether it is sent is a tick there instead of a choice
  made before it; an answer is given only to the invitation it was written for, known by its
  Message-ID, date and the meeting's UID and sequence, so one opened meanwhile is never
  answered in its place (`calendar.invitation` now gives the UID and sequence, under
  `untrusted_content`). Also fixed: right-click and click shared one menu's place in egui, so
  the right-click menu shut the other on the click that opened it. 2 tests.
- **2026-10-28** - M1.9: one Options window for every setting, and the File tab in pages.
  Account Settings, Themes, Time Zones and Check for Updates were four windows, and other
  settings had no window at all; now Options (File > Options, `ui.options`) holds them all, a
  page each with the pages listed at its side: General (simplified or classic ribbon, plain
  text, animations), Mail (Send/Receive every so many minutes, new mail as it comes, the alert,
  sound and taskbar flash), Calendar (the time zone and a second one), Reminders, Saving
  (autosave, saving in place, keeping the file as it was), Accounts, Themes and Updates. Every
  change goes through its command and is kept at once, so there is no OK to forget. The old
  ways in (`ui.accountSettings`, `ui.themes`, `ui.timeZones`, `ui.updates` and their ribbon
  buttons) open Options at their page. The File tab is in pages as Outlook's is: Info shows the
  data files open, where each is, its folders and whether it has unsaved changes, with Save
  Changes, Close Data File, Account Settings and Options beside them; Open & Export and Save
  each offer what they do as a tile with a line saying what it does, shown dimmed when it
  cannot be done now; then Print, Options and Exit. The window keeps one size from page to
  page, and the accounts form scrolls inside it with Save and Test always in sight. 3 tests.
- **2026-10-28** - M5.12: themes saved to a file of their own and opened from one.
  Options > Themes has Open Theme File and Save to a File (`ui.openTheme`, `ui.saveTheme` and
  their `…Dialog` forms). A theme file (`.omtheme`) is JSON that says what it is, with the
  theme's name and what it changes in the light and the dark set; it is written beside its
  place and put there whole. Outlook has no theme files at all. Better than a plain copy: a
  file opened twice chooses the theme it added instead of adding it again, one named as a
  theme already there comes in under a free name, a built-in theme can be saved to start
  another machine's own from, and colours a file names that a theme does not have are left
  out and counted, so a file from a later version still opens. Hostile input: at most 64 KB
  read, a file that is not JSON or does not say it is a theme refused, names cut to one short
  line. 2 tests.
- **2026-10-28** - M5.12: a contact's picture and its details in the contact window.
  The window's head shows the contact's face as its card does, with Add Picture, Change
  Picture, Remove Picture and Undo (`ui.contactPicture`, `ui.contactPictureDialog`); a
  Details part beside the addresses holds Office, Profession, Manager's Name, Assistant's
  Name, Nickname and Spouse/Partner, which `people.change` already kept. Better than
  Outlook: the picture chosen is read and shown at once, before it is saved, and Undo takes
  the window back to the one it had; the details sit with the rest instead of on a page of
  their own. Hostile input: the file is checked when it is chosen as the engine checks it
  (a folder, more than 4 MB, or what is not a JPEG, PNG, GIF or BMP refused), and a refused
  file leaves the picture as it was. 1 test.
- **2026-10-30** - M5.12: Mail Merge printed as letters and saved as one document, and
  contacts chosen by a filter. `people.mailMerge` takes `"deliver": "letters"`: one letter for
  each contact chosen, each starting on a page of its own, saved as one PDF document
  (`"path"`) or printed on a printer of the system's (`"printer"`, through the same spooler as
  file.print), and `"preview": true` shows the first three without laying anything out. A
  `"filter"` of up to 20 conditions on any field (is, is not, contains, does not contain,
  starts with, is empty, is not empty), all of them or any one, chooses who is merged, as
  Word's Filter Records does. Two new fields, Address Block and Greeting Line, leave out what
  a contact lacks. The Mail Merge window has the conditions, a Letters choice, Save as PDF and
  Print. Better than Outlook, which hands a merge to Word: letters go to everyone chosen,
  those without a postal address named instead of passed over, how many the filter left out is
  said while the text is written, and a greeting falls back from title and surname to the
  first name to "Sir or Madam" by itself. Hostile input: conditions and their values bounded,
  an unknown field or comparison refused by name, at most 2,000 letters (a page each at
  least). 2 tests.
- **2026-10-30** - M5.12: Tasks dragged onto the Calendar, appointment notes, and what a
  favourite last sent. A task or flagged message dragged from the To-Do Bar's list and let go
  on the Calendar's hours is an appointment from that half hour, on a day's band or a day of
  the month an all-day event, `calendar.fromTask` (`"start"`, and `"minutes"` if wanted); the
  place it would go is lit while it is held, and the task stays. Appointments now have notes,
  made, changed and read with them (`"notes"` on calendar.new and calendar.change, and
  `calendar.notes`, its text marked as untrusted content), in a Notes box in the appointment's
  window; one occurrence shows its series' notes. Each favourite in the To-Do Bar's People
  shows the last message they sent that an Inbox holds and how many of theirs are not read,
  `"lastHeard"` in todo.bar; a click shows it in Mail, `mail.showMessage`. Better than
  Outlook: the appointment is as long as the work the task is thought to take, its notes name
  the task and the day it is due before the task's own notes, someone else using a favourite's
  name is not taken for them (the message's own sender is looked at), and the unread count is
  beside the last subject. Presence itself needs a chat or presence service and has its own
  row. Hostile input: the length bounded to two weeks, notes capped and their control
  characters dropped, the messages looked at and opened bounded. 3 tests.
- **2026-10-30** - M5.12: Reminders above every other window, and a sound of the owner's. A
  reminder that comes due while the main window is minimized or behind another comes up in a
  Reminders window of its own, above every other window, and goes back into the main window
  once that is in front again. The sound played for a reminder is our own chime or a WAVE file
  the owner chooses, `"soundFile"` on reminders.options, kept with the settings, chosen with
  Browse in Options > Reminders and heard there with Play. WAVE files are read by a reader of
  our own, `crates/formats/src/wav.rs`: integer PCM of 8 to 32 bits and 32- and 64-bit floats,
  the extensible header, up to eight channels. Better than Outlook: the file is made sure of
  as it is chosen, so a bad one is refused then and not found out when a reminder is missed, a
  file gone since is told of in the log and the chime played in its place, and a sound is cut
  at 30 seconds. Hostile input: every chunk length checked against the file, at most 32 MB
  read and 256 chunks looked at, a float that is not a number played as silence. 4 tests.
- **2026-10-30** - M5.12: a reminder of its own on one occurrence changed alone. An occurrence
  changed on its own keeps a reminder of its own from its ExceptionInfo ([MS-OXOCAL]
  2.2.1.44.2): set where the series has none, taken away where it has one, or so many minutes
  before; a series with none is still looked at for them, the lookahead reaches the longest of
  them, and the last dismissed puts them away with it. `crates/model/src/reminder.rs`. What is
  left, the Reminders search folder written into a file and the checks against Outlook, is its
  own row.
- **2026-10-30** - M5.12: zones whose rules changed over the years, and zones that follow
  another calendar. A zone keeps the rules it kept before with the last year of each
  (`Zone::earlier`, `Era`), and each moment is read in its own year's rules: the United
  States' and Canada's of 1987 to 2006, Europe's of 1981 to 1995, south-east Australia's, New
  Zealand's, Mexico's, Turkey's and Russia's. A change may fall on the first weekday on or
  after a day of the month (Israel's, now known), or follow Ramadan by the tabular Islamic
  calendar (Morocco's, now known). A TZDEFINITION is written with a rule for each year the
  rules changed, and those that move one for each year, and all of them are read back; a
  VTIMEZONE gives each era ending with UNTIL and moving days as RDATE, and a known TZID is
  known again by the file's own offsets. `crates/model/src/zone.rs` and `write_eras` in
  `crates/formats/src/ics.rs`.
- **2026-10-30** - M5.12: signatures, chosen for each account and brought from Outlook, with
  POP3 leave-a-copy and Outlook 2010 profiles. Signatures are made, changed, renamed and
  deleted on a page of their own in Options and kept with the settings
  (`crates/engine/src/signatures.rs`, `cmd/signatures.rs`, `crates/ui-
  egui/src/signatures.rs`). Each account chooses one for new messages and one for replies and
  forwards, or takes the choice every account takes, or chooses none. What is chosen is put in
  as a message is started: at the foot of a new one, above the quoted text of a reply or
  forward. When the account a message goes from is changed, the signature changes with it,
  unless one was chosen by hand from the compose window's Signature menu
  (`compose.signature`); a signature put in is taken out whole when another takes its place,
  as long as it was not written over. Going beyond Outlook: {name} and {email} in a signature
  are filled with those of the account a message goes from, so one signature serves every
  account. Import from Outlook brings Outlook's signatures (the .txt words in UTF-16, else the
  words of the .htm) and each account's choice of them for new messages and replies. A name
  already used here with other words is brought under a name of its own. Outlook's files are
  only read. A POP3 account's "leave a copy on the server" setting is now read from its
  profile, and Outlook 2010's profiles (kept under Windows Messaging Subsystem) are found too.
  The compose buttons wrap to a second line where the pane is narrow. Do later: rules and the
  AutoComplete list from Outlook; signatures with formatting and pictures once the compose
  body is formatted. 5 tests.
- **2026-10-30** - M5.13: rules, run on mail as it comes in and on a folder by Run Rules Now.
  Rules are made, changed, turned on and off, put in order and deleted on a Rules page of
  Options, opened from the Rules menu on the Home tab, and kept with the settings
  (`crates/engine/src/rules.rs`, `cmd/rules.rs`, `crates/ui-egui/src/rules.rs`). A rule has
  conditions, exceptions and actions: from, sent to, words in the subject, body or header, my
  address in To or Cc, sent only to me, an attachment, an importance, a size, a meeting's
  message; moved or copied to a folder, put in Deleted Items, marked read, flagged or its flag
  cleared, put in a category, given an importance, and no more rules after it. A rule may be
  for one account alone. They are tried in turn, as Outlook tries them, on mail that comes
  into an Inbox (not on what a first receive finds on the server already), and what was moved
  goes to the IMAP server on the next Send/Receive; mail a rule marks read or deletes is not
  told of as new. Create Rule starts one from the selected message. Run Rules Now runs them on
  the current folder, with its subfolders where asked, chosen rules or all those on. Going
  beyond Outlook: Run Rules Now looks first and says what each message would get, changing
  nothing until Run; and a rule never deletes for good, only into Deleted Items, so a rule
  that catches too much loses nothing. A folder a rule names that is gone is told of, and the
  rest of what the rule says is still done. Do later: rules kept in the data file as Outlook
  keeps them and brought from Outlook; Run Rules Now with Undo; more of Outlook's conditions
  and actions. 5 tests.
- **2026-10-30** - M5.14: the AutoComplete list, added to by each send and brought from
  Outlook. Each message sent adds those it goes to to the AutoComplete list, and names written
  to before are suggested first as an address is typed, the most used at the top, before the
  contacts and never twice (`crates/engine/src/autocomplete.rs`, `people.suggest`). One is
  taken off it by the × beside it or the Delete key, and all of them by Empty Auto-Complete
  List in Options > Mail; the list is kept with the settings, at most 2000 names, the least
  used going first. Outlook's own list is read from the hidden
  `IPM.Configuration.Autocomplete` message in a data file's Inbox, its stream as [MS-OXOCFG]
  2.2.4.1 lays it out, the data file left as it is: Import from Outlook does this for each
  data file it brings, and Import From Data Files on the same page for each one open
  (`people.bringAutoComplete`). Going beyond Outlook: the list can be looked through
  (`people.autoComplete`), a name brought keeps the one already here, and an Exchange entry is
  taken by its SMTP address. Rules from Outlook are left as a row of their own: [MS-OXORULE]
  keeps their stream opaque. 5 tests.
- **2026-10-30** - M5.15: Run Rules Now taken back with Undo, and more conditions and actions.
  Run Rules Now is one step of Undo (Ctrl+Z): every message it changed or moved is put back
  where it was, as it was, and the copies it made are taken away; a message changed or gone
  since is told of and left (`Step::Ruled` in `crates/engine/src/undo.rs`). Rules have more
  conditions: words in the sender's address or a recipient's address alone, not their names,
  and received on or after one day and on or before another, in the calendar's time zone. And
  more actions: forward to addresses, reply to the sender with words of one's own, and play
  the new-mail sound whatever the new-mail options say. Going beyond Outlook: a forward or
  reply goes into the Outbox and is sent with the next Send/Receive, so it can be looked at
  and taken back first; a rule never answers or forwards mail a machine wrote (Auto-Submitted,
  Precedence bulk or list, a list's headers, a delivery report, a no-reply address, [RFC
  3834]); it answers a sender once a run and forwards at most 100 in an hour; nothing goes
  back to one of my own addresses; and what a rule writes is marked Auto-Submitted with
  X-Auto-Response-Suppress, so two people's rules cannot answer each other without end
  (`rules::automatic` and `rules::Answered`). Do later: rules on mail as it is sent. 4 tests.
- **2026-10-30** - M5.16: rules on mail as it is sent. A rule is run after the message arrives
  or after I send the message, as in Outlook's Rules Wizard (`Rule::sent` in
  `crates/engine/src/rules.rs`). A rule on mail as it is sent defers delivery by 1 to 120
  minutes as the message goes into the Outbox, unless its writer held it already, and the
  status says which rule holds it (`rules::as_it_goes` in `cmd/rules.rs`). Once what went is
  kept in Sent Items, it copies it to another folder, puts it in a category, flags it or marks
  its importance (`rules::after_sent`, called from `cmd/send.rs`). What went stays in Sent
  Items: a rule on mail as it is sent cannot move, delete, mark read, forward or answer it,
  and the editor offers only what it can do. Run Rules Now runs these rules on Sent Items and
  the others elsewhere, unless rules are named. Going beyond Outlook: a category put on the
  copy in Sent Items is not sent to the recipients. 1 test.
- **2026-10-30** - M5.17: a task's recurrence made and changed in its window, and the next
  occurrence on completing. A task recurs daily, weekly, monthly or yearly, set and changed in
  its window's Recurrence rows as an appointment's are, and kept as Outlook keeps it:
  PidLidTaskRecurring and a RecurrencePattern alone in PidLidTaskRecurrence ([MS-OXOTASK]
  2.2.2.2.15, `Recurrence::read_pattern` and `write_pattern` in `crates/model/src/recur.rs`);
  a pattern that cannot be read is kept as it was. Mark Complete on a recurring task keeps the
  occurrence done as a completed copy that no longer recurs (PidLidTaskDeadOccurrence) and
  moves the task on to its next occurrence, its start and reminder as far before it as they
  were, both taken back as one step of Undo (`task::complete_occurrence`,
  `tasks.markComplete`); the last of a series is completed as any task is. A task that
  regenerates comes again a period after the day it was done (SlidingFlag). Skip Occurrence
  moves it on without keeping one as done (`tasks.skipOccurrence`), and `tasks.recurrence`
  sets or ends a pattern. Going beyond Outlook: the reminder moves with the task, and
  `tasks.items` gives each pattern in words. 3 tests.
- **2026-10-30** - M5.18: Assign Task: task requests, answers and updates by mail, and TNEF. A
  task is given to someone by mail as Outlook gives one ([MS-OXOTASK] 2.2.4, 3.1.4): Assign
  Task in the task window names who it is for, and the request goes as IPM.TaskRequest with
  the task carried as its first, hidden attachment, inside a winmail.dat ([MS-OXTNEF]) Outlook
  reads, and a body anyone can (`crates/mime/src/tnef.rs`, read and written;
  `crates/model/src/task/request.rs`; `tasks.assign`). Who gave it is kept in
  PidLidTaskAssigners as Outlook writes it, and the copy kept here waits for an answer. The
  one it was given to sees Accept and Decline in the reading pane (`tasks.request`,
  `tasks.respond`): accepted, it is in their Tasks and the answer goes back; declined, it is
  the giver's own again and may be given once more. A task given to several is theirs to share
  and is not answered. Each change the one it was given to makes goes back as an update by
  itself, and its completion as a report, and the giver's copy follows, an answer or update
  taken only from the one it was given to and never one older than what is kept
  (PidLidTaskVersion). Send Status Report sends where a task stands
  (`tasks.sendStatusReport`). Going beyond Outlook: a forged answer is not believed, and what
  an answer did is told on receiving. 8 tests.
- **2026-10-30** - M5.19: several messages selected at once. Messages are selected together as
  in Outlook: Ctrl and a click adds one or takes it away, Shift and a click or an arrow
  selects every row from where the selection began, and Ctrl+A selects the whole list
  (`mail.select` with "add" or "through", `mail.next` and `mail.previous` with "extend",
  `mail.selectAll`; `Session::also`, `chosen`, `toggle_chosen` and `choose_through`). The last
  clicked is shown in the reading pane, and only it is marked read. Mark as Read and Unread,
  Follow Up, Clear Flag, Mark Complete, Categorize, Importance, Delete, Archive, Junk and Move
  are done to each one selected, and taken back by one Undo, which selects them together again
  (`undo::Step::Many`); a toggle goes the way the one shown goes, as in Outlook. Moved or
  deleted, the selection goes on to the row after the last of them. Table Style's selected
  rows print every message selected. Going beyond Outlook: the status bar says how many are
  selected, and one message that cannot be changed leaves the rest changed and says so. 3
  tests.
- **2026-10-30** - M5.20: search folders worked out again after a save. A save now tells the
  data file's search folders what it changed, in the way [MS-PST] 2.4.8.4.1 asks: each folder
  made, moved, renamed or deleted, and each message changed, put in, moved or taken out, goes
  into the Search Management Queue (node 0x1E1) as a Search Update Descriptor of twenty bytes
  (2.4.8.1.1, 2.4.8.2), behind what already waits there, and Outlook works out again which
  searches each message belongs to when it next opens the file. The criteria of a search are
  not to be read or changed (2.4.8.6.4), so this is how a search folder is kept in step, and a
  change left out of the queue leaves the search out of step. Outlook's own files were looked
  at first, and their entries are laid out the same way, with flags 0. A file of ours, changed
  and saved into where it is, was opened in Outlook Classic: it showed every folder and
  message, and took the two entries off the queue (`crates/pst/src/search_queue.rs`,
  `PstStore::updates` and `queue` in `edit.rs`). Going beyond what is asked: entries already
  taken off the front are dropped, so the queue does not grow with every save, and a queue
  that may not be read is left as it is, with nothing added. 4 tests.
- **2026-10-30** - M5.21: the message list's columns chosen, moved and kept. When the message
  list is wide enough (the reading pane at the bottom or off), it is shown as Outlook shows it
  then: one line to a message, under headings of its columns. A click on a heading arranges
  the list by that column, a second click the other way, and an arrow marks it. A right-click
  on the headings shows every column with a tick to show or hide it, Move Left, Move Right and
  Reset Columns (`view.columns`, `headings`, `spans` and `draw_line` in `crates/ui-
  egui/src/panes.rs`). The columns are kept between runs (`listColumns` in the settings), and
  a list of them that makes no sense leaves the list as it was. Going beyond what is asked:
  Table Style prints the same columns as the list shows, so what is printed is what is seen,
  and the last column cannot be taken away, so the list is never left without one. 2 tests.
