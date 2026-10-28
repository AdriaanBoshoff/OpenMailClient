# UI parity

**Status:** revision 25 · 2026-10-28 · every setting in one Options window and the File tab in pages; before that, an account's mail compared with Outlook's showing of it: padding, links, Sent Items rows, previews, date headings, region formats and an `.ost`'s folder pane follow Outlook (M1.8); metafiles are drawn in full, from the specifications and not compared (M3b.28); an RTF highlight is a marker colour as in the one case seen (M3b.27); borders round text within a line and rounded corners on boxes and cells (M3b.26), ours, not compared with Outlook; boxes inside one another, tables inside blocks and a colour behind the whole body in the reading pane (M3b.25), ours, not compared with Outlook; several pictures at the sides of a paragraph and paper behind dark see-through pictures in the dark pane (M3b.24), ours, not compared with Outlook; the print window has a zoom and Table Style's columns, headings and rows (M3b.22); side-by-side looks at the Mail module in both ribbon layouts, and at eight message bodies; layout only.

How OpenMailClient's window compares with Outlook Classic's, from window captures of both at the same size.
Outlook captures hold private mail, stay on the dev machine and are never committed; this page records
only layout and measurements. Numbers are logical pixels at 96 dpi, from a 1440 × 753 window.

## What was compared

Outlook Classic 16.0, Mail module, as configured on the dev machine: **simplified (single-line) ribbon**
and a **dark grey theme**. Both are user settings; the classic ribbon and the light themes are equally
Outlook Classic. The classic ribbon was captured once by switching the layout for one capture and
switching it back; the theme was left alone, so the light themes are still unobserved. OpenMailClient has
both ribbon layouts (`ui.ribbonMode`) and a light and a dark colour set (`ui.theme`); it starts with the
simplified ribbon in the light set. The colours are our own in both sets, not Outlook's.

The two ribbon layouts share one window shell: title bar, module strip, tab row and panes are the same,
and only the ribbon panel's height and contents change. OpenMailClient follows that.

## Measurements

| Part | Outlook Classic (observed) | OpenMailClient | State |
|---|---|---|---|
| Title bar | about 58 high; search box 304 × 32, left edge on the message list's left edge | 48 high; search box up to 304 × 30 on the list's left edge | close |
| Tab row | about 24 high: File, Home, Send / Receive, View, Help, then a "tell me" box; the classic layout adds a Folder tab after Send / Receive | 28 high: File, Home, Send / Receive, Folder, View, Help in both layouts | differs (Folder tab always shown; no tell-me box) |
| Ribbon, simplified | one rounded panel, 48 high, inset from the right window edge | one rounded panel, 48 high, inset 16 | close; no overflow menu when the window is narrow |
| Ribbon, classic | one rounded panel, about 98 high, inset from the right window edge like the simplified one; large buttons with a label of up to two lines under the icon, columns of up to three small buttons, a group name centred under each group, a collapse arrow bottom right; panes start about 6 below it | one rounded panel, 98 high, inset 16; the same button and group arrangement; panes 7 below | close; no collapse arrow, no Quick Steps gallery, fewer groups |
| Module strip | 64 wide at the far left, full height below the title bar; icons 48 apart: Mail, Calendar, People, Tasks, More | 64 wide, full height, 48 apart; four modules | close |
| Folder pane | 235 wide (user-resizable); rows 24 apart; Favorites group first, then one group per account; collapse arrow top right | 216 wide to start with, dragged at its edge and kept between runs; rows 26 apart; no Favorites | differs |
| Message list | 276 wide (user-resizable); header row with All / Unread, "By Date" and a direction arrow; group header 24 high; rows about 70 high with sender, subject, preview | 348 wide to start with, dragged at its edge and kept between runs; same header parts; rows 64 high | close |
| Row details | attachment mark at the right of the first line; date at the right of the second | marks at the right of the first line; time at the right of the second | close |
| Reading pane, nothing open | envelope drawing with a one-line hint, centred | the same arrangement, our own drawing | close |
| Reading pane, message open | formatted body on a page in the theme's colour | formatted body laid out as Outlook lays out the same message: see Message bodies below | close |
| Status bar | 22 high: item count left; connection state, two view buttons and a zoom slider right | 22 high: item and unread counts left, store kind right; while a data file is being saved or exported to, its name, a bar that fills, a percentage and Stop (ours; what Outlook shows while it saves was not observed) | close |

## Message bodies

Two messages from OpenMailClient's own sample mailbox and six probe messages (all synthetic, written by
us) were saved as `.msg` and opened in an Outlook Classic message window, one at a time, then closed
without saving. The window was compared with OpenMailClient's reading pane showing the same message.
Nothing from the owner's mailbox was involved.

| What | Outlook Classic (observed) | OpenMailClient | State |
|---|---|---|---|
| Unstyled HTML text | 12 pt | 12 pt (16 px); relative sizes and headings start from it | same |
| `<table width=600 align=center>` | a column 600 wide, centred in the window | the same; also `max-width`, `width` in a style, and `margin: auto` for centring | same |
| Table without a width | as wide as its content | as wide as its content; cell `width` attributes are kept | same |
| `bgcolor` or a background on a cell | fills the whole cell | fills the whole cell; text on it is kept readable against that colour | same |
| A one-column table with a fill, a width or a place | drawn as a block of that shape | drawn; a one-column table with none of them still reads as ordinary blocks | same |
| Plain `<blockquote>` | set in about 40 to 48 on the left, no bar | set in 40, no bar | close |
| Quoted earlier mail (`type=cite`, a left border, a quote class) | a bar on the left | a bar on the left | close |
| Picture sent with the message (`cid:`) | drawn at the size the body asks for | the same | same |
| Picture on a server, not loaded, with a size | an empty box of the size asked for, with a mark, a notice and the alternative text, standing in the line of text | an empty frame of that size with our own mark, a notice and the alternative text, standing in the line of text | close |
| Picture in a paragraph | stands in the line of text with its bottom on the baseline; without a size, at its own size; several wrap like words | the same | same |
| `<img align=left>` and `align=right` | the picture stands at that side; text runs beside it and then under it | the same | same |
| Paragraphs after a picture at the side | stand beside it for as long as they start above its end, then under it | the same | same |
| A table after a picture at the right | stands at the left beside the picture | the same | same |
| `<br clear="all">` | what follows starts below the picture | the same; a `clear` style does too | same |
| `border` in a style on a paragraph | a box as wide as the room, close round the text, in the width and colour asked for | the same, with 2 px between line and text | close |
| A border on one side only (`border-left`, `border-bottom`) | only that line; a left line stands out in the margin and the text stays where it was | only that line; a left line stands inside the room and the text moves in by its width | close |
| `border` and `padding` on a block of several lines or paragraphs | one box round them all, the text set in by the padding | the same | same |
| `dashed`, `dotted` and `double` lines | drawn as such | the same; a double line under 3 px is one line | same |
| `border` in a style on a table | a line round the table, none round its cells | the same | same |
| `border` in a style on a cell | lines round that cell, each side as asked | the same | same |
| `bordercolor` on a table | the table's lines in that colour | the same | same |
| `border` wider than 1 on a table | a line that wide round the table, lines 1 px wide round the cells | the same, in one colour instead of a raised edge | close |
| `rowspan` | the cell covers the rows; cells under it move aside; its fill covers it all and its text stands in the middle | the same | same |
| `float` in a style on a picture | not followed: the picture stays in the line of text | the same | same |
| Picture written into the body (`data:` in base64) | shown, sized or at its own size | the same, for the picture kinds OpenMailClient decodes | same |
| Font named by `<font face>` or `font-family` | drawn in that font when it is installed, with its own bold | the same; an open font of the same measures stands in for a missing proprietary one | same |
| Italic text in a named font | the font's italic face | the same; the upright face slanted when the font has no italic face installed | same |
| A font that is not installed | the next in the list; `sans-serif` and `serif` are followed; with nothing left, a serif | the same | same |
| Text that names no font | a sans-serif of Outlook's own choosing | OpenMailClient's interface font | differs |
| Picture on a server, not loaded, without a size | a small box with a mark only | a line of text with the alternative text | differs (ours says what the picture is) |
| Table, cell padding and spacing | 1 and 2 where the table sets none; `cellpadding` and `cellspacing` followed; with spacing the table's own border is seen round the cells' | the same | same |
| `border-collapse: collapse` | no room between cells | the same | same |
| `padding` on a cell | followed, above and below apart from left and right | the same; one value each way (the left and right, top and bottom are not told apart) | close |
| Background on a row (`bgcolor`, or a style) | fills the row's cells | the same | same |
| What a cell holds, in a taller cell | in the middle; `valign` and `vertical-align` on the cell or the row followed | the same | same |
| Table width as a percentage | that share of the window | that share of the room | same |
| Cell width as a percentage | that share of the table | the same, for tables that have a width | same |
| Style sheet (`<style>`) | rules for an element name, a class, an id, or a name with a class are followed; only the **first** class an element names counts; rules with several classes, with one element inside another, and inside `@media` are not | the same | same |
| Link colour | the link colour whatever the text around it has; a colour set on the link itself is kept | the same, with our own link colour | same |
| RTF table | cell widths from the row definition (15 twips to the pixel); only the borders the RTF names | the same | same |
| RTF cell lines | each side as named: width, colour, and solid, double, dashed or dotted; a line between two cells is drawn once | the same; dots are 2 px long | close |
| RTF cell with thick lines | the row grows by the lines | the same | same |
| RTF cell fill and place (`\clcbpat`, `\clvertalc`, `\clvertalb`) | the fill covers the cell; text stands at the top, middle or bottom | the same | same |
| RTF cells merged down (`\clvmgf`, `\clvmrg`) | one cell over the rows, its fill over all of them | the same | same |
| Room round an RTF table and in its cells | the table stands right under the paragraph before it; rows are as tall as their text | the same, with 1 px above and below the text and 2 px beside it | close |
| RTF picture (`\pict` with PNG data) | in the line of text on its baseline, at its own size, or at the goal size times the scale | the same; JPEG too | same |
| RTF left indent | 720 twips is 48 | 720 twips is 48 | same |
| RTF highlight | a bright marker colour from a fixed palette: a pale yellow in the colour table is bright yellow | one of sixteen marker colours: the colour itself when it is one, a grey, or the pure colour of its hue | same for the case seen; others not compared |
| Message window type | an RTF-only `.msg` is shown as rich text only when it is marked as in sync (see [file-format-parity.md](file-format-parity.md)) | written so | same |

Not followed yet, in order of how much they show: the space Outlook
leaves between a paragraph and a table; a left border standing out in the margin; lines shared by
neighbouring cells (`border-collapse`). They are in [gaps.md](gaps.md), item 3.

## Printing

Not compared against Outlook's print output yet: printing from Outlook would use a real printer
or its print dialog, and neither is driven on the shared machine. OpenMailClient's Memo Style page (the
owner's name over a line, field names in bold beside their values, the body, a page number at the
foot) is laid out from general knowledge of the style, and its measures are our own. File > Print
and Ctrl+P open a print window: the page as it will be printed beside the paper, direction, margins,
header and footer; its button asks for a PDF file to write. It prints the open message (Memo Style) or the
folder's list (Table Style, with columns that are chosen, group headings and the selected row alone; the page is shown whole or at 25 to 400 hundredths of its size). The window's arrangement is our own;
Outlook's print pane was not observed.

## Behaviour seen

- The File tab is in pages, as Outlook's is: Info, Open & Export, Save, then Print, Options and
  Exit at the side. Outlook keeps its settings in Options and Account Settings apart, with OK
  and Cancel; OpenMailClient keeps every setting, accounts and themes too, in one Options window
  with a page each, and a change is kept as it is made.
- Opening a folder selects the first row but **does not open it** in the reading pane; the pane shows the
  empty state until the user picks a message. OpenMailClient selects nothing when a folder opens. To check
  further before changing: this may depend on Outlook's reading-pane options.
- Seen beside an account's mail (M1.8), and now done the same way:
  - Sent Items, Drafts and the Outbox show whom a message went to where other folders show its sender.
  - A row's preview is the start of the body on one line; when the body is all there, `<end>` follows.
  - Date headings: Today, Yesterday, the other days of this week by name, Last Week, Two Weeks Ago,
    Three Weeks Ago, Earlier This Month, Last Month, Older; weeks begin where the region says.
  - Dates and times in the list, the reading pane and the forward header follow the system's
    region settings (a short date of year/month/day and a 24-hour clock was seen).
  - Links in a signature whose style turns underlining off are not underlined; table-cell padding in
    centimetres and points is kept.
  - An `.ost`'s folder pane starts at the mailbox's folders (Inbox, Drafts, Sent Items...) and leaves
    out Outlook's own hidden folders.

## Next

1. Favorites (the panes are sized by dragging since M3b.3).
2. An overflow menu for the simplified ribbon, and the "tell me" command search.
3. A light theme has not been observed (the theme setting roams to the owner's other machines, so
   it was not switched; ask first).

## Revision history

- 25 · 2026-10-28 — the File tab in pages (Info, Open & Export, Save, then Print, Options and Exit) and one Options window for every setting, a page each (M1.9).
- 24 · 2026-10-17 — an account's mail compared with Outlook's showing of it, locally (nothing of it is kept in the repo); padding, links, Sent Items rows, previews and their end mark, date headings, region formats and an `.ost`'s folder pane now follow Outlook (M1.8).
- 23 · 2026-10-13 — metafiles draw parts of bitmaps, mirrored, turned and run-packed ones, raster operations, hatched and patterned brushes and regions (M3b.28). Drawn from [MS-WMF] and [MS-EMF]; no metafile was compared with Outlook's showing of it.
- 22 · 2026-10-13 — an RTF highlight is shown as one of sixteen marker colours: the pale yellow of the receipt message is bright yellow, as Outlook shows it (M3b.27). Other colours are chosen by hue, which is ours and was not compared.
- 21 · 2026-10-13 — text within a line is drawn in its border; boxes and cells have rounded corners (M3b.26). Not compared with Outlook.
- 20 · 2026-10-13 — boxes inside one another; a table inside the block it was written in; a body colour fills the reading pane (M3b.25). Not compared with Outlook.
- 19 · 2026-10-13 — several pictures at the sides of one paragraph; paper behind a dark see-through picture on the dark pane (M3b.24). Not compared with Outlook.
- 18 · 2026-10-13 — the print window: a zoom, and Table Style's columns, group headings and rows (M3b.22).
- 17 · 2026-10-13 — the status bar shows an export that is running, as it does a save (M3b.14).
- 16 · 2026-10-13 — the status bar shows a save that is running, and stops it (M3b.12).
- 15 · 2026-10-12 — in a paragraph written from the right, tab leaders, a border's indent, text beside a picture and table columns are counted from the right; `<bdo>`, `<bdi>` and `unicode-bidi` are read (M3b.9); not compared with Outlook.
- 14 · 2026-10-12 — fields of one line are our own: right-to-left text reads right, and the cursor follows it, while it is typed (M3b.8); not compared with Outlook.
- 13 · 2026-10-12 — one-line text with Hebrew or Arabic in it is drawn in the order it is read (M3b.4); not compared with Outlook.
- 12 · 2026-10-12 — the folder pane and the list are sized by dragging their edges, and their sizes are kept (M3b.3).
- 11 · 2026-10-11 — printing added; its output is not yet compared with Outlook's.
- 10 · 2026-10-11 — a sixth probe message compared, with an RTF body: pictures, cell lines per side, fills, places and cells merged down.
- 9 · 2026-10-11 — a fifth probe message compared: what follows a picture at the side, breaks that clear, borders set in styles, table border widths and colours, cells that span rows.
- 8 · 2026-10-11 — a fourth probe message compared: fonts by name and their fallbacks, pictures in the line of text and at its side, pictures written into the body.
- 7 · 2026-10-11 — three probe messages compared: picture frames, cell padding and spacing, row fills, vertical places, percentage widths, style sheets, link colour.
- 6 · 2026-10-11 — two message bodies compared with Outlook's display of the same `.msg`; Message bodies section added.
- 5 · 2026-10-11 — attached pictures drawn in the body; row updated.
- 4 · 2026-10-11 — classic ribbon observed; OpenMailClient's classic layout rebuilt on the shared shell.
- 3 · 2026-10-11 — formatted bodies drawn; row added.
- 2 · 2026-10-11 — simplified ribbon, full-height module strip and dark colour set built; table updated.
- 1 · 2026-10-11 — first comparison.
