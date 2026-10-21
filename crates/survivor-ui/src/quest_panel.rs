//! The survivor's quest giver window, in place of the stock one: the giver's story on a sheet of
//! parchment, the objectives and the rewards floating beside it on smaller sheets, one big
//! Accept (or Complete), and an X to close. The text is all there at once and scrolls with the
//! mouse wheel or a finger; a thumb on the window moves no one.
//!
//! The server posts the window through [`survivor_server::quest_board`] and takes the answer the
//! same way.

use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::input::touch::Touches;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::RelativeCursorPosition;
use survivor_server::quest_board::{self, Answer, Panel, PanelKind};

/// Ink on parchment, and the headings'.
const INK: Color = Color::srgb(0.2, 0.12, 0.04);
const HEADING: Color = Color::srgb(0.1, 0.05, 0.0);
const BUTTON_RED: Color = Color::srgb(0.5, 0.04, 0.02);
const BUTTON_RED_LIT: Color = Color::srgb(0.68, 0.08, 0.04);
const BUTTON_EDGE: Color = Color::srgb(0.75, 0.6, 0.25);
const PANEL_Z: i32 = crate::SCREEN_Z - 10;
/// A mouse wheel notch scrolls this many pixels.
const WHEEL_LINE: f32 = 40.0;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, (show_panel, press_buttons, light_buttons, scroll_text, block_touch));
}

#[derive(Component)]
struct PanelRoot;

/// The story's scrolling area.
#[derive(Component)]
struct StoryScroll;

/// A part of the window a thumb must not walk through.
#[derive(Component)]
struct Sheet;

#[derive(Component)]
struct PanelButton {
    panel: u64,
    answer: Answer,
}

#[derive(Default)]
struct Shown {
    root: Option<Entity>,
    panel: Option<(u64, f32)>,
    parchment: Option<Handle<Image>>,
}

fn show_panel(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    mut images: ResMut<Assets<Image>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut shown: Local<Shown>,
) {
    let panel = quest_board::current();
    let height = window.single().map_or(720.0, Window::height);
    let wanted = panel.as_ref().map(|p| (p.id, height));
    if wanted == shown.panel {
        return;
    }
    if let Some(root) = shown.root.take() {
        commands.entity(root).despawn();
    }
    shown.panel = None;
    let Some(panel) = panel else { return };
    let Some(library) = &screen.library else { return };
    let library = library.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let parchment = shown
        .parchment
        .get_or_insert_with(|| images.add(parchment()))
        .clone();
    // Gold, silver and copper side by side in one texture, a quarter of its width each.
    let coins: [Option<Coin>; 3] = library
        .blp("Interface\\MoneyFrame\\UI-MoneyIcons")
        .map(|i| {
            let size = i.size().as_vec2();
            let handle = images.add(i);
            std::array::from_fn(|k| {
                let w = size.x / 4.0;
                Some(Coin {
                    image: handle.clone(),
                    rect: Rect::new(w * k as f32, 0.0, w * (k + 1) as f32, size.y),
                })
            })
        })
        .unwrap_or_default();
    let fonts = (library.font.clone(), library.title_font.clone());
    drop(library);
    let root = spawn_panel(&mut commands, &panel, &parchment, &coins, &fonts);
    shown.root = Some(root);
    shown.panel = wanted;
}

fn spawn_panel(
    commands: &mut Commands,
    panel: &Panel,
    parchment: &Handle<Image>,
    coins: &[Option<Coin>; 3],
    (font, title_font): &(Handle<Font>, Handle<Font>),
) -> Entity {
    let text = |s: &str, f: &Handle<Font>, size_vh: f32, color: Color| {
        (
            Text::new(s.to_owned()),
            TextFont {
                font: f.clone(),
                font_size: 1.0,
                ..default()
            },
            TextColor(color),
            VhText(size_vh),
        )
    };
    let sheet = |extra: Node| {
        (
            Sheet,
            ImageNode::new(parchment.clone()).with_mode(NodeImageMode::Stretch),
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Vh(3.0)),
                row_gap: Val::Vh(1.2),
                ..extra
            },
        )
    };
    let id = panel.id;
    let accept_label = match panel.kind {
        PanelKind::Offer => "Accept",
        PanelKind::Reward => "Complete",
    };
    commands
        .spawn((
            PanelRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                column_gap: Val::Vh(2.5),
                ..default()
            },
            GlobalZIndex(PANEL_Z),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            // The story.
            root.spawn(sheet(Node {
                width: Val::Vh(78.0),
                max_width: Val::Vw(50.0),
                height: Val::Vh(86.0),
                ..default()
            }))
            .with_children(|s| {
                s.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::FlexStart,
                        ..default()
                    },
                ))
                .with_children(|h| {
                    h.spawn((text(&panel.title, title_font, 6.0, HEADING), Node {
                        flex_shrink: 1.0,
                        ..default()
                    }));
                    h.spawn((
                        PanelButton {
                            panel: id,
                            answer: Answer::Close,
                        },
                        Button,
                        Node {
                            width: Val::Vh(7.0),
                            height: Val::Vh(7.0),
                            flex_shrink: 0.0,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(Val::Vh(0.35)),
                            border_radius: BorderRadius::all(Val::Vh(1.0)),
                            ..default()
                        },
                        BackgroundColor(BUTTON_RED),
                        BorderColor::all(BUTTON_EDGE),
                    ))
                    .with_children(|b| {
                        b.spawn(text("X", font, 4.5, crate::GOLD));
                    });
                });
                s.spawn((
                    StoryScroll,
                    RelativeCursorPosition::default(),
                    ScrollPosition::default(),
                    Node {
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        overflow: Overflow::scroll_y(),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                ))
                .with_children(|t| {
                    t.spawn(text(&panel.text, font, 3.4, INK));
                });
            });
            // The objectives and rewards, floating beside it, and the button under them.
            root.spawn(Node {
                width: Val::Vh(62.0),
                max_width: Val::Vw(40.0),
                height: Val::Vh(86.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Vh(2.5),
                ..default()
            })
            .with_children(|col| {
                if panel.kind == PanelKind::Offer && !panel.objectives.is_empty() {
                    col.spawn(sheet(Node::default())).with_children(|o| {
                        o.spawn(text("Quest Objectives", title_font, 5.0, HEADING));
                        o.spawn(text(&panel.objectives, font, 3.3, INK));
                    });
                }
                col.spawn(sheet(Node::default())).with_children(|r| {
                    r.spawn(text("Rewards", title_font, 5.0, HEADING));
                    r.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Vh(1.0),
                        ..default()
                    })
                    .with_children(|line| {
                        line.spawn(text("You will receive:", font, 3.3, INK));
                        let (g, s, c) = (panel.money / 10_000, panel.money / 100 % 100, panel.money % 100);
                        for (amount, coin) in [(g, &coins[0]), (s, &coins[1]), (c, &coins[2])] {
                            if amount == 0 && !(panel.money == 0 && std::ptr::eq(coin, &coins[2])) {
                                continue;
                            }
                            line.spawn(text(&amount.to_string(), font, 3.6, HEADING));
                            if let Some(coin) = coin {
                                line.spawn((
                                    ImageNode {
                                        image: coin.image.clone(),
                                        rect: Some(coin.rect),
                                        ..default()
                                    },
                                    Node {
                                        width: Val::Vh(3.2),
                                        height: Val::Vh(3.2),
                                        ..default()
                                    },
                                ));
                            }
                        }
                    });
                    if panel.xp > 0 {
                        r.spawn(text(&format!("Experience: {}", panel.xp), font, 3.3, INK));
                    }
                });
                col.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                col.spawn((
                    PanelButton {
                        panel: id,
                        answer: Answer::Accept,
                    },
                    Button,
                    Sheet,
                    Node {
                        align_self: AlignSelf::FlexEnd,
                        width: Val::Vh(34.0),
                        height: Val::Vh(10.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(Val::Vh(0.45)),
                        border_radius: BorderRadius::all(Val::Vh(1.4)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_RED),
                    BorderColor::all(BUTTON_EDGE),
                ))
                .with_children(|b| {
                    b.spawn(text(accept_label, font, 5.0, crate::GOLD));
                });
            });
        })
        .id()
}

/// One coin of the money texture.
#[derive(Clone)]
struct Coin {
    image: Handle<Image>,
    rect: Rect,
}

/// A text's size in screen heights, set as the window is laid out.
#[derive(Component)]
struct VhText(f32);

fn press_buttons(buttons: Query<(&PanelButton, &Interaction), Changed<Interaction>>) {
    for (b, i) in &buttons {
        if *i == Interaction::Pressed {
            quest_board::answer(b.panel, b.answer);
        }
    }
}

fn light_buttons(
    mut buttons: Query<(&Interaction, &mut BackgroundColor), (With<PanelButton>, Changed<Interaction>)>,
    mut texts: Query<(&VhText, &mut TextFont), Added<VhText>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    for (i, mut bg) in &mut buttons {
        bg.0 = if *i == Interaction::None { BUTTON_RED } else { BUTTON_RED_LIT };
    }
    let vh = window.single().map_or(7.2, |w| w.height() / 100.0);
    for (size, mut f) in &mut texts {
        f.font_size = size.0 * vh;
    }
}

/// The story scrolls with the wheel over it, and with a thumb dragged on it.
fn scroll_text(
    mut wheel: MessageReader<MouseWheel>,
    touches: Res<Touches>,
    mut areas: Query<(&RelativeCursorPosition, &mut ScrollPosition, &ComputedNode, &UiGlobalTransform), With<StoryScroll>>,
) {
    let notches: f32 = wheel
        .read()
        .map(|w| match w.unit {
            MouseScrollUnit::Line => w.y * WHEEL_LINE,
            MouseScrollUnit::Pixel => w.y,
        })
        .sum();
    for (cursor, mut pos, node, at) in &mut areas {
        let mut dy = 0.0;
        if cursor.cursor_over {
            dy -= notches;
        }
        let k = node.inverse_scale_factor();
        for t in touches.iter() {
            // A thumb put down on the story drags it.
            if node.contains_point(*at, t.start_position() / k) {
                dy -= t.delta().y;
            }
        }
        if dy != 0.0 {
            let top = ((node.content_size.y - node.size.y) * k).max(0.0);
            pos.0.y = (pos.0.y + dy).clamp(0.0, top);
        }
    }
}

/// Tell the touch controls where the window is, so a thumb on it moves no one.
fn block_touch(
    sheets: Query<(&ComputedNode, &UiGlobalTransform), With<Sheet>>,
    mut frames: ResMut<benilla_app::SurvivorFrames>,
) {
    frames.blockers.clear();
    for (node, at) in &sheets {
        let k = node.inverse_scale_factor();
        let centre = at.affine().translation * k;
        let half = node.size * k / 2.0;
        frames.blockers.push(Rect::from_center_half_size(centre, half));
    }
}

/// A sheet of parchment: warm paper with mottling, darker and burnt toward its edges.
fn parchment() -> Image {
    const W: usize = 256;
    const H: usize = 256;
    let hash = |x: i32, y: i32| {
        let n = (x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263)) as u32;
        let n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
        (n ^ (n >> 16)) as f32 / u32::MAX as f32
    };
    let noise = |x: f32, y: f32| {
        let (ix, iy) = (x.floor() as i32, y.floor() as i32);
        let (fx, fy) = (x - ix as f32, y - iy as f32);
        let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
        let a = hash(ix, iy) + (hash(ix + 1, iy) - hash(ix, iy)) * sx;
        let b = hash(ix, iy + 1) + (hash(ix + 1, iy + 1) - hash(ix, iy + 1)) * sx;
        a + (b - a) * sy
    };
    let mut px = vec![0u8; W * H * 4];
    for y in 0..H {
        for x in 0..W {
            let (u, v) = (x as f32 / W as f32, y as f32 / H as f32);
            let mut n = 0.0;
            let mut amp = 0.5;
            let mut f = 4.0;
            for _ in 0..4 {
                n += amp * noise(u * f, v * f);
                f *= 2.0;
                amp *= 0.5;
            }
            // Distance to the nearest edge, roughened, for the burnt rim.
            let edge = u.min(1.0 - u).min(v).min(1.0 - v) + (n - 0.5) * 0.03;
            let rim = (edge / 0.06).clamp(0.0, 1.0);
            let shade = (0.82 + 0.22 * n) * (0.45 + 0.55 * rim.powf(0.6));
            let (r, g, b) = (0.93 * shade, 0.80 * shade, 0.56 * shade);
            let i = (y * W + x) * 4;
            px[i..i + 4].copy_from_slice(&[
                (r.min(1.0) * 255.0) as u8,
                (g.min(1.0) * 255.0) as u8,
                (b.min(1.0) * 255.0) as u8,
                255,
            ]);
        }
    }
    Image::new(
        Extent3d {
            width: W as u32,
            height: H as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        px,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}
