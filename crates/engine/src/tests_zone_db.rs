//! Zones with their whole history from the system's zone database: a directory of tz files of
//! our own, and on Windows the registry's Dynamic DST.

use crate::Session;
use crate::zone_db::{self, Source};
use openmailclient_model::time;
use serde_json::json;

/// A version 2 tz file: two types (standard and summer, seconds east), transitions and a footer.
fn tzif(std: i32, summer: i32, changes: &[(i64, u8)], footer: &str) -> Vec<u8> {
    let header = |times: usize, types: usize| {
        let mut h = b"TZif2".to_vec();
        h.extend_from_slice(&[0; 15]);
        for c in [0, 0, 0, times, types, 1] {
            h.extend_from_slice(&u32::try_from(c).unwrap().to_be_bytes());
        }
        h
    };
    let mut out = header(0, 1);
    out.extend_from_slice(&[0; 7]);
    out.extend(header(changes.len(), 2));
    for (t, _) in changes {
        out.extend_from_slice(&t.to_be_bytes());
    }
    out.extend(changes.iter().map(|(_, k)| *k));
    for (offset, dst) in [(std, 0u8), (summer, 1)] {
        out.extend_from_slice(&offset.to_be_bytes());
        out.extend_from_slice(&[dst, 0]);
    }
    out.push(0);
    out.extend_from_slice(format!("\n{footer}\n").as_bytes());
    out
}

/// Unix seconds at midnight UTC.
fn midnight(year: i64, month: u32, day: u32) -> i64 {
    time::to_unix(time::from_civil(year, month, day, 0, 0, 0, 0))
}

fn noon(year: i64, month: u32, day: u32) -> i64 {
    time::from_civil(year, month, day, 12, 0, 0, 0)
}

/// Clears the thread's source however the test ends.
struct Testing;
impl Drop for Testing {
    fn drop(&mut self) {
        zone_db::testing(None);
    }
}

#[test]
fn zones_take_their_history_from_the_systems_tz_files() {
    let dir = std::env::temp_dir().join(format!("omc-zone-db-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Asia")).unwrap();
    std::fs::create_dir_all(dir.join("Ours")).unwrap();
    // Tehran's summer time, 2008 to 2022, from 22 March to 22 September.
    let mut changes = Vec::new();
    for year in 2008..=2022 {
        changes.push((midnight(year, 3, 22) - 12_600, 1));
        changes.push((midnight(year, 9, 22) - 16_200, 0));
    }
    std::fs::write(dir.join("Asia").join("Tehran"), tzif(12_600, 16_200, &changes, "<+0330>-3:30")).unwrap();
    // A zone only the system knows, with the rules of central Europe.
    std::fs::write(dir.join("Ours").join("Town"), tzif(3600, 7200, &[], "CET-1CEST,M3.5.0,M10.5.0/3")).unwrap();
    let _testing = Testing;

    let mut s = Session::new();
    // As this program knows Tehran alone: no summer time.
    assert_eq!(zone_db::named("Asia/Tehran").unwrap().offset_at(noon(2015, 7, 1)), 210);
    zone_db::testing(Some(Source::Files(dir.clone())));
    s.run("calendar.timeZone", &json!({ "zone": "Asia/Tehran" })).unwrap();
    assert_eq!(s.zone.offset_at(noon(2015, 7, 1)), 270);
    assert_eq!(s.zone.offset_at(noon(2024, 7, 1)), 210);
    assert_eq!(s.zone.key, "Iran Standard Time", "still known by Outlook's key");
    // By the system's name alone.
    s.run("calendar.timeZone", &json!({ "zone": "Ours/Town" })).unwrap();
    assert_eq!((s.zone.offset_at(noon(2026, 7, 1)), s.zone.offset_at(noon(2026, 12, 1))), (120, 60));
    // Names that leave the directory, or name nothing, are not zones.
    for bad in ["../Asia/Tehran", "Asia/../Asia/Tehran", "/etc/passwd", "Asia\\Tehran", "Nowhere/At_All", "Asia/.Tehran"] {
        assert!(s.run("calendar.timeZone", &json!({ "zone": bad })).is_err(), "{bad}");
    }
    // A file that is not one.
    std::fs::write(dir.join("Ours").join("Junk"), b"TZif2 and then nothing").unwrap();
    assert!(zone_db::named("Ours/Junk").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn windows_keeps_each_years_rules_in_its_registry() {
    let _testing = Testing;
    zone_db::testing(Some(Source::Registry));
    let pacific = zone_db::named("Pacific Standard Time").unwrap();
    // In 2006 summer time began on the first Sunday of April and ended on the last of October;
    // from 2007, the second Sunday of March and the first of November.
    assert_eq!(pacific.offset_at(noon(2006, 3, 20)), -480);
    assert_eq!(pacific.offset_at(noon(2006, 7, 1)), -420);
    assert_eq!(pacific.offset_at(noon(2006, 10, 31)), -480);
    assert_eq!(pacific.offset_at(noon(2007, 3, 20)), -420);
    assert_eq!(pacific.offset_at(noon(2007, 10, 31)), -420);
    assert!(zone_db::named("..\\..\\Pacific Standard Time").is_none());
}
