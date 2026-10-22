//! The run's end ([`survivor_server::run_summary`]): a window in the middle of the screen with how
//! it ended, the time, level, kills, gold and souls, staying up until Continue (or Enter)
//! closes it, or the next run begins.

use bevy::prelude::*;
use survivor_server::run_summary::{self, Summary};

const PAGE: Color = Color::srgba(0.035, 0.03, 0.025, 0.92);
const PAGE_EDGE: Color = Color::srgb(0.55, 0.45, 0.25);
const WHITE: Color = Color::srgb(0.95, 0.93, 0.88);
const DIM: Color = Color::srgb(0.62, 0.6, 0.56);
const DEATH: Color = Color::srgb(0.85, 0.2, 0.15);
const SOUL_BLUE: Color = Color::srgb(0.45, 0.85, 1.0);
const BUTTON: Color = Color::srgb(0.45, 0.08, 0.06);
const BUTTON_LIT: Color = Color::srgb(0.65, 0.12, 0.08);

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, (show_summary, close_summary));
}

#[derive(Component)]
struct SummaryRoot;

#[derive(Component)]
struct ContinueButton;

fn text(font: &Handle<Font>, s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s.into()),
        TextFont {
            font: font.clone(),
            font_size: size,
            ..default()
        },
        TextColor(color),
    )
}

fn show_summary(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    roots: Query<Entity, With<SummaryRoot>>,
    mut shown: Local<Option<(u64, f32)>>,
) {
    let current = run_summary::current();
    let height = window.single().map_or(720.0, Window::height);
    let wanted = current.as_ref().map(|s| (s.id, height));
    if wanted == *shown {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    *shown = wanted;
    let Some(s) = current else { return };
    let Some(font) = screen.font() else {
        *shown = None;
        return;
    };
    spawn_summary(&mut commands, &font, &s, height / 100.0);
}

fn spawn_summary(commands: &mut Commands, font: &Handle<Font>, s: &Summary, vh: f32) {
    let (title, color) = if s.died {
        ("You Fell".to_owned(), DEATH)
    } else {
        ("Run Complete".to_owned(), crate::GOLD)
    };
    let minutes = s.seconds as u32 / 60;
    let secs = s.seconds as u32 % 60;
    let money = |c: u32| format!("{c} gold");
    let rows: Vec<(String, String, Color)> = vec![
        ("Time survived".into(), format!("{minutes}:{secs:02}"), WHITE),
        ("Level reached".into(), s.level.to_string(), WHITE),
        ("Enemies slain".into(), s.kills.to_string(), WHITE),
        ("Gold found".into(), money(s.found), WHITE),
        (
            "Gold brought home".into(),
            money(s.brought),
            if s.died { DIM } else { crate::GOLD },
        ),
        ("Souls gathered".into(), s.souls.to_string(), SOUL_BLUE),
        (
            "Souls brought home".into(),
            s.souls_home.to_string(),
            if s.died { DIM } else { SOUL_BLUE },
        ),
    ];
    commands
        .spawn((
            SummaryRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            GlobalZIndex(crate::SCREEN_Z - 10),
            Pickable::IGNORE,
            UiTransform {
                scale: Vec2::splat(crate::SHRINK),
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: Val::Vh(62.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Vh(1.4),
                    padding: UiRect::all(Val::Vh(3.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Vh(1.2)),
                    ..default()
                },
                BackgroundColor(PAGE),
                BorderColor::all(PAGE_EDGE),
            ))
            .with_children(|page| {
                page.spawn(text(font, title, 7.0 * vh, color));
                let how = if s.died {
                    format!("Carried back to {}. What you found is lost.", s.town)
                } else {
                    format!("Safe in {}.", s.town)
                };
                page.spawn(text(font, how, 2.8 * vh, DIM));
                page.spawn((
                    Node {
                        height: Val::Px(1.5),
                        width: Val::Percent(100.0),
                        margin: UiRect::vertical(Val::Vh(0.8)),
                        ..default()
                    },
                    BackgroundColor(PAGE_EDGE),
                ));
                for (label, value, c) in rows {
                    page.spawn(Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        ..default()
                    })
                    .with_children(|r| {
                        r.spawn(text(font, label, 3.2 * vh, DIM));
                        r.spawn(text(font, value, 3.2 * vh, c));
                    });
                }
                page.spawn((
                    ContinueButton,
                    Button,
                    Node {
                        margin: UiRect::top(Val::Vh(2.0)),
                        padding: UiRect::axes(Val::Vh(4.0), Val::Vh(1.2)),
                        border: UiRect::all(Val::Px(1.5)),
                        border_radius: BorderRadius::all(Val::Vh(1.0)),
                        ..default()
                    },
                    BackgroundColor(BUTTON),
                    BorderColor::all(PAGE_EDGE),
                ))
                .with_children(|b| {
                    b.spawn(text(font, "Continue   [Enter]", 3.6 * vh, crate::GOLD));
                });
            });
        });
}

fn close_summary(
    keys: Res<ButtonInput<KeyCode>>,
    mut buttons: Query<(&Interaction, &mut BackgroundColor), (With<ContinueButton>, Changed<Interaction>)>,
) {
    if run_summary::current().is_none() {
        return;
    }
    let mut close = keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]);
    for (i, mut bg) in &mut buttons {
        match i {
            Interaction::Pressed => close = true,
            Interaction::Hovered => bg.0 = BUTTON_LIT,
            Interaction::None => bg.0 = BUTTON,
        }
    }
    if close {
        run_summary::close();
    }
}
