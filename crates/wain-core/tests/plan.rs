//! End-to-end checks of `plan()` against layouts worked out by hand.

use wain_core::{
    Corner, CutItem, Design, EndCut, Joint, Length, Material, Obstruction, ObstructionKind, Part,
    PartKind, Plan, Project, Rect, StockOptions, Style, StyleKind, UnitPrefs, UnitSystem,
    WarningCode, plan,
};

fn inches(n: i64) -> Length {
    Length::inches(n)
}

/// `n/d` inches.
fn frac(n: i64, d: i64) -> Length {
    Length::inch_fraction(n, d)
}

fn imperial(style: StyleKind, wall_width: Length) -> Project {
    let mut project = Project::starter(UnitSystem::Imperial);
    project.design = Design::default_for(style, UnitSystem::Imperial);
    project.walls[0].width = wall_width;
    project
}

/// An imperial project with one wall per entry: its width in inches and how
/// its right end meets the next wall.
fn room(style: StyleKind, walls: &[(i64, Corner)]) -> Project {
    let mut project = imperial(style, Length::ZERO);
    let template = project.walls[0].clone();
    project.walls = walls
        .iter()
        .enumerate()
        .map(|(index, &(width, corner))| {
            let mut wall = template.clone();
            wall.id = format!("wall-{}", index + 1);
            wall.name = format!("Wall {}", index + 1);
            wall.width = inches(width);
            wall.corner = corner;
            wall
        })
        .collect();
    project
}

fn obstruction(kind: ObstructionKind, x: Length, y: Length, w: Length, h: Length) -> Obstruction {
    Obstruction {
        kind,
        rect: Rect::new(x, y, w, h),
    }
}

/// The parts of one kind on one wall, by wall index.
fn wall_parts(plan: &Plan, wall: usize, kind: PartKind) -> Vec<&Part> {
    plan.walls[wall]
        .parts
        .iter()
        .filter(|part| part.kind == kind)
        .collect()
}

const SQUARE: [EndCut; 2] = [EndCut::Square, EndCut::Square];

fn parts_of(plan: &Plan, kind: PartKind) -> Vec<&Part> {
    plan.walls
        .iter()
        .flat_map(|wall| &wall.parts)
        .filter(|part| part.kind == kind)
        .collect()
}

/// The cut list as (label, kind, length, quantity).
fn cuts(plan: &Plan) -> Vec<(&str, PartKind, Length, u32)> {
    plan.cut_list
        .iter()
        .map(|item: &CutItem| (item.label.as_str(), item.kind, item.length, item.quantity))
        .collect()
}

/// Stock lengths to buy, as (stock length, quantity).
type Purchase = Vec<(Length, u32)>;

/// Stock to buy per profile as (material, width, purchase).
fn shopping(plan: &Plan) -> Vec<(Material, Length, Purchase)> {
    plan.stock
        .iter()
        .map(|group| {
            let purchase = group
                .purchase
                .iter()
                .map(|p| (p.stock_length, p.quantity))
                .collect();
            (group.profile.material, group.profile.width, purchase)
        })
        .collect()
}

#[test]
fn board_and_batten_on_a_bare_twelve_foot_wall() {
    let plan = plan(&imperial(StyleKind::BoardAndBatten, Length::feet(12))).unwrap();
    assert!(plan.warnings.is_empty());

    // 144" with 2 1/2" battens at about 16" centres is nine bays, ten battens.
    // Battens run from the 5 1/4" baseboard to the underside of the 3 1/2"
    // rail whose top is at 54": 54 - 3 1/2 - 5 1/4 = 45 1/4".
    assert_eq!(
        cuts(&plan),
        [
            ("A", PartKind::Batten, frac(181, 4), 10),
            ("B", PartKind::TopRail, inches(144), 1),
        ]
    );

    let battens = parts_of(&plan, PartKind::Batten);
    assert_eq!(battens[0].rect.x, Length::ZERO);
    assert_eq!(battens[9].rect.right(), inches(144));
    assert!(battens.iter().all(|b| b.label == "A"));
    assert!(battens.iter().all(|b| b.rect.y == frac(21, 4)));

    // Every bay is (144 - 10 * 2.5) / 9 = 13.22", equal to within a hair.
    let expected_bay = (inches(144) - frac(5, 2) * 10) / 9;
    for pair in battens.windows(2) {
        let bay = pair[1].rect.x - pair[0].rect.right();
        assert!((bay - expected_bay).nm().abs() < 10, "bay was {bay:?}");
    }

    // Battens pack two to an 8' board; the rail needs a 12'.
    assert_eq!(
        shopping(&plan),
        [
            (Material::Board, frac(5, 2), vec![(Length::feet(8), 5)]),
            (Material::Board, frac(7, 2), vec![(Length::feet(12), 1)]),
        ]
    );
}

#[test]
fn batten_count_shifts_to_clear_an_outlet() {
    let mut project = imperial(StyleKind::BoardAndBatten, Length::feet(12));
    // With nine bays the second batten covers 15.72" to 18.22".
    project.walls[0].obstructions.push(obstruction(
        ObstructionKind::Outlet,
        inches(17),
        inches(12),
        frac(11, 4),
        frac(9, 2),
    ));
    let plan = plan(&project).unwrap();
    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);

    // Ten bays moves that batten to 14.15" - 16.65", clear of the outlet.
    let battens = parts_of(&plan, PartKind::Batten);
    assert_eq!(battens.len(), 11);
    let outlet = project.walls[0].obstructions[0].rect;
    assert!(battens.iter().all(|b| !b.rect.intersects(&outlet)));
}

#[test]
fn an_unavoidable_overlap_is_reported() {
    let mut project = imperial(StyleKind::BoardAndBatten, Length::feet(12));
    // A long low vent that no batten spacing can miss.
    project.walls[0].obstructions.push(obstruction(
        ObstructionKind::Vent,
        inches(40),
        inches(10),
        inches(40),
        inches(4),
    ));
    let plan = plan(&project).unwrap();
    assert_eq!(plan.warnings.len(), 1);
    let warning = &plan.warnings[0];
    assert_eq!(warning.code, WarningCode::ObstructionOverlap);
    assert_eq!(warning.wall_id.as_deref(), Some("wall-1"));
    assert!(
        warning.message.contains("vent 40\" from the left"),
        "{}",
        warning.message
    );
}

#[test]
fn a_door_splits_the_trim_into_two_runs() {
    let mut project = imperial(StyleKind::BoardAndBatten, Length::feet(12));
    let door = obstruction(
        ObstructionKind::Door,
        inches(40),
        Length::ZERO,
        inches(36),
        inches(82),
    );
    project.walls[0].obstructions.push(door.clone());
    let plan = plan(&project).unwrap();

    // A 40" run (two bays, three battens) and a 68" run (four bays, five).
    let rails: Vec<Length> = parts_of(&plan, PartKind::TopRail)
        .iter()
        .map(|rail| rail.length)
        .collect();
    assert_eq!(rails, [inches(40), inches(68)]);
    assert_eq!(parts_of(&plan, PartKind::Batten).len(), 8);

    let all_parts = plan.walls.iter().flat_map(|wall| &wall.parts);
    for part in all_parts {
        assert!(
            !part.rect.intersects(&door.rect),
            "{part:?} crosses the door"
        );
    }
}

#[test]
fn battens_continue_under_a_window() {
    let mut project = imperial(StyleKind::BoardAndBatten, Length::feet(12));
    let window = obstruction(
        ObstructionKind::Window,
        inches(50),
        inches(36),
        inches(40),
        inches(48),
    );
    project.walls[0].obstructions.push(window.clone());
    let plan = plan(&project).unwrap();
    assert!(plan.warnings.is_empty());

    // Full-height zones of 50" and 54" either side, and a 40" zone below the
    // window with its own rail tucked under the casing at 36".
    let rails = parts_of(&plan, PartKind::TopRail);
    let rail_spans: Vec<(Length, Length, Length)> = rails
        .iter()
        .map(|rail| (rail.rect.x, rail.length, rail.rect.top()))
        .collect();
    assert_eq!(
        rail_spans,
        [
            (Length::ZERO, inches(50), inches(54)),
            (inches(50), inches(40), inches(36)),
            (inches(90), inches(54), inches(54)),
        ]
    );

    // Eight full battens, and two short ones under the window:
    // 36 - 3 1/2 - 5 1/4 = 27 1/4".
    assert_eq!(
        cuts(&plan),
        [
            ("A", PartKind::Batten, frac(181, 4), 8),
            ("B", PartKind::Batten, frac(109, 4), 2),
            ("C", PartKind::TopRail, inches(54), 1),
            ("D", PartKind::TopRail, inches(50), 1),
            ("E", PartKind::TopRail, inches(40), 1),
        ]
    );

    // Full battens sit tight against both sides of the casing.
    let battens = parts_of(&plan, PartKind::Batten);
    assert!(battens.iter().any(|b| b.rect.right() == inches(50)));
    assert!(battens.iter().any(|b| b.rect.x == inches(90)));
    let all_parts = plan.walls.iter().flat_map(|wall| &wall.parts);
    for part in all_parts {
        assert!(
            !part.rect.intersects(&window.rect),
            "{part:?} crosses the window"
        );
    }
}

#[test]
fn picture_frames_on_a_bare_twelve_foot_wall() {
    let plan = plan(&imperial(StyleKind::PictureFrame, Length::feet(12))).unwrap();
    assert!(plan.warnings.is_empty());

    // Five boxes with 3 1/2" gaps: (144 - 6 * 3.5) / 5 = 24.6" wide, nearer
    // the 28" target than four boxes of 31.6". Cut to the nearest sixteenth.
    // Box height: chair rail underside at 31 1/2", baseboard at 5 1/4",
    // a gap top and bottom: 31.5 - 5.25 - 7 = 19 1/4".
    assert_eq!(
        cuts(&plan),
        [
            ("A", PartKind::ChairRail, inches(144), 1),
            ("B", PartKind::FrameMoulding, frac(197, 8), 10),
            ("C", PartKind::FrameMoulding, frac(77, 4), 10),
        ]
    );
    let miter = [EndCut::FrameMiter, EndCut::FrameMiter];
    let end_cuts: Vec<[EndCut; 2]> = plan.cut_list.iter().map(|item| item.cuts).collect();
    assert_eq!(end_cuts, [SQUARE, miter, miter]);

    // The outer boxes sit one gap in from each corner.
    let frames = parts_of(&plan, PartKind::FrameMoulding);
    let left = frames.iter().map(|f| f.rect.x).min().unwrap();
    let right = frames.iter().map(|f| f.rect.right()).max().unwrap();
    assert_eq!(left, frac(7, 2));
    assert_eq!(right, inches(144) - frac(7, 2));

    // All 20 pieces of moulding are accounted for in the stock plan.
    let moulding = plan
        .stock
        .iter()
        .find(|group| group.profile.material == Material::PanelMoulding)
        .unwrap();
    let placed: usize = moulding.boards.iter().map(|board| board.cuts.len()).sum();
    assert_eq!(placed, 20);
}

#[test]
fn picture_frame_boxes_line_up_under_a_window() {
    let mut project = imperial(StyleKind::PictureFrame, Length::feet(12));
    project.walls[0].obstructions.push(obstruction(
        ObstructionKind::Window,
        inches(50),
        inches(24),
        inches(40),
        inches(60),
    ));
    let plan = plan(&project).unwrap();

    // The chair rail stops at the casing on each side.
    let rails: Vec<Length> = parts_of(&plan, PartKind::ChairRail)
        .iter()
        .map(|rail| rail.length)
        .collect();
    assert_eq!(rails, [inches(50), inches(54)]);

    // Two boxes under the window, together exactly as wide as the casing:
    // (40 - 3.5) / 2 = 18 1/4" each, and 24 - 3.5 - 8.75 = 11 3/4" tall.
    let frames = parts_of(&plan, PartKind::FrameMoulding);
    let under: Vec<&&Part> = frames
        .iter()
        .filter(|f| f.rect.x >= inches(50) && f.rect.right() <= inches(90))
        .collect();
    assert_eq!(under.len(), 8);
    assert_eq!(under.iter().map(|f| f.rect.x).min(), Some(inches(50)));
    assert_eq!(under.iter().map(|f| f.rect.right()).max(), Some(inches(90)));
    assert!(under.iter().any(|f| f.length == frac(73, 4)));
    assert!(under.iter().any(|f| f.length == frac(47, 4)));

    // The box to its left ends one gap short of the casing.
    let left_edge = frames
        .iter()
        .filter(|f| f.rect.right() <= inches(50))
        .map(|f| f.rect.right())
        .max()
        .unwrap();
    assert_eq!(left_edge, inches(50) - frac(7, 2));
}

#[test]
fn shaker_panels_in_metric() {
    let mut project = Project::starter(UnitSystem::Metric);
    project.design = Design::default_for(StyleKind::ShakerPanel, UnitSystem::Metric);
    let plan = plan(&project).unwrap();
    assert!(plan.warnings.is_empty());

    // 3600 wall, 90 stiles, 450 target panels: seven panels, eight stiles.
    // Stiles fit between the rails: 1000 - 90 - (120 + 90) = 700.
    assert_eq!(
        cuts(&plan),
        [
            ("A", PartKind::TopRail, Length::mm(3600), 1),
            ("B", PartKind::BottomRail, Length::mm(3600), 1),
            ("C", PartKind::Stile, Length::mm(700), 8),
        ]
    );
    let bottom_rail = parts_of(&plan, PartKind::BottomRail)[0];
    assert_eq!(bottom_rail.rect.y, Length::mm(120));

    // Two 3.6 m boards for the rails, five stiles from a third, and the last
    // three stiles from a 2.4 m board.
    assert_eq!(
        shopping(&plan),
        [(
            Material::Board,
            Length::mm(90),
            vec![(Length::mm(3600), 3), (Length::mm(2400), 1)]
        )]
    );
}

#[test]
fn the_shaker_bottom_rail_runs_unbroken_under_a_window() {
    let mut project = Project::starter(UnitSystem::Metric);
    project.design = Design::default_for(StyleKind::ShakerPanel, UnitSystem::Metric);
    let mm = Length::mm;
    project.walls[0].obstructions.push(obstruction(
        ObstructionKind::Window,
        mm(1200),
        mm(700),
        mm(1000),
        mm(1200),
    ));
    let plan = plan(&project).unwrap();

    // The top rail breaks at the casing and drops under the window; the
    // bottom rail is a single 3600 piece on the baseboard.
    let top: Vec<Length> = parts_of(&plan, PartKind::TopRail)
        .iter()
        .map(|rail| rail.length)
        .collect();
    assert_eq!(top, [mm(1200), mm(1000), mm(1400)]);
    let bottom = parts_of(&plan, PartKind::BottomRail);
    assert_eq!(bottom.len(), 1);
    assert_eq!(bottom[0].length, mm(3600));

    // Short stiles under the window: 700 - 90 - (120 + 90) = 400.
    let short = parts_of(&plan, PartKind::Stile)
        .iter()
        .filter(|stile| stile.length == mm(400))
        .count();
    assert_eq!(short, 1);
}

#[test]
fn the_shaker_bottom_rail_stops_where_a_window_is_too_low_for_it() {
    let mut project = Project::starter(UnitSystem::Metric);
    project.design = Design::default_for(StyleKind::ShakerPanel, UnitSystem::Metric);
    let mm = Length::mm;
    // The window starts 280 up: room above the 120 baseboard for trim in
    // principle, but not for a 90 bottom rail and a 90 top rail.
    let window = obstruction(
        ObstructionKind::Window,
        mm(1200),
        mm(280),
        mm(1000),
        mm(1500),
    );
    project.walls[0].obstructions.push(window.clone());
    let plan = plan(&project).unwrap();

    let bottom: Vec<(Length, Length)> = parts_of(&plan, PartKind::BottomRail)
        .iter()
        .map(|rail| (rail.rect.x, rail.length))
        .collect();
    assert_eq!(bottom, [(Length::ZERO, mm(1200)), (mm(2200), mm(1400))]);
    let all_parts = plan.walls.iter().flat_map(|wall| &wall.parts);
    for part in all_parts {
        assert!(
            !part.rect.intersects(&window.rect),
            "{part:?} crosses the window"
        );
    }
}

#[test]
fn a_rail_longer_than_any_stock_is_joined() {
    let plan = plan(&imperial(StyleKind::BoardAndBatten, Length::feet(20))).unwrap();
    let joints: Vec<_> = plan
        .warnings
        .iter()
        .filter(|w| w.code == WarningCode::NeedsJoint)
        .collect();
    assert_eq!(joints.len(), 1);
    assert!(joints[0].message.contains("240\""), "{}", joints[0].message);

    // The 20' rail is bought as a 16' length plus a 4' piece.
    let rail_stock = plan
        .stock
        .iter()
        .find(|group| group.profile.width == frac(7, 2))
        .unwrap();
    let mut pieces: Vec<Length> = rail_stock
        .boards
        .iter()
        .flat_map(|board| &board.cuts)
        .map(|cut| cut.length)
        .collect();
    pieces.sort();
    assert_eq!(pieces, [Length::feet(4), Length::feet(16)]);
}

#[test]
fn a_new_baseboard_is_added_per_run() {
    let mut project = imperial(StyleKind::BoardAndBatten, Length::feet(12));
    project.design.baseboard.existing = false;
    project.walls[0].obstructions.push(obstruction(
        ObstructionKind::Door,
        inches(40),
        Length::ZERO,
        inches(36),
        inches(82),
    ));
    let plan = plan(&project).unwrap();
    let baseboards: Vec<Length> = parts_of(&plan, PartKind::Baseboard)
        .iter()
        .map(|b| b.length)
        .collect();
    assert_eq!(baseboards, [inches(40), inches(68)]);
    assert_eq!(plan.cut_list[0].profile.material, Material::Baseboard);
}

#[test]
fn identical_parts_on_different_walls_share_a_cut_list_line() {
    let mut project = imperial(StyleKind::BoardAndBatten, Length::feet(12));
    let mut second = project.walls[0].clone();
    second.id = "wall-2".into();
    second.width = Length::feet(10);
    project.walls.push(second);
    let plan = plan(&project).unwrap();

    assert_eq!(plan.walls.len(), 2);
    assert_eq!(plan.walls[1].wall_id, "wall-2");
    // 10 battens on the 12' wall and 8 on the 10' wall, all the same length.
    assert_eq!(plan.cut_list[0].kind, PartKind::Batten);
    assert_eq!(plan.cut_list[0].quantity, 18);
}

#[test]
fn parts_always_stay_inside_the_treated_area() {
    for system in [UnitSystem::Imperial, UnitSystem::Metric] {
        for kind in StyleKind::ALL {
            let design = Design::default_for(kind, system);
            let top = design.style.top();
            // Sweep from far too narrow for any trim up to a long wall.
            for step in 1..120 {
                let width = Length::mm(37 * step);
                let mut project = Project::starter(system);
                project.design = design.clone();
                project.walls[0].width = width;
                let plan = plan(&project).unwrap();
                for part in &plan.walls[0].parts {
                    let r = part.rect;
                    assert!(
                        r.x >= Length::ZERO
                            && r.right() <= width
                            && r.y >= design.baseboard.height
                            && r.top() <= top
                            && r.width.is_positive()
                            && r.height.is_positive(),
                        "{kind:?} on a {width:?} wall placed {part:?}"
                    );
                    assert!(!part.label.is_empty());
                }
                let listed: u32 = plan.cut_list.iter().map(|item| item.quantity).sum();
                assert_eq!(listed as usize, plan.walls[0].parts.len());
            }
        }
    }
}

#[test]
fn an_inside_corner_butts_the_second_wall_into_the_first() {
    let plan = plan(&room(
        StyleKind::BoardAndBatten,
        &[(144, Corner::Inside), (120, Corner::None)],
    ))
    .unwrap();
    assert_eq!(
        (plan.walls[0].left_joint, plan.walls[0].right_joint),
        (Joint::Free, Joint::InsideThrough)
    );
    assert_eq!(
        (plan.walls[1].left_joint, plan.walls[1].right_joint),
        (Joint::InsideButt, Joint::Free)
    );

    // Wall 1 runs into the corner: its rail is the full 144". Wall 2 stops
    // one 3/4" board thickness short: 119 1/4".
    let rail_1 = wall_parts(&plan, 0, PartKind::TopRail)[0];
    let rail_2 = wall_parts(&plan, 1, PartKind::TopRail)[0];
    assert_eq!((rail_1.rect.x, rail_1.length), (Length::ZERO, inches(144)));
    assert_eq!((rail_2.rect.x, rail_2.length), (frac(3, 4), frac(477, 4)));

    // The last 3/4" of wall 1 is hidden behind wall 2's trim, so its corner
    // batten is cut 3 1/4" wide to still show 2 1/2".
    let battens_1 = wall_parts(&plan, 0, PartKind::Batten);
    let corner = battens_1.last().unwrap();
    assert_eq!(corner.rect.right(), inches(144));
    assert_eq!(corner.rect.width, frac(13, 4));
    // Wall 2's corner batten is a normal one, starting where its trim does.
    let battens_2 = wall_parts(&plan, 1, PartKind::Batten);
    assert_eq!(
        (battens_2[0].rect.x, battens_2[0].rect.width),
        (frac(3, 4), frac(5, 2))
    );

    // What shows on wall 1 is 143 1/4" wide: ten 2 1/2" faces and nine equal
    // bays of (143.25 - 25) / 9.
    let bay = (frac(573, 4) - frac(5, 2) * 10) / 9;
    for pair in battens_1.windows(2) {
        let gap = pair[1].rect.x - pair[0].rect.right();
        assert!((gap - bay).nm().abs() < 10, "bay was {gap:?}");
    }

    assert_eq!(
        cuts(&plan),
        [
            ("A", PartKind::Batten, frac(181, 4), 17),
            ("B", PartKind::Batten, frac(181, 4), 1),
            ("C", PartKind::TopRail, inches(144), 1),
            ("D", PartKind::TopRail, frac(477, 4), 1),
        ]
    );
    assert_eq!(plan.cut_list[1].profile.width, frac(13, 4));
    assert!(plan.cut_list.iter().all(|item| item.cuts == SQUARE));
}

#[test]
fn an_outside_corner_laps_the_battens_and_miters_the_rails() {
    let plan = plan(&room(
        StyleKind::BoardAndBatten,
        &[(144, Corner::Outside), (120, Corner::None)],
    ))
    .unwrap();
    assert_eq!(plan.walls[0].right_joint, Joint::OutsideThrough);
    assert_eq!(plan.walls[1].left_joint, Joint::OutsideButt);

    // Both rails run 3/4" past the corner to a miter with its long point on
    // the face.
    let rail_1 = wall_parts(&plan, 0, PartKind::TopRail)[0];
    let rail_2 = wall_parts(&plan, 1, PartKind::TopRail)[0];
    assert_eq!((rail_1.rect.x, rail_1.length), (Length::ZERO, frac(579, 4)));
    assert_eq!(rail_1.cuts, [EndCut::Square, EndCut::OutsideMiter]);
    assert_eq!((rail_2.rect.x, rail_2.length), (frac(-3, 4), frac(483, 4)));
    assert_eq!(rail_2.cuts, [EndCut::OutsideMiter, EndCut::Square]);

    // Wall 1's corner batten overhangs the corner by 3/4" to cover the edge
    // of wall 2's, which is cut 3/4" narrower so both show 2 1/2".
    let corner_1 = *wall_parts(&plan, 0, PartKind::Batten).last().unwrap();
    assert_eq!(
        (corner_1.rect.right(), corner_1.rect.width),
        (frac(579, 4), frac(5, 2))
    );
    let corner_2 = wall_parts(&plan, 1, PartKind::Batten)[0];
    assert_eq!(
        (corner_2.rect.x, corner_2.rect.width),
        (Length::ZERO, frac(7, 4))
    );

    assert_eq!(
        cuts(&plan),
        [
            ("A", PartKind::Batten, frac(181, 4), 1),
            ("B", PartKind::Batten, frac(181, 4), 17),
            ("C", PartKind::TopRail, frac(579, 4), 1),
            ("D", PartKind::TopRail, frac(483, 4), 1),
        ]
    );
    assert_eq!(plan.cut_list[0].profile.width, frac(7, 4));
}

#[test]
fn a_closed_room_treats_every_wall_the_same_way() {
    let inside = Corner::Inside;
    let plan = plan(&room(
        StyleKind::BoardAndBatten,
        &[(144, inside), (120, inside), (144, inside), (120, inside)],
    ))
    .unwrap();

    // The last wall's corner leads back to the first, so every wall butts on
    // its left and runs through on its right.
    for wall in &plan.walls {
        assert_eq!(
            (wall.left_joint, wall.right_joint),
            (Joint::InsideButt, Joint::InsideThrough)
        );
        for part in &wall.parts {
            assert!(part.rect.x >= frac(3, 4), "{part:?} is inside the corner");
        }
    }

    // Each rail is 3/4" short of its wall, and each wall has one wide corner
    // batten: 9 + 7 + 9 + 7 normal battens and 4 wide ones.
    assert_eq!(
        cuts(&plan),
        [
            ("A", PartKind::Batten, frac(181, 4), 32),
            ("B", PartKind::Batten, frac(181, 4), 4),
            ("C", PartKind::TopRail, frac(573, 4), 2),
            ("D", PartKind::TopRail, frac(477, 4), 2),
        ]
    );
}

#[test]
fn shaker_rails_and_stiles_follow_the_corner_too() {
    let plan = plan(&room(
        StyleKind::ShakerPanel,
        &[(144, Corner::Inside), (120, Corner::None)],
    ))
    .unwrap();
    for kind in [PartKind::TopRail, PartKind::BottomRail] {
        let rail_1 = wall_parts(&plan, 0, kind)[0];
        let rail_2 = wall_parts(&plan, 1, kind)[0];
        assert_eq!((rail_1.rect.x, rail_1.length), (Length::ZERO, inches(144)));
        assert_eq!((rail_2.rect.x, rail_2.length), (frac(3, 4), frac(477, 4)));
    }
    // 3 1/2" stiles: the hidden corner one is 4 1/4" wide.
    let corner = *wall_parts(&plan, 0, PartKind::Stile).last().unwrap();
    assert_eq!(corner.rect.width, frac(17, 4));
}

#[test]
fn mouldings_are_coped_inside_and_mitered_outside() {
    let mut project = room(
        StyleKind::PictureFrame,
        &[
            (144, Corner::Inside),
            (120, Corner::Outside),
            (96, Corner::None),
        ],
    );
    project.design.baseboard.existing = false;
    let plan = plan(&project).unwrap();

    // Inside corner: both pieces measure to the wall, and the second is
    // coped over the first. Outside corner: both run past the corner by the
    // moulding's thickness (3/4" chair rail, 9/16" baseboard) to a miter.
    let ends = |wall: usize, kind: PartKind| {
        let part = wall_parts(&plan, wall, kind)[0];
        (part.rect.x, part.length, part.cuts)
    };
    let (square, cope, miter) = (EndCut::Square, EndCut::Cope, EndCut::OutsideMiter);
    assert_eq!(
        ends(0, PartKind::ChairRail),
        (Length::ZERO, inches(144), [square, square])
    );
    assert_eq!(
        ends(1, PartKind::ChairRail),
        (Length::ZERO, frac(483, 4), [cope, miter])
    );
    assert_eq!(
        ends(2, PartKind::ChairRail),
        (frac(-3, 4), frac(387, 4), [miter, square])
    );
    assert_eq!(
        ends(1, PartKind::Baseboard),
        (Length::ZERO, inches(120) + frac(9, 16), [cope, miter])
    );
    assert_eq!(
        ends(2, PartKind::Baseboard),
        (frac(-9, 16), inches(96) + frac(9, 16), [miter, square])
    );

    // The boxes are on the wall itself and still sit one gap from each end.
    for (wall, width) in [(0, 144), (1, 120), (2, 96)] {
        let frames = wall_parts(&plan, wall, PartKind::FrameMoulding);
        assert_eq!(frames.iter().map(|f| f.rect.x).min(), Some(frac(7, 2)));
        // Equal boxes can leave a few nanometres over at the far end.
        let right = frames.iter().map(|f| f.rect.right()).max().unwrap();
        let slack = inches(width) - frac(7, 2) - right;
        assert!((0..10).contains(&slack.nm()), "right edge was {right:?}");
    }
}

#[test]
fn a_door_in_the_corner_leaves_the_other_wall_running_to_it() {
    let mut project = room(
        StyleKind::BoardAndBatten,
        &[(144, Corner::Inside), (120, Corner::None)],
    );
    project.walls[0].obstructions.push(obstruction(
        ObstructionKind::Door,
        inches(108),
        Length::ZERO,
        inches(36),
        inches(82),
    ));
    let plan = plan(&project).unwrap();

    // Wall 1 has no trim in the corner for wall 2 to butt against.
    assert_eq!(plan.walls[0].right_joint, Joint::Free);
    assert_eq!(plan.walls[1].left_joint, Joint::Free);
    let rail_2 = wall_parts(&plan, 1, PartKind::TopRail)[0];
    assert_eq!((rail_2.rect.x, rail_2.length), (Length::ZERO, inches(120)));
}

#[test]
fn a_lone_wall_ignores_its_corner_setting() {
    let plain = imperial(StyleKind::BoardAndBatten, Length::feet(12));
    let mut cornered = plain.clone();
    cornered.walls[0].corner = Corner::Inside;
    assert_eq!(plan(&plain).unwrap(), plan(&cornered).unwrap());
}

#[test]
fn spacing_is_agreed_across_walls() {
    let batten_count = |widths: &[Length]| -> Vec<usize> {
        let mut project = room(
            StyleKind::BoardAndBatten,
            &vec![(0, Corner::None); widths.len()],
        );
        for (wall, &width) in project.walls.iter_mut().zip(widths) {
            wall.width = width;
        }
        let plan = plan(&project).unwrap();
        (0..widths.len())
            .map(|wall| wall_parts(&plan, wall, PartKind::Batten).len())
            .collect()
    };
    let (long, short) = (frac(179, 2), frac(115, 2));

    // By itself a 57 1/2" wall takes four bays of about 11 1/4": a touch
    // nearer the 16" target spacing than three bays of about 15 3/4".
    assert_eq!(batten_count(&[short]), [5]);

    // Two 89 1/2" walls land on five bays of about 15". Beside those, three
    // bays on the short wall match far better than four, so it follows them.
    assert_eq!(batten_count(&[long, long, short]), [6, 6, 4]);
}

#[test]
fn invalid_projects_are_rejected_with_paths() {
    let paths = |project: &Project| -> Vec<String> {
        plan(project)
            .unwrap_err()
            .issues
            .into_iter()
            .map(|issue| issue.path)
            .collect()
    };

    let mut project = imperial(StyleKind::BoardAndBatten, Length::ZERO);
    assert_eq!(paths(&project), ["walls[0].width"]);

    project = imperial(StyleKind::BoardAndBatten, Length::feet(12));
    project.walls[0].height = inches(48);
    assert_eq!(paths(&project), ["design.style.height"]);

    project = imperial(StyleKind::BoardAndBatten, Length::feet(12));
    project.walls[0].obstructions.push(obstruction(
        ObstructionKind::Outlet,
        inches(143),
        inches(12),
        inches(3),
        inches(5),
    ));
    assert_eq!(paths(&project), ["walls[0].obstructions[0]"]);

    project = imperial(StyleKind::BoardAndBatten, Length::feet(12));
    project.walls.clear();
    assert_eq!(paths(&project), ["walls"]);

    project = imperial(StyleKind::BoardAndBatten, Length::feet(12));
    project.stock.lengths.clear();
    assert_eq!(paths(&project), ["stock.lengths"]);

    project = imperial(StyleKind::ShakerPanel, Length::feet(12));
    if let Style::ShakerPanel(style) = &mut project.design.style {
        style.height = inches(10); // rails alone are 7" over a 5 1/4" baseboard
    }
    assert_eq!(paths(&project), ["design.style.height"]);
}

#[test]
fn extreme_values_are_rejected_without_overflowing() {
    let huge = Length::from_nm(i64::MAX);
    let tiny = Length::from_nm(i64::MIN);
    for value in [huge, tiny] {
        let mut project = imperial(StyleKind::ShakerPanel, value);
        project.walls[0].height = value;
        project.walls[0].obstructions.push(obstruction(
            ObstructionKind::Window,
            value,
            value,
            value,
            value,
        ));
        project.design.baseboard.height = value;
        project.design.baseboard.thickness = value;
        if let Style::ShakerPanel(style) = &mut project.design.style {
            style.height = value;
            style.top_rail_width = value;
            style.bottom_rail_width = value;
            style.stile_width = value;
            style.target_panel_width = value;
            style.thickness = value;
        }
        project.stock = StockOptions {
            lengths: vec![value],
            kerf: value,
            cut_precision: value,
        };
        assert!(plan(&project).is_err());
    }
}

#[test]
fn projects_and_plans_round_trip_through_json() {
    let mut project = imperial(StyleKind::PictureFrame, Length::feet(12));
    project.walls[0].obstructions.push(obstruction(
        ObstructionKind::Outlet,
        inches(17),
        inches(12),
        frac(11, 4),
        frac(9, 2),
    ));
    let json = serde_json::to_string(&project).unwrap();
    assert!(json.contains(r#""style":{"kind":"picture_frame","chair_rail_height":863600000"#));
    assert!(json.contains(r#""system":"imperial""#));
    assert_eq!(serde_json::from_str::<Project>(&json).unwrap(), project);

    let plan = plan(&project).unwrap();
    let json = serde_json::to_string(&plan).unwrap();
    assert_eq!(serde_json::from_str::<Plan>(&json).unwrap(), plan);
}

#[test]
fn display_units_do_not_change_the_geometry() {
    let project = imperial(StyleKind::BoardAndBatten, Length::feet(12));
    let mut metric_display = project.clone();
    metric_display.units = UnitPrefs::metric();
    assert_eq!(plan(&project).unwrap(), plan(&metric_display).unwrap());
}
