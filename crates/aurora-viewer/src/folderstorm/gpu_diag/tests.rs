use super::*;

fn sample(vram: (u64, u64), card: Option<(u64, u64)>, peers: &[(u64, bool)]) -> Sample {
    Sample {
        vram_used: Some(vram.0),
        vram_budget: Some(vram.1),
        allocated: Some(vram.0.saturating_sub(100)),
        textures: 1800,
        texture_pages: 2100,
        texture_target: 2048,
        pressure: 3,
        pressure_max: 7,
        card_used: card.map(|c| c.0),
        card_total: card.map(|c| c.1),
        peers: peers.iter().map(|&(used_mb, in_front)| Peer { used_mb, in_front }).collect(),
        window: (2560, 1377),
        place: Place::Behind,
    }
}

fn hang() -> Context {
    Context {
        driver: Some(("unknown", "device-lost", "callback")),
        work: Some(Work::Hang),
        role: Some("background"),
        standby: Some("economy"),
        in_front: false,
    }
}

#[test]
fn the_history_keeps_one_sample_every_five_seconds_for_a_minute() {
    let t = Instant::now();
    let mut h = History::default();
    assert!(h.due(t));
    for i in 0..20u64 {
        let now = t + SAMPLE_EVERY * i as u32;
        assert!(h.due(now));
        h.push(now, sample((i, 4000), None, &[]));
        assert!(!h.due(now + Duration::from_secs(4)));
    }
    let end = t + SAMPLE_EVERY * 19;
    let aged = h.aged(end);
    // the minute before the newest, both ends included
    assert_eq!(aged.len(), 13);
    assert_eq!(aged.first().map(|(age, _)| *age), Some(HISTORY_SPAN));
    assert_eq!(aged.last().map(|(age, s)| (*age, s.vram_used)), Some((Duration::ZERO, Some(19))));
    h.clear();
    assert!(h.aged(end).is_empty());
}

#[test]
fn a_full_card_is_the_reason_even_when_the_frame_hung() {
    let recent = [
        sample((3000, 3600), Some((10_000, 12_288)), &[(4000, true), (3500, false)]),
        sample((3300, 3400), Some((11_878, 12_288)), &[(4200, true), (3900, false)]),
    ];
    assert_eq!(
        reason(&hang(), &recent),
        Reason::Memory {
            viewers: 3,
            used_total: Some((11_878, 12_288)),
        }
    );
}

#[test]
fn without_the_card_s_counters_the_viewer_s_own_budget_decides() {
    // past 97 % of the driver's budget (Linux, macOS: no card-wide counter)
    let recent = [sample((3950, 4000), None, &[])];
    assert_eq!(
        reason(&hang(), &recent),
        Reason::Memory {
            viewers: 1,
            used_total: Some((3950, 4000)),
        }
    );
    // room left: the hang is the reason
    let recent = [sample((2000, 4000), Some((6000, 12_288)), &[])];
    assert_eq!(reason(&hang(), &recent), Reason::Hang);
}

#[test]
fn the_driver_s_words_decide_when_the_memory_had_room() {
    let roomy = [sample((2000, 4000), Some((6000, 12_288)), &[])];
    let mut c = hang();
    c.work = Some(Work::Fault);
    assert_eq!(reason(&c, &roomy), Reason::Fault);
    c.work = Some(Work::Undecided);
    assert_eq!(reason(&c, &roomy), Reason::Unknown);
    c.driver = Some(("unknown", "timeout", "callback"));
    assert_eq!(reason(&c, &roomy), Reason::Hang);
    // the driver's own "out of memory", with or without figures
    c.driver = Some(("unknown", "out-of-memory", "callback"));
    assert_eq!(
        reason(&c, &roomy),
        Reason::Memory {
            viewers: 1,
            used_total: Some((6000, 12_288)),
        }
    );
    assert_eq!(
        reason(&c, &[]),
        Reason::Memory {
            viewers: 1,
            used_total: None
        }
    );
    // nothing known at all
    assert_eq!(reason(&Context::default(), &[]), Reason::Unknown);
}

#[test]
fn the_verdict_words_of_the_renderer_map_to_hang_or_fault() {
    let v = aurora_render::GPU_WORK_VERDICTS;
    assert_eq!(Work::from_verdict(v[0]), Some(Work::Fault));
    assert_eq!(Work::from_verdict(v[1]), Some(Work::Hang));
    assert_eq!(Work::from_verdict(v[2]), Some(Work::Fault));
    assert_eq!(Work::from_verdict(v[3]), Some(Work::Undecided));
    assert_eq!(Work::from_verdict("something else"), None);
}

#[test]
fn the_notice_says_the_reason_in_plain_words_or_keeps_aurora_s_text() {
    let memory = Reason::Memory {
        viewers: 3,
        used_total: Some((11_878, 12_288)),
    };
    let text = notice(memory, true, false, "AURORA");
    assert!(
        text.starts_with("Video memory was full (3 characters open, 11.6 of 12 GB in use)."),
        "{text}"
    );
    assert!(text.contains("you stayed logged in"), "{text}");
    assert!(text.contains("Show the log folder"), "{text}");
    assert!(!text.contains("AURORA"));
    let viewers = notice(memory, false, false, "AURORA");
    assert!(viewers.contains("3 viewer windows open"), "{viewers}");
    let alone = notice(
        Reason::Memory {
            viewers: 1,
            used_total: Some((3950, 4000)),
        },
        false,
        false,
        "AURORA",
    );
    assert!(alone.starts_with("Video memory was full (3.9 of 3.9 GB in use)."), "{alone}");
    let hang = notice(Reason::Hang, true, true, "AURORA");
    assert!(hang.starts_with("The card took too long to draw a frame."), "{hang}");
    assert!(hang.contains("Potato"), "{hang}");
    assert!(notice(Reason::Fault, false, false, "AURORA").starts_with("The graphics driver stopped"));
    let unknown = notice(Reason::Unknown, false, false, "AURORA");
    assert!(unknown.starts_with("AURORA\n\n"), "{unknown}");
    assert!(unknown.contains("Show the log folder"), "{unknown}");
}

/// (the messages with variables are checked in every language by
/// aurora-i18n's catalogs test)
#[test]
fn every_reason_has_its_words_in_each_language() {
    for key in [
        "fs-gpu-reason-memory",
        "fs-gpu-reason-hang",
        "fs-gpu-reason-fault",
        "fs-gpu-reset-then",
        "fs-gpu-reset-then-safe",
        "fs-gpu-reset-log",
        "fs-gpu-show-logs",
    ] {
        let english = aurora_i18n::text_in("en", key);
        assert!(!english.is_empty() && english != key, "en {key}");
        for language in ["fr", "ja"] {
            assert_ne!(aurora_i18n::text_in(language, key), english, "{language} {key}");
        }
    }
}

fn entry(when: &str) -> Entry {
    let t = Instant::now();
    let mut h = History::default();
    h.push(t, sample((3000, 3600), Some((10_000, 12_288)), &[(4000, true)]));
    h.push(t + SAMPLE_EVERY, sample((3300, 3400), Some((11_878, 12_288)), &[(4200, true)]));
    let recent: Vec<Sample> = h.aged(t).into_iter().map(|(_, s)| s).collect();
    Entry {
        when: when.to_owned(),
        context: hang(),
        reason: reason(&hang(), &recent),
        history: h.aged(t + SAMPLE_EVERY),
    }
}

#[test]
fn an_entry_holds_the_memory_picture_and_the_run_up_in_figures() {
    let text = entry("2026-11-17 14:03:22 +01:00").format(Outcome::Pending);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "== GPU loss 2026-11-17 14:03:22 +01:00");
    assert!(lines[1].starts_with("outcome: not recovered yet"), "{text}");
    for wanted in [
        "likely reason: video memory full",
        "driver: reason unknown, message about device-lost, first seen by callback",
        "gpu work at the loss: hang",
        "window: 2560x1377, behind, not in front for the GPU rules; session host: background, economy",
        "this viewer: video memory 3300 MB used of a 3400 MB budget, 3200 MB allocated",
        "textures: 1800 MB in use in 2100 MB of pages, target 2048 MB, pressure step 3/7",
        "card: 11878 MB used by every program, 12288 MB in all",
        "other viewers on the card: 1 (4200 MB in front)",
        "  - 5 s: vram 3000/3600 MB, textures 1800/2048 MB, card 10000/12288 MB, step 3, viewers 2, behind",
        "  at the loss: vram 3300/3400 MB",
    ] {
        assert!(text.contains(wanted), "missing {wanted:?} in\n{text}");
    }
    let log = entry("x").log_line();
    assert!(
        log.starts_with("GPU loss: likely reason video memory full; vram 3300/3400 MB"),
        "{log}"
    );
}

#[test]
fn the_file_keeps_the_last_twenty_losses_and_the_newest_gets_its_outcome() {
    let mut file = "an older line of another format\n".to_owned();
    for i in 0..25 {
        file = append(&file, &entry(&format!("loss {i}")).format(Outcome::Pending), KEEP_ENTRIES);
    }
    assert_eq!(file.matches(HEADER).count(), KEEP_ENTRIES);
    assert!(!file.contains("older line"));
    assert!(!file.contains("loss 4\n"));
    assert!(file.starts_with("== GPU loss loss 5\n"), "{}", &file[..40]);
    let recovered = set_outcome(&file, Outcome::Recovered { ms: 812, safe: false });
    assert_eq!(recovered.matches("outcome: recovered in 812 ms, settings unchanged").count(), 1);
    // only the newest entry changes
    let newest = recovered.rfind(HEADER).unwrap_or(0);
    assert!(recovered[newest..].contains("outcome: recovered in 812 ms"));
    assert_eq!(&recovered[..newest], &file[..newest]);
    assert_eq!(recovered.matches(HEADER).count(), KEEP_ENTRIES);
    let waiting = set_outcome(&file, Outcome::Waiting);
    assert!(waiting[newest..].contains("outcome: waiting: the window recovers"));
    let safe = set_outcome(&waiting, Outcome::Recovered { ms: 2000, safe: true });
    assert!(safe[newest..].contains("recovered in 2000 ms, with the safe graphics level"));
    // nothing to fill in an empty file
    assert_eq!(set_outcome("", Outcome::Waiting), "");
}

#[test]
fn the_demo_reason_is_read_only_in_demo() {
    assert_eq!(parse_demo_reason(Some("hang")), Some(Reason::Hang));
    assert_eq!(parse_demo_reason(Some(" fault ")), Some(Reason::Fault));
    assert!(matches!(parse_demo_reason(Some("memory")), Some(Reason::Memory { viewers: 3, .. })));
    assert_eq!(parse_demo_reason(Some("other")), None);
    assert_eq!(parse_demo_reason(None), None);
    assert_eq!(demo_reason(false), None);
}
