//! Opening and closing doors, fence gates and trapdoors while pathfinding.
//!
//! Wooden and copper doors and trapdoors, and fence gates, can be opened by
//! right-clicking them. The moves treat such a block in a body cell (feet or
//! head) as passable at an extra cost as long as its *open* shape leaves room
//! for the move, and the executor opens it before stepping in. Once the bot has
//! walked through, [`close_opened_doors`] closes it again.
//!
//! Iron doors and iron trapdoors need redstone, so they're never touched.
//!
//! # Geometry
//!
//! An open door or trapdoor is a thin slab against one side of its block, so it
//! only lets through moves that run parallel to that slab:
//!
//! - doors: closed, the slab is on the `facing` side; open, it's turned by 90
//!   degrees (whichever hinge side). Moves along the `facing` axis pass.
//! - trapdoors: open, the slab is on the `facing` axis. Moves across the
//!   `facing` axis pass.
//! - fence gates: open gates have no collision, so any move passes.
//!
//! Trapdoors below the feet (floors) are never opened or walked through.

use std::{collections::HashSet, sync::Arc};

use azalea_block::{BlockState, properties};
use azalea_client::{
    StartWalkEvent, WalkDirection, interact::StartUseItemQueued, local_player::WorldHolder,
};
use azalea_core::{
    direction::CardinalDirection,
    position::{BlockPos, Vec3},
};
use azalea_entity::Position;
use azalea_protocol::packets::game::s_interact::InteractionHand;
use azalea_registry::{builtin::BlockKind, tags};
use bevy_ecs::prelude::*;

use crate::{
    bot::LookAtEvent,
    pathfinder::{Pathfinder, moves::MovesCtx, positions::RelBlockPos},
};

/// The extra cost for passing through a closed door, gate or trapdoor block.
///
/// A door is two blocks tall, so walking through it pays this twice.
const OPEN_COST: f32 = 4.;
/// How many ticks to wait for a click to take effect before clicking again.
const CLICK_RETRY_TICKS: u32 = 5;
/// How many ticks after opening a door the bot has to walk into it before the
/// pathfinder forgets that it opened it (and leaves it open).
const GIVE_UP_TICKS: u32 = 600;
/// How often the pathfinder tries to close a door before it gives up.
const MAX_CLOSE_ATTEMPTS: u8 = 5;
/// The block interaction range.
const REACH: f64 = 4.5;
/// Half the width of the player.
const HALF_WIDTH: f64 = 0.3;
/// How far outside of its block the bot must be before we close a door, so the
/// closing door can't clip it.
const LEAVE_MARGIN: f64 = 0.1;

/// Whether the pathfinder may open hand-openable blocks, and which positions
/// it must never touch.
///
/// This is configured with [`PathfinderOpts::allow_opening`] and
/// [`PathfinderOpts::avoid_interacting`].
///
/// [`PathfinderOpts::allow_opening`]: crate::pathfinder::PathfinderOpts::allow_opening
/// [`PathfinderOpts::avoid_interacting`]: crate::pathfinder::PathfinderOpts::avoid_interacting
#[derive(Clone, Debug)]
pub struct DoorRules {
    pub(crate) allow: bool,
    pub(crate) avoid: Option<Arc<HashSet<BlockPos>>>,
}

impl DoorRules {
    /// The default rules: opening is allowed and no position is avoided.
    pub const fn new() -> Self {
        Self {
            allow: true,
            avoid: None,
        }
    }

    /// Rules that never allow any interaction.
    pub(crate) const fn never() -> Self {
        Self {
            allow: false,
            avoid: None,
        }
    }

    /// Returns what kind of openable the block at `pos` is, unless the rules
    /// forbid toggling it.
    pub(crate) fn toggleable(&self, pos: BlockPos, state: BlockState) -> Option<Openable> {
        if !self.allow {
            return None;
        }
        let kind = openable_kind(state)?;
        (!is_avoided(self.avoid.as_deref(), pos, state, kind)).then_some(kind)
    }
}

impl Default for DoorRules {
    fn default() -> Self {
        Self::new()
    }
}

/// The kinds of block that can be opened by right-clicking them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Openable {
    Door,
    Gate,
    Trapdoor,
}

fn openable_kind(state: BlockState) -> Option<Openable> {
    // doors, gates and trapdoors all have this, and most blocks don't
    state.property::<properties::Open>()?;
    let kind = BlockKind::from(state);
    if matches!(kind, BlockKind::IronDoor | BlockKind::IronTrapdoor) {
        None
    } else if tags::blocks::DOORS.contains(&kind) {
        Some(Openable::Door)
    } else if tags::blocks::FENCE_GATES.contains(&kind) {
        Some(Openable::Gate)
    } else if tags::blocks::TRAPDOORS.contains(&kind) {
        Some(Openable::Trapdoor)
    } else {
        None
    }
}

fn is_open(state: BlockState) -> bool {
    state.property::<properties::Open>().unwrap_or_default()
}

/// The lowest block of the openable at `pos`. Only doors are two blocks tall.
fn base_pos(pos: BlockPos, state: BlockState, kind: Openable) -> BlockPos {
    match (kind, state.property::<properties::Half>()) {
        (Openable::Door, Some(properties::Half::Upper)) => pos.down(1),
        _ => pos,
    }
}

fn is_avoided(
    avoid: Option<&HashSet<BlockPos>>,
    pos: BlockPos,
    state: BlockState,
    kind: Openable,
) -> bool {
    let Some(avoid) = avoid else {
        return false;
    };
    let base = base_pos(pos, state, kind);
    avoid.contains(&pos)
        || avoid.contains(&base)
        || (kind == Openable::Door && avoid.contains(&base.up(1)))
}

/// Whether the player can walk through this block horizontally (along the X
/// axis if `along_x`, otherwise along Z) once it's open, or if it already is.
fn passes(state: BlockState, kind: Openable, along_x: bool) -> bool {
    let Some(facing) = state.property::<properties::FacingCardinal>() else {
        return false;
    };
    let facing_x = matches!(
        facing,
        properties::FacingCardinal::East | properties::FacingCardinal::West
    );
    match kind {
        Openable::Gate => true,
        Openable::Door => facing_x == along_x,
        Openable::Trapdoor => facing_x != along_x,
    }
}

/// The cost of passing through the body cell `cell` while moving along `dir`.
///
/// This is the cost of breaking the block, except that a hand-openable block
/// that the rules allow us to toggle and that doesn't block `dir` once open is
/// passable for a small toll (or for free if it's open already).
pub(crate) fn body_cost(ctx: &MovesCtx, cell: RelBlockPos, dir: CardinalDirection) -> f32 {
    let cost = ctx.world.cost_for_breaking_block(cell, ctx.mining_cache);
    if cost == 0. {
        return cost;
    }
    door_cost(ctx, cell, dir).unwrap_or(cost)
}

fn door_cost(ctx: &MovesCtx, cell: RelBlockPos, dir: CardinalDirection) -> Option<f32> {
    if !ctx.doors.allow || ctx.world.is_block_solid(cell) {
        // this is checked for every wall, so skip the slow lookup for those
        return None;
    }
    let state = ctx.world.get_block_state(cell);
    let pos = cell.apply(ctx.world.origin());
    let kind = ctx.doors.toggleable(pos, state)?;
    if !passes(state, kind, dir.x() != 0) {
        return None;
    }
    Some(if is_open(state) { 0. } else { OPEN_COST })
}

/// The cost of passing through the feet and head cells at `pos` while moving
/// along `dir`.
pub(crate) fn passing_cost(ctx: &MovesCtx, pos: RelBlockPos, dir: CardinalDirection) -> f32 {
    body_cost(ctx, pos, dir) + body_cost(ctx, pos.up(1), dir)
}

/// Like [`passing_cost`], but infinite unless the block below can be stood on.
pub(crate) fn standing_cost(ctx: &MovesCtx, pos: RelBlockPos, dir: CardinalDirection) -> f32 {
    if !ctx.world.is_block_standable(pos.down(1)) {
        return f32::INFINITY;
    }
    passing_cost(ctx, pos, dir)
}

/// Whether the node at `pos` is in a doorway, meaning that its feet or head
/// cell holds a hand-openable block that isn't passable (a door or trapdoor).
pub(crate) fn in_doorway(ctx: &MovesCtx, pos: RelBlockPos) -> bool {
    !ctx.world.is_passable(pos)
        && [pos, pos.up(1)]
            .into_iter()
            .any(|cell| openable_kind(ctx.world.get_block_state(cell)).is_some())
}

/// Whether the bot can leave a doorway at `pos` along `dir` without walking
/// into the door.
pub(crate) fn can_leave_doorway(ctx: &MovesCtx, pos: RelBlockPos, dir: CardinalDirection) -> bool {
    [pos, pos.up(1)].into_iter().all(|cell| {
        let state = ctx.world.get_block_state(cell);
        openable_kind(state).is_none_or(|kind| passes(state, kind, dir.x() != 0))
    })
}

/// Whether the executor can use the openable at `pos` on a move along the X
/// axis (if `along_x`) or Z axis, and what kind of block it is.
pub(crate) fn usable(
    rules: &DoorRules,
    pos: BlockPos,
    state: BlockState,
    along_x: bool,
) -> Option<Openable> {
    let kind = rules.toggleable(pos, state)?;
    passes(state, kind, along_x).then_some(kind)
}

/// Whether this openable block is closed.
pub(crate) fn is_closed(state: BlockState) -> bool {
    !is_open(state)
}

/// The doors, gates and trapdoors that the pathfinder has opened and still has
/// to close, plus the bookkeeping for not clicking every tick.
#[derive(Clone, Debug, Default)]
pub struct DoorState {
    ticks: u32,
    last_click: Option<u32>,
    avoid: Option<Arc<HashSet<BlockPos>>>,
    opened: Vec<OpenedDoor>,
}

#[derive(Clone, Debug)]
struct OpenedDoor {
    /// The lowest block of the door.
    pos: BlockPos,
    opened_at: u32,
    /// Whether the bot has been inside the door's block since opening it.
    entered: bool,
    close_attempts: u8,
}

impl DoorState {
    /// Whether enough ticks have passed since the last click to click again.
    pub(crate) fn can_click(&self) -> bool {
        self.last_click
            .is_none_or(|tick| self.ticks - tick >= CLICK_RETRY_TICKS)
    }

    /// Record that we're opening the door at `pos` (which can be either half)
    /// so it gets closed after the bot has walked through.
    pub(crate) fn track_opened(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        kind: Openable,
        avoid: &Option<Arc<HashSet<BlockPos>>>,
    ) {
        self.last_click = Some(self.ticks);
        self.avoid.clone_from(avoid);

        let pos = base_pos(pos, state, kind);
        let fresh = OpenedDoor {
            pos,
            opened_at: self.ticks,
            entered: false,
            close_attempts: 0,
        };
        match self.opened.iter_mut().find(|door| door.pos == pos) {
            Some(door) => *door = fresh,
            None => self.opened.push(fresh),
        }
    }
}

/// Whether the player at `position` is within `margin` blocks of the block
/// column of height `height` that starts at `base`.
fn overlaps(position: Vec3, base: BlockPos, height: f64, margin: f64) -> bool {
    let reach = 0.5 + HALF_WIDTH + margin;
    let base_y = f64::from(base.y);
    (position.x - (f64::from(base.x) + 0.5)).abs() < reach
        && (position.z - (f64::from(base.z) + 0.5)).abs() < reach
        && position.y < base_y + height
        && position.y + 1.8 > base_y
}

/// Close the doors that the pathfinder opened once the bot has walked through
/// them.
///
/// A door is only closed after the bot has been inside of its block and has
/// fully left it again, so closing it can't hit the bot. Doors that the
/// pathfinder didn't open are never touched, nor are positions that were passed
/// to [`PathfinderOpts::avoid_interacting`].
///
/// [`PathfinderOpts::avoid_interacting`]: crate::pathfinder::PathfinderOpts::avoid_interacting
pub fn close_opened_doors(
    mut commands: Commands,
    mut query: Query<(Entity, &mut Pathfinder, &Position, &WorldHolder)>,
    mut look_at_events: MessageWriter<LookAtEvent>,
    mut walk_events: MessageWriter<StartWalkEvent>,
) {
    for (entity, mut pathfinder, position, world_holder) in &mut query {
        let DoorState {
            ticks,
            last_click,
            avoid,
            opened,
        } = &mut pathfinder.doors;
        *ticks += 1;
        if opened.is_empty() {
            continue;
        }

        let world = world_holder.shared.read();
        opened.retain_mut(|door| {
            let state = world.get_block_state(door.pos).unwrap_or_default();
            let Some(kind) = openable_kind(state) else {
                // the block is gone
                return false;
            };
            let height = if kind == Openable::Door { 2. } else { 1. };
            let inside = |margin| overlaps(**position, door.pos, height, margin);

            if !door.entered {
                if inside(0.) {
                    door.entered = true;
                } else if *ticks - door.opened_at > GIVE_UP_TICKS {
                    return false;
                }
                return true;
            }

            if !is_open(state) {
                // it's closed now, either by us or by someone else
                return false;
            }
            if inside(LEAVE_MARGIN) {
                return true;
            }
            if is_avoided(avoid.as_deref(), door.pos, state, kind)
                || door.close_attempts >= MAX_CLOSE_ATTEMPTS
                || (position.up(1.53) - door.pos.center()).length() > REACH
            {
                return false;
            }
            if last_click.is_some_and(|tick| *ticks - tick < CLICK_RETRY_TICKS) {
                return true;
            }

            look_at_events.write(LookAtEvent {
                entity,
                position: door.pos.center(),
            });
            walk_events.write(StartWalkEvent {
                entity,
                direction: WalkDirection::None,
            });
            commands.entity(entity).insert(StartUseItemQueued {
                hand: InteractionHand::MainHand,
                force_block: Some(door.pos),
            });
            *last_click = Some(*ticks);
            door.close_attempts += 1;
            true
        });
    }
}

/// The rules used when the pathfinder has no [`PathfinderOpts`].
///
/// [`PathfinderOpts`]: crate::pathfinder::PathfinderOpts
static DEFAULT_RULES: DoorRules = DoorRules::new();

impl Pathfinder {
    /// The door rules of the current goal and the doors that have been opened.
    pub(crate) fn door_parts(&mut self) -> (&DoorRules, &mut DoorState) {
        let rules = self
            .opts
            .as_ref()
            .map_or(&DEFAULT_RULES, |opts| &opts.doors);
        (rules, &mut self.doors)
    }
}
