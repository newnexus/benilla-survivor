//! The focus frame: the trainer defended or escorted ([`survivor_server::focus`]) in a copy of the
//! target frame, under the player frame ([`FOCUS_AT`]). 1.12 has
//! no focus unit, so the frame is drawn here, from `TargetFrame.xml`'s layout in its own units
//! (232 x 100), scaled as the player frame is. The portrait is the client's own bake of the unit,
//! in the `"focus"` slot ([`benilla_app::SURVIVOR_FOCUS`]).

use bevy::prelude::*;
use std::sync::atomic::Ordering;

/// The frame measured against: always shown, 232 x 100 units at UIParent TOPLEFT -19, -4.
const PLAYER_FRAME: &str = "PlayerFrame";
const PLAYER_AT: Vec2 = Vec2::new(-19.0, 4.0);
/// Where the frame stands (UIParent TOPLEFT, y down): under the player frame, set in a little
/// (Donis's place; the later game's focus frame stood at 250, 240).
const FOCUS_AT: Vec2 = Vec2::new(20.0, 84.0);
const SIZE: Vec2 = Vec2::new(232.0, 100.0);
/// `UnitSelectionColor` of a friendly unit, which tints the name's band.
const FRIENDLY: Color = Color::srgb(0.0, 1.0, 0.0);
/// `GameFontNormalSmall`: 10 high, gold.
const FONT: f32 = 10.0;
const GOLD: Color = Color::srgb(1.0, 0.82, 0.0);

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, watch_frames)
        .add_systems(Update, show_focus);
}

fn watch_frames(mut frames: ResMut<benilla_app::SurvivorFrames>) {
    frames.watch.push(PLAYER_FRAME.to_owned());
}

#[derive(Component)]
struct FocusRoot;

#[derive(Component)]
struct FocusPortrait;

#[derive(Component)]
struct FocusHealth;

/// The frame on screen: whom for, and the unit size it was laid out at.
#[derive(Default)]
struct Shown {
    root: Option<Entity>,
    guid: u64,
    unit: f32,
    art: Option<Art>,
}

struct Art {
    frame: Handle<Image>,
    band: Handle<Image>,
    bar: Handle<Image>,
}

#[allow(clippy::too_many_arguments)]
fn show_focus(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    frames: Res<benilla_app::SurvivorFrames>,
    portrait: Res<benilla_app::SurvivorFocusPortrait>,
    mut images: ResMut<Assets<Image>>,
    mut faces: Query<(&mut ImageNode, &mut Visibility), With<FocusPortrait>>,
    mut bars: Query<(&mut Node, &mut ImageNode), (With<FocusHealth>, Without<FocusPortrait>)>,
    mut shown: Local<Shown>,
) {
    let focus = survivor_server::focus::current();
    let guid = focus.as_ref().map_or(0, |f| f.guid);
    benilla_app::SURVIVOR_FOCUS.store(guid, Ordering::Relaxed);
    let player = frames.rects.get(PLAYER_FRAME).copied();
    let unit = player.map_or(0.0, |r| r.height() / SIZE.y);
    // Gone, or for someone else, or the screen resized: laid out again.
    if shown.guid != guid || (unit - shown.unit).abs() > 0.01 {
        if let Some(root) = shown.root.take() {
            commands.entity(root).despawn();
        }
        shown.guid = 0;
    }
    let (Some(f), Some(player)) = (focus, player) else { return };
    if unit <= 0.0 {
        return;
    }
    if shown.root.is_none() {
        let Some(font) = screen.font() else { return };
        if shown.art.is_none() {
            let mut load = |path: &str| screen.icon(path).map(|i| images.add(i));
            let (Some(frame), Some(band), Some(bar)) = (
                load("Interface\\TargetingFrame\\UI-TargetingFrame"),
                load("Interface\\TargetingFrame\\UI-TargetingFrame-LevelBackground"),
                load("Interface\\TargetingFrame\\UI-StatusBar"),
            ) else {
                return;
            };
            shown.art = Some(Art { frame, band, bar });
        }
        let art = shown.art.as_ref().expect("loaded above");
        // UIParent's top left, from where the player frame stands in it.
        let origin = player.min - PLAYER_AT * unit;
        let at = origin + FOCUS_AT * unit;
        let px = |v: f32| Val::Px(v * unit);
        let place = |x: f32, y: f32, w: f32, h: f32| Node {
            position_type: PositionType::Absolute,
            left: px(x),
            top: px(y),
            width: px(w),
            height: px(h),
            ..default()
        };
        let text = |s: String| {
            (
                Text::new(s),
                TextFont {
                    font: font.clone(),
                    font_size: FONT * unit,
                    ..default()
                },
                TextColor(GOLD),
                TextShadow::default(),
                TextLayout::new_with_justify(Justify::Center),
            )
        };
        let root = commands
            .spawn((
                FocusRoot,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(at.x),
                    top: Val::Px(at.y),
                    width: px(SIZE.x),
                    height: px(SIZE.y),
                    ..default()
                },
                GlobalZIndex(crate::SCREEN_Z - 46),
                Pickable::IGNORE,
            ))
            .with_children(|r| {
                // The layers in TargetFrame.xml's order: the dark back and the name's band, the
                // portrait, the health bar, then the frame's art over them and its writing.
                r.spawn((place(7.0, 22.0, 119.0, 41.0), BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5))));
                r.spawn((
                    place(7.0, 22.0, 119.0, 19.0),
                    ImageNode::new(art.band.clone()).with_color(FRIENDLY),
                ));
                r.spawn((
                    FocusPortrait,
                    Node {
                        border_radius: BorderRadius::all(Val::Percent(50.0)),
                        ..place(126.0, 12.0, 64.0, 64.0)
                    },
                    ImageNode::default(),
                    Visibility::Hidden,
                ));
                r.spawn((
                    FocusHealth,
                    place(7.0, 41.0, 119.0, 12.0),
                    ImageNode::new(art.bar.clone()).with_color(FRIENDLY),
                ));
                // The art's 232 x 100 of its 256 x 128, from 24 across (TexCoords 0.09375).
                r.spawn((
                    place(0.0, 0.0, SIZE.x, SIZE.y),
                    ImageNode {
                        image: art.frame.clone(),
                        rect: Some(Rect::new(24.0, 0.0, 256.0, 100.0)),
                        ..default()
                    },
                ));
                // Name centred at (66, 31), level at (179, 66).
                r.spawn((
                    Node {
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..place(16.0, 25.0, 100.0, 12.0)
                    },
                ))
                .with_children(|n| {
                    n.spawn(text(f.name.clone()));
                });
                r.spawn((
                    Node {
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..place(167.0, 60.0, 24.0, 12.0)
                    },
                ))
                .with_children(|n| {
                    n.spawn(text(f.level.to_string()));
                });
            })
            .id();
        shown.root = Some(root);
        shown.guid = guid;
        shown.unit = unit;
    }
    // The bar's length and colour as `HealthBar_OnValueChanged` has it: green, through yellow
    // at half, to red.
    let share = if f.max == 0 { 0.0 } else { (f.health as f32 / f.max as f32).clamp(0.0, 1.0) };
    let (r, g) = if share > 0.5 { ((1.0 - share) * 2.0, 1.0) } else { (1.0, share * 2.0) };
    for (mut node, mut image) in &mut bars {
        let width = Val::Px(119.0 * share * unit);
        if node.width != width {
            node.width = width;
        }
        image.color = Color::srgb(r, g, 0.0);
    }
    for (mut image, mut vis) in &mut faces {
        match &portrait.0 {
            Some(h) => {
                if image.image != *h {
                    image.image = h.clone();
                }
                vis.set_if_neq(Visibility::Inherited);
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
}
