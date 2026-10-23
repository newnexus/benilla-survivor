//! The survivor's quest giver window, in place of the stock one, in two steps. Walking up to a
//! giver starts them talking: a dark, tinted page with their portrait and name, the quest's
//! title, and its story in white, appearing as they speak it (paced by their spoken line, the
//! VoiceOver addon's). Continue (or Space) skips ahead; when they finish, or on Continue, a
//! second page brings the objectives, the rewards, and Accept (or Complete) and Decline. An X
//! closes it at any time. The story, the objectives and rewards, and a long list of quests
//! scroll with the mouse wheel or a finger, their top and bottom edges fading into the page
//! while there is more that way; a thumb on the window moves no one.
//!
//! The server posts the window through [`survivor_server::quest_board`] and takes the answer the
//! same way.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::input::touch::Touches;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use survivor_server::quest_board::{self, Answer, Panel, PanelKind};

/// The pages: a dark tint over the world, a faint gold edge, white text, gold headings.
pub(crate) const PAGE: Color = Color::srgba(0.035, 0.03, 0.025, 0.86);
pub(crate) const PAGE_EDGE: Color = Color::srgba(0.8, 0.62, 0.28, 0.5);
pub(crate) const WHITE: Color = Color::srgb(0.95, 0.93, 0.88);
const DIM: Color = Color::srgb(0.68, 0.65, 0.58);
const TAB: Color = Color::srgba(1.0, 1.0, 1.0, 0.04);
const TAB_ON: Color = Color::srgba(0.85, 0.65, 0.25, 0.22);
const BUTTON_RED: Color = Color::srgb(0.5, 0.04, 0.02);
const BUTTON_RED_LIT: Color = Color::srgb(0.68, 0.08, 0.04);
const BUTTON_DARK: Color = Color::srgb(0.16, 0.14, 0.12);
const BUTTON_DARK_LIT: Color = Color::srgb(0.26, 0.23, 0.2);
const BUTTON_EDGE: Color = Color::srgb(0.75, 0.6, 0.25);
const PANEL_Z: i32 = crate::SCREEN_Z - 10;
/// A mouse wheel notch scrolls this many pixels.
const WHEEL_LINE: f32 = 40.0;
/// Without a spoken line, the story appears at about this many letters a second, a speaking pace.
const LETTERS_PER_SECOND: f32 = 16.0;
/// The portrait's frame: its gold, and its size in screen heights.
const FRAME_GOLD: Color = Color::srgb(0.85, 0.65, 0.2);
const PORTRAIT: f32 = 13.0;
/// A scrolling area's fade at an edge with more beyond it: its depth in screen heights.
const FADE: f32 = 7.0;
/// The fade: a faint gold glint at the very edge, then the page's dark, then nothing.
const FADE_GLINT: Color = Color::srgba(0.85, 0.65, 0.25, 0.28);
const FADE_DARK: Color = Color::srgba(0.035, 0.03, 0.025, 0.95);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Talk>()
        .init_resource::<RewardPick>().add_systems(
        Update,
        (
            show_panel,
            reveal_story,
            show_portrait,
            press_buttons,
            light_buttons,
            scroll_text,
            show_fades,
            show_preview,
            block_touch,
        )
            .chain(),
    );
}

#[derive(Component)]
struct PanelRoot;

/// The speaking giver's portrait on the window, filled once the client has baked it.
#[derive(Component)]
struct TalkerPortrait;

/// The story's scrolling area: kept on its newest line as it is told.
#[derive(Component)]
struct StoryScroll;

/// A scrolling area of the window: the wheel over it or a thumb on it moves it.
#[derive(Component)]
struct ScrollArea;

/// The fade over a scrolling area's top (or bottom) edge, shown while there is more that way.
#[derive(Component)]
struct Fade {
    area: Entity,
    top: bool,
}

/// The story's text, appearing as the giver speaks it.
#[derive(Component)]
struct Story(String);

/// A part of the window a thumb must not walk through.
#[derive(Component)]
pub(crate) struct Sheet;

/// A button answering the server: Accept, or Decline and X (both close).
#[derive(Component)]
struct PanelButton {
    panel: u64,
    answer: Answer,
}

/// The talk step's button: on to the objectives.
#[derive(Component)]
struct ContinueButton;

/// The two steps of the window.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Step {
    /// The giver speaks, the story appears.
    #[default]
    Talk,
    /// The objectives, the rewards, Accept and Decline.
    Decide,
}

/// The talk on screen: when it began, how long it lasts, and the step the window is at.
#[derive(Resource, Default)]
struct Talk {
    start: f32,
    seconds: f32,
    step: Step,
}

impl Talk {
    /// How much of the story is out, 0 to 1.
    fn told(&self, now: f32) -> f32 {
        match self.step {
            Step::Decide => 1.0,
            Step::Talk => ((now - self.start) / self.seconds.max(0.1)).clamp(0.0, 1.0),
        }
    }
}

#[derive(Default)]
struct Shown {
    root: Option<Entity>,
    /// The window, its tab, the screen height and the step it was laid out for.
    panel: Option<(u64, usize, f32, Step)>,
    /// The tab open, for the window `.0`.
    tab: (u64, usize),
    /// The quest whose talk began last: the window and its tab.
    spoken: Option<(u64, usize)>,
}

/// The chosen reward's preview beside the window: its icon and tooltip, filled by
/// [`show_preview`].
#[derive(Component)]
struct RewardPreview;

/// A quest's tab on the window's left.
#[derive(Component)]
struct QuestTab(usize);

#[allow(clippy::too_many_arguments)]
fn show_panel(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    mut images: ResMut<Assets<Image>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    tabs: Query<(&QuestTab, &Interaction), Changed<Interaction>>,
    continues: Query<&Interaction, (With<ContinueButton>, Changed<Interaction>)>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut talk: ResMut<Talk>,
    mut shown: Local<Shown>,
    mut pick: ResMut<RewardPick>,
) {
    let panel = quest_board::current();
    let height = window.single().map_or(720.0, Window::height);
    let now = time.elapsed_secs();
    if let Some(p) = &panel {
        if shown.tab.0 != p.id {
            shown.tab = (p.id, 0);
        }
        for (t, i) in &tabs {
            if *i == Interaction::Pressed && t.0 < p.entries.len() {
                shown.tab.1 = t.0;
            }
        }
    }
    let selected = shown.tab.1;
    // A new quest on screen: the giver starts reading it aloud (the VoiceOver addon's line,
    // `survivor_server::quest_voice`), and the story starts to appear; another tab or a closed
    // window cuts the line off.
    let line = panel
        .as_ref()
        .and_then(|p| p.entries.get(selected).map(|e| ((p.id, selected), e)));
    if line.map(|l| l.0) != shown.spoken {
        shown.spoken = line.map(|l| l.0);
        // The giver is the client's "npc" unit while their window is open: their portrait bakes.
        benilla_app::SURVIVOR_TALKER.store(
            panel.as_ref().map_or(0, |p| p.giver),
            std::sync::atomic::Ordering::Relaxed,
        );
        let voice = line
            .and_then(|(_, e)| survivor_server::quest_voice(e.quest, e.kind == PanelKind::Reward));
        let spoken = voice.is_some();
        benilla_app::survivor_voice(voice);
        if let Some((_, e)) = line {
            let complete = e.kind == PanelKind::Reward;
            let read = e.text.chars().count() as f32 / LETTERS_PER_SECOND;
            talk.start = now;
            talk.seconds = survivor_server::quest_voice_seconds(e.quest, complete)
                .filter(|_| spoken)
                .unwrap_or(read);
            talk.step = Step::Talk;
        }
    }
    // On to the objectives: the talk ends, or the player goes on.
    let go_on =
        keys.just_pressed(KeyCode::Space) || continues.iter().any(|i| *i == Interaction::Pressed);
    if line.is_some() && talk.step == Step::Talk && (go_on || now - talk.start >= talk.seconds) {
        talk.step = Step::Decide;
    }
    let wanted = panel.as_ref().map(|p| (p.id, selected, height, talk.step));
    if wanted == shown.panel {
        return;
    }
    if let Some(root) = shown.root.take() {
        commands.entity(root).despawn();
    }
    shown.panel = None;
    let Some(panel) = panel else { return };
    let Some(library) = &screen.library else {
        return;
    };
    let library = library
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    // The reward items' icons.
    let icons: std::collections::HashMap<u32, Handle<Image>> = panel
        .entries
        .iter()
        .flat_map(|e| e.rewards.iter())
        .filter_map(|r| Some((r.entry, images.add(library.item_icon(r.display)?))))
        .collect();
    drop(library);
    if panel.entries.is_empty() {
        return;
    }
    let selected = selected.min(panel.entries.len() - 1);
    // The first reward chosen until the player picks another.
    let first = panel.entries[selected].rewards.first().map_or(0, |r| r.entry);
    if pick.0 != (panel.id, selected) {
        *pick = RewardPick((panel.id, selected), first);
    }
    let root = spawn_panel(&mut commands, &panel, selected, talk.step, &coins, &fonts, &icons, pick.1);
    shown.root = Some(root);
    shown.panel = wanted;
}

/// A dark page of the window.
fn page(extra: Node) -> impl Bundle {
    (
        Sheet,
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Vh(3.0)),
            row_gap: Val::Vh(1.4),
            border: UiRect::all(Val::Px(1.5)),
            border_radius: BorderRadius::all(Val::Vh(1.2)),
            ..extra
        },
        BackgroundColor(PAGE),
        BorderColor::all(PAGE_EDGE),
    )
}

/// A scrolling column taking what its page leaves (`extra` added to it, its rows `gap` apart),
/// filled by `fill`, with a fade over each edge.
fn scroll_area(
    parent: &mut ChildSpawnerCommands,
    extra: impl Bundle,
    gap: Val,
    fill: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            flex_shrink: 1.0,
            min_height: Val::Px(0.0),
            ..default()
        })
        .with_children(|w| {
            let area = w
                .spawn((
                    extra,
                    ScrollArea,
                    RelativeCursorPosition::default(),
                    ScrollPosition::default(),
                    Node {
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        flex_shrink: 1.0,
                        min_height: Val::Px(0.0),
                        row_gap: gap,
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ))
                .with_children(fill)
                .id();
            for top in [true, false] {
                let stops = vec![
                    ColorStop::percent(FADE_GLINT, 0.0),
                    ColorStop::percent(FADE_DARK, 12.0),
                    ColorStop::percent(Color::NONE, 100.0),
                ];
                w.spawn((
                    Fade { area, top },
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        right: Val::Px(0.0),
                        top: if top { Val::Px(0.0) } else { Val::Auto },
                        bottom: if top { Val::Auto } else { Val::Px(0.0) },
                        height: Val::Vh(FADE),
                        ..default()
                    },
                    BackgroundGradient::from(if top {
                        LinearGradient::to_bottom(stops)
                    } else {
                        LinearGradient::to_top(stops)
                    }),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                ));
            }
        });
}

/// A button's colours, at rest and under the cursor.
#[derive(Component, Clone, Copy)]
struct Shade(Color, Color);

const RED: Shade = Shade(BUTTON_RED, BUTTON_RED_LIT);
const DARK: Shade = Shade(BUTTON_DARK, BUTTON_DARK_LIT);

/// A big button in `shade`, its label added by the caller.
fn button(shade: Shade, width_vh: f32) -> impl Bundle {
    (
        shade,
        Button,
        Sheet,
        Node {
            width: Val::Vh(width_vh),
            height: Val::Vh(8.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Vh(0.35)),
            border_radius: BorderRadius::all(Val::Vh(1.2)),
            ..default()
        },
        BackgroundColor(shade.0),
        BorderColor::all(BUTTON_EDGE),
    )
}

fn spawn_panel(
    commands: &mut Commands,
    panel: &Panel,
    selected: usize,
    step: Step,
    coins: &[Option<Coin>; 3],
    (font, title_font): &(Handle<Font>, Handle<Font>),
    icons: &std::collections::HashMap<u32, Handle<Image>>,
    chosen: u32,
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
    let id = panel.id;
    let e = &panel.entries[selected];
    let accept_label = match e.kind {
        PanelKind::Offer => "Accept",
        PanelKind::Reward => "Complete",
    };
    // The talk stands on the left of the screen, as WoW's own quest window; the choice after it
    // in the middle.
    let left = step == Step::Talk;
    commands
        .spawn((
            PanelRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                justify_content: if left {
                    JustifyContent::FlexStart
                } else {
                    JustifyContent::Center
                },
                align_items: AlignItems::Center,
                column_gap: Val::Vh(2.0),
                padding: UiRect::left(Val::Vh(if left { 3.0 } else { 0.0 })),
                ..default()
            },
            GlobalZIndex(PANEL_Z),
            Pickable::IGNORE,
            // Shrunk about the screen's centre, then, on the left, slid back to its left edge.
            UiTransform {
                translation: Val2::new(
                    Val::Percent(if left {
                        -(1.0 - crate::SHRINK) * 50.0
                    } else {
                        0.0
                    }),
                    Val::Px(0.0),
                ),
                scale: Vec2::splat(crate::SHRINK),
                ..default()
            },
        ))
        .with_children(|root| {
            // The giver's quests, a tab each, when there is more than one.
            if panel.entries.len() > 1 {
                root.spawn(page(Node {
                    width: Val::Vh(30.0),
                    max_height: Val::Vh(74.0),
                    align_self: AlignSelf::FlexStart,
                    margin: UiRect::top(Val::Vh(10.0)),
                    padding: UiRect::all(Val::Vh(2.0)),
                    row_gap: Val::Vh(1.0),
                    ..default()
                }))
                .with_children(|page| {
                    page.spawn(text("Quests", title_font, 4.0, crate::GOLD));
                    scroll_area(page, (), Val::Vh(1.0), |tabs| {
                    for (k, q) in panel.entries.iter().enumerate() {
                        let mark = match q.kind {
                            PanelKind::Offer => "!",
                            PanelKind::Reward => "?",
                        };
                        tabs.spawn((
                            QuestTab(k),
                            Button,
                            Node {
                                flex_direction: FlexDirection::Row,
                                column_gap: Val::Vh(1.0),
                                padding: UiRect::axes(Val::Vh(1.2), Val::Vh(0.8)),
                                border_radius: BorderRadius::all(Val::Vh(0.8)),
                                ..default()
                            },
                            BackgroundColor(if k == selected { TAB_ON } else { TAB }),
                        ))
                        .with_children(|b| {
                            b.spawn(text(mark, title_font, 3.4, crate::GOLD));
                            b.spawn((
                                text(&q.title, font, 2.8, WHITE),
                                Node {
                                    flex_shrink: 1.0,
                                    ..default()
                                },
                            ));
                        });
                    }
                    });
                });
            }
            // The talk: who speaks, the quest, the story as it is told.
            root.spawn(page(Node {
                width: Val::Vh(84.0),
                max_width: Val::Vw(52.0),
                height: Val::Vh(74.0),
                ..default()
            }))
            .with_children(|s| {
                s.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Vh(2.0),
                    ..default()
                })
                .with_children(|h| {
                    h.spawn((
                        Node {
                            width: Val::Vh(PORTRAIT),
                            height: Val::Vh(PORTRAIT),
                            flex_shrink: 0.0,
                            border: UiRect::all(Val::Vh(0.5)),
                            border_radius: BorderRadius::all(Val::Percent(50.0)),
                            padding: UiRect::all(Val::Vh(0.25)),
                            ..default()
                        },
                        BorderColor::all(FRAME_GOLD),
                        BackgroundColor(Color::srgb(0.02, 0.015, 0.01)),
                    ))
                    .with_children(|frame| {
                        frame.spawn((
                            TalkerPortrait,
                            ImageNode::default(),
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Percent(100.0),
                                border_radius: BorderRadius::all(Val::Percent(50.0)),
                                ..default()
                            },
                            Visibility::Hidden,
                        ));
                    });
                    h.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        flex_shrink: 1.0,
                        row_gap: Val::Vh(0.4),
                        ..default()
                    })
                    .with_children(|names| {
                        if !panel.giver_name.is_empty() {
                            names.spawn(text(&panel.giver_name, title_font, 3.0, crate::GOLD));
                        }
                        names.spawn(text(&e.title, title_font, 5.4, WHITE));
                    });
                    h.spawn((
                        PanelButton {
                            panel: id,
                            answer: Answer::Close,
                        },
                        Button,
                        RED,
                        Node {
                            width: Val::Vh(6.0),
                            height: Val::Vh(6.0),
                            flex_shrink: 0.0,
                            align_self: AlignSelf::FlexStart,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(Val::Vh(0.3)),
                            border_radius: BorderRadius::all(Val::Vh(1.0)),
                            ..default()
                        },
                        BackgroundColor(BUTTON_RED),
                        BorderColor::all(BUTTON_EDGE),
                    ))
                    .with_children(|b| {
                        b.spawn(text("X", font, 3.8, crate::GOLD));
                    });
                });
                // A thin gold rule under the heading.
                s.spawn((
                    Node {
                        height: Val::Px(1.5),
                        width: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(PAGE_EDGE),
                ));
                // Step one: the story as it is told. Step two takes its place on the same page.
                if step == Step::Talk {
                    scroll_area(s, StoryScroll, Val::Px(0.0), |t| {
                        t.spawn((Story(e.text.clone()), text("", font, 3.4, WHITE)));
                    });
                    // While the giver talks: go on without waiting.
                    s.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::FlexEnd,
                        ..default()
                    })
                    .with_children(|f| {
                        f.spawn((ContinueButton, button(DARK, 34.0)))
                            .with_children(|b| {
                                b.spawn(text("Continue   [Space]", font, 3.4, crate::GOLD));
                            });
                    });
                }
                // Then: what to do, what it pays, and the answer.
                if step == Step::Decide {
                    scroll_area(s, (), Val::Vh(1.2), |col| {
                        if e.kind == PanelKind::Offer && !e.objectives.is_empty() {
                            col.spawn(text("Quest Objectives", title_font, 4.4, crate::GOLD));
                            col.spawn(text(&e.objectives, font, 3.2, WHITE));
                        }
                        col.spawn(text("Rewards", title_font, 4.4, crate::GOLD));
                        if e.money > 0 {
                            col.spawn(Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: Val::Vh(1.0),
                                ..default()
                            })
                            .with_children(|line| {
                                line.spawn(text("You will receive:", font, 3.2, DIM));
                                let (g, s, c) =
                                    (e.money, 0, 0);
                                for (amount, coin) in
                                    [(g, &coins[0]), (s, &coins[1]), (c, &coins[2])]
                                {
                                    if amount == 0 {
                                        continue;
                                    }
                                    line.spawn(text(&amount.to_string(), font, 3.4, WHITE));
                                    if let Some(coin) = coin {
                                        line.spawn((
                                            ImageNode {
                                                image: coin.image.clone(),
                                                rect: Some(coin.rect),
                                                ..default()
                                            },
                                            Node {
                                                width: Val::Vh(3.0),
                                                height: Val::Vh(3.0),
                                                ..default()
                                            },
                                        ));
                                    }
                                }
                            });
                        }
                        if e.xp > 0 {
                            col.spawn(text(&format!("Experience: {}", e.xp), font, 3.2, WHITE));
                        }
                        // The items to choose one of: click one, the gold frame is the choice.
                        if !e.rewards.is_empty() {
                            col.spawn(text("Choose your reward:", font, 3.2, DIM));
                            col.spawn(Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Vh(0.8),
                                ..default()
                            })
                            .with_children(|list| {
                                for r in &e.rewards {
                                    let mut b = list.spawn((
                                        RewardChoice(r.entry),
                                        Button,
                                        Node {
                                            flex_direction: FlexDirection::Row,
                                            align_items: AlignItems::Center,
                                            column_gap: Val::Vh(1.2),
                                            padding: UiRect::all(Val::Vh(0.6)),
                                            border: UiRect::all(Val::Vh(0.3)),
                                            border_radius: BorderRadius::all(Val::Vh(0.8)),
                                            ..default()
                                        },
                                        BorderColor::all(if r.entry == chosen {
                                            crate::GOLD
                                        } else {
                                            Color::NONE
                                        }),
                                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
                                    ));
                                    b.with_children(|row| {
                                        let mut icon = row.spawn(Node {
                                            width: Val::Vh(5.0),
                                            height: Val::Vh(5.0),
                                            ..default()
                                        });
                                        if let Some(h) = icons.get(&r.entry) {
                                            icon.insert(ImageNode::new(h.clone()));
                                        }
                                        row.spawn(text(&r.name, font, 3.0, WHITE));
                                    });
                                }
                            });
                        }
                    });
                    // The answer stays at the page's foot, the list above it scrolling.
                    s.spawn(Node {
                            flex_direction: FlexDirection::Row,
                            justify_content: JustifyContent::SpaceBetween,
                            column_gap: Val::Vh(2.0),
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn((
                                PanelButton {
                                    panel: id,
                                    answer: Answer::Close,
                                },
                                button(DARK, 24.0),
                            ))
                            .with_children(|b| {
                                b.spawn(text("Decline", font, 4.0, crate::GOLD));
                            });
                            row.spawn((
                                PanelButton {
                                    panel: id,
                                    // The reward is filled in as it is pressed.
                                    answer: Answer::Accept(e.quest, 0),
                                },
                                button(RED, 26.0),
                            ))
                            .with_children(|b| {
                                b.spawn(text(accept_label, font, 4.4, crate::GOLD));
                            });
                        });
                }
            });
            // The reward chosen, shown beside the window: on a phone there is no pointer to
            // hover an icon with.
            if step == Step::Decide && !e.rewards.is_empty() {
                root.spawn((
                    RewardPreview,
                    Sheet,
                    Node {
                        width: Val::Vh(52.0),
                        align_self: AlignSelf::FlexStart,
                        margin: UiRect::top(Val::Vh(10.0)),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                ));
            }
        })
        .id()
}

/// The chosen reward's preview: its icon over its tooltip, drawn again when another is chosen.
fn show_preview(
    mut commands: Commands,
    pick: Res<RewardPick>,
    screen: Res<crate::Screen>,
    mut images: ResMut<Assets<Image>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    previews: Query<Entity, With<RewardPreview>>,
    mut drawn: Local<Option<(Entity, u32)>>,
) {
    let Ok(holder) = previews.single() else {
        *drawn = None;
        return;
    };
    if *drawn == Some((holder, pick.1)) {
        return;
    }
    let Some(panel) = quest_board::current() else { return };
    let Some(row) = panel
        .entries
        .iter()
        .flat_map(|e| e.rewards.iter())
        .find(|r| r.entry == pick.1)
        .and_then(|r| r.preview.clone())
    else {
        return;
    };
    let Some(font) = screen.font() else { return };
    let icon = screen.library.as_ref().and_then(|l| {
        let l = l.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        l.item_icon(row.display)
    });
    let icon = icon.map(|i| images.add(i));
    *drawn = Some((holder, pick.1));
    let vh = window.single().map_or(7.2, |w| w.height() / 100.0);
    commands.entity(holder).despawn_related::<Children>();
    commands.entity(holder).with_children(|p| {
        p.spawn((
            Node {
                row_gap: Val::Vh(1.0),
                column_gap: Val::Vh(1.2),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::FlexStart,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|r| {
            let mut i = r.spawn((
                Node {
                    width: Val::Vh(9.0),
                    height: Val::Vh(9.0),
                    flex_shrink: 0.0,
                    border: UiRect::all(Val::Vh(0.25)),
                    border_radius: BorderRadius::all(Val::Vh(0.6)),
                    ..default()
                },
                BorderColor::all(crate::armory::TOOLTIP_EDGE),
                Pickable::IGNORE,
            ));
            if let Some(h) = icon {
                i.insert(ImageNode::new(h));
            }
            r.spawn((
                crate::armory::tooltip_node(),
                BackgroundColor(crate::armory::TOOLTIP_BACK),
                BorderColor::all(crate::armory::TOOLTIP_EDGE),
                Pickable::IGNORE,
            ))
            // Larger than the collection's: the window around it is drawn shrunk.
            .with_children(|tip| crate::armory::tooltip_lines(tip, &row, &font, vh * 1.5));
        });
    });
}

/// The story appears as it is told, the newest line kept in view; whole once the talk is done.
fn reveal_story(
    talk: Res<Talk>,
    time: Res<Time<Real>>,
    mut stories: Query<(&Story, &mut Text)>,
    mut areas: Query<(&mut ScrollPosition, &ComputedNode), With<StoryScroll>>,
) {
    let told = talk.told(time.elapsed_secs());
    for (story, mut text) in &mut stories {
        let total = story.0.chars().count();
        let shown = ((total as f32 * told).ceil() as usize).min(total);
        let end = story
            .0
            .char_indices()
            .nth(shown)
            .map_or(story.0.len(), |(i, _)| i);
        if text.0.len() != end {
            text.0 = story.0[..end].to_owned();
            if talk.step == Step::Talk {
                for (mut pos, node) in &mut areas {
                    let k = node.inverse_scale_factor();
                    pos.0.y = ((node.content_size.y - node.size.y) * k).max(0.0);
                }
            }
        }
    }
}

/// One coin of the money texture.
#[derive(Clone)]
struct Coin {
    image: Handle<Image>,
    rect: Rect,
}

/// A text's size in screen heights, set as the window is laid out.
#[derive(Component)]
pub(crate) struct VhText(pub(crate) f32);

/// The giver's face, once the client has baked it.
fn show_portrait(
    portrait: Res<benilla_app::SurvivorPortrait>,
    mut nodes: Query<(&mut ImageNode, &mut Visibility), With<TalkerPortrait>>,
) {
    for (mut image, mut vis) in &mut nodes {
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

fn press_buttons(
    buttons: Query<(&PanelButton, &Interaction), Changed<Interaction>>,
    choices: Query<(&RewardChoice, &Interaction), Changed<Interaction>>,
    mut frames: Query<(&RewardChoice, &mut BorderColor)>,
    mut pick: ResMut<RewardPick>,
) {
    for (c, i) in &choices {
        if *i == Interaction::Pressed {
            pick.1 = c.0;
            for (f, mut edge) in &mut frames {
                *edge = BorderColor::all(if f.0 == c.0 { crate::GOLD } else { Color::NONE });
            }
        }
    }
    for (b, i) in &buttons {
        if *i == Interaction::Pressed {
            let answer = match b.answer {
                Answer::Accept(q, _) => Answer::Accept(q, pick.1),
                a => a,
            };
            quest_board::answer(b.panel, answer);
        }
    }
}

/// A reward item's row on the window: click to choose it.
#[derive(Component)]
struct RewardChoice(u32);

/// The reward chosen: for which window and tab, and the item.
#[derive(Resource, Default)]
struct RewardPick((u64, usize), u32);

fn light_buttons(
    mut buttons: Query<(&Interaction, &Shade, &mut BackgroundColor), Changed<Interaction>>,
    mut texts: Query<(&VhText, &mut TextFont), Added<VhText>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    for (i, shade, mut bg) in &mut buttons {
        bg.0 = if *i == Interaction::None {
            shade.0
        } else {
            shade.1
        };
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
    mut areas: Query<
        (
            &RelativeCursorPosition,
            &mut ScrollPosition,
            &ComputedNode,
            &UiGlobalTransform,
        ),
        With<ScrollArea>,
    >,
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

/// Each fade shows while its area has more beyond that edge.
fn show_fades(
    areas: Query<(&ScrollPosition, &ComputedNode), With<ScrollArea>>,
    mut fades: Query<(&Fade, &mut Visibility)>,
) {
    for (fade, mut vis) in &mut fades {
        let Ok((pos, node)) = areas.get(fade.area) else { continue };
        let k = node.inverse_scale_factor();
        let top = ((node.content_size.y - node.size.y) * k).max(0.0);
        let more = if fade.top { pos.0.y > 1.0 } else { pos.0.y < top - 1.0 };
        vis.set_if_neq(if more {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// Tell the touch controls where the window is, so a thumb on it moves no one.
pub(crate) fn block_touch(
    sheets: Query<(&ComputedNode, &UiGlobalTransform), With<Sheet>>,
    mut frames: ResMut<benilla_app::SurvivorFrames>,
) {
    frames.blockers.clear();
    for (node, at) in &sheets {
        let k = node.inverse_scale_factor();
        let centre = at.affine().translation * k;
        let half = node.size * k / 2.0;
        frames
            .blockers
            .push(Rect::from_center_half_size(centre, half));
    }
}
