//! `AddScriptPackage` lifecycle requests, using synthetic PACK records.

use esm::{ActivePlugins, FormId, LoadOrder};
use testdata::functions::ids::{ADULT_REF, CUP_REF, SANDBOX, TRAVEL};
use world::scripting::{Event, GameState, PackageActionKind, Runner, ScriptCache};

fn order() -> (testdata::TempData, LoadOrder) {
    let data = testdata::functions::functions("script-package-actions");
    let order = LoadOrder::from_data_dir(data.path(), &ActivePlugins::OfficialOnly).unwrap();
    (data, order)
}

fn run(order: &LoadOrder, state: &mut GameState, source: &str) {
    Runner::new(order, &ScriptCache::default(), state).run_source(source, None, None);
}

fn action(who: u32, package: u32, kind: PackageActionKind) -> Event {
    Event::PackageAction {
        who: FormId(who),
        package: FormId(package),
        kind,
    }
}

#[test]
fn add_requests_begin_then_previous_change_even_for_same_package() {
    let (_data, order) = order();
    let mut state = GameState::new(&order);
    let who = ADULT_REF;

    run(&order, &mut state, "AdultRef.AddScriptPackage TestTravel");
    assert_eq!(
        state.events,
        [action(who, TRAVEL, PackageActionKind::Begin)]
    );
    assert_eq!(
        state.script_packages.get(&FormId(who)),
        Some(&FormId(TRAVEL))
    );

    state.events.clear();
    run(&order, &mut state, "AdultRef.AddScriptPackage TestSandbox");
    assert_eq!(
        state.events,
        [
            action(who, TRAVEL, PackageActionKind::Change),
            action(who, SANDBOX, PackageActionKind::Begin),
        ]
    );
    assert_eq!(
        state.script_packages.get(&FormId(who)),
        Some(&FormId(SANDBOX))
    );

    state.events.clear();
    run(&order, &mut state, "AdultRef.AddScriptPackage TestSandbox");
    assert_eq!(
        state.events,
        [
            action(who, SANDBOX, PackageActionKind::Change),
            action(who, SANDBOX, PackageActionKind::Begin),
        ]
    );
}

#[test]
fn a_non_package_form_leaves_package_state_and_events_untouched() {
    let (_data, order) = order();
    let mut state = GameState::new(&order);
    state
        .script_packages
        .insert(FormId(ADULT_REF), FormId(TRAVEL));
    state.evaluate.remove(&FormId(ADULT_REF));
    state.events.push(Event::PlayerControls(true));
    let before_events = state.events.clone();

    run(&order, &mut state, "AdultRef.AddScriptPackage AdultRef");

    assert_eq!(
        state.script_packages.get(&FormId(ADULT_REF)),
        Some(&FormId(TRAVEL))
    );
    assert!(!state.evaluate.contains(&FormId(ADULT_REF)));
    assert_eq!(state.events, before_events);

    run(&order, &mut state, "AdultRef.AddScriptPackage 57005");
    assert_eq!(
        state.script_packages.get(&FormId(ADULT_REF)),
        Some(&FormId(TRAVEL))
    );
    assert!(!state.evaluate.contains(&FormId(ADULT_REF)));
    assert_eq!(state.events, before_events);
}

#[test]
fn only_actor_targets_receive_package_requests_and_the_player_is_an_actor() {
    let (_data, order) = order();
    let mut state = GameState::new(&order);

    run(&order, &mut state, "CupRef.AddScriptPackage TestTravel");
    assert!(state.script_packages.is_empty());
    assert!(state.events.is_empty());
    assert!(!state.evaluate.contains(&FormId(CUP_REF)));

    run(&order, &mut state, "player.AddScriptPackage TestTravel");
    assert_eq!(
        state.events,
        [action(0x14, TRAVEL, PackageActionKind::Begin)]
    );
    assert_eq!(
        state.script_packages.get(&FormId(0x14)),
        Some(&FormId(TRAVEL))
    );
}

#[test]
fn player_package_blocks_looking_independently_of_controls_and_survives_save() {
    let (_data, order) = order();
    let mut state = GameState::new(&order);
    run(&order, &mut state, "AdultRef.AddScriptPackage TestTravel");
    assert!(!state.player_looking_blocked());
    run(&order, &mut state, "player.AddScriptPackage AdultRef");
    assert!(
        !state.player_looking_blocked(),
        "invalid packages do not lock input"
    );
    run(&order, &mut state, "player.AddScriptPackage TestTravel");
    run(&order, &mut state, "EnablePlayerControls");
    assert!(
        state.player_looking_blocked(),
        "package lock is separate from controls"
    );
    run(&order, &mut state, "player.AddScriptPackage TestSandbox");
    let (mut state, _) = world::save::load(&world::save::save(&state, None)).unwrap();
    assert!(state.player_looking_blocked());
    run(&order, &mut state, "player.RemoveScriptPackage");
    assert!(!state.player_looking_blocked());
    run(&order, &mut state, "DisablePlayerControls 0 0 0 0 1");
    run(&order, &mut state, "player.AddScriptPackage TestTravel");
    run(&order, &mut state, "player.RemoveScriptPackage");
    assert!(
        state.player_looking_blocked(),
        "removal must not enable script-disabled looking"
    );
}

fn value(state: &GameState) -> f32 {
    state.globals[&FormId(testdata::functions::ids::VALUE)]
}

fn travel() -> impl Fn(&LoadOrder) -> world::ai::Package {
    |order| world::ai::Package::load(order, FormId(TRAVEL)).unwrap()
}

/// `HighProcess` slots 358 to 360 run the begin, end and change actions'
/// scripts with the person as their reference.
#[test]
fn a_package_action_runs_its_script_on_the_person() {
    let (_data, order) = order();
    let mut state = GameState::new(&order);
    let scripts = ScriptCache::default();
    let run_action = |state: &mut GameState, kind| {
        Runner::new(&order, &scripts, state).package_action(FormId(ADULT_REF), FormId(TRAVEL), kind)
    };
    assert!(run_action(&mut state, PackageActionKind::Begin));
    assert_eq!(value(&state), 10.0);
    assert!(run_action(&mut state, PackageActionKind::End));
    assert_eq!(value(&state), 20.0);
    assert!(run_action(&mut state, PackageActionKind::Change));
    assert_eq!(value(&state), 30.0);
    // A package with no script in the action, and one that isn't a package.
    let run_other = |state: &mut GameState, package: u32| {
        Runner::new(&order, &scripts, state).package_action(
            FormId(ADULT_REF),
            FormId(package),
            PackageActionKind::End,
        )
    };
    assert!(!run_other(&mut state, SANDBOX));
    assert!(!run_other(&mut state, ADULT_REF));
    assert_eq!(value(&state), 30.0);
}

/// A script's travel package finishes once; giving it again, or taking it
/// away, starts it over.
#[test]
fn a_script_travel_package_finishes_once_and_asks_for_its_end_action() {
    let (_data, order) = order();
    let mut state = GameState::new(&order);
    let package = travel()(&order);
    let who = FormId(ADULT_REF);
    // Nothing given: nothing to finish.
    assert!(!world::ai::finish_travel(&mut state, who, &package));
    run(&order, &mut state, "AdultRef.AddScriptPackage TestTravel");
    state.events.clear();

    assert!(world::ai::finish_travel(&mut state, who, &package));
    assert_eq!(
        state.events,
        [action(ADULT_REF, TRAVEL, PackageActionKind::End)]
    );
    // Once.
    state.events.clear();
    assert!(!world::ai::finish_travel(&mut state, who, &package));
    assert!(state.events.is_empty());
    // Given again: it can finish again.
    run(&order, &mut state, "AdultRef.AddScriptPackage TestTravel");
    state.events.clear();
    assert!(world::ai::finish_travel(&mut state, who, &package));
    // Taken away and given: the same.
    run(&order, &mut state, "AdultRef.RemoveScriptPackage");
    assert!(!world::ai::finish_travel(&mut state, who, &package));
    run(&order, &mut state, "AdultRef.AddScriptPackage TestTravel");
    assert!(world::ai::finish_travel(&mut state, who, &package));
}

#[test]
fn only_a_travel_package_the_script_gave_finishes() {
    let (_data, order) = order();
    let mut state = GameState::new(&order);
    let sandbox = world::ai::Package::load(&order, FormId(SANDBOX)).unwrap();
    let travel = travel()(&order);
    let who = FormId(ADULT_REF);
    // A sandbox package the script gave doesn't end with a walk.
    run(&order, &mut state, "AdultRef.AddScriptPackage TestSandbox");
    assert!(!world::ai::finish_travel(&mut state, who, &sandbox));
    // A travel package that isn't the one the script gave isn't theirs.
    assert!(!world::ai::finish_travel(&mut state, who, &travel));
    assert!(!state.ended_script_packages.contains(&who));
}
