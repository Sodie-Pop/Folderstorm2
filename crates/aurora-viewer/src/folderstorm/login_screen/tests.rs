use super::*;
use crate::settings::Settings;
use crate::ui::icons::Icons;
use crate::ui::login::{LoginForm, show};
use crate::ui::news::LoginInfo;
use egui::{Rect, Vec2, pos2, vec2};

fn layout(window: [i32; 4], monitors: &[[i32; 4]]) -> MonitorLayout {
    MonitorLayout {
        window,
        monitors: monitors.to_vec(),
    }
}

const LEFT: [i32; 4] = [0, 0, 1920, 1080];
const RIGHT: [i32; 4] = [1920, 0, 1920, 1080];

#[test]
fn a_window_over_two_monitors_keeps_the_login_on_one() {
    let s = ViewportSettings::default();
    // stretched evenly over both: the primary one, at the desktop's origin
    let both = layout([0, 0, 3840, 1080], &[LEFT, RIGHT]);
    assert_eq!(rect((3840, 1080), &s, Some(&both)), Some([0, 0, 1920, 1080]));
    // listed the other way round: still the one at the origin
    let reversed = layout([0, 0, 3840, 1080], &[RIGHT, LEFT]);
    assert_eq!(rect((3840, 1080), &s, Some(&reversed)), Some([0, 0, 1920, 1080]));
    // a monitor left of the primary one, the window evenly over both
    let west = layout([-1920, 0, 3840, 1080], &[[-1920, 0, 1920, 1080], LEFT]);
    assert_eq!(rect((3840, 1080), &s, Some(&west)), Some([1920, 0, 1920, 1080]));
    // the monitor holding most of the window wins
    let mostly_right = layout([1000, 0, 2840, 1080], &[LEFT, RIGHT]);
    assert_eq!(rect((2840, 1080), &s, Some(&mostly_right)), Some([920, 0, 1920, 1080]));
    // a tie between monitors none of which is at the origin: the first listed
    let away = layout([3840, 0, 3840, 1080], &[[5760, 0, 1920, 1080], [3840, 0, 1920, 1080]]);
    assert_eq!(rect((3840, 1080), &s, Some(&away)), Some([1920, 0, 1920, 1080]));
}

#[test]
fn a_window_on_one_monitor_is_laid_out_as_before() {
    let s = ViewportSettings::default();
    assert_eq!(rect((1920, 1080), &s, Some(&layout([0, 0, 1920, 1080], &[LEFT, RIGHT]))), None);
    assert_eq!(rect((1600, 900), &s, Some(&layout([100, 100, 1600, 900], &[LEFT]))), None);
    // a few pixels over the neighbour (less than a tenth of the window)
    assert_eq!(rect((1920, 1080), &s, Some(&layout([100, 0, 1920, 1080], &[LEFT, RIGHT]))), None);
    // Wayland: no position, no monitors
    assert_eq!(rect((3840, 1080), &s, None), None);
    // an empty window
    assert_eq!(rect((0, 0), &s, Some(&layout([0, 0, 0, 0], &[LEFT, RIGHT]))), None);
}

#[test]
fn the_world_viewport_holds_the_login_when_set() {
    let s = ViewportSettings {
        enabled: true,
        left: 1920,
        ..Default::default()
    };
    let both = layout([0, 0, 3840, 1080], &[LEFT, RIGHT]);
    assert_eq!(rect((3840, 1080), &s, Some(&both)), Some([1920, 0, 1920, 1080]));
    assert_eq!(rect((3840, 1080), &s, None), Some([1920, 0, 1920, 1080]), "even without monitors");
    // margins leaving the world under a quarter of the window: set aside,
    // and the monitors decide (the window shrunk to one monitor: as before)
    assert_eq!(rect((1920, 1080), &s, Some(&layout([0, 0, 1920, 1080], &[LEFT, RIGHT]))), None);
    let narrow = ViewportSettings { left: 3500, ..s };
    assert_eq!(rect((3840, 1080), &narrow, Some(&both)), Some([0, 0, 1920, 1080]));
}

#[test]
fn the_insets_leave_the_rectangle_and_the_bar_strip() {
    let none = insets(None, (3840, 1080), None, 1.0);
    assert_eq!(none.0, MarginF32::ZERO);
    let right = insets(None, (3840, 1080), Some([1920, 0, 1920, 1080]), 2.0);
    assert_eq!(
        Rect::from_min_size(pos2(0.0, 0.0), vec2(1920.0, 540.0)) - right,
        Rect::from_min_size(pos2(960.0, 0.0), vec2(960.0, 540.0))
    );
    // the session host's bar across the top of the window: both kept out
    let bar = egui::SafeAreaInsets(MarginF32 {
        top: 40.0,
        ..Default::default()
    });
    let both = insets(Some(bar), (3840, 1080), Some([0, 0, 1920, 1080]), 1.0);
    assert_eq!(
        Rect::from_min_size(pos2(0.0, 0.0), vec2(3840.0, 1080.0)) - both,
        Rect::from_min_max(pos2(0.0, 40.0), pos2(1920.0, 1080.0))
    );
    assert_eq!(insets(Some(bar), (3840, 1080), None, 1.0).0, bar.0, "the bar alone, as before");
}

/// A frame of the login screen in a 3840 x 1080 window over two 1920 x 1080
/// monitors, laid out as the app does it: `rect` the login's rectangle.
fn frame(
    ctx: &egui::Context,
    settings: &mut Settings,
    form: &mut LoginForm,
    rect: Option<[u32; 4]>,
    bar: Option<egui::SafeAreaInsets>,
) -> egui::FullOutput {
    let window = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(3840.0, 1080.0));
    let theme = crate::theme::Theme::default();
    theme.apply(ctx, 1.0);
    let room = insets(bar, (3840, 1080), rect, 1.0);
    set_sky(ctx, rect.map(|_| window - bar.unwrap_or_default()));
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(window),
            safe_area_insets: Some(room),
            ..Default::default()
        },
        |ui| {
            show(
                ui,
                &theme.palette(),
                &Icons::default(),
                None,
                settings,
                form,
                &LoginInfo::preview_release("0.3.0"),
                None,
            );
        },
    );
    crate::ui::login::test_output(output)
}

/// The widest rectangle anything of the frame is painted in: the sky's.
fn widest_paint(output: &egui::FullOutput) -> f32 {
    output.shapes.iter().map(|s| s.clip_rect.width()).fold(0.0, f32::max)
}

fn area(ctx: &egui::Context, name: &str) -> Option<Rect> {
    ctx.memory(|m| m.area_rect(egui::Id::unique(name)))
}

fn assert_inside(ctx: &egui::Context, names: &[&str], monitor: Rect) {
    for name in names {
        let shown = area(ctx, name).unwrap_or_else(|| panic!("{name} shown"));
        assert!(monitor.expand(0.5).contains_rect(shown), "{name} {shown:?} inside {monitor:?}");
    }
}

const SCREEN: [&str; 7] = [
    "login_area",
    "login_news",
    "login_version",
    "login_links",
    "login_settings",
    "login_accessibility",
    "login_quit",
];

#[test]
fn the_login_form_is_laid_out_on_one_monitor() {
    let left = Rect::from_min_size(pos2(0.0, 0.0), vec2(1920.0, 1080.0));
    let right = Rect::from_min_size(pos2(1920.0, 0.0), vec2(1920.0, 1080.0));
    for (rect, monitor) in [([0, 0, 1920, 1080], left), ([1920, 0, 1920, 1080], right)] {
        let ctx = egui::Context::default();
        let (mut settings, mut form) = (Settings::default(), LoginForm::default());
        form.show_mfa = true;
        let mut output = None;
        for _ in 0..4 {
            output = Some(frame(&ctx, &mut settings, &mut form, Some(rect), None));
        }
        assert_inside(&ctx, &SCREEN, monitor);
        let painted = widest_paint(&output.expect("drawn"));
        assert!(painted >= 3839.0, "the sky is painted over the window: {painted}");
        let card = area(&ctx, "login_area").expect("the login card");
        assert!(
            (card.center().x - monitor.center().x).abs() < 1.0,
            "centred on the monitor: {card:?}"
        );
        // the sky still fills the window
        assert_eq!(sky(&ctx), Rect::from_min_size(egui::Pos2::ZERO, vec2(3840.0, 1080.0)));
    }
}

#[test]
fn without_a_rectangle_the_login_fills_the_window_as_before() {
    let ctx = egui::Context::default();
    let (mut settings, mut form) = (Settings::default(), LoginForm::default());
    for _ in 0..4 {
        frame(&ctx, &mut settings, &mut form, None, None);
    }
    let card = area(&ctx, "login_area").expect("the login card");
    assert!((card.center().x - 1920.0).abs() < 1.0, "centred on the window: {card:?}");
    let quit = area(&ctx, "login_quit").expect("Quit");
    assert!(quit.right() > 3800.0, "in the window's corner: {quit:?}");
    assert_eq!(sky(&ctx), ctx.content_rect());
}

#[test]
fn the_login_keeps_under_the_session_hosts_bar_on_its_monitor() {
    let ctx = egui::Context::default();
    let (mut settings, mut form) = (Settings::default(), LoginForm::default());
    let bar = egui::SafeAreaInsets(MarginF32 {
        top: 48.0,
        ..Default::default()
    });
    for _ in 0..4 {
        frame(&ctx, &mut settings, &mut form, Some([0, 0, 1920, 1080]), Some(bar));
    }
    assert_inside(&ctx, &SCREEN, Rect::from_min_max(pos2(0.0, 48.0), pos2(1920.0, 1080.0)));
    assert_eq!(sky(&ctx), Rect::from_min_max(pos2(0.0, 48.0), pos2(3840.0, 1080.0)));
}

fn accounts(count: usize) -> (Settings, LoginForm) {
    let settings = Settings {
        username: "synthetic-0".into(),
        ..Default::default()
    };
    let form = LoginForm {
        accounts: (0..count)
            .map(|index| {
                let mut account = crate::login_accounts::Account::from_settings(&settings, true, None);
                account.username = format!("synthetic-{index}");
                account
            })
            .collect(),
        accounts_loaded: true,
        ..Default::default()
    };
    (settings, form)
}

#[test]
fn the_account_cards_and_the_second_factor_stay_on_one_monitor() {
    let right = Rect::from_min_size(pos2(1920.0, 0.0), vec2(1920.0, 1080.0));
    let ctx = egui::Context::default();
    let (mut settings, mut form) = accounts(3);
    for _ in 0..4 {
        frame(&ctx, &mut settings, &mut form, Some([1920, 0, 1920, 1080]), None);
    }
    assert_inside(&ctx, &["accounts_content", "login_links", "login_settings", "login_quit"], right);
    let cards = area(&ctx, "accounts_content").expect("the cards");
    assert!(
        (cards.center().x - right.center().x).abs() < 30.0,
        "centred on the monitor: {cards:?}"
    );
    // the second-factor challenge opens the account's dialog: on the monitor
    form.show_mfa = true;
    for _ in 0..6 {
        frame(&ctx, &mut settings, &mut form, Some([1920, 0, 1920, 1080]), None);
    }
    assert_inside(&ctx, &["account_login_dialog"], right);
}
