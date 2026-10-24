//! The profession trainers' events around Deathknell: each trainer is held somewhere out in the
//! zone (in a mine, a cage, a camp, among wolves, in an empty house past the barrier), and a
//! chain of three quests frees them: bring back what was taken, beat off the attack that
//! follows, and walk them home. Freed, they stand in Deathknell for good.
//!
//! Public so `survivor-data` exports the trainers' creature templates.

/// One trainer's event.
pub struct Event {
    /// The trainer (1.12's own, for their model and name).
    pub trainer: u32,
    pub profession: &'static str,
    /// Where they are held: x, y, z, facing.
    pub spot: [f32; 4],
    /// Where they stand in Deathknell once home.
    pub home: [f32; 4],
    /// The second step's attackers, sent at the player by the spot: `(entry, count)` each.
    pub ambush: &'static [(u32, u32)],
    /// What is shouted as the attack comes.
    pub ambush_line: &'static str,
    /// What the trainer says while the player holds the attack off, and while walking home.
    pub defend_lines: &'static [&'static str],
    pub escort_lines: &'static [&'static str],
    /// What they say as they run off home at the barrier.
    pub home_line: &'static str,
    pub steps: [Step; 3],
}

/// How long the second quest's defence lasts (s).
pub const DEFEND_SECS: u64 = 60;

/// One quest of the chain.
pub struct Step {
    pub title: &'static str,
    /// What the trainer says, offering it.
    pub details: &'static str,
    pub objectives: &'static str,
    /// What they say as it is handed in.
    pub done: &'static str,
    /// The kills it wants (for the third, the trainer brought home), with its log line.
    pub kill: (u32, u32),
    pub objective: &'static str,
    pub xp: u32,
    pub money: u32,
}

/// The chain's quest ids: `90000 + event * 10 + step` (steps 1 to 3).
pub fn quest_id(event: usize, step: usize) -> u32 {
    90_000 + event as u32 * 10 + step as u32
}

/// Every trainer's entry.
pub fn trainers() -> impl Iterator<Item = u32> {
    EVENTS.iter().map(|e| e.trainer)
}

const NIGHT_WEB_SPIDER: u32 = 1505;
const YOUNG_NIGHT_WEB_SPIDER: u32 = 1504;
const NIGHT_WEB_MATRIARCH: u32 = 1688;
const SCARLET_CONVERT: u32 = 1506;
const SCARLET_INITIATE: u32 = 1507;
const SCARLET_WARRIOR: u32 = 1535;
const SCARLET_MISSIONARY: u32 = 1536;
const RAGGED_SCAVENGER: u32 = 1509;
const CURSED_DARKHOUND: u32 = 1548;
const DUSKBAT: u32 = 1512;
const ROTTING_DEAD: u32 = 1525;
const WRETCHED_ZOMBIE: u32 = 1502;
const RATTLECAGE_SKELETON: u32 = 1890;

pub const EVENTS: &[Event] = &[
    Event {
        trainer: 4598, // Brom Killian
        profession: "Mining and Blacksmithing",
        spot: [2040.3, 1847.3, 104.2, 3.4],
        home: [1872.0, 1604.0, 94.0, 3.1],
        ambush: &[(NIGHT_WEB_MATRIARCH, 1), (YOUNG_NIGHT_WEB_SPIDER, 5)],
        ambush_line: "The Night Web Matriarch comes for the noise!",
        defend_lines: &[
            "Mind the big one, she spits!",
            "Keep 'em off me back, lad!",
            "Hah! Another one squashed!",
            "Nearly done, hold on!",
        ],
        escort_lines: &[
            "Watch the ceiling, they drop down on you.",
            "Can't wait to smell a forge again.",
            "This hollow gave good copper once. Bad luck to it now.",
            "Keep walking, I'm right behind.",
        ],
        home_line: "There's the flames! I'll find me own way to the forge from here. Come see me, $N!",
        steps: [
            Step {
                title: "Deep in the Hollow",
                details: "Oi! Over here, $N, before the webs get you too! Name's Brom Killian. I was \
                    working the old seam in this hollow when the spiders came up out of the deep \
                    shafts. Dragged off me pick and me hammer, they did, wrapped 'em up like \
                    supper.$B$BI can't dig me way out with bare hands, and I won't leave without \
                    me tools. Cut 'em free of the brutes that took 'em.",
                objectives: "Kill Night Web Spiders in the Hollow until Brom's tools are cut \
                    free, then speak to him again.",
                done: "Me pick! Me hammer! Bless your rotten heart.",
                kill: (NIGHT_WEB_SPIDER, 6),
                objective: "Brom's tools cut from the webs",
                xp: 250,
                money: 50,
            },
            Step {
                title: "The Brood Wakes",
                details: "Now then. I'll knock the last of the web off with a few good swings... \
                    but mark me, that racket'll wake her. The big one. The Matriarch. She nests \
                    below and she don't like visitors.$B$BWhen she comes, you put her down. I'll \
                    keep me back to the wall.",
                objectives: "Defend Brom Killian for a minute while the brood attacks.",
                done: "Hah! Squashed like a cave beetle. You've a fighter's arm, $N.",
                kill: (4598, 1),
                objective: "Defend Brom from the brood",
                xp: 350,
                money: 75,
            },
            Step {
                title: "Out of the Dark",
                details: "That's me done with this hole. Deathknell's got a forge, or what's left \
                    of one, and I could teach a body to mine and smith there, if they had a mind \
                    to learn.$B$BLead the way out, $N. I'll be right behind you. Keep the little \
                    ones off me.",
                objectives: "Bring Brom Killian safely to Deathknell.",
                done: "Daylight, or what passes for it. You bring me ore and I'll show you what \
                    a hammer's for.",
                kill: (4598, 1),
                objective: "Brom Killian brought to Deathknell",
                xp: 500,
                money: 100,
            },
        ],
    },
    Event {
        trainer: 3523, // Bowen Brisboise
        profession: "Tailoring",
        spot: [1795.9, 1342.8, 89.1, 1.6],
        home: [1866.0, 1620.0, 95.7, 3.6],
        ambush: &[(SCARLET_CONVERT, 4), (SCARLET_INITIATE, 2)],
        ambush_line: "The camp's bell rings! The Scarlet Crusade comes running!",
        defend_lines: &[
            "Oh dear, oh dear, there are so many of them!",
            "Mind my good coat!",
            "Behind you! No, the other behind!",
            "Is it nearly over? It must be nearly over.",
        ],
        escort_lines: &[
            "My legs have quite forgotten how to walk.",
            "Do you think they have decent wool in Deathknell?",
            "If anyone asks, I was never in that cage.",
            "Lovely evening for not being purified.",
        ],
        home_line: "Ah, Deathknell! I'll go and find a corner to set up shop. Do visit, $N!",
        steps: [
            Step {
                title: "A Tailor in a Cage",
                details: "Psst. You. Yes, you, the one with the face falling off. Bowen \
                    Brisboise, tailor, at your service, or I would be if I weren't in a \
                    birdcage.$B$BThese zealots mean to 'purify' me at dawn. Their initiates \
                    carry the keys and took my shears besides. Get them back and I'll stitch you \
                    something that doesn't smell of grave.",
                objectives: "Kill Scarlet Initiates at the farmstead for the cage key and \
                    Bowen's shears.",
                done: "The key! And my good shears, not a nick on them. Splendid.",
                kill: (SCARLET_INITIATE, 3),
                objective: "Key and shears taken back",
                xp: 250,
                money: 50,
            },
            Step {
                title: "Alarm Bell",
                details: "Now, a small confession. This cage is hung on the same chain as the \
                    camp's bell. When the door opens, the bell rings, and the whole flock comes \
                    running with their torches.$B$BSo, open it, and then do whatever it is you do. \
                    Loudly.",
                objectives: "Defend Bowen Brisboise for a minute while the Scarlet Crusade rushes the cage.",
                done: "Well. That was very loud. Thank you, I think.",
                kill: (3523, 1),
                objective: "Defend Bowen from the Crusade",
                xp: 350,
                money: 75,
            },
            Step {
                title: "Thread Home",
                details: "I've heard there's a village of our kind up the valley. Deathknell. A \
                    tailor could do good work there. Linen, wool, silk if we're lucky, and \
                    nobody burning anybody.$B$BWalk me there? I'm handy with a needle, rather \
                    less with a sword.",
                objectives: "Bring Bowen Brisboise safely to Deathknell.",
                done: "Home, of a sort. Bring me cloth and I'll show you how to make it worth \
                    wearing.",
                kill: (3523, 1),
                objective: "Bowen Brisboise brought to Deathknell",
                xp: 500,
                money: 100,
            },
        ],
    },
    Event {
        trainer: 5695, // Vance Undergloom
        profession: "Enchanting",
        spot: [2430.1, 1577.9, 37.2, 4.0],
        home: [1858.0, 1632.0, 95.7, 4.3],
        ambush: &[(SCARLET_MISSIONARY, 2), (SCARLET_WARRIOR, 3)],
        ambush_line: "\"Heretic! The purification is not finished!\"",
        defend_lines: &[
            "The rod remembers. Burn, zealots!",
            "Their prayers are louder than their blades.",
            "Do not let them reach the rope!",
            "Steady. The runes are holding.",
        ],
        escort_lines: &[
            "My knees will remember that tower for years.",
            "The dust out here hums. Do you hear it?",
            "Walk faster. Missionaries are patient.",
            "Your Deathknell had better have a quiet corner.",
        ],
        home_line: "Your barrier. I can feel it from here. I will find my own place inside. Come and see me, $N.",
        steps: [
            Step {
                title: "The Kneeling Heretic",
                details: "Don't... stare. Kneeling is all they allow. I am Vance Undergloom, and \
                    they call what I do witchcraft. It is enchanting, which is worse, if you ask \
                    them.$B$BThey took my runed rod and my pouches of dust as 'devil's tools'. \
                    Their warriors carry them about as trophies. Without them I am only a man in \
                    rope.",
                objectives: "Kill Scarlet Warriors at the tower camp for Vance's rod and dust.",
                done: "My rod. It still hums. They didn't even know how to break it.",
                kill: (SCARLET_WARRIOR, 5),
                objective: "Runed rod and dust taken back",
                xp: 250,
                money: 50,
            },
            Step {
                title: "Purification Interrupted",
                details: "Now cut the ropes. Quickly, before the missionaries come back with \
                    their candles and their prayers. They will come back. They always come \
                    back.$B$BWhen they do, I will do what I can with the rod. You do the rest.",
                objectives: "Defend Vance Undergloom for a minute while the missionaries try to finish what they began.",
                done: "Quiet again. I had forgotten what quiet sounded like.",
                kill: (5695, 1),
                objective: "Defend Vance from the zealots",
                xp: 350,
                money: 75,
            },
            Step {
                title: "By Candle and Rune",
                details: "Take me to your Deathknell, $N. Bring me what you find, the strange \
                    dust from fallen things, and I will show you how to bind it into steel and \
                    cloth.$B$BLead on. Mind the road.",
                objectives: "Bring Vance Undergloom safely to Deathknell.",
                done: "A safe place, among the dead. How fitting. Come to me with dust and \
                    I will make it shine.",
                kill: (5695, 1),
                objective: "Vance Undergloom brought to Deathknell",
                xp: 500,
                money: 100,
            },
        ],
    },
    Event {
        trainer: 6289, // Rand Rhobart
        profession: "Skinning and Leatherworking",
        spot: [2045.6, 1421.0, 64.7, 2.0],
        home: [1878.0, 1592.0, 90.5, 2.9],
        ambush: &[(CURSED_DARKHOUND, 1), (RAGGED_SCAVENGER, 5)],
        ambush_line: "The pack is back, and the alpha leads it!",
        defend_lines: &[
            "Here they come! Watch the big one!",
            "Keep them off my leg!",
            "That's it! Make them think twice!",
            "The alpha's circling, don't lose it!",
        ],
        escort_lines: &[
            "Slow, slow. This leg's not what it was.",
            "Hear them? They're following us still.",
            "Good hides on those wolves. Shame about the teeth.",
            "Not far now, I can smell the smoke.",
        ],
        home_line: "That's the barrier! I can limp the rest of the way. Come find me, $N.",
        steps: [
            Step {
                title: "Among the Wolves",
                details: "Easy, easy! Don't spook them more than they are. Rand Rhobart. I skin \
                    for a living, and today the living tried to skin me back.$B$BThe pack ran me \
                    up here and tore off my satchel with my knives in it. The scavengers still \
                    have bits of it in their teeth. Get it back?",
                objectives: "Kill Ragged Scavengers in the forest for Rand's satchel and knives.",
                done: "My knives. Every one. You've earned a pelt or two of your own.",
                kill: (RAGGED_SCAVENGER, 5),
                objective: "Satchel and knives taken back",
                xp: 250,
                money: 50,
            },
            Step {
                title: "The Alpha Returns",
                details: "Hear that? They've circled round. And that one howling, that's no \
                    scavenger. That's the alpha, the cursed hound that leads them.$B$BIt'll come \
                    straight for the one who's bleeding. That's me. Stand between us.",
                objectives: "Defend Rand Rhobart for a minute while the pack comes back for him.",
                done: "Big brute. That hide alone would keep a tanner busy a week.",
                kill: (6289, 1),
                objective: "Defend Rand from the pack",
                xp: 350,
                money: 75,
            },
            Step {
                title: "Limping Home",
                details: "My leg's torn up. I'll walk slow, so walk slow with me. Get me to \
                    Deathknell and I'll teach you to take a hide off a beast clean, and what to \
                    make of it after.",
                objectives: "Bring Rand Rhobart safely to Deathknell.",
                done: "Safe behind the flames. Bring me hides and I'll show you leather.",
                kill: (6289, 1),
                objective: "Rand Rhobart brought to Deathknell",
                xp: 500,
                money: 100,
            },
        ],
    },
    Event {
        trainer: 4552, // Eunice Burch
        profession: "Cooking",
        spot: [1968.0, 1522.0, 88.2, 3.1],
        home: [1850.0, 1590.0, 93.2, 0.3],
        ambush: &[(ROTTING_DEAD, 5)],
        ambush_line: "The dead smell the cooking fire!",
        defend_lines: &[
            "Shoo! Shoo! Out of my kitchen!",
            "Don't you dare knock over the pot!",
            "More of them? Goodness, it's a very good broth.",
            "Nearly there, dear, keep them back!",
        ],
        escort_lines: &[
            "Careful with the pot, it's still warm.",
            "I'll need a proper hearth, mind.",
            "Everyone in Deathknell looks so thin.",
            "You'll be my first guest, of course.",
        ],
        home_line: "Here we are! I'll carry the pot the rest of the way myself. Come by for supper, $N!",
        steps: [
            Step {
                title: "The Larder",
                details: "A visitor! Come in, come in, mind the floorboards. Eunice Burch. This \
                    was a farm once and I mean to cook in it, dead or not.$B$BBut the larder's \
                    bare. Bats roost over the fields; their wings make a fine broth, believe it \
                    or not. Bring me some and I'll get the pot going.",
                objectives: "Kill Duskbats around Deathknell for Eunice's broth.",
                done: "Lovely, lovely. Into the pot with you.",
                kill: (DUSKBAT, 6),
                objective: "Bat wings brought for the broth",
                xp: 250,
                money: 50,
            },
            Step {
                title: "Smell of the Pot",
                details: "Ah. I was afraid of this. The smell carries, and the mindless ones \
                    outside still remember being hungry. They'll be at the door in a moment.$B$B\
                    Keep them off my kitchen, dear. I'll keep stirring.",
                objectives: "Defend Eunice Burch's kitchen for a minute while the dead come for the smell.",
                done: "Not a drop spilled. You'd make a fine kitchen guard.",
                kill: (4552, 1),
                objective: "Defend Eunice's kitchen",
                xp: 350,
                money: 75,
            },
            Step {
                title: "Supper in Deathknell",
                details: "This old place won't hold another night of that. I'll take my pot \
                    behind the flames, where folk can eat in peace. Walk me in, would you? \
                    And after, bring me whatever meat you find. I'll teach you to make it \
                    worth eating.",
                objectives: "Bring Eunice Burch safely into Deathknell.",
                done: "There. A proper fire, a proper crowd. Bring me meat and we'll cook.",
                kill: (4552, 1),
                objective: "Eunice Burch brought into Deathknell",
                xp: 500,
                money: 100,
            },
        ],
    },
    Event {
        trainer: 5759, // Nurse Neela
        profession: "First Aid",
        spot: [1984.8, 1591.6, 82.4, 3.1],
        home: [1859.0, 1615.0, 95.7, 0.0],
        ambush: &[(RATTLECAGE_SKELETON, 5)],
        ambush_line: "Skeletons break through the boards!",
        defend_lines: &[
            "Keep them away from the cot!",
            "Rattling things. Hold them back!",
            "He's still breathing. Keep going!",
            "Just a little longer!",
        ],
        escort_lines: &[
            "Easy, soldier. Nearly there.",
            "Don't jostle him, please.",
            "When we're inside I'll teach you a proper bandage.",
            "Every step's a step closer.",
        ],
        home_line: "The barrier! I can manage him from here. Come and learn your bandages, $N.",
        steps: [
            Step {
                title: "Linen and Mercy",
                details: "Quietly, please. I have a Deathguard on the cot with half his ribs \
                    gone. Nurse Neela. This house was empty, so I made it a ward.$B$BI have \
                    nothing to bind him with. The zombies out there still wear their old \
                    clothes. Linen is linen, wherever it came from. Bring me some.",
                objectives: "Kill Wretched Zombies for strips of linen.",
                done: "Good, clean enough. Hold this while I tie it.",
                kill: (WRETCHED_ZOMBIE, 6),
                objective: "Linen strips gathered",
                xp: 250,
                money: 50,
            },
            Step {
                title: "Rattling at the Door",
                details: "Do you hear that rattling? The Rattlecages. They followed the smell of \
                    the wound. They'll come through the walls of this old place like they're \
                    paper.$B$BI can't move him yet. Hold them off.",
                objectives: "Defend Nurse Neela's ward for a minute while the skeletons break in.",
                done: "Bones everywhere. I'll be sweeping for a week. Thank you.",
                kill: (5759, 1),
                objective: "Defend Neela's ward",
                xp: 350,
                money: 75,
            },
            Step {
                title: "The Last Mile",
                details: "He'll live, as much as any of us. But not here. Walk us back behind \
                    the barrier, $N. Once we're safe, I'll teach you to bind a wound yourself. \
                    You'll need it, the way you live.",
                objectives: "Bring Nurse Neela safely into Deathknell.",
                done: "Safe. Bring me linen and I'll teach you bandages that keep you on your \
                    feet.",
                kill: (5759, 1),
                objective: "Nurse Neela brought into Deathknell",
                xp: 500,
                money: 100,
            },
        ],
    },
];

/// The icons of the trades `trainer` teaches (`Interface\Icons\` names, the trade skills' own),
/// shown over their head; none for anyone else.
pub fn profession_icons(trainer: u32) -> &'static [&'static str] {
    match trainer {
        4598 => &["Trade_Mining", "Trade_BlackSmithing"],
        3523 => &["Trade_Tailoring"],
        5695 => &["Trade_Engraving"],
        6289 => &["INV_Misc_Pelt_Wolf_01", "INV_Misc_ArmorKit_17"],
        4552 => &["INV_Misc_Food_15"],
        5759 => &["Spell_Holy_SealOfSacrifice"],
        _ => &[],
    }
}

/// What `trainer` says to the player walking up to them at home in Deathknell.
pub fn greeting(trainer: u32) -> &'static str {
    match trainer {
        4598 => "Back from the mine with all your bones, $N? The forge is hot and the hammer's \
            itching. Ore goes in, steel comes out. What'll it be?",
        3523 => "Ah, $N! Mind the pins. Every coat in Deathknell came off this table, and there's \
            always another seam to sew.",
        5695 => "You again. Good. The dust remembers what things once were, $N. I can make it \
            remember a little more.",
        6289 => "Leg's holding up, thanks to you. Bring me hides, $N, and I'll make you something \
            that turns a claw.",
        4552 => "Sit, sit! There's always something in the pot, $N. Don't ask what. It's good for \
            you.",
        5759 => "Back in one piece? Mostly? Let me look at you, $N. Then tell me what you need.",
        _ => "Yes, $N?",
    }
}
