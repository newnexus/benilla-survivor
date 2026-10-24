//! A profession trainer's greeting ([`survivor_server::greeting`]): their face and name, their
//! line told a letter at a time as the quest window tells a story, then what they offer as links
//! under it, WoW's gossip icons beside them. A link not there yet is dimmed and does nothing.
//! Space, or a click on the line, tells it whole at once.

use bevy::prelude::*;
use survivor_server::greeting::{self, Answer, Greeting, Service};

use crate::quest_panel::{
    page, vh, TalkerPortrait, VhText, DIM, FRAME_GOLD, LETTERS_PER_SECOND, PANEL_Z,
    PORTRAIT, TAB_ON, WHITE,
};

/// A link under the cursor.
const LINK_LIT: Color = TAB_ON;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Told>()
        .add_systems(Update, (show_greeting, tell_line, press_links).chain());
}

#[derive(Component)]
struct GreetingRoot;

/// The line, told as far as the clock says.
#[derive(Component)]
struct Line(String);

/// The links, shown once the line is told.
#[derive(Component)]
struct Links;

#[derive(Component)]
struct GreetLink {
    greeting: u64,
    answer: Answer,
    soon: bool,
}

/// The greeting on screen, and the screen height it was laid out for.
#[derive(Default)]
struct Shown {
    id: Option<u64>,
    root: Option<Entity>,
    height: f32,
}

fn show_greeting(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    mut images: ResMut<Assets<Image>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    time: Res<Time<Real>>,
    mut told: ResMut<Told>,
    mut shown: Local<Shown>,
) {
    let g = greeting::current();
    let height = window.single().map_or(720.0, Window::height);
    let id = g.as_ref().map(|g| g.id);
    if id == shown.id && height == shown.height {
        return;
    }
    let fresh = id != shown.id;
    if let Some(root) = shown.root.take() {
        commands.entity(root).despawn();
    }
    shown.id = None;
    let Some(g) = g else {
        // Closed: the face goes, unless the quest window took over (it shows its own giver's).
        if fresh && survivor_server::quest_board::current().is_none() {
            benilla_app::SURVIVOR_TALKER.store(0, std::sync::atomic::Ordering::Relaxed);
        }
        return;
    };
    let Some(library) = &screen.library else { return };
    let library = library
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fonts = (library.font.clone(), library.title_font.clone());
    let mut icon = |name: &str| {
        library
            .blp(&format!("Interface\\GossipFrame\\{name}"))
            .map(|i| images.add(i))
    };
    let icons = Icons {
        offer: icon("AvailableQuestIcon"),
        reward: icon("ActiveQuestIcon"),
        shop: icon("VendorGossipIcon"),
        craft: icon("TrainerGossipIcon"),
        bye: icon("GossipGossipIcon"),
    };
    drop(library);
    if fresh {
        // The trainer is the client's "npc" unit while they speak: their portrait bakes.
        benilla_app::SURVIVOR_TALKER.store(g.giver, std::sync::atomic::Ordering::Relaxed);
        *told = Told {
            start: time.elapsed_secs(),
            seconds: g.text.chars().count() as f32 / LETTERS_PER_SECOND,
            whole: false,
        };
    }
    shown.root = Some(spawn_greeting(&mut commands, &g, &fonts, &icons));
    shown.id = Some(g.id);
    shown.height = height;
}

/// How far the line is told: shared by the telling and the links waiting on it.
#[derive(Resource, Default)]
struct Told {
    start: f32,
    seconds: f32,
    whole: bool,
}

struct Icons {
    offer: Option<Handle<Image>>,
    reward: Option<Handle<Image>>,
    shop: Option<Handle<Image>>,
    craft: Option<Handle<Image>>,
    bye: Option<Handle<Image>>,
}

fn spawn_greeting(
    commands: &mut Commands,
    g: &Greeting,
    (font, title_font): &(Handle<Font>, Handle<Font>),
    icons: &Icons,
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
            VhText(size_vh * crate::SHRINK),
        )
    };
    let id = g.id;
    commands
        .spawn((
            GreetingRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::Center,
                padding: UiRect::left(vh(3.0)),
                ..default()
            },
            GlobalZIndex(PANEL_Z),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn(page(Node {
                width: vh(78.0),
                max_width: Val::Vw(50.0 * crate::SHRINK),
                ..default()
            }))
            .with_children(|s| {
                // Who speaks: their face in a gold ring, their name.
                s.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: vh(2.0),
                    ..default()
                })
                .with_children(|h| {
                    h.spawn((
                        Node {
                            width: vh(PORTRAIT),
                            height: vh(PORTRAIT),
                            flex_shrink: 0.0,
                            border: UiRect::all(vh(0.5)),
                            border_radius: BorderRadius::all(Val::Percent(50.0)),
                            padding: UiRect::all(vh(0.25)),
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
                    h.spawn(text(&g.giver_name, title_font, 5.0, crate::GOLD));
                });
                s.spawn((
                    Line(g.text.clone()),
                    Button,
                    text("", font, 3.2, WHITE),
                    Node {
                        min_height: vh(8.0),
                        ..default()
                    },
                ));
                s.spawn((
                    Links,
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: vh(0.6),
                        ..default()
                    },
                    Visibility::Hidden,
                ))
                .with_children(|list| {
                    let mut link = |answer: Answer, label: &str, soon: bool, icon: &Option<Handle<Image>>| {
                        list.spawn((
                            GreetLink {
                                greeting: id,
                                answer,
                                soon,
                            },
                            Button,
                            Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: vh(1.4),
                                padding: UiRect::axes(vh(1.2), vh(0.7)),
                                border_radius: BorderRadius::all(vh(0.8)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|row| {
                            let mut img = row.spawn(Node {
                                width: vh(3.6),
                                height: vh(3.6),
                                flex_shrink: 0.0,
                                ..default()
                            });
                            if let Some(h) = icon {
                                let tint = if soon { Color::srgba(1.0, 1.0, 1.0, 0.4) } else { Color::WHITE };
                                img.insert(ImageNode::new(h.clone()).with_color(tint));
                            }
                            let shown = if soon { format!("{label}  (soon)") } else { label.to_owned() };
                            row.spawn(text(&shown, font, 3.0, if soon { DIM } else { crate::GOLD }));
                        });
                    };
                    for l in &g.links {
                        let icon = match l.service {
                            Service::Quest { reward: false, .. } => &icons.offer,
                            Service::Quest { reward: true, .. } => &icons.reward,
                            Service::Shop => &icons.shop,
                            Service::Craft => &icons.craft,
                        };
                        link(Answer::Pick(l.service), &l.label, l.soon, icon);
                    }
                    link(Answer::Close, "Goodbye.", false, &icons.bye);
                });
            });
        })
        .id()
}

/// The line comes out a letter at a time; told, the links show. Space or a click on the line
/// tells it whole.
fn tell_line(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut told: ResMut<Told>,
    mut lines: Query<(&Line, &mut Text, &Interaction)>,
    mut links: Query<&mut Visibility, With<Links>>,
) {
    if lines.is_empty() {
        return;
    }
    if keys.just_pressed(KeyCode::Space) || lines.iter().any(|(_, _, i)| *i == Interaction::Pressed) {
        told.whole = true;
    }
    let share = if told.whole {
        1.0
    } else {
        ((time.elapsed_secs() - told.start) / told.seconds.max(0.1)).clamp(0.0, 1.0)
    };
    for (line, mut text, _) in &mut lines {
        let total = line.0.chars().count();
        let n = ((total as f32 * share).ceil() as usize).min(total);
        let end = line.0.char_indices().nth(n).map_or(line.0.len(), |(i, _)| i);
        if text.0.len() != end {
            text.0 = line.0[..end].to_owned();
        }
    }
    for mut v in &mut links {
        v.set_if_neq(if share >= 1.0 { Visibility::Inherited } else { Visibility::Hidden });
    }
}

fn press_links(
    mut links: Query<(&GreetLink, &Interaction, &mut BackgroundColor), Changed<Interaction>>,
) {
    for (l, i, mut bg) in &mut links {
        if l.soon {
            continue;
        }
        bg.0 = if *i == Interaction::None { Color::NONE } else { LINK_LIT };
        if *i == Interaction::Pressed {
            greeting::answer(l.greeting, l.answer);
        }
    }
}
