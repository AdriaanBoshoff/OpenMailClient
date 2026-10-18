# Gaps

**Status:** revision 110 · 2026-10-18 · ranked: the top of the list is what hurts most.

## Not built

0. **No sign-in to Gmail, Outlook.com, Microsoft 365 or Yahoo.** They refuse a password over IMAP
   and SMTP (or ask for an app password); they want OAuth 2.0 (SASL XOAUTH2 or OAUTHBEARER,
   RFC 7628). This is M7, put after the first release by the owner's choice: sign-in through the
   system's browser with PKCE, tokens kept and renewed in the system's credential store. Until
   then such accounts sign in with an app password, and Find settings says so.
1. **IMAP is kept in step by message, not by folder, and there is no Exchange.** Accounts are kept
   and tested (M4.3), mail is sent (M4.4) and received by POP3 and from the IMAP Inbox (M4.5);
   every folder of an IMAP server is taken down into the account's own data file, a copy of what
   is sent is put in the server's Sent folder, and read, flagged, moved and deleted messages go
   both ways (M4.8); only what changed is fetched where the server keeps mod-sequences (CONDSTORE,
   QRESYNC, M4.12); new mail is told of as it comes (IDLE, M4.12); folders renamed, moved and deleted are followed both ways (M4.12); drafts are kept on the server as they are written (M4.12);
   accounts are set up in the window's Account Settings (M4.6), their servers found from the address and its domain's DNS records (M4.14, M4.15), and each keeps its mail in a
   data file of its own (M4.7).
2. **Changes to a data file are kept only for a Unicode or ANSI `.pst`.** `file.save`
   (M3.4) writes what changed into the file where it is, with what it writes over kept in an undo file beside it until the save is checked and read back (M3b.29), or, for a first save that keeps the file as it was and for a file with much unused space, rewrites the whole file beside itself and swaps it in; what we write, saved whole or into the file, opens in
   Outlook Classic with every folder and message as we wrote them (M3c.1), files from before M3c.1 too after one save (M3c.5), but no Outlook-written `.pst` has been saved into. Unsaved changes are journalled beside the file as they are made (M3b.1) and come back
   after a run that ended without saving; a power cut may cost the last second of them (M3b.7), and a folder that cannot be written no longer means no journal. An ANSI `.pst` takes changes and is saved as an ANSI file (M3b.21), and a 4K-page `.pst` as a 4K-page file, written whole (M3c.2); an `.ost` is read-only. Once a file has been saved, later changes are saved on their own after five minutes (M3c.3, `file.autoSave`). `file.export` copies a
   Unicode `.pst` as it is stored (M3b.2); from any other source it goes through the model, which
   carries hidden (associated) items and folder properties (M3c.6; named properties of a folder
   and search folders are not carried then). Changes
   made in the sample mailbox live in memory and are lost on exit unless exported.
3. **Formatted bodies are partly drawn.** Every "not yet" below has a slot in
   [ROADMAP.md](../ROADMAP.md#do-later-from-m2). HTML bodies show text styles, colours, headings, lists,
   quotations, rules and tables, and pictures the message carries as attachments (`cid:`) or
   writes into itself (`data:`; PNG, JPEG, GIF, BMP, WebP, TIFF and SVG; one that moves is played, M3b.23) at the size the body asks for, in
   the line of text or at its side (`align`) with what follows beside them until they end or a
   break clears them. Text is drawn in the fonts the message names when they are installed, in
   their own bold and italic faces, with open fonts of the same measures standing in; no font is
   bundled. Borders set in styles are drawn round paragraphs, tables and cells, and cells span
   rows as well as columns. Several pictures stand at the sides of one paragraph, and a dark see-through picture has
   paper behind it on the dark pane (M3b.24). Boxes stand inside one another, a table stands
   inside the block it was written in, and a body has its colour behind all of it (M3b.25).
   Text within a line is drawn in its border, and boxes and cells have rounded corners
   (M3b.26). Not yet: a border round right-to-left text in the window; a radius for each
   corner. Style sheets
   are read as far as Outlook follows them (rules by element name, first class and id); tables keep
   their width, place, cell fills, padding, spacing and the vertical place of what cells hold; a
   picture that is not loaded leaves a frame of the size it asks for ([ui-parity.md](ui-parity.md),
   Message bodies). RTF bodies
   ([MS-OXRTFCP], and the HTML or text they wrap, [MS-OXRTFEX]) are decoded and drawn with character
   styles, fonts by name, colours, alignment, indents, links, pictures written into the body as hex digits or as bytes (PNG,
   JPEG, bitmaps, metafiles that hold only one bitmap, and metafiles of either kind that are drawings of
   lines, curves, filled shapes and text in an installed font, [MS-WMF] 2.3.3 and [MS-EMF] 2.3.5), attachments in their places in the text
   (`\objattph`, [MS-OXRTFEX] 2.2.3.4: an attached object by the best picture it has, [MS-OLEDS]
   2.3.4 and 2.3.6, any other as a card with its name and size that a click saves),
   paragraph borders and the room paragraphs ask for above and below,
   and tables with each cell's own lines, fill, place and merge down and across, and tables inside their cells; a metafile draws a part of a bitmap, one mirrored, turned or packed in runs, every way of mixing, hatched and patterned brushes and regions (M3b.28); not yet in a metafile: copies with a mask or see-through ones, pens that mix, palette colours, or text that needs shaping
   (a placeholder). List numbers follow what an override changes (`\lfolevel`) and a highlight is one of the sixteen marker colours (M3b.27). OpenMailClient never writes compressed RTF
   (only the uncompressed stored form). Printing (`file.print`) writes a PDF file in Memo Style, with
   text, lists, borders and tables; pictures the message carries are drawn, in the line or at the side with text beside them; pairs of glyphs are kerned; not yet: pictures on a server (on screen they are downloaded when asked for, M4.10; on paper their place is named),
   right-to-left text in the body of a message being written (its one-line fields are done, M3b.8) (see ROADMAP, Do later), several selected rows in Table Style (it prints chosen columns, group headings and the one selected row, M3b.22), other print styles, a printer's own settings (trays, two-sided; it prints on any printer the system has, with copies and pages, M1.10),
   a zoom in the print preview (`file.printPreview`, and the print window), and any comparison with what Outlook prints.
4. **People and Tasks** modules show a placeholder. The Calendar has its views, appointments (M5.1) and recurring appointments with occurrences changed or deleted alone (M5.3) and calendar files opened, imported and saved (M5.4); CalDAV sync, time zones and meetings are scheduled in ROADMAP's M5 plan.
5. **MCP server and control channel** (`openmailclient-cli mcp`, `--control`) are not built.
6. **Web build.** The engine and everything under it compiles for wasm32; there is no web app.

## Built but unproven

7. **`.pst` reader has met few real files.** It is written from [MS-PST] and tested against files
   produced by a test-only builder, including truncated, corrupted and cyclic ones. Checked locally
   against a few small Outlook-written 4K-page `.ost` files (wVer 36): every folder and every item
   loaded. No Outlook-written Unicode or ANSI `.pst`, and no large mailbox, has been tried yet.
8. **4K-page files in tests are our own.** The real files used for the check above are private and
   stay local. The writer makes 4K-page files (M3c.2) and `crates/pst/src/tests_4k.rs` writes, reads,
   changes and checks whole mailboxes in them, one of 1,500 messages among them; Outlook opens such a
   file. None of them has a heap of more than one block written by Outlook to compare with.
9. **`.msg`/`.oft` output is checked in Outlook Classic only for plain mail.** Outlook opens our
   messages and templates and reads back subject, sender, recipients, times, flags, categories, bodies
   and a file attachment ([file-format-parity.md](file-format-parity.md)). Attached messages, inline
   pictures, non-mail items and opening from the Outlook window are unchecked.
10. **The UI differs from Outlook Classic in measured ways.** A first side-by-side look is in
    [ui-parity.md](ui-parity.md): no Favorites, no ribbon overflow or command search.

## Known shortcomings

11. Special folders in a `.pst` are found by English name when the store does not identify them, so a
    localised Inbox may sort as an ordinary folder.
12. Message-list rows from a `.pst` have no preview line.
13. Date groups are Today, Yesterday, This Week, Last Week, This Month, Older; Outlook's are finer.
14. One message selected at a time; no multi-select, no conversations, no drag and drop.
15. Keyboard shortcuts cover the common commands only; there is no command search.
16. Password-protected and encrypted data files are refused or opened without the password check.
17. No localisation; times use a 12-hour clock and month/day/year order.
18. A long formatted body is laid out in full on every repaint; nothing skips the part off screen.

## Revision history

- 110 · 2026-10-18 — M5.4: iCalendar files opened, imported and saved.
- 109 · 2026-10-18 — M5.3: recurring appointments, one occurrence changed or deleted alone.
- 108 · 2026-10-17 — M1.10: printing on a printer with copies and pages; the message body alone.
- 107 · 2026-10-17 — M5.2: Import from Outlook: accounts and their data files.
- 106 · 2026-10-17 — M1.9: dialogs in the theme's colours; themes of your own for light and dark.
- 105 · 2026-10-17 — M5.1: the Calendar: views and appointments made, changed and deleted.
- 104 · 2026-10-17 — M7 (OAuth sign-in) put after the first release; M5 next.
- 103 · 2026-10-17 — M4.15: an account's servers found from its domain's DNS records; M4 closed.
- 102 · 2026-10-17 — M4.14: an account's servers found from its address.
- 101 · 2026-10-17 — M4.13: pictures decoded off the thread that draws the window.
- 100 · 2026-10-17 — M4.12: the places of messages read apart from the window.
- 99 · 2026-10-17 — M4.12: Drafts on the server.
- 98 · 2026-10-17 — M4.12: IMAP folders renamed, moved and deleted followed both ways.
- 97 · 2026-10-17 — M1.8: shown as Outlook shows it; third-party sign-in (OAuth 2.0) is the last milestone, M7.
- 96 · 2026-10-15 — M4.12: new mail told of as it comes (IDLE).
- 95 · 2026-10-15 — M4.12: only what changed fetched from an IMAP server (CONDSTORE, QRESYNC).
- 94 · 2026-10-15 — M4.9: Delay Delivery and Expires After.
- 93 · 2026-10-15 — M4.9: Show Progress.
- 92 · 2026-10-15 — M4.9: new mail told of; Send/Receive on a timer.
- 91 · 2026-10-15 — M4.9: a server's own certificate trusted on the owner's word; larger buttons.
- 90 · 2026-10-15 — M4.8: IMAP kept in step both ways; a flag kept in the list of a folder written empty.
- 89 · 2026-10-15 — M4.8: IMAP folders taken down as the server has them, the Sent copy put on the server.
- 88 · 2026-10-15 — M4.11: attachments opened or saved on a double-click, a program or script only saved.
- 87 · 2026-10-15 — M4.10: pictures on a server downloaded when asked for, over HTTPS only.
- 86 · 2026-10-14 — M4.7: HTML messages laid out as Outlook lays them out (centring, padding, fills, rounded tables, picture boxes).
- 85 · 2026-10-14 — M4.7: each account keeps its mail in a data file of its own; Account Settings laid out anew.
- 84 · 2026-10-14 — M4.6: accounts are added, changed and tested in the window's Account Settings.
- 83 · 2026-10-14 — M4.5: mail is received by POP3 and from the IMAP Inbox into the data file's Inbox.
- 82 · 2026-10-14 — M4.4: mail is sent through an account's SMTP server and kept in Sent Items.
- 81 · 2026-10-14 — M4.3: accounts are kept, changed and tested in the engine; passwords in the credential store.
- 80 · 2026-10-14 — M4.2: TCP and TLS connections with checked certificates, and passwords in the system's credential store.
- 79 · 2026-10-14 — M4.1: the `net` crate speaks IMAP, POP3 and SMTP against an in-process fake server.
- 78 · 2026-10-14 — M3c.7: larger data blocks of a 4K-page file are written deflated, as Outlook writes them; M3c is closed.
- 77 · 2026-10-14 — M3c.6: an `.ost` is exported with its hidden items and folder properties, and Outlook opens the export; a file that lacks what Outlook asks for is written anew with its hidden items.
- 76 · 2026-10-14 — M3c.5: a file from before M3c.1 is written whole on its next save, keeping its ids, and Outlook opens it.
- 75 · 2026-10-14 — M3c.4: a message changed by a save has as its size what is now stored of it.
- 74 · 2026-10-14 — M3c.3: unsaved changes are saved on their own after five minutes, never for a file's first save.
- 73 · 2026-10-14 — M3c.2: data files with 4K pages are written, take changes and are saved as that kind; Outlook Classic opens one of ours.
- 72 · 2026-10-14 — M3c.1: Outlook Classic opens the data files our writer makes, whole, ANSI and saved into in place, and sees what we wrote.
- 71 · 2026-10-13 — M3b.29: a save writes what changed into the data file where it is, with an undo file beside it until the save is checked; M3b is closed.
- 70 · 2026-10-13 — M3b.28: metafiles draw parts of bitmaps, mirrored, turned and run-packed ones, every way of mixing, hatched and patterned brushes, and regions.
- 69 · 2026-10-13 — M3b.27: an RTF highlight is one of the sixteen marker colours, and list numbers follow what their override changes.
- 68 · 2026-10-13 — M3b.26: text within a line is drawn in its border, and boxes and cells have rounded corners, in the window, on paper and in the PDF.
- 67 · 2026-10-13 — M3b.25: boxes stand inside one another, a table stands inside its block, and a body has its colour behind it, in the window and on paper.
- 66 · 2026-10-13 — M3b.24: several pictures stand at the sides of one paragraph, in the window and on paper; a dark see-through picture has paper behind it on the dark pane.
- 65 · 2026-10-13 — M3b.23: SVG, WebP and TIFF pictures are shown; animated GIF, PNG and WebP are played (`view.animations`).
- 64 · 2026-10-13 — M3b.22: Table Style prints chosen columns, group headings and the selected message; the print preview has a zoom.
- 63 · 2026-10-13 — M3b.21: ANSI data files are written (`file.export` with `"kind": "ansi"`), take changes and stay ANSI when saved; 4K-page files stay read-only.
- 62 · 2026-10-13 — M3b.20: the structure check covers ANSI files (from the specification) and 4K-page files (from measurement); three local Outlook-written 4K-page files pass it.
- 61 · 2026-10-13 — M3b.19: `file.export` with `"wait": false` runs on a thread of its own for every kind of mailbox, the writing from what is read included.
- 60 · 2026-10-13 — M3b.18: values of another type than their column's are shown as the column holds them; the structure check hears Stop and compares fill levels.
- 59 · 2026-10-13 — M3b.17: a file attached again with other contents keeps its stored properties; attachments that cannot be read are carried through a save.
- 58 · 2026-10-13 — M3b.16: 32-bit floats, currency, times as days and error codes keep their kind; a saved file goes on from the header's counters; `file.keepOriginal`.
- 57 · 2026-10-13 — M3b.15: an exported file names the folders of the other modules and the drafts by id; the reader knows a Drafts folder by that id.
- 56 · 2026-10-13 — M3b.14: `file.export` of a Unicode `.pst` runs on a thread of its own, with progress and Stop; the mailbox stays in use meanwhile.
- 55 · 2026-10-13 — M3b.13: the structure check reaches into heaps, indexes, property lists and tables; a save is stopped only by what it got wrong itself.
- 54 · 2026-10-13 — M3b.12: a save from the window runs on a thread of its own, with progress and a way to stop it; a save that writes only what changed is still to do.
- 53 · 2026-10-13 — M3b.11: attachments keep what is stored on them when the list changes; search-folder rows of changed messages are brought up to date.
- 52 · 2026-10-13 — M3b.10: text copied out of a printed PDF was tried in one reader; right-to-left text is given as it is seen and only ligatures and clusters say what they stand for.
- 51 · 2026-10-12 — M3b.9: tab stops, leaders, a border's indent, text beside a picture and table columns follow a paragraph written from the right; `<bdo>`, `<bdi>` and `unicode-bidi` are read.
- 50 · 2026-10-12 — M3b.8: fields of one line are our own; right-to-left text reads right, and the cursor follows it, while it is typed.
- 49 · 2026-10-12 — M3b.7: the journal is kept elsewhere when the folder cannot be written, forced to disk once a second, and counts what it lost exactly.
- 48 · 2026-10-12 — M3b.6: a right-to-left paragraph's indent, list marker and quotation bars stand at the right.
- 47 · 2026-10-12 — M3b.5: the head and foot of a printed page and list markers are shaped and turned as the body is.
- 46 · 2026-10-12 — M3b.4: right-to-left text in the list, the header and folder names; turning marks left out of one-line text.
- 45 · 2026-10-12 — M3b.3: pane edges are dragged; window size and place and pane sizes are kept between runs.
- 44 · 2026-10-12 — M3b.2: `file.export` copies a Unicode `.pst` as it is stored (hidden items, folder properties, unknown properties).
- 43 · 2026-10-12 — M3b.1: unsaved changes to a Unicode `.pst` are journalled beside the file and come back after a crash.
- 42 · 2026-10-12 — M3.5: settings (page setup, panes, theme, open data files) are kept between runs; M3 closed.
- 41 · 2026-10-12 — M3.4: changes to an open Unicode `.pst` are kept by `file.save`; `.ost`, ANSI and 4K-page files stay read-only.
- 40 · 2026-10-12 — M3.3: a mailbox is saved as a new `.pst` (`file.export`); changing an open `.pst` is M3.4.
- 39 · 2026-10-12 — property lists and tables of a `.pst` are written (item 2).
- 38 · 2026-10-12 — the node database of a `.pst` is written (item 2); a structure check exists (item 7).
- 37 · 2026-10-12 — shaping from the font's own tables in print (item 1).
- 36 · 2026-10-12 — right-to-left text and joined Arabic letters (item 1).
- 35 · 2026-10-12 — what item 3 leaves is scheduled in ROADMAP.md, "Do later (from M2)".
- 34 · 2026-10-12 — M2 closed; item 3 is what it leaves.
- 33 · 2026-10-12 — bitmaps beside a metafile's drawing (item 1).
- 32 · 2026-10-12 — Table Style (item 3).
- 31 · 2026-10-12 — print preview (item 3).
- 30 · 2026-10-12 — page setup for print (item 3).
- 29 · 2026-10-12 — text beside side pictures and kerning in print (item 3).
- 28 · 2026-10-12 — opening a link after a question (item 3).
- 27 · 2026-10-12 — fills behind blocks and padding on each side (item 3).
- 26 · 2026-10-12 — RTF list numbers from the list tables (item 3).
- 25 · 2026-10-12 — RTF tab leaders and the line between paragraphs in one border (item 3).
- 24 · 2026-10-12 — Line spacing in RTF and HTML bodies (item 3).
- 23 · 2026-10-12 — RTF tab stops: left, centre, right and decimal (item 3).
- 22 · 2026-10-12 — RTF list paragraphs are list items with the writer's number or bullet (item 3).
- 21 · 2026-10-12 — paragraph borders and paragraph spacing are read (item 3).
- 20 · 2026-10-12 — nested tables and cells merged across are read (item 3).
- 19 · 2026-10-12 — pictures written as bytes (`\bin`) are read (item 3).
- 18 · 2026-10-12 — an attachment in the text is a card with its name, size and caution (item 3).
- 17 · 2026-10-12 — text in metafiles is drawn, and is the picture's own text (item 3).
- 16 · 2026-10-11 — enhanced metafiles that are drawings are drawn (item 3).
- 15 · 2026-10-11 — Windows metafiles that are drawings are drawn (item 3).
- 14 · 2026-10-11 — attached objects of other programs are kept through `.msg` and shown by their own pictures (item 3).
- 13 · 2026-10-11 — RTF bodies show attachments in their places, bitmaps and metafile bitmaps (item 3).
- 12 · 2026-10-11 — pictures are printed; what remains for them listed (item 3).
- 11 · 2026-10-11 — a message prints to a PDF file; what printing lacks listed (item 3).
- 10 · 2026-10-11 — RTF pictures (PNG, JPEG) and RTF cell lines, fills, places and merges done (item 3).
- 9 · 2026-10-11 — italic faces, text beside a picture across blocks, borders set in styles and cells that span rows done (item 3).
- 8 · 2026-10-11 — fonts by name, pictures in the line of text and at its side, and pictures written into the body done (item 3).
- 7 · 2026-10-11 — style sheets, cell padding and spacing, row fills, vertical places, percentage widths and picture frames done (item 3).
- 6 · 2026-10-11 — bodies compared with Outlook's; table widths, placement and cell fills done (item 3).
- 5 · 2026-10-11 — pictures attached to the message are drawn (item 3).
- 4 · 2026-10-11 — RTF bodies are decoded and drawn (item 3).
- 3 · 2026-10-11 — HTML bodies are drawn formatted (item 3 rewritten); item 18 added.
- 2 · 2026-10-11 — `.msg` checked in Outlook Classic; first UI comparison.
- 1 · 2026-10-11 — first list.
