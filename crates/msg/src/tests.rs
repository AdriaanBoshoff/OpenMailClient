use super::*;
use openmailclient_model::{Attachment, Item, NameId, NamedKey, PropValue, RECIP_CC, RECIP_TO, pid, pset, time};

fn sample() -> Item {
    let mut it = Item::new_mail();
    it.set_subject("Größe ✓ quarterly numbers");
    it.set_sender("Alex Morgan", "alex.morgan@example.org");
    it.add_recipient(RECIP_TO, "Ana Costa", "ana@example.org");
    it.add_recipient(RECIP_CC, "Zöe Ångström", "zoe@example.org");
    it.set_body_text("Line one\nLine two – done");
    it.props.set(pid::HTML, PropValue::Binary("<p>Hi ✓</p>".as_bytes().to_vec()));
    it.props.set(pid::INTERNET_CODEPAGE, PropValue::Int32(65001));
    it.props.set(pid::CLIENT_SUBMIT_TIME, PropValue::Time(time::from_civil(2026, 10, 5, 9, 14, 30, 0)));
    it.props.set(0x0E1F, PropValue::Bool(true));
    it.props.set(0x6001, PropValue::MultiBinary(vec![vec![1, 2, 3], vec![], vec![9; 5000]]));
    it.props.set(0x6002, PropValue::MultiInt32(vec![1, -2, 3]));
    it.props.set(0x6003, PropValue::Guid([7; 16]));
    it.props.set(0x6004, PropValue::Other { ty: 0x00FB, raw: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10] });
    it.props.set(0x6005, PropValue::Int64(-5));
    it.categories = vec!["Red Category".to_string(), "Projekt Ä".to_string()];
    it.named.push((NamedKey { guid: pset::PSETID_COMMON, name: NameId::Id(0x8503) }, PropValue::Bool(true)));
    it.named.push((NamedKey { guid: [0x42; 16], name: NameId::Name("x-custom".to_string()) }, PropValue::String("kept".to_string())));
    it.attachments.push(Attachment::file("Bericht für März.bin", "application/octet-stream", (0..=255u8).cycle().take(70_000).collect()));
    let mut inner = Item::new_mail();
    inner.set_subject("Inner");
    inner.set_body_text("inner body");
    inner.add_recipient(RECIP_TO, "Bo", "bo@example.org");
    inner.categories = vec!["Blue".to_string()];
    inner.attachments.push(Attachment::file("deep.txt", "text/plain", b"deep".to_vec()));
    let mut wrapper = Attachment::file("Inner.msg", "", Vec::new());
    wrapper.embedded = Some(Box::new(inner));
    it.attachments.push(wrapper);
    it
}

#[test]
fn write_then_read_round_trips() {
    let it = sample();
    let bytes = write(&it).unwrap();
    let back = read(&bytes).unwrap();
    assert_eq!(back.subject(), it.subject());
    assert_eq!(back.body_text(), it.body_text());
    assert_eq!(back.body_html(), it.body_html());
    assert_eq!(back.sender_email(), "alex.morgan@example.org");
    assert_eq!(back.display_recipients(RECIP_CC), "Zöe Ångström");
    assert_eq!(back.recipients.len(), 2);
    assert_eq!(back.time(), it.time());
    assert_eq!(back.categories, it.categories);
    assert_eq!(back.named, it.named);
    for id in [0x0E1F, 0x6001, 0x6002, 0x6003, 0x6004, 0x6005] {
        assert_eq!(back.props.get(id), it.props.get(id), "property 0x{id:04X}");
    }
    assert_eq!(back.attachments.len(), 2);
    assert_eq!(back.attachments[0].name(), "Bericht für März.bin");
    assert_eq!(back.attachments[0].data, it.attachments[0].data);
    let inner = back.attachments[1].embedded.as_ref().unwrap();
    assert_eq!(inner.subject(), "Inner");
    assert_eq!(inner.categories, vec!["Blue".to_string()]);
    assert_eq!(inner.attachments[0].data, b"deep");
    assert_eq!(back.attachments[1].props.i32(pid::ATTACH_METHOD), Some(5));
    // Writing what was read gives the same message again.
    let again = read(&write(&back).unwrap()).unwrap();
    assert_eq!(again.props, back.props);
    assert_eq!(again.named, back.named);
}

#[test]
fn layout_follows_the_specification() {
    let bytes = write(&sample()).unwrap();
    let mut cf = cfb::CompoundFile::open(std::io::Cursor::new(&bytes)).unwrap();
    assert_eq!(cf.version(), cfb::Version::V3);
    assert_eq!(cf.root_entry().clsid().to_string(), "00020d0b-0000-0000-c000-000000000046");
    for stream in [
        "/__properties_version1.0",
        "/__substg1.0_0037001F",
        "/__nameid_version1.0/__substg1.0_00030102",
        "/__recip_version1.0_#00000001/__substg1.0_3001001F",
        "/__attach_version1.0_#00000000/__substg1.0_37010102",
    ] {
        assert!(cf.is_stream(stream), "{stream}");
    }
    assert!(cf.is_storage("/__attach_version1.0_#00000001/__substg1.0_3701000D"));
    // Top-level header: recipient and attachment counts at 16 and 20 ([MS-OXMSG] 2.4.1.1).
    let mut props = Vec::new();
    std::io::Read::read_to_end(&mut cf.open_stream("/__properties_version1.0").unwrap(), &mut props).unwrap();
    assert_eq!(&props[16..24], &[2, 0, 0, 0, 2, 0, 0, 0]);
    assert_eq!((props.len() - 32) % 16, 0);
    // The subject entry: tag 0x0037001F, size = UTF-16 bytes + 2.
    let entry = props[32..].chunks(16).find(|e| e[..4] == [0x1F, 0x00, 0x37, 0x00]).unwrap();
    let len = "Größe ✓ quarterly numbers".encode_utf16().count() * 2 + 2;
    assert_eq!(u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize, len);
    assert_eq!(write_as(&sample(), Kind::Template).unwrap().len(), bytes.len());
}

/// Outlook finds a property by name through the lookup stream its checksum selects. With the wrong
/// checksum the file opens but the categories are gone (seen with Outlook Classic).
#[test]
fn named_property_lookup_uses_the_zero_seeded_checksum() {
    let utf16 = |s: &str| s.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
    assert_eq!(crc32(&utf16("Keywords")), 0x2EDA_4D3B);
    assert_eq!(crc32(&utf16("InTransitMessageCorrelator")), 0xFF0C_62AC);
    assert_eq!(crc32(&[]), 0);

    let bytes = write(&sample()).unwrap();
    let mut cf = cfb::CompoundFile::open(std::io::Cursor::new(&bytes)).unwrap();
    // Keywords is PS_PUBLIC_STRINGS (index 2) with a string name: 0x1000 + (crc ^ 5) % 0x1F.
    let mut lookup = Vec::new();
    std::io::Read::read_to_end(&mut cf.open_stream("/__nameid_version1.0/__substg1.0_10150102").unwrap(), &mut lookup).unwrap();
    assert_eq!(&lookup[..8], &[0x3B, 0x4D, 0xDA, 0x2E, 0x05, 0x00, 0x00, 0x00]);
}

#[test]
fn hostile_input_never_panics() {
    assert!(matches!(read(b""), Err(MsgError::NotMsg)));
    assert!(matches!(read(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]), Err(MsgError::NotMsg)));
    // A compound file that is not a message.
    let mut cf = cfb::CompoundFile::create(std::io::Cursor::new(Vec::new())).unwrap();
    cf.create_stream("/WordDocument").unwrap();
    assert!(matches!(read(&cf.into_inner().into_inner()), Err(MsgError::NotMsg)));
    let mut small = Item::new_mail();
    small.set_subject("s");
    small.add_recipient(RECIP_TO, "A", "a@example.org");
    small.categories = vec!["c".into()];
    small.attachments.push(Attachment::file("a.txt", "text/plain", b"abc".to_vec()));
    let good = write(&small).unwrap();
    for n in (0..good.len()).step_by(7) {
        let _ = read(&good[..n]);
    }
    for n in 0..good.len() {
        let mut bad = good.clone();
        bad[n] = bad[n].wrapping_mul(31).wrapping_add(n as u8 | 1);
        let _ = read(&bad);
    }
}

/// A bitmap of two by two pixels, without a file's header.
fn small_bitmap() -> Vec<u8> {
    let mut dib = Vec::new();
    for n in [40u32, 2, 2] {
        dib.extend_from_slice(&n.to_le_bytes());
    }
    dib.extend_from_slice(&[1, 0, 24, 0]);
    dib.extend_from_slice(&[0; 24]);
    dib.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0]);
    dib
}

/// An object as another program might lay it out: a class, streams with the odd first characters
/// OLE uses, and a storage inside a storage.
fn custom_object() -> Vec<u8> {
    use std::io::Write;
    let mut cf = cfb::CompoundFile::create(std::io::Cursor::new(Vec::new())).unwrap();
    cf.set_storage_clsid("/", uuid::Uuid::from_u128(0x1234_5678_9ABC_DEF0_1122_3344_5566_7788)).unwrap();
    cf.create_stream("/\u{1}Ole").unwrap().write_all(&[1, 0, 0, 2, 0, 0, 0, 0]).unwrap();
    cf.create_stream("/CONTENTS").unwrap().write_all(&vec![7u8; 9000]).unwrap();
    cf.create_storage("/Parts").unwrap();
    cf.set_storage_clsid("/Parts", uuid::Uuid::from_u128(42)).unwrap();
    cf.create_storage("/Parts/Deeper").unwrap();
    cf.create_stream("/Parts/Deeper/leaf").unwrap().write_all(b"leaf").unwrap();
    cf.create_stream("/Parts/empty").unwrap();
    cf.flush().unwrap();
    cf.into_inner().into_inner()
}

/// Every storage and stream of a compound file: path, class and state of a storage, bytes of a stream.
fn listing(bytes: &[u8]) -> Vec<(String, String, u32, Vec<u8>)> {
    use std::io::Read;
    let mut cf = cfb::CompoundFile::open(std::io::Cursor::new(bytes)).unwrap();
    let entries: Vec<_> = cf.walk().map(|e| (e.path().to_path_buf(), e.is_stream(), e.clsid().to_string(), e.state_bits())).collect();
    let mut out = Vec::new();
    for (path, stream, clsid, state) in entries {
        let mut data = Vec::new();
        if stream {
            cf.open_stream(&path).unwrap().read_to_end(&mut data).unwrap();
        }
        out.push((path.to_string_lossy().replace('\\', "/"), clsid, state, data));
    }
    out.sort();
    out
}

#[test]
fn an_attached_object_of_another_program_round_trips_as_its_storage() {
    let object = custom_object();
    let mut it = Item::new_mail();
    it.set_subject("With an object");
    let mut a = Attachment::file("Drawing", "", object.clone());
    a.props.set(pid::ATTACH_METHOD, PropValue::Int32(6));
    a.props.set(pid::RENDERING_POSITION, PropValue::Int32(12));
    it.attachments.push(a);
    it.attachments.push(Attachment::file("plain.txt", "text/plain", b"plain".to_vec()));

    let bytes = write(&it).unwrap();
    // In the file the object is a storage under its attachment, not a stream of bytes
    // ([MS-OXMSG] 2.2.2.2), and its entry says it is one: no size, object kind 4 (2.4.2.2).
    let mut cf = cfb::CompoundFile::open(std::io::Cursor::new(&bytes[..])).unwrap();
    assert!(cf.is_storage("/__attach_version1.0_#00000000/__substg1.0_3701000D"));
    assert!(cf.is_stream("/__attach_version1.0_#00000000/__substg1.0_3701000D/Parts/Deeper/leaf"));
    assert!(!cf.exists("/__attach_version1.0_#00000000/__substg1.0_37010102"));
    let mut props = Vec::new();
    std::io::Read::read_to_end(&mut cf.open_stream("/__attach_version1.0_#00000000/__properties_version1.0").unwrap(), &mut props).unwrap();
    let entry = props[8..].chunks(16).find(|e| e[..4] == [0x0D, 0x00, 0x01, 0x37]).unwrap();
    assert_eq!(entry[8..], [0xFF, 0xFF, 0xFF, 0xFF, 4, 0, 0, 0]);

    let back = read(&bytes).unwrap();
    assert_eq!(back.attachments.len(), 2);
    let got = &back.attachments[0];
    assert!(got.embedded.is_none(), "an object is not a message");
    assert_eq!(got.props.i32(pid::ATTACH_METHOD), Some(6));
    assert_eq!(got.props.i32(pid::RENDERING_POSITION), Some(12));
    assert_eq!(listing(&got.data), listing(&object), "every stream and storage, with classes and state");
    assert_eq!(back.attachments[1].data, b"plain");
    // And again: what was read writes the same object.
    let again = read(&write(&back).unwrap()).unwrap();
    assert_eq!(listing(&again.attachments[0].data), listing(&object));

    // Bytes that are not a compound file are still kept, as the file they are: Outlook lists
    // an object that is not a storage as no attachment.
    let mut odd = Item::new_mail();
    let mut a = Attachment::file("Odd", "", b"not a compound file".to_vec());
    a.props.set(pid::ATTACH_METHOD, PropValue::Int32(6));
    odd.attachments.push(a);
    let back = read(&write(&odd).unwrap()).unwrap();
    assert_eq!(back.attachments[0].data, b"not a compound file");
    assert_eq!(back.attachments[0].props.i32(pid::ATTACH_METHOD), Some(1));
}

/// A presentation stream ([MS-OLEDS] 2.3.4) of a standard format, with a target device of `device` bytes.
fn presentation(format: u32, device: usize, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    out.extend_from_slice(&format.to_le_bytes());
    out.extend_from_slice(&u32::try_from(4 + device).unwrap().to_le_bytes());
    out.extend_from_slice(&vec![0xEE; device]);
    out.extend_from_slice(&[0; 24]);
    out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    out.extend_from_slice(data);
    // What follows a metafile: 18 reserved bytes.
    out.extend_from_slice(&[0; 18]);
    out
}

fn object_of(streams: &[(&str, Vec<u8>)]) -> Vec<u8> {
    use std::io::Write;
    let mut cf = cfb::CompoundFile::create(std::io::Cursor::new(Vec::new())).unwrap();
    for (name, data) in streams {
        cf.create_stream(format!("/{name}")).unwrap().write_all(data).unwrap();
    }
    cf.flush().unwrap();
    cf.into_inner().into_inner()
}

#[test]
fn an_attached_object_tells_its_kind_and_its_pictures() {
    use ole::{Picture, Shape};
    let dib = small_bitmap();
    // Our own writer, read back.
    let made = ole::picture_object("Permit card", &dib).unwrap();
    let object = ole::object(&made).unwrap();
    assert_eq!(object.kind.as_deref(), Some("Permit card"));
    assert_eq!(object.pictures, [Picture { shape: Shape::Bitmap, data: dib.clone() }]);

    // The object's own data first when it is a picture file, then its presentations in their
    // order; a format with a name, an unknown one and an empty one are passed over.
    let mut native = 5u32.to_le_bytes().to_vec();
    native.extend_from_slice(b"GIF89 and more that the size leaves out");
    let mut named = 5u32.to_le_bytes().to_vec();
    named.extend_from_slice(b"Mine\0 whatever follows");
    let mut comp = vec![9u8; 28];
    comp.extend_from_slice(&12u32.to_le_bytes());
    comp.extend_from_slice(b" Sketch\x07 2\0zz");
    let object = ole::object(&object_of(&[
        ("\u{1}CompObj", comp),
        ("\u{1}Ole10Native", native),
        ("\u{2}OlePres000", presentation(3, 0, b"metafile")),
        ("\u{2}OlePres001", named),
        ("\u{2}OlePres002", presentation(14, 40, b"enhanced")),
        ("\u{2}OlePres003", presentation(2, 0, b"device bitmap")),
        ("\u{2}OlePres004", presentation(8, 0, b"")),
        ("\u{2}OlePres006", presentation(8, 0, b"after a gap")),
    ]))
    .unwrap();
    assert_eq!(object.kind.as_deref(), Some("Sketch 2"));
    let got: Vec<(Shape, &[u8])> = object.pictures.iter().map(|p| (p.shape, &p.data[..])).collect();
    assert_eq!(got, [(Shape::File, &b"GIF89"[..]), (Shape::Metafile, b"metafile"), (Shape::Enhanced, b"enhanced")]);

    // Data that is not a picture file is not offered as one.
    let mut native = 4u32.to_le_bytes().to_vec();
    native.extend_from_slice(b"MZ\x90\0");
    let object = ole::object(&object_of(&[("\u{1}Ole10Native", native)])).unwrap();
    assert_eq!(object, ole::Object::default());
}

#[test]
fn hostile_objects_never_panic() {
    assert!(ole::object(b"").is_none());
    assert!(ole::object(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]).is_none());
    // Lengths that reach past the stream, wrap around, or leave no room.
    let huge = |at: usize, mut b: Vec<u8>| {
        b[at..at + 4].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
        b
    };
    let good = presentation(8, 0, b"data");
    let mut comp = vec![0u8; 28];
    comp.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    for streams in [
        vec![("\u{2}OlePres000", huge(8, good.clone()))],
        vec![("\u{2}OlePres000", huge(36, good.clone()))],
        vec![("\u{2}OlePres000", good[..20].to_vec())],
        vec![("\u{2}OlePres000", vec![0xFF; 7])],
        vec![("\u{2}OlePres000", {
            let mut b = good.clone();
            b[8] = 3;
            b
        })],
        vec![("\u{2}OlePres000", Vec::new()), ("\u{1}Ole10Native", vec![0xFF; 4]), ("\u{1}CompObj", comp.clone())],
        vec![("\u{1}Ole10Native", vec![1]), ("\u{1}CompObj", vec![0; 30])],
    ] {
        let object = ole::object(&object_of(&streams)).unwrap();
        assert_eq!(object, ole::Object::default());
    }
    // A damaged compound file, cut and overwritten at every kind of place.
    let whole = custom_object();
    for cut in (0..whole.len()).step_by(97) {
        let _ = ole::object(&whole[..cut]);
        let mut bad = whole.clone();
        for b in bad.iter_mut().skip(cut).take(64) {
            *b = 0xFF;
        }
        let _ = ole::object(&bad);
        // As an attachment, written and read: an error or a message, never a panic.
        let mut it = Item::new_mail();
        let mut a = Attachment::file("Drawing", "", bad);
        a.props.set(pid::ATTACH_METHOD, PropValue::Int32(6));
        it.attachments.push(a);
        if let Ok(bytes) = write(&it) {
            let _ = read(&bytes);
        }
    }
}
