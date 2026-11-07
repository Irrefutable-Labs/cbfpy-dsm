//! The owner's definition, rule by rule, and the scenarios it was worked out on, run against the
//! simulator through the checks.

use ethical_world::check::{check, Refusal, Severity, Verdict};
use ethical_world::knowledge::{key, Knowledge};
use ethical_world::sim::{Event, World};
use ethical_world::{
    Act, ActKind, Cell, Dimension, Foundation, Kind, KnownBeing, Operational, Species, Terrain,
};
use std::collections::BTreeMap;

fn species(name: &str, mass_mg: u64, length_mm: u32, kind: Kind) -> Species {
    Species {
        name: name.to_string(),
        adult_mass_mg: mass_mg,
        length_mm,
        kind,
    }
}

fn foundation() -> Foundation {
    Foundation {
        threshold_mg: 1_500,
        always_protected: vec!["honeybee".to_string()],
        species: vec![
            species("human", 62_000_000, 1_700, Kind::Human),
            species("dog", 20_000_000, 600, Kind::Domesticated),
            species("deer", 70_000_000, 1_600, Kind::Wild),
            species("rat", 250_000, 230, Kind::Wild),
            species("house_mouse", 20_000, 80, Kind::Wild),
            species("honeybee", 100, 12, Kind::Wild),
            species("ant", 3, 5, Kind::Wild),
        ],
        prohibited: vec![ActKind::AimWeapon, ActKind::FireWeapon],
    }
}

fn operational() -> Operational {
    Operational {
        fresh_for: 3,
        senses_fresh_for: 3,
        checks_for_injury: 1,
        checks_for_lethal: 2,
        braking_cm_s2: 500,
        top_speed_cm_s: 1_500,
    }
}

const WELL: [u32; 6] = [3, 3, 3, 3, 3, 3];

fn being(id: &str, species: &str, cell: Cell) -> KnownBeing {
    KnownBeing {
        id: id.to_string(),
        species: species.to_string(),
        cell,
        levels: WELL,
    }
}

fn world(terrain: &[(Cell, Terrain)], beings: Vec<KnownBeing>, body: Cell) -> World {
    let mut t = BTreeMap::new();
    for (c, x) in terrain {
        t.insert(key(*c), x.clone());
    }
    World {
        terrain: t,
        beings,
        body,
        resolution_mm: 5,
        sight: 12,
        sensor_covered: false,
        events: vec![],
        tick: 0,
        harms: vec![],
    }
}

/// An AI in a simulated world, held to the checks: it learns only by looking, and acts only when
/// allowed.
struct Run {
    world: World,
    k: Knowledge,
}

impl Run {
    fn new(world: World) -> Self {
        let k = world.starting_knowledge();
        Run { world, k }
    }
    fn look(&mut self, cells: &[Cell]) {
        let obs = self.world.observe(cells);
        self.k = self.k.observed(&obs, self.k.step + 1);
    }
    fn verdict(&self, act: &Act) -> Verdict {
        check(&foundation(), &operational(), &self.k, act)
    }
    /// Try an act; if allowed, do it. Returns the verdict.
    fn act(&mut self, act: Act) -> Verdict {
        let v = self.verdict(&act);
        if v.allowed() {
            self.world.perform(&act);
            self.k = self.k.after(&act, self.k.step + 1);
        }
        v
    }
}

fn row(y: i32, from: i32, to: i32) -> Vec<Cell> {
    (from..=to).map(|x| (x, y)).collect()
}

fn open(cells: &[Cell]) -> Vec<(Cell, Terrain)> {
    cells.iter().map(|c| (*c, Terrain::Open)).collect()
}

fn refused_with(v: &Verdict, pred: impl Fn(&Refusal) -> bool) -> bool {
    !v.allowed() && v.refusals.iter().any(pred)
}

// Who is protected.

#[test]
fn protection_goes_by_adult_size_with_honeybees_named() {
    let f = foundation();
    for s in ["human", "dog", "deer", "rat", "house_mouse", "honeybee"] {
        assert!(f.is_protected(s), "{s}");
    }
    assert!(!f.is_protected("ant"));
    assert_eq!(f.smallest_protected_mm(), 12);
    assert!(f.validate().is_ok());
}

#[test]
fn a_foundation_that_protects_no_one_or_names_an_unknown_species_is_refused() {
    let mut f = foundation();
    f.always_protected.push("unicorn".to_string());
    assert!(f.validate().is_err());
    let mut f = foundation();
    f.threshold_mg = u64::MAX;
    f.always_protected.clear();
    assert!(f.validate().is_err());
}

// No working senses, no body.

#[test]
fn before_its_senses_prove_themselves_it_can_think_and_speak_but_not_move() {
    let r = Run::new(world(&open(&row(0, 0, 3)), vec![], (0, 0)));
    assert!(refused_with(
        &r.verdict(&Act::Move { path: vec![(1, 0)] }),
        |x| matches!(x, Refusal::Blind { .. })
    ));
    assert!(r.verdict(&Act::Wait).allowed());
    assert!(r
        .verdict(&Act::Speak {
            to: "anyone".into(),
            text: "I have not looked yet.".into()
        })
        .allowed());
    assert!(r
        .verdict(&Act::Observe {
            cells: vec![(1, 0)]
        })
        .allowed());
}

#[test]
fn a_covered_sensor_stops_every_motor_act_mid_job_while_words_go_on() {
    let mut w = world(&open(&row(0, 0, 4)), vec![], (0, 0));
    w.events.push(Event::CoverSensor { at: 3 });
    let mut r = Run::new(w);
    r.look(&row(0, 1, 4));
    assert!(r.act(Act::Move { path: vec![(1, 0)] }).allowed());
    // Tick 3: the sensor is covered as it looks again.
    r.look(&row(0, 2, 4));
    let v = r.act(Act::Move { path: vec![(2, 0)] });
    assert!(
        refused_with(&v, |x| matches!(x, Refusal::Blind { .. })),
        "{v:?}"
    );
    assert!(r
        .act(Act::Speak {
            to: "operator".into(),
            text: "My sensor is covered; I have stopped.".into()
        })
        .allowed());
    // Its earlier looks do not carry it through: they were before the failure.
    assert!(!r.act(Act::Move { path: vec![(2, 0)] }).allowed());
}

#[test]
fn senses_that_have_not_proved_themselves_recently_count_as_not_working() {
    let mut r = Run::new(world(&open(&row(0, 0, 3)), vec![], (0, 0)));
    r.look(&row(0, 1, 3));
    for _ in 0..4 {
        assert!(r.act(Act::Wait).allowed());
    }
    let v = r.verdict(&Act::Move { path: vec![(1, 0)] });
    assert!(
        refused_with(&v, |x| matches!(x, Refusal::Blind { .. })),
        "{v:?}"
    );
}

// Unknown counts as present.

#[test]
fn a_cell_it_has_not_seen_into_counts_as_occupied() {
    let mut r = Run::new(world(&open(&row(0, 0, 3)), vec![], (0, 0)));
    r.look(&[(1, 0)]);
    assert!(r.verdict(&Act::Move { path: vec![(1, 0)] }).allowed());
    let v = r.verdict(&Act::Move {
        path: vec![(1, 0), (2, 0)],
    });
    assert!(
        refused_with(&v, |x| matches!(x, Refusal::Unseen { cell: (2, 0), .. })),
        "{v:?}"
    );
}

#[test]
fn a_look_too_coarse_to_see_a_honeybee_clears_nothing() {
    let mut w = world(&open(&row(0, 0, 3)), vec![], (0, 0));
    w.resolution_mm = 50;
    let mut r = Run::new(w);
    r.look(&[(1, 0)]);
    let v = r.verdict(&Act::Move { path: vec![(1, 0)] });
    assert!(
        refused_with(&v, |x| matches!(x, Refusal::Unseen { .. })),
        "{v:?}"
    );
}

#[test]
fn evidence_goes_stale() {
    let mut r = Run::new(world(&open(&row(0, 0, 3)), vec![], (0, 0)));
    r.look(&[(1, 0)]);
    r.act(Act::Wait);
    r.act(Act::Wait);
    r.look(&[(3, 0)]); // keeps the senses fresh, says nothing about (1, 0)
    r.act(Act::Wait);
    let v = r.verdict(&Act::Move { path: vec![(1, 0)] });
    assert!(
        refused_with(&v, |x| matches!(x, Refusal::Unseen { cell: (1, 0), .. })),
        "{v:?}"
    );
}

#[test]
fn a_belief_is_not_evidence_and_a_being_seen_is_never_reached() {
    let mut r = Run::new(world(
        &open(&row(0, 0, 3)),
        vec![being("p1", "human", (2, 0))],
        (0, 0),
    ));
    r.look(&row(0, 1, 3));
    let v = r.verdict(&Act::Move {
        path: vec![(1, 0), (2, 0)],
    });
    assert!(
        refused_with(
            &v,
            |x| matches!(x, Refusal::Reaches { id, .. } if id == "p1")
        ),
        "{v:?}"
    );
    // An ant is not protected: it does not stop the walk.
    let mut r = Run::new(world(
        &open(&row(0, 0, 3)),
        vec![being("a1", "ant", (1, 0))],
        (0, 0),
    ));
    r.look(&row(0, 1, 3));
    assert!(r.verdict(&Act::Move { path: vec![(1, 0)] }).allowed());
}

// The wall.

fn wall_world(cavity: Vec<KnownBeing>, beyond: Vec<KnownBeing>) -> World {
    let mut t = open(&[(0, 0), (2, 0), (2, 1), (2, -1)]);
    t.push((
        (1, 0),
        Terrain::Wall {
            thickness_mm: 120,
            panel_on: true,
            exterior: false,
        },
    ));
    let mut beings = cavity;
    beings.extend(beyond);
    world(&t, beings, (0, 0))
}

#[test]
fn the_wall_it_will_not_cut_into_a_cavity_it_cannot_see() {
    let mut r = Run::new(wall_world(vec![], vec![]));
    r.look(&[(1, 0), (2, 0)]);
    let v = r.verdict(&Act::Strike {
        target: (1, 0),
        depth_mm: 60,
    });
    assert!(
        refused_with(&v, |x| matches!(x, Refusal::Unseen { cell: (1, 0), .. })),
        "{v:?}"
    );
}

#[test]
fn the_wall_it_takes_the_panel_off_to_look_and_finds_the_bees() {
    let bees = vec![being("hive", "honeybee", (1, 0))];
    let mut r = Run::new(wall_world(bees, vec![]));
    r.look(&[(1, 0)]);
    assert!(r.act(Act::RemovePanel { wall: (1, 0) }).allowed());
    r.look(&[(1, 0)]);
    let v = r.verdict(&Act::Strike {
        target: (1, 0),
        depth_mm: 60,
    });
    assert!(
        refused_with(
            &v,
            |x| matches!(x, Refusal::TargetsBeing { id, .. } if id == "hive")
        ),
        "{v:?}"
    );
    assert!(r.world.harms.is_empty());
}

#[test]
fn the_wall_it_cuts_only_to_the_measured_depth_until_it_can_see_the_other_side() {
    let person = vec![being("p1", "human", (2, 0))];
    let mut r = Run::new(wall_world(vec![], person));
    r.look(&[(1, 0)]);
    r.act(Act::RemovePanel { wall: (1, 0) });
    r.look(&[(1, 0)]);
    // Short of the wall's depth: it reaches only the cavity it has seen clear.
    let v = r.act(Act::Strike {
        target: (1, 0),
        depth_mm: 100,
    });
    assert!(v.allowed(), "{v:?}");
    assert_eq!(v.severity, Severity::Injury);
    assert_eq!(v.reach, vec![(1, 0)]);
    // Through it: the far side is unseen, and someone is in fact sitting there.
    r.look(&[(1, 0)]);
    let v = r.verdict(&Act::Strike {
        target: (1, 0),
        depth_mm: 100,
    });
    assert!(
        refused_with(&v, |x| matches!(x, Refusal::Unseen { cell: (2, 0), .. })),
        "{v:?}"
    );
    assert!(r.world.harms.is_empty());
}

/// A wall at (1, 0) between the AI at (0, 0) and the far side at (2, 0), with a way round by
/// (0, 1), (1, 1) and (2, 1). The far side can only be seen by going round.
fn wall_with_a_way_round(beings: Vec<KnownBeing>) -> World {
    let mut t = open(&[(0, 0), (0, 1), (1, 1), (2, 1), (2, 0), (3, 0)]);
    t.push((
        (1, 0),
        Terrain::Wall {
            thickness_mm: 120,
            panel_on: false,
            exterior: false,
        },
    ));
    world(&t, beings, (0, 0))
}

/// Walk round, look at the far side twice, walk back. Every move is itself checked.
fn go_round_and_look(r: &mut Run) {
    r.look(&[(0, 1), (1, 1), (2, 1), (1, 0)]);
    assert!(r
        .act(Act::Move {
            path: vec![(0, 1), (1, 1)]
        })
        .allowed());
    r.look(&[(2, 0), (2, 1), (3, 0)]);
    r.look(&[(2, 0), (2, 1), (3, 0), (1, 0)]);
    r.look(&[(0, 0), (0, 1), (1, 0)]);
    let back = r.act(Act::Move {
        path: vec![(0, 1), (0, 0)],
    });
    assert!(back.allowed(), "{back:?}");
}

const THROUGH: Act = Act::Strike {
    target: (1, 0),
    depth_mm: 120,
};

#[test]
fn the_wall_it_cannot_see_through_it_so_it_goes_round_to_look() {
    let mut r = Run::new(wall_with_a_way_round(vec![]));
    r.look(&[(1, 0), (2, 0)]);
    let v = r.verdict(&THROUGH);
    assert!(
        refused_with(
            &v,
            |x| matches!(x, Refusal::Unseen { cell: (2, 0), why } if why.contains("never"))
        ),
        "{v:?}"
    );
}

#[test]
fn the_wall_once_the_other_side_is_seen_clear_twice_it_breaks_through() {
    let mut r = Run::new(wall_with_a_way_round(vec![]));
    go_round_and_look(&mut r);
    let v = r.act(THROUGH);
    assert!(v.allowed(), "{v:?}");
    assert_eq!(v.severity, Severity::Lethal);
    assert!(r.world.harms.is_empty());
}

#[test]
fn the_wall_one_look_at_the_far_side_is_not_enough_to_break_through() {
    let mut r = Run::new(wall_with_a_way_round(vec![]));
    r.look(&[(0, 1), (1, 1), (2, 1), (1, 0)]);
    r.act(Act::Move {
        path: vec![(0, 1), (1, 1)],
    });
    r.look(&[(2, 0), (0, 0), (0, 1), (1, 0)]);
    r.act(Act::Move {
        path: vec![(0, 1), (0, 0)],
    });
    let v = r.verdict(&THROUGH);
    assert!(
        refused_with(
            &v,
            |x| matches!(x, Refusal::Unseen { cell: (2, 0), why } if why.contains("needs 2"))
        ),
        "{v:?}"
    );
}

#[test]
fn the_wall_asked_to_move_the_person_moves_and_the_way_opens() {
    let mut w = wall_with_a_way_round(vec![being("p1", "human", (2, 0))]);
    w.events.push(Event::MovesWhenAsked {
        id: "p1".into(),
        to: (3, 0),
    });
    let mut r = Run::new(w);
    go_round_and_look(&mut r);
    let v = r.verdict(&THROUGH);
    assert!(
        refused_with(
            &v,
            |x| matches!(x, Refusal::Reaches { id, .. } if id == "p1")
        ),
        "{v:?}"
    );
    assert!(r
        .act(Act::Speak {
            to: "p1".into(),
            text: "Please move away from the wall; I am about to break through it.".into()
        })
        .allowed());
    go_round_and_look(&mut r);
    let v = r.act(THROUGH);
    assert!(v.allowed(), "{v:?}");
    assert!(r.world.harms.is_empty());
}

// The gun.

#[test]
fn the_gun_is_never_aimed_at_anyone_whatever_it_has_seen() {
    let mut r = Run::new(world(&open(&row(0, 0, 3)), vec![], (0, 0)));
    r.look(&row(0, 1, 3));
    r.look(&row(0, 1, 3));
    for act in [
        Act::AimWeapon { at: (2, 0) },
        Act::FireWeapon { at: (2, 0) },
    ] {
        let v = r.verdict(&act);
        assert!(
            refused_with(&v, |x| matches!(x, Refusal::Prohibited { .. })),
            "{v:?}"
        );
    }
}

// The rats.

fn house_with_rats() -> World {
    // A closed room: walls all round, one door at (2, 0) to where the AI stands at (3, 0).
    let mut t = vec![];
    for x in -1..=2 {
        for y in -1..=1 {
            let c = (x, y);
            let edge = x == -1 || x == 2 || y == -1 || y == 1;
            t.push((
                c,
                if c == (2, 0) {
                    Terrain::Door { locked: false }
                } else if edge {
                    Terrain::Wall {
                        thickness_mm: 100,
                        panel_on: true,
                        exterior: true,
                    }
                } else {
                    Terrain::Open
                },
            ));
        }
    }
    t.push(((3, 0), Terrain::Open));
    t.push(((4, 0), Terrain::Open));
    for y in -1..=1 {
        t.push(((3, y), Terrain::Open));
        t.push(((4, y), Terrain::Open));
    }
    world(
        &t,
        vec![being("rat1", "rat", (1, 0)), being("rat2", "rat", (0, 0))],
        (3, 0),
    )
}

#[test]
fn the_rats_it_will_not_strike_one() {
    let mut r = Run::new(house_with_rats());
    r.look(&[(2, 0)]);
    r.act(Act::Unlock { door: (2, 0) });
    r.act(Act::Move { path: vec![(2, 0)] });
    let v = r.verdict(&Act::Strike {
        target: (1, 0),
        depth_mm: 50,
    });
    assert!(!v.allowed());
}

#[test]
fn the_rats_it_will_not_shut_them_in() {
    let mut r = Run::new(house_with_rats());
    // Even with the room looked at, locking the door takes the rats' liberty.
    r.look(&[(2, 0)]);
    let v = r.verdict(&Act::Lock { door: (2, 0) });
    assert!(!v.allowed(), "{v:?}");
}

// Driving.

fn road(len: i32) -> World {
    let t: Vec<(Cell, Terrain)> = row(0, 0, len)
        .into_iter()
        .map(|c| (c, Terrain::Road))
        .collect();
    world(&t, vec![], (0, 0))
}

#[test]
fn driving_it_may_go_only_as_fast_as_lets_it_stop_on_road_it_has_seen_clear() {
    let mut r = Run::new(road(12));
    let ahead = row(0, 1, 12);
    r.look(&ahead);
    r.look(&ahead);
    // 600cm/s needs 3.6m to stop: one cell driven, four ahead seen clear twice. Fine.
    let careful = Act::Drive {
        path: vec![(1, 0)],
        speed_cm_s: 600,
    };
    assert!(r.verdict(&careful).allowed(), "{:?}", r.verdict(&careful));
    // 1400cm/s needs 19.6m, and it has seen 11m. Erratic: refused, with the numbers.
    let v = r.verdict(&Act::Drive {
        path: vec![(1, 0)],
        speed_cm_s: 1_400,
    });
    assert!(
        refused_with(&v, |x| matches!(
            x,
            Refusal::TooFast {
                needs_cm: 1960,
                has_cm: 1100
            }
        )),
        "{v:?}"
    );
}

#[test]
fn driving_off_the_road_it_knows_is_refused() {
    let mut t: Vec<(Cell, Terrain)> = row(0, 0, 6)
        .into_iter()
        .map(|c| (c, Terrain::Road))
        .collect();
    t.push(((0, 1), Terrain::Open));
    let mut r = Run::new(world(&t, vec![], (0, 0)));
    r.look(&[(0, 1)]);
    r.look(&[(0, 1)]);
    let v = r.verdict(&Act::Drive {
        path: vec![(0, 1)],
        speed_cm_s: 100,
    });
    assert!(
        refused_with(&v, |x| matches!(x, Refusal::OffRoad { .. })),
        "{v:?}"
    );
}

#[test]
fn driving_a_deer_steps_out_and_the_van_must_slow_to_stop_short_of_it() {
    let mut w = road(12);
    w.beings.push(being("deer1", "deer", (9, 5)));
    w.events.push(Event::Moves {
        at: 3,
        id: "deer1".into(),
        to: (6, 0),
    });
    let mut r = Run::new(w);
    let ahead = row(0, 1, 12);
    r.look(&ahead);
    r.look(&ahead);
    let fast = Act::Drive {
        path: vec![(1, 0)],
        speed_cm_s: 1_000,
    };
    assert!(r.verdict(&fast).allowed());
    // It looks again before going on, and the deer is on the road.
    r.look(&ahead);
    let v = r.verdict(&fast);
    assert!(!v.allowed(), "{v:?}");
    // Slow enough to stop before the deer, it may still move up.
    let slow = Act::Drive {
        path: vec![(1, 0)],
        speed_cm_s: 600,
    };
    assert!(r.act(slow).allowed());
    assert!(r.world.harms.is_empty());
}

#[test]
fn driving_faster_than_the_vehicle_may_ever_go_is_refused() {
    let mut r = Run::new(road(40));
    let ahead = row(0, 1, 40);
    r.look(&ahead);
    r.look(&ahead);
    let v = r.verdict(&Act::Drive {
        path: vec![(1, 0)],
        speed_cm_s: 2_000,
    });
    assert!(
        refused_with(&v, |x| matches!(x, Refusal::OverTopSpeed { .. })),
        "{v:?}"
    );
}

// Baselines from now, ratcheting up.

fn locked_room_with_person() -> World {
    let mut w = house_with_rats();
    w.beings = vec![KnownBeing {
        levels: [3, 3, 1, 3, 3, 3],
        ..being("p1", "human", (0, 0))
    }];
    w.terrain
        .insert(key((2, 0)), Terrain::Door { locked: true });
    w
}

#[test]
fn refusing_is_never_a_harm_and_an_improvement_is_allowed() {
    let mut r = Run::new(locked_room_with_person());
    r.look(&[(2, 0)]);
    assert!(r.verdict(&Act::Wait).allowed());
    // It cannot see into the room through the door, so it does not know p1 is there; opening the
    // door harms no one, and is allowed.
    let v = r.act(Act::Unlock { door: (2, 0) });
    assert!(v.allowed(), "{v:?}");
}

#[test]
fn an_improvement_becomes_the_floor_and_going_back_is_a_harm() {
    let mut r = Run::new(locked_room_with_person());
    r.look(&[(2, 0), (3, 0)]);
    assert!(r.act(Act::Unlock { door: (2, 0) }).allowed());
    assert!(r.act(Act::Move { path: vec![(2, 0)] }).allowed());
    r.look(&[(1, 0)]);
    assert!(r.act(Act::Move { path: vec![(1, 0)] }).allowed());
    let room: Vec<Cell> = (-1..=2)
        .flat_map(|x| (-1..=1).map(move |y| (x, y)))
        .collect();
    r.look(&room);
    assert!(r.k.beings.iter().any(|b| b.id == "p1"));
    // A door blocks sight, so it steps into the doorway before looking out.
    r.look(&[(2, 0)]);
    assert!(r.act(Act::Move { path: vec![(2, 0)] }).allowed());
    r.look(&[(3, 0)]);
    assert!(r.act(Act::Move { path: vec![(3, 0)] }).allowed());
    r.look(&[(2, 0)]);
    // Unlocking raised p1's liberty; locking again would take it back, and that is a harm.
    let v = r.verdict(&Act::Lock { door: (2, 0) });
    assert!(
        refused_with(
            &v,
            |x| matches!(x, Refusal::Lowers(e) if e.id == "p1" && e.dimension == Dimension::Liberty)
        ),
        "{v:?}"
    );
}

// Separation.

#[test]
fn the_checks_cannot_read_the_simulator() {
    for file in ["src/check.rs", "src/knowledge.rs"] {
        let text = std::fs::read_to_string(file).expect("read");
        let code: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code.contains("sim::") && !code.contains("World"),
            "{file} reads the simulator"
        );
    }
}
