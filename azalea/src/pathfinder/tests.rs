use std::{
    collections::HashSet,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use azalea_block::{BlockState, BlockTrait, blocks, properties};
use azalea_client::packet::game::SendGamePacketEvent;
use azalea_core::position::{BlockPos, ChunkPos};
use azalea_protocol::packets::game::ServerboundGamePacket;
use azalea_registry::builtin::BlockKind;
use azalea_world::{Chunk, ChunkStorage, PartialChunkStorage};
use bevy_ecs::observer::On;

use super::{
    GotoEvent,
    astar::PathfinderTimeout,
    doors::DoorRules,
    goals::BlockPosGoal,
    moves,
    simulation::{SimulatedPlayerBundle, Simulation},
};
use crate::pathfinder::goto_event::PathfinderOpts;

fn setup_blockposgoal_simulation(
    partial_chunks: &mut PartialChunkStorage,
    start_pos: BlockPos,
    end_pos: BlockPos,
    solid_blocks: &[BlockPos],
) -> Simulation {
    let mut simulation = setup_simulation_world(partial_chunks, start_pos, solid_blocks, &[]);

    // you can uncomment this while debugging tests to get trace logs
    // simulation.app.add_plugins(bevy_log::LogPlugin {
    //     level: bevy_log::Level::TRACE,
    //     filter: "".to_owned(),
    //     ..Default::default()
    // });

    simulation.app.world_mut().write_message(GotoEvent {
        entity: simulation.entity,
        goal: Arc::new(BlockPosGoal(end_pos)),
        opts: PathfinderOpts {
            successors_fn: moves::default_move,
            allow_mining: false,
            doors: DoorRules::new(),
            retry_on_no_path: true,
            min_timeout: PathfinderTimeout::Nodes(1_000_000),
            max_timeout: PathfinderTimeout::Nodes(5_000_000),
        },
    });
    simulation
}

fn setup_simulation_world(
    partial_chunks: &mut PartialChunkStorage,
    start_pos: BlockPos,
    solid_blocks: &[BlockPos],
    extra_blocks: &[(BlockPos, BlockState)],
) -> Simulation {
    let mut chunk_positions = HashSet::new();
    for block_pos in solid_blocks {
        chunk_positions.insert(ChunkPos::from(block_pos));
    }
    for (block_pos, _) in extra_blocks {
        chunk_positions.insert(ChunkPos::from(block_pos));
    }

    let mut chunks = ChunkStorage::default();
    for chunk_pos in chunk_positions {
        partial_chunks.set(&chunk_pos, Some(Chunk::default()), &mut chunks);
    }
    for block_pos in solid_blocks {
        chunks.set_block_state(*block_pos, BlockKind::Stone.into());
    }
    for (block_pos, block_state) in extra_blocks {
        chunks.set_block_state(*block_pos, *block_state);
    }

    let player = SimulatedPlayerBundle::new(start_pos.center_bottom());
    Simulation::new(chunks, player)
}

pub fn assert_simulation_reaches(simulation: &mut Simulation, ticks: usize, end_pos: BlockPos) {
    wait_until_bot_starts_moving(simulation);
    for _ in 0..ticks {
        simulation.tick();
    }
    assert_eq!(BlockPos::from(simulation.position()), end_pos);
}

pub fn wait_until_bot_starts_moving(simulation: &mut Simulation) {
    let start_pos = simulation.position();
    let start_time = Instant::now();
    while simulation.position() == start_pos
        && !simulation.is_mining()
        && start_time.elapsed() < Duration::from_millis(5000)
    {
        simulation.tick();
        thread::yield_now();
    }
}

#[test]
fn test_simple_forward() {
    let mut partial_chunks = PartialChunkStorage::default();
    let mut simulation = setup_blockposgoal_simulation(
        &mut partial_chunks,
        BlockPos::new(0, 71, 0),
        BlockPos::new(0, 71, 1),
        &[BlockPos::new(0, 70, 0), BlockPos::new(0, 70, 1)],
    );
    assert_simulation_reaches(&mut simulation, 20, BlockPos::new(0, 71, 1));
}

#[test]
fn test_double_diagonal_with_walls() {
    let mut partial_chunks = PartialChunkStorage::default();
    let mut simulation = setup_blockposgoal_simulation(
        &mut partial_chunks,
        BlockPos::new(0, 71, 0),
        BlockPos::new(2, 71, 2),
        &[
            BlockPos::new(0, 70, 0),
            BlockPos::new(1, 70, 1),
            BlockPos::new(2, 70, 2),
            BlockPos::new(1, 72, 0),
            BlockPos::new(2, 72, 1),
        ],
    );
    assert_simulation_reaches(&mut simulation, 30, BlockPos::new(2, 71, 2));
}

#[test]
fn test_jump_with_sideways_momentum() {
    let mut partial_chunks = PartialChunkStorage::default();
    let mut simulation = setup_blockposgoal_simulation(
        &mut partial_chunks,
        BlockPos::new(0, 71, 3),
        BlockPos::new(5, 76, 0),
        &[
            BlockPos::new(0, 70, 3),
            BlockPos::new(0, 70, 2),
            BlockPos::new(0, 70, 1),
            BlockPos::new(0, 70, 0),
            BlockPos::new(1, 71, 0),
            BlockPos::new(2, 72, 0),
            BlockPos::new(3, 73, 0),
            BlockPos::new(4, 74, 0),
            BlockPos::new(5, 75, 0),
        ],
    );
    assert_simulation_reaches(&mut simulation, 120, BlockPos::new(5, 76, 0));
}

#[test]
fn test_parkour_2_block_gap() {
    let mut partial_chunks = PartialChunkStorage::default();
    let mut simulation = setup_blockposgoal_simulation(
        &mut partial_chunks,
        BlockPos::new(0, 71, 0),
        BlockPos::new(0, 71, 3),
        &[BlockPos::new(0, 70, 0), BlockPos::new(0, 70, 3)],
    );
    assert_simulation_reaches(&mut simulation, 40, BlockPos::new(0, 71, 3));
}

#[test]
fn test_descend_and_parkour_2_block_gap() {
    let mut partial_chunks = PartialChunkStorage::default();
    let mut simulation = setup_blockposgoal_simulation(
        &mut partial_chunks,
        BlockPos::new(0, 71, 0),
        BlockPos::new(3, 67, 4),
        &[
            BlockPos::new(0, 70, 0),
            BlockPos::new(0, 69, 1),
            BlockPos::new(0, 68, 2),
            BlockPos::new(0, 67, 3),
            BlockPos::new(0, 66, 4),
            BlockPos::new(3, 66, 4),
        ],
    );
    assert_simulation_reaches(&mut simulation, 100, BlockPos::new(3, 67, 4));
}

#[test]
fn test_small_descend_and_parkour_2_block_gap() {
    let mut partial_chunks = PartialChunkStorage::default();
    let mut simulation = setup_blockposgoal_simulation(
        &mut partial_chunks,
        BlockPos::new(0, 71, 0),
        BlockPos::new(0, 70, 5),
        &[
            BlockPos::new(0, 70, 0),
            BlockPos::new(0, 70, 1),
            BlockPos::new(0, 69, 2),
            BlockPos::new(0, 69, 5),
        ],
    );
    assert_simulation_reaches(&mut simulation, 40, BlockPos::new(0, 70, 5));
}

#[test]
fn test_quickly_descend() {
    let mut partial_chunks = PartialChunkStorage::default();
    let mut simulation = setup_blockposgoal_simulation(
        &mut partial_chunks,
        BlockPos::new(0, 71, 0),
        BlockPos::new(0, 68, 3),
        &[
            BlockPos::new(0, 70, 0),
            BlockPos::new(0, 69, 1),
            BlockPos::new(0, 68, 2),
            BlockPos::new(0, 67, 3),
        ],
    );
    assert_simulation_reaches(&mut simulation, 60, BlockPos::new(0, 68, 3));
}

#[test]
fn test_2_gap_ascend_thrice() {
    let mut partial_chunks = PartialChunkStorage::default();
    let mut simulation = setup_blockposgoal_simulation(
        &mut partial_chunks,
        BlockPos::new(0, 71, 0),
        BlockPos::new(3, 74, 0),
        &[
            BlockPos::new(0, 70, 0),
            BlockPos::new(0, 71, 3),
            BlockPos::new(3, 72, 3),
            BlockPos::new(3, 73, 0),
        ],
    );
    assert_simulation_reaches(&mut simulation, 60, BlockPos::new(3, 74, 0));
}

#[test]
fn test_consecutive_3_gap_parkour() {
    let mut partial_chunks = PartialChunkStorage::default();
    let mut simulation = setup_blockposgoal_simulation(
        &mut partial_chunks,
        BlockPos::new(0, 71, 0),
        BlockPos::new(4, 71, 12),
        &[
            BlockPos::new(0, 70, 0),
            BlockPos::new(0, 70, 4),
            BlockPos::new(0, 70, 8),
            BlockPos::new(0, 70, 12),
            BlockPos::new(4, 70, 12),
        ],
    );
    assert_simulation_reaches(&mut simulation, 80, BlockPos::new(4, 71, 12));
}

#[test]
fn test_jumps_with_more_sideways_momentum() {
    let mut partial_chunks = PartialChunkStorage::default();
    let mut simulation = setup_blockposgoal_simulation(
        &mut partial_chunks,
        BlockPos::new(0, 71, 0),
        BlockPos::new(4, 74, 9),
        &[
            BlockPos::new(0, 70, 0),
            BlockPos::new(0, 70, 1),
            BlockPos::new(0, 70, 2),
            BlockPos::new(0, 71, 3),
            BlockPos::new(0, 72, 6),
            BlockPos::new(0, 73, 9),
            // this is the point where the bot might fall if it has too much momentum
            BlockPos::new(2, 73, 9),
            BlockPos::new(4, 73, 9),
        ],
    );
    assert_simulation_reaches(&mut simulation, 80, BlockPos::new(4, 74, 9));
}

#[test]
fn test_mine_through_non_colliding_block() {
    let mut partial_chunks = PartialChunkStorage::default();

    let mut simulation = setup_simulation_world(
        &mut partial_chunks,
        BlockPos::new(0, 72, 1),
        &[BlockPos::new(0, 71, 1)],
        &[
            (BlockPos::new(0, 71, 0), BlockKind::SculkVein.into()),
            (BlockPos::new(0, 70, 0), BlockKind::GrassBlock.into()),
            // this is an extra check to make sure that we don't accidentally break the block
            // below (since tnt will break instantly)
            (BlockPos::new(0, 69, 0), BlockKind::Tnt.into()),
        ],
    );

    simulation.app.world_mut().write_message(GotoEvent {
        entity: simulation.entity,
        goal: Arc::new(BlockPosGoal(BlockPos::new(0, 70, 0))),
        opts: PathfinderOpts::new()
            .min_timeout(PathfinderTimeout::Nodes(1_000_000))
            .max_timeout(PathfinderTimeout::Nodes(5_000_000)),
    });

    assert_simulation_reaches(&mut simulation, 200, BlockPos::new(0, 70, 0));
}

/// Stand in for the server in the simulation: right-clicking a door, fence
/// gate or trapdoor toggles its `open` property (for both halves of a door).
fn emulate_door_server(simulation: &mut Simulation) {
    let world = simulation.world.clone();
    simulation
        .app
        .world_mut()
        .add_observer(move |trigger: On<SendGamePacketEvent>| {
            let ServerboundGamePacket::UseItemOn(packet) = &trigger.packet else {
                return;
            };
            let pos = packet.block_hit.block_pos;
            let world = world.read();
            let state = world.get_block_state(pos).unwrap_or_default();
            let mut block: Box<dyn BlockTrait> = state.into();
            let Some(open) = block.get_property("open") else {
                return;
            };
            let toggled = if open == "true" { "false" } else { "true" };
            block.set_property("open", toggled).unwrap();
            world.set_block_state(pos, block.as_block_state());

            let other_half = match block.get_property("half") {
                Some("lower") => Some(pos.up(1)),
                Some("upper") => Some(pos.down(1)),
                _ => None,
            };
            if let Some(other_half) = other_half {
                let mut other: Box<dyn BlockTrait> =
                    world.get_block_state(other_half).unwrap_or_default().into();
                other.set_property("open", toggled).unwrap();
                world.set_block_state(other_half, other.as_block_state());
            }
        });
}

/// A one-block-wide corridor along +Z from `(0, 71, 0)` to `(0, 71, 4)` with
/// the given blocks placed on top of it (all in the `x = 0` column).
fn setup_corridor_simulation(
    partial_chunks: &mut PartialChunkStorage,
    obstacles: &[(BlockPos, BlockState)],
    opts: PathfinderOpts,
) -> Simulation {
    let floor = (0..=4).map(|z| BlockPos::new(0, 70, z)).collect::<Vec<_>>();
    let mut simulation =
        setup_simulation_world(partial_chunks, BlockPos::new(0, 71, 0), &floor, obstacles);
    emulate_door_server(&mut simulation);
    simulation.app.world_mut().write_message(GotoEvent {
        entity: simulation.entity,
        goal: Arc::new(BlockPosGoal(BlockPos::new(0, 71, 4))),
        opts: opts
            .allow_mining(false)
            .min_timeout(PathfinderTimeout::Nodes(1_000_000))
            .max_timeout(PathfinderTimeout::Nodes(5_000_000)),
    });
    simulation
}

fn door_halves(door: impl Fn(properties::Half) -> BlockState) -> Vec<(BlockPos, BlockState)> {
    vec![
        (BlockPos::new(0, 71, 2), door(properties::Half::Lower)),
        (BlockPos::new(0, 72, 2), door(properties::Half::Upper)),
    ]
}

fn oak_door_with(half: properties::Half, open: bool) -> BlockState {
    blocks::OakDoor {
        facing: properties::FacingCardinal::North,
        half,
        hinge: properties::Hinge::Left,
        open,
        powered: false,
    }
    .into()
}

fn oak_door(half: properties::Half) -> BlockState {
    oak_door_with(half, false)
}

fn open_oak_door(half: properties::Half) -> BlockState {
    oak_door_with(half, true)
}

fn iron_door(half: properties::Half) -> BlockState {
    blocks::IronDoor {
        facing: properties::FacingCardinal::North,
        half,
        hinge: properties::Hinge::Left,
        open: false,
        powered: false,
    }
    .into()
}

fn is_open(simulation: &Simulation, pos: BlockPos) -> bool {
    let state = simulation.world.read().get_block_state(pos).unwrap();
    state.property::<properties::Open>().unwrap()
}

#[test]
fn test_walk_through_closed_door_and_close_it() {
    let mut partial_chunks = PartialChunkStorage::default();
    let mut simulation = setup_corridor_simulation(
        &mut partial_chunks,
        &door_halves(oak_door),
        PathfinderOpts::new(),
    );
    assert_simulation_reaches(&mut simulation, 120, BlockPos::new(0, 71, 4));
    assert!(!is_open(&simulation, BlockPos::new(0, 71, 2)));
    assert!(!is_open(&simulation, BlockPos::new(0, 72, 2)));
}

#[test]
fn test_walk_through_fence_gate_and_close_it() {
    let mut partial_chunks = PartialChunkStorage::default();
    let gate: BlockState = blocks::OakFenceGate {
        facing: properties::FacingCardinal::North,
        in_wall: false,
        open: false,
        powered: false,
    }
    .into();
    let mut simulation = setup_corridor_simulation(
        &mut partial_chunks,
        &[(BlockPos::new(0, 71, 2), gate)],
        PathfinderOpts::new(),
    );
    assert_simulation_reaches(&mut simulation, 120, BlockPos::new(0, 71, 4));
    assert!(!is_open(&simulation, BlockPos::new(0, 71, 2)));
}

fn oak_trapdoor(facing: properties::FacingCardinal) -> BlockState {
    blocks::OakTrapdoor {
        facing,
        half: properties::TopBottom::Top,
        open: false,
        powered: false,
        waterlogged: false,
    }
    .into()
}

#[test]
fn test_walk_through_trapdoor_in_head_cell() {
    let mut partial_chunks = PartialChunkStorage::default();
    // an open trapdoor facing east sits against the east/west side of its
    // block, so it doesn't block a walk along Z
    let mut simulation = setup_corridor_simulation(
        &mut partial_chunks,
        &[(
            BlockPos::new(0, 72, 2),
            oak_trapdoor(properties::FacingCardinal::East),
        )],
        PathfinderOpts::new(),
    );
    assert_simulation_reaches(&mut simulation, 120, BlockPos::new(0, 71, 4));
    assert!(!is_open(&simulation, BlockPos::new(0, 72, 2)));
}

#[test]
fn test_trapdoor_that_would_block_the_way_when_open_is_not_used() {
    let mut partial_chunks = PartialChunkStorage::default();
    // facing north, the open trapdoor sits across the corridor
    let mut simulation = setup_corridor_simulation(
        &mut partial_chunks,
        &[(
            BlockPos::new(0, 72, 2),
            oak_trapdoor(properties::FacingCardinal::North),
        )],
        PathfinderOpts::new(),
    );
    for _ in 0..200 {
        simulation.tick();
    }
    assert!(!is_open(&simulation, BlockPos::new(0, 72, 2)));
    assert!(BlockPos::from(simulation.position()).z < 2);
}

#[test]
fn test_door_in_avoid_set_is_not_opened() {
    let mut partial_chunks = PartialChunkStorage::default();
    let mut simulation = setup_corridor_simulation(
        &mut partial_chunks,
        &door_halves(oak_door),
        // giving only the upper half is enough to protect the whole door
        PathfinderOpts::new().avoid_interacting([BlockPos::new(0, 72, 2)]),
    );
    for _ in 0..200 {
        simulation.tick();
    }
    assert!(!is_open(&simulation, BlockPos::new(0, 71, 2)));
    assert!(BlockPos::from(simulation.position()).z < 2);
}

#[test]
fn test_opening_disabled_does_not_open_doors() {
    let mut partial_chunks = PartialChunkStorage::default();
    let mut simulation = setup_corridor_simulation(
        &mut partial_chunks,
        &door_halves(oak_door),
        PathfinderOpts::new().allow_opening(false),
    );
    for _ in 0..200 {
        simulation.tick();
    }
    assert!(!is_open(&simulation, BlockPos::new(0, 71, 2)));
    assert!(BlockPos::from(simulation.position()).z < 2);
}

#[test]
fn test_iron_door_is_not_used() {
    let mut partial_chunks = PartialChunkStorage::default();
    let mut simulation = setup_corridor_simulation(
        &mut partial_chunks,
        &door_halves(iron_door),
        PathfinderOpts::new(),
    );
    for _ in 0..200 {
        simulation.tick();
    }
    assert!(!is_open(&simulation, BlockPos::new(0, 71, 2)));
    assert!(BlockPos::from(simulation.position()).z < 2);
}

#[test]
fn test_door_that_was_already_open_stays_open() {
    let mut partial_chunks = PartialChunkStorage::default();
    let mut simulation = setup_corridor_simulation(
        &mut partial_chunks,
        &door_halves(open_oak_door),
        PathfinderOpts::new(),
    );
    assert_simulation_reaches(&mut simulation, 120, BlockPos::new(0, 71, 4));
    assert!(is_open(&simulation, BlockPos::new(0, 71, 2)));
    assert!(is_open(&simulation, BlockPos::new(0, 72, 2)));
}

#[test]
fn test_door_closed_by_someone_else_mid_path_is_opened_again() {
    let mut partial_chunks = PartialChunkStorage::default();
    let mut simulation = setup_corridor_simulation(
        &mut partial_chunks,
        &door_halves(open_oak_door),
        PathfinderOpts::new(),
    );
    wait_until_bot_starts_moving(&mut simulation);
    for (pos, state) in door_halves(oak_door) {
        simulation.world.read().set_block_state(pos, state);
    }
    assert_simulation_reaches(&mut simulation, 150, BlockPos::new(0, 71, 4));
    assert!(!is_open(&simulation, BlockPos::new(0, 71, 2)));
    assert!(!is_open(&simulation, BlockPos::new(0, 72, 2)));
}
