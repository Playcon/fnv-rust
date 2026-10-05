//! The script functions in `world::more_functions`, each as the game's own
//! handler carries it out (`testdata::more` builds the world). A function
//! not carried out stops the script, so each check fails without it.

use esm::{ActivePlugins, FormId, LoadOrder};
use testdata::more::ids::*;
use world::dialogue::PLAYER_REF;
use world::more_functions::{self as more, movement, SaveKind, Seen, Shown};
use world::scripting::{Event, GameState, Runner, ScriptCache};

fn order(tag: &str) -> (testdata::TempData, LoadOrder) {
    let data = testdata::more::more(tag);
    let order = LoadOrder::from_data_dir(data.path(), &ActivePlugins::OfficialOnly).unwrap();
    (data, order)
}

/// Not a value any function here gives: what `ask` returns when the
/// script stopped (the function wasn't carried out).
const STOPPED: f32 = -12345.0;

fn ask(order: &LoadOrder, scripts: &ScriptCache, state: &mut GameState, expr: &str) -> f32 {
    state.globals.insert(FormId(VALUE), STOPPED);
    Runner::new(order, scripts, state).run_source(&format!("set TestValue to {expr}"), None, None);
    state.globals[&FormId(VALUE)]
}

fn run(order: &LoadOrder, scripts: &ScriptCache, state: &mut GameState, source: &str) {
    Runner::new(order, scripts, state).run_source(source, None, None);
}

fn new_game(order: &LoadOrder) -> GameState {
    let mut state = GameState::new(order);
    state.player_cell = Some(FormId(ROOM));
    state.player_world = None;
    state.player_position = Some([0.0, 100.0, 0.0]);
    state
}

#[test]
fn ghosts_have_no_reaction_to_hits() {
    let (_data, order) = order("more-ghost");
    let scripts = ScriptCache::default();
    let mut state = new_game(&order);
    let q = |state: &mut GameState, e: &str| ask(&order, &scripts, state, e);
    assert_eq!(q(&mut state, "PersonRef.GetIsGhost"), 0.0);
    run(
        &order,
        &scripts,
        &mut state,
        "PersonRef.SetGhost 1\nBarrelRef.SetGhost 1",
    );
    assert_eq!(q(&mut state, "PersonRef.GetIsGhost"), 1.0);
    // Not an actor: nothing.
    assert_eq!(q(&mut state, "BarrelRef.GetIsGhost"), 0.0);
    assert!(state.events.contains(&Event::More(Shown::Ghost {
        who: FormId(PERSON_REF),
        on: true
    })));
    // Hit, the ghost takes nothing and doesn't fight back; the dog does.
    let hit = Runner::new(&order, &scripts, &mut state).hit(PLAYER_REF, FormId(PERSON_REF), None);
    assert_eq!(hit, None);
    assert!(!state.combat.contains_key(&FormId(PERSON_REF)));
    assert!(!state.damage.contains_key(&FormId(PERSON_REF)));
    Runner::new(&order, &scripts, &mut state).hit(PLAYER_REF, FormId(DOG_REF), None);
    assert_eq!(state.combat.get(&FormId(DOG_REF)), Some(&PLAYER_REF));
    run(&order, &scripts, &mut state, "PersonRef.SetGhost 0");
    assert_eq!(q(&mut state, "PersonRef.GetIsGhost"), 0.0);
}

#[test]
fn alpha_alert_essential_and_subtitles() {
    let (_data, order) = order("more-flags");
    let scripts = ScriptCache::default();
    let mut state = new_game(&order);
    let q = |state: &mut GameState, e: &str| ask(&order, &scripts, state, e);
    run(
        &order,
        &scripts,
        &mut state,
        "PersonRef.SetActorAlpha 1.5\nDogRef.SetActorAlpha -1",
    );
    assert_eq!(state.more.alpha[&FormId(PERSON_REF)], 1.0);
    assert_eq!(state.more.alpha[&FormId(DOG_REF)], 0.0);
    assert_eq!(q(&mut state, "PersonRef.GetIsAlerted"), 0.0);
    run(&order, &scripts, &mut state, "PersonRef.SetAlert 1");
    assert_eq!(q(&mut state, "PersonRef.GetIsAlerted"), 1.0);
    // Essential: the hero's record; the person by base, by reference, as a
    // teammate outside hardcore.
    assert_eq!(q(&mut state, "HeroRef.IsEssential"), 1.0);
    assert_eq!(q(&mut state, "PersonRef.IsEssential"), 0.0);
    run(&order, &scripts, &mut state, "SetEssential TestPerson 1");
    assert_eq!(q(&mut state, "PersonRef.IsEssential"), 1.0);
    run(&order, &scripts, &mut state, "SetEssential TestPerson 0");
    assert_eq!(q(&mut state, "PersonRef.IsEssential"), 0.0);
    assert_eq!(q(&mut state, "PersonRef.IsActorRefEssential"), 0.0);
    run(
        &order,
        &scripts,
        &mut state,
        "PersonRef.SetActorRefEssential 1",
    );
    assert_eq!(q(&mut state, "PersonRef.IsActorRefEssential"), 1.0);
    assert_eq!(q(&mut state, "PersonRef.IsEssential"), 1.0);
    run(
        &order,
        &scripts,
        &mut state,
        "PersonRef.SetActorRefEssential 0",
    );
    state.teammates.insert(FormId(PERSON_REF));
    assert_eq!(q(&mut state, "PersonRef.IsEssential"), 1.0);
    state.living.hardcore = true;
    assert_eq!(q(&mut state, "PersonRef.IsEssential"), 0.0);
    run(
        &order,
        &scripts,
        &mut state,
        "PersonRef.AlwaysShowActorSubtitles 1",
    );
    assert!(state.more.always_subtitles.contains(&FormId(PERSON_REF)));
}

#[test]
fn talking_activators_radios_and_combat_styles() {
    let (_data, order) = order("more-tact");
    let scripts = ScriptCache::default();
    let mut state = new_game(&order);
    let q = |state: &mut GameState, e: &str| ask(&order, &scripts, state, e);
    run(
        &order,
        &scripts,
        &mut state,
        "TalkerRef.SetTalkingActivatorActor PersonRef",
    );
    assert_eq!(
        q(&mut state, "PersonRef.IsActorTalkingThroughActivator"),
        0.0
    );
    state.speaking.insert(FormId(TALKER_REF));
    assert_eq!(
        q(&mut state, "PersonRef.IsActorTalkingThroughActivator"),
        1.0
    );
    assert_eq!(q(&mut state, "HeroRef.IsActorTalkingThroughActivator"), 0.0);
    // The station's record broadcasts; switched off and on again.
    assert_eq!(q(&mut state, "RadioRef.GetBroadcastState"), 1.0);
    assert_eq!(q(&mut state, "TalkerRef.GetBroadcastState"), 0.0);
    run(&order, &scripts, &mut state, "RadioRef.SetBroadcastState 0");
    assert_eq!(q(&mut state, "RadioRef.GetBroadcastState"), 0.0);
    // A combat style given to someone is the one they fight with.
    assert_ne!(
        more::combat_style(&order, &state, FormId(PERSON_REF)).form_id,
        Some(FormId(STYLE))
    );
    run(
        &order,
        &scripts,
        &mut state,
        "PersonRef.SetCombatStyle TestStyle",
    );
    assert_eq!(
        more::combat_style(&order, &state, FormId(PERSON_REF)).form_id,
        Some(FormId(STYLE))
    );
}

#[test]
fn small_questions() {
    let (_data, order) = order("more-small");
    let scripts = ScriptCache::default();
    let mut state = new_game(&order);
    let mut q = |e: &str| ask(&order, &scripts, &mut state, e);
    assert_eq!(q("Sqrt 16"), 4.0);
    assert_eq!(q("PersonRef.IsActor"), 1.0);
    assert_eq!(q("BarrelRef.IsActor"), 0.0);
    assert_eq!(q("IsWin32"), 1.0);
    assert_eq!(q("IsXBox"), 0.0);
    // NPC_ is form type 42, ACTI 21.
    assert_eq!(q("PersonRef.GetIsObjectType 42"), 1.0);
    assert_eq!(q("BarrelRef.GetIsObjectType 21"), 1.0);
    assert_eq!(q("BarrelRef.GetIsObjectType 42"), 0.0);
    assert_eq!(q("ChildRef.GetParentRef"), BARREL_REF as f32);
    assert_eq!(q("BarrelRef.GetParentRef"), 0.0);
    assert_eq!(q("PersonRef.IsLimbGone 3"), 0.0);
    assert_eq!(q("PersonRef.IsLimbGone 1 5"), 0.0);
    assert_eq!(q("player.GetSandman"), 0.0);
    assert_eq!(q("IsPlayerGrabbedRef CupRef"), 0.0);
    // Level 1: 200 experience to level 2 (the exe's iXPBase).
    assert_eq!(q("GetXPForNextLevel"), 200.0);
    assert_eq!(q("PersonRef.GetIgnoreCrime"), 0.0);
    assert_eq!(q("PersonRef.GetIgnoreFriendlyHits"), 0.0);
    assert_eq!(q("GetInCharGen"), 0.0);
    assert_eq!(q("PersonRef.IsInCriticalStage 2"), 0.0);
    run(
        &order,
        &scripts,
        &mut state,
        "SetInChargen 1\nPersonRef.SetCriticalStage 2\nPersonRef.IgnoreCrime 1",
    );
    let mut q = |e: &str| ask(&order, &scripts, &mut state, e);
    assert_eq!(q("GetInCharGen"), 1.0);
    assert_eq!(q("PersonRef.IsInCriticalStage 2"), 1.0);
    assert_eq!(q("PersonRef.GetIgnoreCrime"), 1.0);
}

#[test]
fn what_the_viewer_saw() {
    let (_data, order) = order("more-seen");
    let scripts = ScriptCache::default();
    let mut state = new_game(&order);
    let q = |state: &mut GameState, e: &str| ask(&order, &scripts, state, e);
    // Nothing seen: standing still, no idle, no procedure.
    assert_eq!(q(&mut state, "PersonRef.IsMoving"), 0.0);
    assert_eq!(q(&mut state, "PersonRef.IsLastIdlePlayed TestIdle"), 0.0);
    assert_eq!(q(&mut state, "PersonRef.GetCurrentAIProcedure"), 0.0);
    more::report(
        &mut state,
        FormId(PERSON_REF),
        Seen {
            movement: movement::LEFT | movement::RUNNING,
            last_idle: Some(FormId(IDLE)),
            procedure: Some(10),
            swimming: true,
            idle_playing: true,
        },
    );
    assert_eq!(q(&mut state, "PersonRef.IsIdlePlaying"), 1.0);
    assert_eq!(q(&mut state, "PersonRef.IsMoving"), 3.0);
    assert_eq!(q(&mut state, "PersonRef.IsRunning"), 1.0);
    assert_eq!(q(&mut state, "PersonRef.IsSneaking"), 0.0);
    assert_eq!(q(&mut state, "PersonRef.IsSwimming"), 1.0);
    assert_eq!(q(&mut state, "PersonRef.IsLastIdlePlayed TestIdle"), 1.0);
    assert_eq!(q(&mut state, "PersonRef.GetCurrentAIProcedure"), 10.0);
    // Sneaking, unless the flag that cancels it is up.
    more::report(
        &mut state,
        FormId(PERSON_REF),
        Seen {
            movement: movement::SNEAKING,
            ..Seen::default()
        },
    );
    assert_eq!(q(&mut state, "PersonRef.IsSneaking"), 1.0);
    more::report(
        &mut state,
        FormId(PERSON_REF),
        Seen {
            movement: movement::SNEAKING | movement::NOT_SNEAKING,
            ..Seen::default()
        },
    );
    assert_eq!(q(&mut state, "PersonRef.IsSneaking"), 0.0);
    // The player, before the viewer says: from the state.
    state.player_sneaking = true;
    assert_eq!(q(&mut state, "player.IsSneaking"), 1.0);
    // Menus.
    assert_eq!(q(&mut state, "MenuMode 0"), 0.0);
    state.more.menu_open = Some(1002);
    assert_eq!(q(&mut state, "MenuMode 0"), 1.0);
    assert_eq!(q(&mut state, "MenuMode 1"), 1.0);
    assert_eq!(q(&mut state, "MenuMode 1002"), 1.0);
    assert_eq!(q(&mut state, "MenuMode 1036"), 0.0);
}

#[test]
fn challenges_count_complete_and_recur() {
    let (_data, order) = order("more-chal");
    let scripts = ScriptCache::default();
    let mut state = new_game(&order);
    let q = |state: &mut GameState, e: &str| ask(&order, &scripts, state, e);
    let done = |state: &GameState| state.globals[&FormId(DONE)];
    for _ in 0..2 {
        run(
            &order,
            &scripts,
            &mut state,
            "IncrementScriptedChallenge TestScripted",
        );
    }
    assert_eq!(q(&mut state, "GetChallengeCompleted TestScripted"), 0.0);
    assert!(state.events.iter().any(|e| matches!(e,
        Event::Message { text, .. } if text == "Scripted   2\\3\nDo it.")));
    run(
        &order,
        &scripts,
        &mut state,
        "IncrementScriptedChallenge TestScripted",
    );
    // Completed: its script ran, the statistic went up, the notice.
    assert_eq!(done(&state), 1.0);
    assert_eq!(q(&mut state, "GetChallengeCompleted TestScripted"), 1.0);
    assert_eq!(world::stats::get(&state, 27), 1);
    assert!(state.events.iter().any(|e| matches!(e,
        Event::Message { text, .. } if text == "Scripted   3\\3\nDo it.")));
    // No more counting once completed.
    run(
        &order,
        &scripts,
        &mut state,
        "IncrementScriptedChallenge TestScripted",
    );
    assert_eq!(done(&state), 1.0);
    // The statistic counted for the challenge about it when scripts next
    // ran.
    assert_eq!(state.more.challenges.progress[&FormId(COUNTING)].0, 1);

    // Recurring: done twice over, counting again each time.
    for _ in 0..2 {
        run(
            &order,
            &scripts,
            &mut state,
            "IncrementScriptedChallenge TestRecurring",
        );
    }
    assert_eq!(done(&state), 2.0);
    assert_eq!(q(&mut state, "GetChallengeCompleted TestRecurring"), 1.0);
    assert_eq!(state.more.challenges.progress[&FormId(RECURRING)].0, 0);
    // That was the second challenge completed: the one counting them
    // completes when scripts next run (its own completion doesn't count
    // for it).
    assert_eq!(done(&state), 3.0);
    assert_eq!(q(&mut state, "GetChallengeCompleted TestCounting"), 1.0);
    // The condition asks the completed flag itself: not set while it recurs.
    let facts = world::scripting::Facts {
        order: &order,
        state: &state,
        speaker: None,
    };
    let get_challenge_completed = script::functions::FUNCTIONS
        .iter()
        .position(|f| f.name == "GetChallengeCompleted")
        .unwrap() as u16;
    let arg = [world::scripting::Value::Form(FormId(RECURRING))];
    assert_eq!(facts.value(get_challenge_completed, None, &arg), Some(0.0));
    run(
        &order,
        &scripts,
        &mut state,
        "RemoveRecurringFromChallenge TestRecurring",
    );
    for _ in 0..2 {
        run(
            &order,
            &scripts,
            &mut state,
            "IncrementScriptedChallenge TestRecurring",
        );
    }
    assert_eq!(done(&state), 4.0);
    let facts = world::scripting::Facts {
        order: &order,
        state: &state,
        speaker: None,
    };
    assert_eq!(facts.value(get_challenge_completed, None, &arg), Some(2.0));
    // The counting one, completed, counts no more.
    run(&order, &scripts, &mut state, "set TestValue to 0");
    assert_eq!(done(&state), 4.0);

    // Starting disabled: nothing until unlocked.
    run(
        &order,
        &scripts,
        &mut state,
        "IncrementScriptedChallenge TestLocked",
    );
    assert_eq!(q(&mut state, "GetChallengeCompleted TestLocked"), 0.0);
    run(
        &order,
        &scripts,
        &mut state,
        "UnlockChallenge TestLocked\nIncrementScriptedChallenge TestLocked",
    );
    assert_eq!(q(&mut state, "GetChallengeCompleted TestLocked"), 1.0);

    // Kept in a save.
    let saved = world::save::save(&state, None);
    let (back, _) = world::save::load(&saved).unwrap();
    assert_eq!(
        back.more.challenges.progress,
        state.more.challenges.progress
    );
}

#[test]
fn place_at_me_makes_references() {
    let (_data, order) = order("more-place");
    let scripts = ScriptCache::default();
    let mut state = new_game(&order);
    run(
        &order,
        &scripts,
        &mut state,
        "ref r\nset r to PersonRef.PlaceAtMe TestCup 3\nset TestValue to r.GetIsID TestCup",
    );
    assert_eq!(state.globals[&FormId(VALUE)], 1.0);
    let made: Vec<_> = state
        .more
        .placed
        .refs
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect();
    assert_eq!(made.len(), 3);
    let at = |i: usize| made[i].1.position;
    // The first on the caller's spot, then a ring of 100 units, 45 degrees
    // apart, at the caller's height; all in its room, turned as it is.
    assert_eq!(at(0), [0.0, 0.0, 0.0]);
    let d = |p: [f32; 3]| (p[0] * p[0] + p[1] * p[1]).sqrt();
    assert!((d(at(1)) - 100.0).abs() < 1e-3 && (d(at(2)) - 100.0).abs() < 1e-3);
    let a = |p: [f32; 3]| p[0].atan2(p[1]);
    let step = (a(at(2)) - a(at(1))).rem_euclid(std::f32::consts::TAU);
    assert!((step - std::f32::consts::FRAC_PI_4).abs() < 1e-4, "{step}");
    assert!(made.iter().all(|(_, m)| m.base == FormId(CUP)
        && m.cell == FormId(ROOM)
        && (m.rotation[2] - 90f32.to_radians()).abs() < 1e-5));
    let events = state
        .events
        .iter()
        .filter(|e| matches!(e, Event::More(Shown::Placed { .. })))
        .count();
    assert_eq!(events, 3);
    // Where they are, for other functions.
    let third = made[2].0;
    assert_eq!(state.place(&order, third).unwrap().2, at(2));

    // A leveled list: on one spot, 100 units in front of the person (who
    // faces east); from a marker (no model) on the marker's own spot.
    run(
        &order,
        &scripts,
        &mut state,
        "PersonRef.PlaceAtMe TestDogs 1 100 0",
    );
    let dog = state.more.placed.refs.values().last().copied().unwrap();
    assert_eq!(dog.base, FormId(DOG));
    assert!((dog.position[0] - 100.0).abs() < 1e-3 && dog.position[1].abs() < 1e-3);
    run(
        &order,
        &scripts,
        &mut state,
        "MarkerRef.PlaceAtMe TestDogs 1 100 0",
    );
    let dog = state.more.placed.refs.values().last().copied().unwrap();
    assert_eq!(dog.position, [500.0, 0.0, 0.0]);
    // An actor's leveled form, on the caller.
    run(
        &order,
        &scripts,
        &mut state,
        "HeroRef.PlaceLeveledActorAtMe TestDog",
    );
    let dog = state.more.placed.refs.values().last().copied().unwrap();
    assert_eq!((dog.base, dog.position), (FormId(DOG), [0.0, 200.0, 0.0]));
    // Kept in a save.
    let saved = world::save::save(&state, None);
    let (back, _) = world::save::load(&saved).unwrap();
    assert_eq!(back.more.placed.refs, state.more.placed.refs);
    assert_eq!(back.more.placed.next, state.more.placed.next);
}

#[test]
fn objects_break_in_stages() {
    let (_data, order) = order("more-dest");
    let scripts = ScriptCache::default();
    let mut state = new_game(&order);
    let q = |state: &mut GameState, e: &str| ask(&order, &scripts, state, e);
    assert_eq!(q(&mut state, "BarrelRef.GetDestructionStage"), -1.0);
    run(&order, &scripts, &mut state, "BarrelRef.DamageObject 30");
    // 70 %: no stage passed yet.
    assert_eq!(q(&mut state, "BarrelRef.GetDestructionStage"), 0.0);
    run(&order, &scripts, &mut state, "BarrelRef.DamageObject 25");
    // 45 %: the first stage (50 %), with its model.
    assert_eq!(q(&mut state, "BarrelRef.GetDestructionStage"), 1.0);
    assert!(state.events.iter().any(|e| matches!(e,
        Event::More(Shown::Destruction { stage: 1, model: Some(m), .. })
            if m == "test\\barreldamaged.nif")));
    assert_eq!(q(&mut state, "BarrelRef.GetDestroyed"), 0.0);
    run(&order, &scripts, &mut state, "BarrelRef.DamageObject 100");
    assert_eq!(q(&mut state, "BarrelRef.GetDestructionStage"), 2.0);
    assert_eq!(q(&mut state, "BarrelRef.GetDestroyed"), 1.0);
    run(&order, &scripts, &mut state, "BarrelRef.ClearDestruction");
    assert_eq!(q(&mut state, "BarrelRef.GetDestructionStage"), -1.0);
    assert_eq!(q(&mut state, "BarrelRef.GetDestroyed"), 0.0);
    // The crate's first stage caps the damage at 60 %; the next disables it.
    run(&order, &scripts, &mut state, "CrateRef.DamageObject 90");
    assert_eq!(state.more.damaged.health[&FormId(CRATE_REF)], 60.0);
    assert_eq!(q(&mut state, "CrateRef.GetDisabled"), 0.0);
    run(&order, &scripts, &mut state, "CrateRef.DamageObject 50");
    assert_eq!(q(&mut state, "CrateRef.GetDisabled"), 1.0);
    // Nothing to destroy on a person (another path in the game).
    assert_eq!(q(&mut state, "PersonRef.DamageObject 10"), STOPPED);
}

#[test]
fn saves_time_owners_and_waking() {
    let (_data, order) = order("more-misc");
    let scripts = ScriptCache::default();
    let mut state = new_game(&order);
    run(
        &order,
        &scripts,
        &mut state,
        "Autosave\nForceSave\nSystemSave",
    );
    for kind in [SaveKind::Autosave, SaveKind::Force, SaveKind::System] {
        assert!(state.events.contains(&Event::More(Shown::Save(kind))));
    }
    run(&order, &scripts, &mut state, "SetGlobalTimeMultiplier 0.5");
    assert_eq!(state.more.time_multiplier, Some(0.5));
    run(&order, &scripts, &mut state, "CupRef.SetOwnership");
    run(
        &order,
        &scripts,
        &mut state,
        "BarrelRef.SetOwnership\nBarrelRef.ClearOwnership",
    );
    assert!(!state
        .set_by_scripts
        .owners
        .contains_key(&FormId(BARREL_REF)));
    // Waking: only while sleeping.
    run(&order, &scripts, &mut state, "WakeUpPC 3");
    assert_eq!(state.living.hours_left, 0);
    state.living.sleeping = true;
    state.living.hours_left = 8;
    run(&order, &scripts, &mut state, "WakeUpPC 0");
    assert_eq!(state.living.hours_left, 0);
    // What's kept comes back from a save.
    run(
        &order,
        &scripts,
        &mut state,
        "PersonRef.SetGhost 1\nPersonRef.SetActorAlpha 0.25\nSetEssential TestPerson 1",
    );
    let saved = world::save::save(&state, None);
    let (back, _) = world::save::load(&saved).unwrap();
    assert!(back.more.ghosts.contains(&FormId(PERSON_REF)));
    assert_eq!(back.more.alpha[&FormId(PERSON_REF)], 0.25);
    assert!(back.more.essential_bases[&FormId(PERSON)]);
    assert_eq!(back.more.time_multiplier, Some(0.5));
}

#[test]
fn fights_ranks_and_effect_seconds() {
    let (_data, order) = order("more-fights");
    let scripts = ScriptCache::default();
    let mut state = new_game(&order);
    let q = |state: &mut GameState, e: &str| ask(&order, &scripts, state, e);
    // Ranks: the person's 2 against the hero's 0; the dog isn't in the gang.
    assert_eq!(
        q(
            &mut state,
            "PersonRef.GetFactionRankDifference TestGang HeroRef"
        ),
        2.0
    );
    assert_eq!(
        q(
            &mut state,
            "HeroRef.GetFactionRankDifference TestGang PersonRef"
        ),
        -2.0
    );
    assert_eq!(
        q(
            &mut state,
            "PersonRef.GetFactionRankDifference TestGang DogRef"
        ),
        0.0
    );
    // Fights: who targets whom; all of it stopped for the hero.
    state.combat.insert(FormId(PERSON_REF), FormId(HERO_REF));
    state.combat.insert(FormId(DOG_REF), FormId(HERO_REF));
    state.combat.insert(FormId(HERO_REF), FormId(PERSON_REF));
    assert_eq!(q(&mut state, "PersonRef.IsCombatTarget HeroRef"), 1.0);
    assert_eq!(q(&mut state, "HeroRef.IsCombatTarget DogRef"), 0.0);
    run(
        &order,
        &scripts,
        &mut state,
        "HeroRef.StopCombatAlarmOnActor",
    );
    assert!(state.combat.is_empty());
    // For the player: the factions of the people around forgive crimes.
    state.crime_enemies.insert(FormId(GANG));
    state.combat.insert(FormId(DOG_REF), PLAYER_REF);
    run(
        &order,
        &scripts,
        &mut state,
        "player.StopCombatAlarmOnActor",
    );
    assert!(state.crime_enemies.is_empty() && state.combat.is_empty());
    // Damage resistance now.
    run(
        &order,
        &scripts,
        &mut state,
        "PersonRef.SetActorValue DamageResist 15",
    );
    assert_eq!(q(&mut state, "PersonRef.GetArmorRating"), 15.0);
    // Turning, as the viewer saw it.
    more::report(
        &mut state,
        FormId(PERSON_REF),
        Seen {
            movement: movement::TURNING_RIGHT,
            ..Seen::default()
        },
    );
    assert_eq!(q(&mut state, "PersonRef.IsTurning"), 2.0);
    // An effect script sees its update's seconds; outside one, 0.
    assert_eq!(q(&mut state, "ScriptEffectElapsedSeconds"), 0.0);
    run(
        &order,
        &scripts,
        &mut state,
        "player.CastImmediateOnSelf TestTick",
    );
    Runner::new(&order, &scripts, &mut state).update(0.5);
    assert_eq!(state.globals[&FormId(ELAPSED)], 0.5);
    // Menus' points, a Securitron's face, rumble.
    run(
        &order,
        &scripts,
        &mut state,
        "AddSPECIALPoints 1\nAddSPECIALPoints 1\nAddTagSkills 2\nSetRumble 0.5 0.5 1",
    );
    assert_eq!((state.more.special_points, state.more.tag_points), (2, 2));
    run(&order, &scripts, &mut state, "SetSPECIALPoints 5");
    assert_eq!(state.more.special_points, 5);
    run(
        &order,
        &scripts,
        &mut state,
        "SetSecuritronExpression PersonRef Infantry Neutral",
    );
    assert_eq!(
        state.more.securitron_faces[&FormId(PERSON_REF)],
        ("Infantry".to_string(), "Neutral".to_string())
    );
    assert!(state.unhandled.is_empty(), "{:?}", state.unhandled);
}

#[test]
fn the_pipboy_radio_and_its_stations() {
    let (_data, order) = order("more-radio");
    let scripts = ScriptCache::default();
    let mut state = new_game(&order);
    let radio = |state: &GameState| state.more.radio.clone();
    // Tuning while off does nothing; on with a station tunes to it.
    run(&order, &scripts, &mut state, "PipboyRadio Tune RadioRef");
    assert!(!radio(&state).on);
    run(&order, &scripts, &mut state, "PipboyRadio on RadioRef");
    assert!(radio(&state).on);
    assert_eq!(radio(&state).tuned, Some(FormId(RADIO_REF)));
    // Dead Money's words: `Tune` with a capital (compared without case).
    run(&order, &scripts, &mut state, "PipboyRadio Tune TalkerRef");
    assert_eq!(radio(&state).tuned, Some(FormId(TALKER_REF)));
    // Something that can't be a station switches the radio off.
    run(&order, &scripts, &mut state, "PipboyRadio tune BarrelRef");
    assert!(!radio(&state).on);
    assert_eq!(radio(&state).tuned, None);
    // A number starting with 1 is on; off forgets the station.
    run(&order, &scripts, &mut state, "PipboyRadio 1 RadioRef");
    assert_eq!(radio(&state).tuned, Some(FormId(RADIO_REF)));
    run(&order, &scripts, &mut state, "PipBoyRadioOff");
    assert_eq!((radio(&state).on, radio(&state).tuned), (false, None));

    // A station's conversation: the topic given, or the default one;
    // not a station: nothing.
    run(
        &order,
        &scripts,
        &mut state,
        "RadioRef.StartRadioConversation TestRadioTopic\nTalkerRef.StartRadioConversation\n\
         BarrelRef.StartRadioConversation TestRadioTopic",
    );
    let c = radio(&state).conversations;
    assert_eq!(c.get(&FormId(RADIO_REF)), Some(&Some(FormId(RADIO_TOPIC))));
    assert_eq!(c.get(&FormId(TALKER_REF)), Some(&None));
    assert!(!c.contains_key(&FormId(BARREL_REF)));

    // A person plays a station and stops; 2 and things that aren't
    // people do nothing.
    run(
        &order,
        &scripts,
        &mut state,
        "PersonRef.SetNPCRadio 1 RadioRef\nBarrelRef.SetNPCRadio 1 RadioRef\n\
         HeroRef.SetNPCRadio 1 RadioRef\nHeroRef.SetNPCRadio 2 RadioRef",
    );
    let n = radio(&state).npc_radio;
    assert_eq!(n.get(&FormId(PERSON_REF)), Some(&FormId(RADIO_REF)));
    assert_eq!(n.get(&FormId(HERO_REF)), Some(&FormId(RADIO_REF)));
    assert!(!n.contains_key(&FormId(BARREL_REF)));
    run(
        &order,
        &scripts,
        &mut state,
        "HeroRef.SetNPCRadio 0 RadioRef",
    );
    assert!(!radio(&state).npc_radio.contains_key(&FormId(HERO_REF)));

    run(
        &order,
        &scripts,
        &mut state,
        "ForceRadioStationUpdate\nResetPipboyManager\nPipboyRadio enable TalkerRef",
    );
    assert!(radio(&state).pipboy_reset);

    // Kept in a save.
    let saved = world::save::save(&state, None);
    let (back, _) = world::save::load(&saved).unwrap();
    assert_eq!(back.more.radio, state.more.radio);
    assert_eq!(back.more.radio.tuned, Some(FormId(TALKER_REF)));
}

#[test]
fn objects_animations_playing() {
    let (_data, order) = order("more-anim");
    let scripts = ScriptCache::default();
    let mut state = new_game(&order);
    let q = |state: &mut GameState, e: &str| ask(&order, &scripts, state, e);
    // Before the viewer reports anything, nothing has 3D: nothing plays.
    assert_eq!(q(&mut state, "BarrelRef.IsAnimPlaying"), 0.0);
    more::report_sequences(
        &mut state,
        [
            (FormId(BARREL_REF), vec!["SpecialIdle".to_string()]),
            (FormId(RADIO_REF), vec!["Forward".to_string()]),
            (FormId(CRATE_REF), Vec::new()),
        ]
        .into_iter()
        .collect(),
    );
    assert_eq!(q(&mut state, "BarrelRef.IsAnimPlaying"), 1.0);
    assert_eq!(q(&mut state, "BarrelRef.IsAnimPlaying Forward"), 0.0);
    // Group names compare without case (Dead Money writes `Forward`).
    assert_eq!(q(&mut state, "RadioRef.IsAnimPlaying forward"), 1.0);
    assert_eq!(q(&mut state, "RadioRef.IsAnimPlaying Backward"), 0.0);
    assert_eq!(q(&mut state, "CrateRef.IsAnimPlaying"), 0.0);
    // People's animation data isn't carried out: the script stops.
    assert_eq!(q(&mut state, "PersonRef.IsAnimPlaying"), STOPPED);
}

/// A camera and collision for `GetLineOfSight`: boxes for the people and
/// the barrel, everything in view or nothing, and every ray stopped at
/// the same distance (or none).
struct TestSight {
    in_view: bool,
    hit: Option<f32>,
}

impl world::sight::Sight for TestSight {
    fn bound(&self, r: FormId) -> Option<([f32; 3], [f32; 3])> {
        let at = match r.0 {
            PERSON_REF => [0.0, 0.0, 0.0],
            HERO_REF => [0.0, 200.0, 0.0],
            BARREL_REF => [100.0, 0.0, 0.0],
            _ => return None,
        };
        Some((
            [at[0] - 20.0, at[1] - 20.0, at[2]],
            [at[0] + 20.0, at[1] + 20.0, at[2] + 120.0],
        ))
    }
    fn camera(&self) -> Option<[f32; 3]> {
        Some([0.0, 100.0, 120.0])
    }
    fn in_view(&self, _lo: [f32; 3], _hi: [f32; 3]) -> bool {
        self.in_view
    }
    fn ray(&self, _from: [f32; 3], _to: [f32; 3]) -> Option<f32> {
        self.hit
    }
}

fn ask_seeing(
    order: &LoadOrder,
    scripts: &ScriptCache,
    state: &mut GameState,
    sight: &TestSight,
    expr: &str,
) -> f32 {
    state.globals.insert(FormId(VALUE), STOPPED);
    Runner::new(order, scripts, state)
        .with_sight(sight)
        .run_source(&format!("set TestValue to {expr}"), None, None);
    state.globals[&FormId(VALUE)]
}

#[test]
fn line_of_sight() {
    let (_data, order) = order("more-sight");
    let scripts = ScriptCache::default();
    let mut state = new_game(&order);
    let clear = TestSight {
        in_view: true,
        hit: None,
    };
    let walled = TestSight {
        in_view: true,
        hit: Some(10.0),
    };
    let q =
        |state: &mut GameState, s: &TestSight, e: &str| ask_seeing(&order, &scripts, state, s, e);
    // The player: in view and a ray gets through.
    assert_eq!(q(&mut state, &clear, "Player.GetLineOfSight HeroRef"), 1.0);
    // Out of view: no, however clear.
    let away = TestSight {
        in_view: false,
        hit: None,
    };
    assert_eq!(q(&mut state, &away, "Player.GetLOS HeroRef"), 0.0);
    // A ray stopped where it reaches the hero's box hit the hero (its
    // 0.75 ray from (0, 100, 120) to (0, 200, 90) enters the box 83.5
    // units along); stopped well short, a wall.
    let at_box = TestSight {
        in_view: true,
        hit: Some(84.0),
    };
    assert_eq!(q(&mut state, &at_box, "Player.GetLOS HeroRef"), 1.0);
    let short = TestSight {
        in_view: true,
        hit: Some(50.0),
    };
    assert_eq!(q(&mut state, &short, "Player.GetLOS HeroRef"), 0.0);
    // Walled off, the player's own detection data decides.
    assert_eq!(q(&mut state, &walled, "Player.GetLOS HeroRef"), 0.0);
    more::report_detection_sight(&mut state, PLAYER_REF, FormId(HERO_REF), true);
    assert_eq!(q(&mut state, &walled, "Player.GetLOS HeroRef"), 1.0);
    // No 3D: not seen.
    assert_eq!(q(&mut state, &clear, "Player.GetLOS DogRef"), 0.0);

    // Someone else: their last detection run's line of sight.
    assert_eq!(q(&mut state, &clear, "HeroRef.GetLOS PersonRef"), 0.0);
    more::report_detection_sight(&mut state, FormId(HERO_REF), FormId(PERSON_REF), true);
    assert_eq!(q(&mut state, &clear, "HeroRef.GetLOS PersonRef"), 1.0);
    // The caller must be an actor.
    assert_eq!(q(&mut state, &clear, "BarrelRef.GetLOS Player"), 0.0);
    // Not carried out: an object target for someone else, and the player
    // headless (no camera).
    assert_eq!(q(&mut state, &clear, "HeroRef.GetLOS BarrelRef"), STOPPED);
    assert_eq!(
        ask(&order, &scripts, &mut state, "Player.GetLOS HeroRef"),
        STOPPED
    );
    // Headless, someone else's test still answers from detection.
    assert_eq!(
        ask(&order, &scripts, &mut state, "HeroRef.GetLOS PersonRef"),
        1.0
    );
}

#[test]
fn effect_shaders_on_references() {
    use world::more_functions::shaders;
    let (_data, order) = order("more-shaders");
    let scripts = ScriptCache::default();
    let mut state = new_game(&order);
    more::report_loaded(
        &mut state,
        [PLAYER_REF, FormId(PERSON_REF), FormId(BARREL_REF)]
            .into_iter()
            .collect(),
    );
    let running = |state: &GameState| -> Vec<(u32, Option<f64>)> {
        shaders::active(state)
            .map(|v| {
                assert_eq!(v.shader, FormId(SHADER));
                (v.reference.0, v.until)
            })
            .collect()
    };
    // Until stopped; again, a second one (they stack); for 2 s; with no
    // 3D, nothing; no reference, the player.
    run(
        &order,
        &scripts,
        &mut state,
        "PersonRef.PlayMagicShaderVisuals TestShader\nPersonRef.pms TestShader\n\
         BarrelRef.pms TestShader 2\nHeroRef.pms TestShader\npms TestShader",
    );
    let now = state.seconds;
    assert_eq!(
        running(&state),
        vec![
            (PERSON_REF, None),
            (PERSON_REF, None),
            (BARREL_REF, Some(now + 2.0)),
            (PLAYER_REF.0, None),
        ]
    );
    assert!(state.events.contains(&Event::More(Shown::ShaderVisual {
        reference: FormId(BARREL_REF),
        shader: FormId(SHADER),
        seconds: Some(2.0),
    })));
    // Its seconds over, the barrel's ends.
    state.seconds += 3.0;
    assert_eq!(running(&state).len(), 3);
    // Stopping ends every one with that shader on the reference.
    state.events.clear();
    run(
        &order,
        &scripts,
        &mut state,
        "PersonRef.StopMagicShaderVisuals TestShader\nHeroRef.sms TestShader",
    );
    assert_eq!(running(&state), vec![(PLAYER_REF.0, None)]);
    assert_eq!(
        state.events,
        vec![Event::More(Shown::ShaderVisualStopped {
            reference: FormId(PERSON_REF),
            shader: FormId(SHADER),
        })]
    );
}

#[test]
fn terminals_go_back_only_while_open() {
    let (_data, order) = order("more-terminal-back");
    let scripts = ScriptCache::default();
    let mut state = new_game(&order);
    let back = Event::More(Shown::TerminalBack);
    // No terminal open: nothing.
    run(&order, &scripts, &mut state, "ForceTerminalBack");
    assert!(!state.events.contains(&back));
    // Another menu open: nothing.
    state.more.menu_open = Some(1036);
    run(&order, &scripts, &mut state, "ForceTerminalBack");
    assert!(!state.events.contains(&back));
    // The terminal menu: back a screen, once per call.
    state.more.menu_open = Some(world::terminal::TERMINAL_MENU);
    run(
        &order,
        &scripts,
        &mut state,
        "ForceTerminalBack\nForceTerminalBack",
    );
    assert_eq!(state.events.iter().filter(|e| **e == back).count(), 2);
}

#[test]
fn caravan_cards_picked_up() {
    let (_data, order) = order("more-cards");
    let scripts = ScriptCache::default();
    let mut state = new_game(&order);
    let held = |state: &GameState, holder: FormId, item: u32| {
        state
            .items
            .get(&(holder, FormId(item)))
            .copied()
            .unwrap_or(0)
    };
    // Outside an item's script there's no container.
    assert_eq!(
        ask(&order, &scripts, &mut state, "CardRef.GetContainer"),
        0.0
    );
    // The player picks up a card: its `OnAdd` sees the player as the
    // container, the card joins their cards and leaves the inventory.
    state.pick_up(&order, FormId(CARD_REF), FormId(CARD), 1);
    Runner::new(&order, &scripts, &mut state).on_add(FormId(CARD_REF), PLAYER_REF);
    assert_eq!(state.globals[&FormId(VALUE)], PLAYER_REF.0 as f32);
    assert!(state.more.cards.0.contains(&FormId(CARD)));
    assert_eq!(held(&state, PLAYER_REF, CARD), 0);
    // Not a card: it isn't added to the cards, but `RemoveMe` still takes
    // it out.
    state.pick_up(&order, FormId(CARD_CUP_REF), FormId(CARD_CUP), 1);
    Runner::new(&order, &scripts, &mut state).on_add(FormId(CARD_CUP_REF), PLAYER_REF);
    assert!(!state.more.cards.0.contains(&FormId(CARD_CUP)));
    assert_eq!(held(&state, PLAYER_REF, CARD_CUP), 0);
    // Into another container: the script returns before anything.
    state.items.insert((FormId(CRATE_REF), FormId(CARD_CUP)), 1);
    Runner::new(&order, &scripts, &mut state).on_add(FormId(CARD_CUP_REF), FormId(CRATE_REF));
    assert_eq!(state.globals[&FormId(VALUE)], CRATE_REF as f32);
    assert_eq!(held(&state, FormId(CRATE_REF), CARD_CUP), 1);
    // `RemoveMe` with a container moves the item there.
    state.items.insert((PLAYER_REF, FormId(CARD_CUP)), 2);
    let mut runner = Runner::new(&order, &scripts, &mut state);
    runner.container = Some(PLAYER_REF);
    runner.run_source("RemoveMe CrateRef", Some(FormId(CARD_CUP_REF)), None);
    assert_eq!(held(&state, PLAYER_REF, CARD_CUP), 1);
    assert_eq!(held(&state, FormId(CRATE_REF), CARD_CUP), 2);

    // The cards are kept in a save.
    let saved = world::save::save(&state, None);
    let (back, _) = world::save::load(&saved).unwrap();
    assert_eq!(back.more.cards, state.more.cards);
}
